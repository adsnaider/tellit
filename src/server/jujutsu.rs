//! Jujutsu and Git repository inspection, graph parsing, rebase-isolated diffing, and store access.

extern crate alloc;

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::time::Duration;
use std::collections::{HashMap, HashSet};
use std::io;
use std::path::{Component, Path, PathBuf};
use std::process::Output;

use tellit_types::{
    ApproveCommitResponse, CommentSide, JjChange, JjChangesResponse, JjCommentThread, JjCommit,
    JjDiffResponse, JjGraphRow, JjStatusResponse, LineCountResponse, LineRangeSpec,
};
use thiserror::Error;
use tokio::process::Command;

use super::diff_parser::{self, COMMIT_MESSAGE_PATH};
use crate::{LineRange, ThreadInfo};

const JJ_LOG_NODE_MARKER: &str = "__TELLIT_NODE__";
const JJ_LOG_EDGE_MARKER: &str = "__TELLIT_EDGE__";
const ROOT_CHANGE_ID: &str = "00000000000000000000000000000000";
const UPWARD_CONNECTING_GLYPHS: &str = "│|:╯┘'╰└`├┤┴┼+";
const MAX_ETXTBSY_RETRIES: u32 = 10;
const MAX_BLOB_BYTES: usize = 16 * 1024 * 1024;

const JJ_CHANGE_FIELDS_EXPR: &str = r#"change_id ++ "\t" ++ change_id.shortest() ++ "\t" ++ commit_id ++ "\t" ++ if(current_working_copy, "1", "0") ++ "\t" ++ if(empty, "1", "0") ++ "\t" ++ if(immutable, "1", "0") ++ "\t" ++ if(conflict, "1", "0") ++ "\t" ++ if(root, "1", "0") ++ "\t" ++ bookmarks.map(|b| b.name()).join(",") ++ "\t" ++ parents.map(|p| p.commit_id()).join(",") ++ "\t" ++ if(author.name(), author.name(), author.email()) ++ "\t" ++ committer.timestamp().format("%Y-%m-%d %H:%M:%S") ++ "\t" ++ description.first_line()"#;

const JJ_COMMIT_META_TEMPLATE: &str = r#"commit_id ++ "\t" ++ change_id ++ "\t" ++ parents.map(|p| p.commit_id()).join(",") ++ "\t" ++ description.escape_json() ++ "\n""#;

const JJ_EVOLOG_TEMPLATE: &str = r#"commit.commit_id() ++ "\t" ++ commit.committer().timestamp().format("%Y-%m-%d %H:%M:%S") ++ "\t" ++ if(commit.current_working_copy(), "1", "0") ++ "\t" ++ commit.parents().map(|p| p.commit_id()).join(",") ++ "\t" ++ commit.description().escape_json() ++ "\t" ++ commit.description().first_line() ++ "\n""#;

const JJ_EVOLOG_FALLBACK_TEMPLATE: &str = r#"commit_id ++ "\t" ++ committer.timestamp().format("%Y-%m-%d %H:%M:%S") ++ "\t" ++ if(current_working_copy, "1", "0") ++ "\t" ++ parents.map(|p| p.commit_id()).join(",") ++ "\t" ++ description.escape_json() ++ "\t" ++ description.first_line() ++ "\n""#;

/// Errors produced when inspecting Jujutsu/Git repositories or updating `.tellit/` state.
#[derive(Debug, Error)]
pub enum JujutsuError {
    /// Spawning an external `jj` or `git` process failed.
    #[error("failed to execute {command}: {source}")]
    CommandSpawn {
        /// Program name that failed to spawn.
        command: &'static str,
        /// Underlying OS process launch error.
        #[source]
        source: io::Error,
    },
    /// External `jj` or `git` process exited with a non-zero status.
    #[error("{command} error: {stderr}")]
    CommandFailed {
        /// Program name that failed.
        command: &'static str,
        /// Trimmed stderr message from the failed command.
        stderr: String,
    },
    /// Request parameter failed validation (missing, empty, or unsafe path).
    #[error("{0}")]
    InvalidInput(String),
    /// Requested comment thread or revision could not be found.
    #[error("{0}")]
    NotFound(String),
    /// Underlying `.tellit/` review store operation failed.
    #[error("{0}")]
    Store(String),
    /// Filesystem or blocking task I/O error.
    #[error(transparent)]
    Io(#[from] io::Error),
}

/// Resolved main repository root and `.jj/repo` directory for primary or secondary workspaces.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JjRepoPaths {
    /// Primary repository root directory containing the shared `.tellit/` store.
    pub main_repo_root: PathBuf,
    /// Shared `.jj/repo` metadata directory (holding `op_heads` and `store`).
    pub jj_repo_dir: PathBuf,
}

/// Raw repository file bytes and detected MIME type returned by `GET /api/blob/{*path}`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlobPayload {
    /// Unmodified file bytes at the requested revision.
    pub bytes: Vec<u8>,
    /// MIME content type for HTTP response headers.
    pub content_type: &'static str,
}

/// Result of counting lines for `GET /api/line-count/{*path}` with lookup-completeness metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineCountOutcome {
    /// Serialized old/new line count response payload.
    pub counts: LineCountResponse,
    /// `true` when every requested revision lookup succeeded without falling back to `0`.
    pub all_lookups_succeeded: bool,
}

#[derive(Debug, Clone)]
struct CommitMeta {
    commit_id: String,
    change_id: String,
    parents: Vec<String>,
    description: String,
}

struct ParsedChangeRow {
    change: JjChange,
    is_root: bool,
}

struct RawEvologEntry {
    commit_id: String,
    timestamp: String,
    is_working_copy: bool,
    parents: Vec<String>,
    description: String,
    first_line: String,
}

/// Asynchronous Jujutsu and `.tellit/` review service bound to a repository directory.
#[derive(Debug, Clone)]
pub struct JujutsuService {
    repo_path: PathBuf,
    store_override: Option<PathBuf>,
}

impl JujutsuService {
    /// Creates a service operating on `repo_path` with an optional custom `.tellit/` path.
    #[must_use]
    pub const fn new(repo_path: PathBuf, store_override: Option<PathBuf>) -> Self {
        Self {
            repo_path,
            store_override,
        }
    }

    /// Returns the workspace root directory configured for this service.
    #[must_use]
    pub fn repo_path(&self) -> &Path {
        &self.repo_path
    }

    /// Resolves the `.tellit/` store directory for this workspace.
    #[must_use]
    pub fn store_dir(&self) -> PathBuf {
        if let Some(ref custom) = self.store_override {
            return custom.clone();
        }
        if let Some(paths) = resolve_jj_repo_paths(&self.repo_path) {
            return paths.main_repo_root.join(".tellit");
        }
        self.repo_path.join(".tellit")
    }

    /// Detects whether `repo_path` is a Jujutsu workspace and resolves optional initial revisions.
    pub async fn detect_status(
        &self,
        commit_ish: Option<&str>,
        compare_with: Option<&str>,
    ) -> JjStatusResponse {
        let Ok(root) = self.run_jj(&["root"]).await else {
            return JjStatusResponse::not_jj();
        };
        if root.is_empty() {
            return JjStatusResponse::not_jj();
        }

        let target_arg = commit_ish.unwrap_or("@");
        let initial_target = self.resolve_initial_target_commit(target_arg).await;
        let initial_base = match compare_with {
            Some(base) if !base.trim().is_empty() => {
                Some(self.resolve_revision_to_commit_id(base.trim()).await)
            }
            _ => None,
        };

        JjStatusResponse::jj(root).with_initial_revisions(Some(initial_target), initial_base)
    }

    /// Lists Jujutsu revisions and DAG graph rows, or looks up a single change/revset `query`.
    ///
    /// # Errors
    ///
    /// Returns [`JujutsuError`] if running `jj log` fails.
    pub async fn get_changes(
        &self,
        query: Option<&str>,
    ) -> Result<JjChangesResponse, JujutsuError> {
        if let Some(q) = query.map(str::trim).filter(|s| !s.is_empty()) {
            let Some(change) = self.get_single_change(q).await? else {
                return Ok(JjChangesResponse {
                    changes: Vec::new(),
                    graph_rows: Vec::new(),
                });
            };
            let graph_rows = alloc::vec![JjGraphRow::standalone(&change)];
            return Ok(JjChangesResponse {
                changes: alloc::vec![change],
                graph_rows,
            });
        }

        self.get_changes_graph().await
    }

    /// Fetches the evolution log (`jj evolog`) for `change_id`, enriched with review state.
    ///
    /// # Errors
    ///
    /// Returns [`JujutsuError`] if `change_id` is empty or running `jj`/`git` fails.
    pub async fn get_commits_for_change(
        &self,
        change_id: &str,
    ) -> Result<Vec<JjCommit>, JujutsuError> {
        let trimmed = change_id.trim();
        if trimmed.is_empty() {
            return Err(JujutsuError::InvalidInput(String::from(
                "changeId is required",
            )));
        }

        let raw = self.run_evolog_raw(trimmed).await?;
        let mut entries_by_id: HashMap<String, usize> = HashMap::new();
        let mut entries: Vec<RawEvologEntry> = Vec::new();
        let mut meta_cache: HashMap<String, CommitMeta> = HashMap::new();

        for line in raw.lines().filter(|l| !l.trim().is_empty()) {
            let Some(entry) = parse_evolog_line(line) else {
                continue;
            };
            meta_cache.insert(
                entry.commit_id.clone(),
                CommitMeta {
                    commit_id: entry.commit_id.clone(),
                    change_id: String::from(trimmed),
                    parents: entry.parents.clone(),
                    description: entry.description.clone(),
                },
            );
            if let Some(&prev_idx) = entries_by_id.get(&entry.commit_id) {
                // Keep the latest occurrence at the end of causal evolution order.
                entries.remove(prev_idx);
                entries_by_id.clear();
                for (idx, item) in entries.iter().enumerate() {
                    entries_by_id.insert(item.commit_id.clone(), idx);
                }
            }
            entries_by_id.insert(entry.commit_id.clone(), entries.len());
            entries.push(entry);
        }

        let store = self.store_dir();
        let change_key = String::from(trimmed);
        let (threads, approved_commits) = tokio::task::spawn_blocking(move || {
            let threads = crate::get_change_threads(&store, &change_key).unwrap_or_default();
            let approved =
                crate::get_change_approved_commits(&store, &change_key).unwrap_or_default();
            (threads, approved)
        })
        .await
        .map_err(|err| JujutsuError::Io(io::Error::other(err)))?;

        let mut commits: Vec<JjCommit> = entries
            .into_iter()
            .map(|entry| JjCommit {
                short_commit_id: short_id(&entry.commit_id),
                commit_id: entry.commit_id,
                parent_commit_ids: entry.parents,
                timestamp: entry.timestamp,
                description: if entry.first_line.is_empty() {
                    String::from("(no description set)")
                } else {
                    entry.first_line
                },
                is_working_copy: entry.is_working_copy,
                comment_count: 0,
                unresolved_comment_count: 0,
                approved: false,
                clean_rebase_approved: false,
                clean_rebase_source_commit_id: None,
                clean_rebase_source_commit_number: None,
            })
            .collect();

        // Prepend historical revisions referenced in `.tellit/` that are absent from evolog.
        let mut historical: Vec<JjCommit> = Vec::new();
        for thread in &threads {
            let exists = commits
                .iter()
                .chain(historical.iter())
                .any(|c| commit_id_matches(&c.commit_id, &thread.commit_id));
            if !exists {
                historical.push(JjCommit {
                    commit_id: thread.commit_id.clone(),
                    short_commit_id: short_id(&thread.commit_id),
                    parent_commit_ids: Vec::new(),
                    timestamp: String::new(),
                    description: String::from("(historical revision)"),
                    is_working_copy: false,
                    comment_count: 0,
                    unresolved_comment_count: 0,
                    approved: false,
                    clean_rebase_approved: false,
                    clean_rebase_source_commit_id: None,
                    clean_rebase_source_commit_number: None,
                });
            }
        }
        if !historical.is_empty() {
            historical.extend(commits);
            commits = historical;
        }

        for thread in &threads {
            if let Some(commit) = commits
                .iter_mut()
                .find(|c| commit_id_matches(&c.commit_id, &thread.commit_id))
            {
                commit.comment_count += 1;
                if !thread.resolved {
                    commit.unresolved_comment_count += 1;
                }
            }
        }

        for commit in &mut commits {
            commit.approved = approved_commits
                .iter()
                .any(|id| commit_id_matches(id, &commit.commit_id));
        }

        for i in 0..commits.len() {
            if commits[i].approved {
                continue;
            }
            let target_id = commits[i].commit_id.clone();
            let Some(target_meta) = self.cached_commit_meta(&target_id, &mut meta_cache).await
            else {
                continue;
            };
            if target_meta.parents.len() != 1 {
                continue;
            }
            let target_parent = target_meta.parents[0].clone();

            for j in (0..i).rev() {
                if !commits[j].approved && !commits[j].clean_rebase_approved {
                    continue;
                }
                let earlier_id = commits[j].commit_id.clone();
                let Some(earlier_meta) =
                    self.cached_commit_meta(&earlier_id, &mut meta_cache).await
                else {
                    continue;
                };
                if earlier_meta.parents.len() != 1 {
                    continue;
                }
                let earlier_parent = earlier_meta.parents[0].clone();

                if self
                    .is_clean_rebase_between_commits(
                        &earlier_id,
                        &earlier_parent,
                        &earlier_meta.description,
                        &target_id,
                        &target_parent,
                        &target_meta.description,
                    )
                    .await
                {
                    let source_id = commits[j]
                        .clean_rebase_source_commit_id
                        .clone()
                        .unwrap_or(earlier_id);
                    let source_number = commits[j]
                        .clean_rebase_source_commit_number
                        .unwrap_or(j + 1);
                    commits[i].clean_rebase_approved = true;
                    commits[i].clean_rebase_source_commit_id = Some(source_id);
                    commits[i].clean_rebase_source_commit_number = Some(source_number);
                    break;
                }
            }
        }

        Ok(commits)
    }

    /// Computes a structured diff for `target` (optionally compared from `base`), isolating upstream rebases.
    ///
    /// # Errors
    ///
    /// Returns [`JujutsuError`] if `target` is empty or `jj diff` fails.
    pub async fn get_diff(
        &self,
        target: &str,
        base: Option<&str>,
        ignore_rebase: bool,
    ) -> Result<JjDiffResponse, JujutsuError> {
        let trimmed_target = target.trim();
        if trimmed_target.is_empty() {
            return Err(JujutsuError::InvalidInput(String::from(
                "target commit or revision is required",
            )));
        }

        let target_meta = self.fetch_commit_meta(trimmed_target).await;
        let resolved_target = target_meta
            .as_ref()
            .map_or_else(|| String::from(trimmed_target), |m| m.commit_id.clone());

        let normalized_base = normalize_interdiff_base(base, trimmed_target, &resolved_target);
        let base_meta = match normalized_base {
            Some(base_ref) => self.fetch_commit_meta(base_ref).await,
            None => None,
        };

        let mut is_rebase_diff = false;
        let mut rebase_ignored = false;
        let mut rebase_conflict = false;
        let mut raw_diff: Option<String> = None;

        if let (Some(_), Some(b_meta), Some(t_meta)) =
            (normalized_base, base_meta.as_ref(), target_meta.as_ref())
            && b_meta.change_id == t_meta.change_id
            && b_meta.parents.len() == 1
            && t_meta.parents.len() == 1
            && b_meta.parents[0] != t_meta.parents[0]
        {
            is_rebase_diff = true;
            if ignore_rebase {
                if let Some(isolated) = self
                    .compute_rebase_isolated_diff(
                        &b_meta.parents[0],
                        &t_meta.parents[0],
                        &b_meta.commit_id,
                        &t_meta.commit_id,
                    )
                    .await
                {
                    raw_diff = Some(isolated);
                    rebase_ignored = true;
                } else {
                    rebase_conflict = true;
                }
            }
        }

        let raw_diff = match raw_diff {
            Some(diff) => diff,
            None => match normalized_base {
                Some(base_ref) => {
                    let from_commit = base_meta
                        .as_ref()
                        .map_or(base_ref, |m| m.commit_id.as_str());
                    let from_arg = alloc::format!("--from={from_commit}");
                    let to_arg = alloc::format!("--to={resolved_target}");
                    self.run_jj_raw(&["diff", &from_arg, &to_arg, "--git"])
                        .await?
                }
                None => {
                    let rev_arg = alloc::format!("--revision={resolved_target}");
                    self.run_jj_raw(&["diff", &rev_arg, "--git"]).await?
                }
            },
        };

        let mut files = diff_parser::parse_unified_git_diff(&raw_diff);

        let target_desc = match target_meta {
            Some(ref meta) => meta.description.clone(),
            None => self.get_commit_description(&resolved_target).await,
        };
        let is_inter_diff = normalized_base.is_some();
        let base_desc = if is_inter_diff {
            match base_meta {
                Some(ref meta) => meta.description.clone(),
                None => {
                    self.get_commit_description(normalized_base.unwrap_or_default())
                        .await
                }
            }
        } else {
            String::new()
        };

        let commit_msg_file =
            diff_parser::build_commit_message_file(&base_desc, &target_desc, is_inter_diff);
        files.insert(0, commit_msg_file);

        let is_empty = files
            .iter()
            .all(|f| f.path == COMMIT_MESSAGE_PATH && f.additions == 0 && f.deletions == 0);

        let effective_base = match normalized_base {
            Some(base_ref) => base_meta.map_or_else(|| String::from(base_ref), |m| m.commit_id),
            None => target_meta
                .as_ref()
                .and_then(|m| m.parents.first().cloned())
                .unwrap_or_else(|| alloc::format!("{resolved_target}-")),
        };

        Ok(JjDiffResponse {
            commit: resolved_target,
            base_commit: effective_base,
            files,
            is_empty,
            is_rebase_diff,
            rebase_ignored,
            rebase_conflict,
        })
    }

    /// Reads raw file bytes for `raw_path` at Jujutsu revision `raw_rev` (or `working`/`.`).
    ///
    /// # Errors
    ///
    /// Returns [`JujutsuError::InvalidInput`] for invalid paths/revisions or oversized files,
    /// [`JujutsuError::NotFound`] if the file does not exist at `raw_rev`, or command/I/O errors.
    pub async fn get_blob_content(
        &self,
        raw_path: &str,
        raw_rev: Option<&str>,
    ) -> Result<BlobPayload, JujutsuError> {
        let clean_path = validate_repo_relative_path(raw_path)?;
        if clean_path == Path::new(COMMIT_MESSAGE_PATH) {
            return Err(JujutsuError::NotFound(String::from(
                "blob content not available for virtual commit message",
            )));
        }

        let rev = normalize_blob_rev(raw_rev)?;
        let content_type = blob_content_type(&clean_path);

        if rev == "working" || rev == "." {
            let repo_path = self.repo_path.clone();
            let bytes =
                tokio::task::spawn_blocking(move || read_working_file(&repo_path, &clean_path))
                    .await
                    .map_err(|err| JujutsuError::Io(io::Error::other(err)))??;
            return Ok(BlobPayload {
                bytes,
                content_type,
            });
        }

        let rel_posix = clean_path.to_string_lossy().replace('\\', "/");
        let rev_arg = alloc::format!("--revision={rev}");
        let args = [
            "--no-pager",
            "--ignore-working-copy",
            "file",
            "show",
            &rev_arg,
            "--",
            &rel_posix,
        ];
        let output = run_command_with_retry("jj", &self.repo_path, &args, None).await?;
        if !output.status.success() {
            return Err(JujutsuError::NotFound(alloc::format!(
                "file '{rel_posix}' not found at revision '{rev}'"
            )));
        }
        if output.stdout.len() > MAX_BLOB_BYTES {
            return Err(JujutsuError::InvalidInput(String::from(
                "file exceeds 16 MiB display limit",
            )));
        }

        Ok(BlobPayload {
            bytes: output.stdout,
            content_type,
        })
    }

    /// Counts total lines for `raw_path` at `old_ref` (`old_path` if renamed) and/or `new_ref`.
    ///
    /// # Errors
    ///
    /// Returns [`JujutsuError::InvalidInput`] if any path or revision argument fails validation.
    pub async fn get_line_count(
        &self,
        raw_path: &str,
        old_ref: Option<&str>,
        new_ref: Option<&str>,
        old_path: Option<&str>,
    ) -> Result<LineCountOutcome, JujutsuError> {
        let clean_new = validate_repo_relative_path(raw_path)?;
        let new_str = clean_new.to_string_lossy().replace('\\', "/");
        if new_str == COMMIT_MESSAGE_PATH {
            return Ok(LineCountOutcome {
                counts: LineCountResponse {
                    old_line_count: Some(0),
                    new_line_count: Some(0),
                },
                all_lookups_succeeded: false,
            });
        }

        let clean_old = match old_path.map(str::trim).filter(|s| !s.is_empty()) {
            Some(p) => validate_repo_relative_path(p)?
                .to_string_lossy()
                .replace('\\', "/"),
            None => new_str.clone(),
        };

        let old_rev = match old_ref.map(str::trim).filter(|s| !s.is_empty()) {
            Some(r) => Some(normalize_blob_rev(Some(r))?),
            None => None,
        };
        let new_rev = match new_ref.map(str::trim).filter(|s| !s.is_empty()) {
            Some(r) => Some(normalize_blob_rev(Some(r))?),
            None => None,
        };

        let ((old_line_count, old_ok), (new_line_count, new_ok)) = tokio::join!(
            async {
                match old_rev.as_deref() {
                    Some(rev) => match self.get_blob_content(&clean_old, Some(rev)).await {
                        Ok(blob) => (Some(count_blob_lines(&blob.bytes)), true),
                        Err(_) => (Some(0), false),
                    },
                    None => (None, true),
                }
            },
            async {
                match new_rev.as_deref() {
                    Some(rev) => match self.get_blob_content(&new_str, Some(rev)).await {
                        Ok(blob) => (Some(count_blob_lines(&blob.bytes)), true),
                        Err(_) => (Some(0), false),
                    },
                    None => (None, true),
                }
            }
        );

        Ok(LineCountOutcome {
            counts: LineCountResponse {
                old_line_count,
                new_line_count,
            },
            all_lookups_succeeded: old_ok && new_ok,
        })
    }

    /// Loads `.tellit/` comment threads mapped to `old` (base) and `new` (target) sides.
    ///
    /// # Errors
    ///
    /// Returns [`JujutsuError`] if `target` is empty or the blocking task fails.
    pub async fn get_comments_for_diff(
        &self,
        target: &str,
        base: Option<&str>,
    ) -> Result<Vec<JjCommentThread>, JujutsuError> {
        let trimmed_target = target.trim();
        if trimmed_target.is_empty() {
            return Err(JujutsuError::InvalidInput(String::from(
                "target commit or revision is required",
            )));
        }

        let target_meta = self.fetch_commit_meta(trimmed_target).await;
        let target_commit_id = target_meta
            .as_ref()
            .map_or_else(|| String::from(trimmed_target), |m| m.commit_id.clone());
        let target_change_id = target_meta.as_ref().map(|m| m.change_id.clone());

        let normalized_base = normalize_interdiff_base(base, trimmed_target, &target_commit_id);
        let base_meta = match normalized_base {
            Some(base_ref) => self.fetch_commit_meta(base_ref).await,
            None => None,
        };
        let base_commit_id = normalized_base.map(|base_ref| {
            base_meta
                .as_ref()
                .map_or_else(|| String::from(base_ref), |m| m.commit_id.clone())
        });

        let store = self.store_dir();
        let change_id_for_store = target_change_id.clone();
        let target_commit_for_store = target_commit_id.clone();
        let base_commit_for_store = base_commit_id.clone();

        let (change_threads, target_only_threads, base_only_threads) =
            tokio::task::spawn_blocking(move || {
                let change_threads = change_id_for_store
                    .as_deref()
                    .and_then(|cid| crate::get_change_threads(&store, cid).ok())
                    .unwrap_or_default();
                let target_threads = if change_threads.is_empty() {
                    crate::get_commit_thread_infos(&store, &target_commit_for_store)
                        .unwrap_or_default()
                } else {
                    Vec::new()
                };
                let base_threads = match base_commit_for_store.as_deref() {
                    Some(base_cid) => {
                        crate::get_commit_thread_infos(&store, base_cid).unwrap_or_default()
                    }
                    None => Vec::new(),
                };
                (change_threads, target_threads, base_threads)
            })
            .await
            .map_err(|err| JujutsuError::Io(io::Error::other(err)))?;

        Ok(map_threads_for_diff(
            &target_commit_id,
            base_commit_id.as_deref(),
            change_threads,
            target_only_threads,
            base_only_threads,
        ))
    }

    /// Creates a new `.tellit/` comment thread on `commit_id` and returns its UUID.
    ///
    /// # Errors
    ///
    /// Returns [`JujutsuError`] if any field is invalid or writing to `.tellit/` fails.
    pub async fn add_comment(
        &self,
        commit_id: &str,
        file_path: &str,
        line_range: Option<LineRangeSpec>,
        content: &str,
    ) -> Result<String, JujutsuError> {
        let trimmed_commit = commit_id.trim();
        let trimmed_content = content.trim();
        if trimmed_commit.is_empty() || trimmed_content.is_empty() {
            return Err(JujutsuError::InvalidInput(String::from(
                "commitId and non-empty content are required",
            )));
        }

        let clean_path = validate_repo_relative_path(file_path)?;
        let range = match line_range {
            Some(spec) => {
                if spec.start == 0 || spec.end == 0 || spec.start > spec.end {
                    return Err(JujutsuError::InvalidInput(String::from(
                        "invalid lineRange: expected 1-based start <= end",
                    )));
                }
                Some(LineRange {
                    start: spec.start,
                    end: spec.end,
                })
            }
            None => None,
        };

        let store = self.store_dir();
        let repo_root = self.repo_path.clone();
        let commit_owned = String::from(trimmed_commit);
        let content_owned = String::from(content);

        tokio::task::spawn_blocking(move || {
            crate::add_comment_in(
                &store,
                &repo_root,
                &commit_owned,
                &clean_path,
                range,
                content_owned,
            )
            .map_err(|err| JujutsuError::Store(err.to_string()))
        })
        .await
        .map_err(|err| JujutsuError::Io(io::Error::other(err)))?
    }

    /// Appends a reply to an existing `.tellit/` comment thread and returns the canonical UUID.
    ///
    /// # Errors
    ///
    /// Returns [`JujutsuError`] if `content` is empty, the thread is missing, or writing fails.
    pub async fn reply_comment(
        &self,
        thread_id: &str,
        content: &str,
    ) -> Result<String, JujutsuError> {
        let trimmed_id = thread_id.trim();
        if trimmed_id.is_empty() || content.trim().is_empty() {
            return Err(JujutsuError::InvalidInput(String::from(
                "threadId and non-empty content are required",
            )));
        }

        let store = self.store_dir();
        let id_owned = String::from(trimmed_id);
        let content_owned = String::from(content);
        tokio::task::spawn_blocking(move || {
            crate::reply_comment(&store, &id_owned, content_owned).map_err(map_store_lookup_error)
        })
        .await
        .map_err(|err| JujutsuError::Io(io::Error::other(err)))?
    }

    /// Marks an existing `.tellit/` thread resolved or unresolved.
    ///
    /// # Errors
    ///
    /// Returns [`JujutsuError`] if the thread is missing or writing fails.
    pub async fn resolve_comment(
        &self,
        thread_id: &str,
        unresolve: bool,
    ) -> Result<tellit_types::ResolveCommentResponse, JujutsuError> {
        let trimmed_id = thread_id.trim();
        if trimmed_id.is_empty() {
            return Err(JujutsuError::InvalidInput(String::from(
                "threadId is required",
            )));
        }

        let store = self.store_dir();
        let id_owned = String::from(trimmed_id);
        let (canonical_id, resolved) = tokio::task::spawn_blocking(move || {
            crate::resolve_comment(&store, &id_owned, unresolve).map_err(map_store_lookup_error)
        })
        .await
        .map_err(|err| JujutsuError::Io(io::Error::other(err)))??;

        Ok(tellit_types::ResolveCommentResponse {
            thread_id: canonical_id,
            resolved,
        })
    }

    /// Records or revokes approval for `target` in `.tellit/`.
    ///
    /// # Errors
    ///
    /// Returns [`JujutsuError`] if `target` is empty, cannot be resolved, or writing fails.
    pub async fn set_approval(
        &self,
        target: &str,
        disapprove: bool,
    ) -> Result<ApproveCommitResponse, JujutsuError> {
        let trimmed = target.trim();
        if trimmed.is_empty() {
            return Err(JujutsuError::InvalidInput(String::from(
                "commitId is required",
            )));
        }

        let store = self.store_dir();
        let repo_root = self.repo_path.clone();
        let target_owned = String::from(trimmed);
        let outcome = tokio::task::spawn_blocking(move || {
            crate::set_commit_approval_in(&repo_root, &store, Some(&target_owned), disapprove)
                .map_err(|err| JujutsuError::Store(err.to_string()))
        })
        .await
        .map_err(|err| JujutsuError::Io(io::Error::other(err)))??;

        Ok(ApproveCommitResponse {
            change_id: outcome.change_id,
            commit_id: outcome.commit_id,
            approved: outcome.approved,
        })
    }

    async fn get_changes_graph(&self) -> Result<JjChangesResponse, JujutsuError> {
        let template = alloc::format!(
            "\"{JJ_LOG_NODE_MARKER}\\t\" ++ {JJ_CHANGE_FIELDS_EXPR} ++ \"\\n{JJ_LOG_EDGE_MARKER}\\n\""
        );
        let output = self
            .run_jj_raw(&["log", "--color=never", "-T", &template])
            .await?;
        let mut parsed = parse_jj_log_graph(&output);
        let mut meta_cache: HashMap<String, CommitMeta> = HashMap::new();

        // Batch-load `.tellit/` threads and approvals per distinct change ID.
        let store = self.store_dir();
        let unique_change_ids: Vec<String> = {
            let mut seen = HashSet::new();
            parsed
                .changes
                .iter()
                .filter(|c| seen.insert(c.change_id.clone()))
                .map(|c| c.change_id.clone())
                .collect()
        };

        let store_map = tokio::task::spawn_blocking(move || {
            let mut map: HashMap<String, (Vec<ThreadInfo>, Vec<String>)> = HashMap::new();
            for change_id in unique_change_ids {
                let threads = crate::get_change_threads(&store, &change_id).unwrap_or_default();
                let approved =
                    crate::get_change_approved_commits(&store, &change_id).unwrap_or_default();
                map.insert(change_id, (threads, approved));
            }
            map
        })
        .await
        .map_err(|err| JujutsuError::Io(io::Error::other(err)))?;

        for change in &mut parsed.changes {
            if let Some((threads, approved_commits)) = store_map.get(&change.change_id) {
                self.populate_change_review_state(
                    change,
                    threads,
                    approved_commits,
                    &mut meta_cache,
                )
                .await;
            }
        }

        // Sync enriched `JjChange` values back into `graph_rows`.
        let enriched_by_commit: HashMap<&str, &JjChange> = parsed
            .changes
            .iter()
            .map(|c| (c.commit_id.as_str(), c))
            .collect();
        for row in &mut parsed.graph_rows {
            if let JjGraphRow::Commit { change, .. } = row
                && let Some(&updated) = enriched_by_commit.get(change.commit_id.as_str())
            {
                **change = updated.clone();
            }
        }

        Ok(parsed)
    }

    async fn get_single_change(&self, query: &str) -> Result<Option<JjChange>, JujutsuError> {
        let template = alloc::format!("{JJ_CHANGE_FIELDS_EXPR} ++ \"\\n\"");
        let output = if is_plain_id_token(query) {
            let change_rev = alloc::format!("--revision=change_id({query})");
            match self
                .run_jj(&[
                    "log",
                    &change_rev,
                    "--limit=1",
                    "--no-graph",
                    "-T",
                    &template,
                ])
                .await
            {
                Ok(out) if !out.is_empty() => out,
                _ => {
                    let direct_rev = alloc::format!("--revision={query}");
                    match self
                        .run_jj(&[
                            "log",
                            &direct_rev,
                            "--limit=1",
                            "--no-graph",
                            "-T",
                            &template,
                        ])
                        .await
                    {
                        Ok(out) => out,
                        Err(_) => return Ok(None),
                    }
                }
            }
        } else {
            let direct_rev = alloc::format!("--revision={query}");
            match self
                .run_jj(&[
                    "log",
                    &direct_rev,
                    "--limit=1",
                    "--no-graph",
                    "-T",
                    &template,
                ])
                .await
            {
                Ok(out) => out,
                Err(_) => return Ok(None),
            }
        };

        let Some(first_line) = output.lines().find(|l| !l.trim().is_empty()) else {
            return Ok(None);
        };
        let parts: Vec<&str> = first_line.split('\t').collect();
        let Some(parsed) = parse_jj_change_fields(&parts) else {
            return Ok(None);
        };

        let mut change = parsed.change;
        let store = self.store_dir();
        let cid = change.change_id.clone();
        let (threads, approved_commits) = tokio::task::spawn_blocking(move || {
            let threads = crate::get_change_threads(&store, &cid).unwrap_or_default();
            let approved = crate::get_change_approved_commits(&store, &cid).unwrap_or_default();
            (threads, approved)
        })
        .await
        .map_err(|err| JujutsuError::Io(io::Error::other(err)))?;

        let mut meta_cache = HashMap::new();
        self.populate_change_review_state(
            &mut change,
            &threads,
            &approved_commits,
            &mut meta_cache,
        )
        .await;

        Ok(Some(change))
    }

    async fn populate_change_review_state(
        &self,
        change: &mut JjChange,
        threads: &[ThreadInfo],
        approved_commits: &[String],
        meta_cache: &mut HashMap<String, CommitMeta>,
    ) {
        change.comment_count = threads.len();
        change.unresolved_comment_count = threads.iter().filter(|t| !t.resolved).count();

        let store = self.store_dir();
        let commit_id_owned = change.commit_id.clone();
        let direct_store_approved = approved_commits
            .iter()
            .any(|id| commit_id_matches(id, &change.commit_id))
            || tokio::task::spawn_blocking(move || {
                crate::is_commit_approved(&store, &commit_id_owned)
            })
            .await
            .unwrap_or(false);

        if direct_store_approved {
            change.approved = true;
            change.clean_rebase_approved = false;
            return;
        }

        change.approved = false;
        change.clean_rebase_approved = false;

        if approved_commits.is_empty() || change.parent_commit_ids.len() != 1 {
            return;
        }
        let target_parent = change.parent_commit_ids[0].clone();
        let Some(target_meta) = self.cached_commit_meta(&change.commit_id, meta_cache).await else {
            return;
        };

        for earlier_id in approved_commits.iter().rev() {
            let Some(earlier_meta) = self.cached_commit_meta(earlier_id, meta_cache).await else {
                continue;
            };
            if earlier_meta.parents.len() != 1 {
                continue;
            }
            if self
                .is_clean_rebase_between_commits(
                    &earlier_meta.commit_id,
                    &earlier_meta.parents[0],
                    &earlier_meta.description,
                    &change.commit_id,
                    &target_parent,
                    &target_meta.description,
                )
                .await
            {
                change.clean_rebase_approved = true;
                return;
            }
        }
    }

    async fn resolve_initial_target_commit(&self, commitish: &str) -> String {
        if commitish == "@" || commitish == "HEAD" {
            let template = r#"commit_id ++ "\t" ++ change_id ++ "\t" ++ if(empty, "1", "0") ++ "\t" ++ if(root, "1", "0") ++ "\t" ++ description.first_line() ++ "\n""#;
            if let Ok(output) = self
                .run_jj(&[
                    "log",
                    "--revision=ancestors(@, 20)",
                    "--no-graph",
                    "-T",
                    template,
                ])
                .await
            {
                let mut first_non_root: Option<String> = None;
                let store = self.store_dir();
                for line in output.lines().filter(|l| !l.trim().is_empty()) {
                    let parts: Vec<&str> = line.split('\t').collect();
                    if parts.len() < 4 {
                        continue;
                    }
                    let commit_id = parts[0];
                    let change_id = parts[1];
                    let is_empty = parts[2] == "1";
                    let is_root = parts[3] == "1" || change_id == ROOT_CHANGE_ID;
                    let desc = parts[4..].join("\t");
                    if commit_id.is_empty() || change_id.is_empty() || is_root {
                        continue;
                    }
                    if first_non_root.is_none() {
                        first_non_root = Some(String::from(commit_id));
                    }
                    if !(is_empty && desc.trim().is_empty()) {
                        return String::from(commit_id);
                    }
                    let store_clone = store.clone();
                    let cid_owned = String::from(change_id);
                    let has_threads = tokio::task::spawn_blocking(move || {
                        crate::get_change_threads(&store_clone, &cid_owned)
                            .is_ok_and(|t| !t.is_empty())
                    })
                    .await
                    .unwrap_or(false);
                    if has_threads {
                        return String::from(commit_id);
                    }
                }
                if let Some(fallback) = first_non_root {
                    return fallback;
                }
            }
            return String::from(commitish);
        }

        self.resolve_revision_to_commit_id(commitish).await
    }

    async fn resolve_revision_to_commit_id(&self, rev: &str) -> String {
        if is_plain_id_token(rev) {
            let change_rev = alloc::format!("--revision=change_id({rev})");
            if let Ok(out) = self
                .run_jj(&[
                    "log",
                    &change_rev,
                    "--limit=1",
                    "--no-graph",
                    "-T",
                    "commit_id",
                ])
                .await
                && !out.is_empty()
            {
                return out;
            }
        }
        let direct_rev = alloc::format!("--revision={rev}");
        if let Ok(out) = self
            .run_jj(&[
                "log",
                &direct_rev,
                "--limit=1",
                "--no-graph",
                "-T",
                "commit_id",
            ])
            .await
            && !out.is_empty()
        {
            return out;
        }
        String::from(rev)
    }

    async fn run_evolog_raw(&self, change_id: &str) -> Result<String, JujutsuError> {
        let candidates = if is_plain_id_token(change_id) {
            alloc::vec![
                alloc::format!("--revision=change_id({change_id})"),
                alloc::format!("--revision={change_id}"),
            ]
        } else {
            alloc::vec![alloc::format!("--revision={change_id}")]
        };

        for rev_arg in &candidates {
            if let Ok(out) = self
                .run_jj_raw(&[
                    "evolog",
                    rev_arg,
                    "--reversed",
                    "--no-graph",
                    "-T",
                    JJ_EVOLOG_TEMPLATE,
                ])
                .await
            {
                return Ok(out);
            }
        }

        for rev_arg in &candidates {
            if let Ok(out) = self
                .run_jj_raw(&[
                    "log",
                    rev_arg,
                    "--reversed",
                    "--no-graph",
                    "-T",
                    JJ_EVOLOG_FALLBACK_TEMPLATE,
                ])
                .await
            {
                return Ok(out);
            }
        }

        Ok(String::new())
    }

    async fn cached_commit_meta(
        &self,
        rev: &str,
        cache: &mut HashMap<String, CommitMeta>,
    ) -> Option<CommitMeta> {
        if let Some(existing) = cache.get(rev) {
            return Some(existing.clone());
        }
        let fetched = self.fetch_commit_meta(rev).await?;
        cache.insert(String::from(rev), fetched.clone());
        cache.insert(fetched.commit_id.clone(), fetched.clone());
        Some(fetched)
    }

    async fn fetch_commit_meta(&self, rev: &str) -> Option<CommitMeta> {
        let rev_arg = alloc::format!("--revision={rev}");
        let output = self
            .run_jj(&[
                "log",
                &rev_arg,
                "--limit=1",
                "--no-graph",
                "-T",
                JJ_COMMIT_META_TEMPLATE,
            ])
            .await
            .ok()?;
        let line = output.lines().find(|l| !l.trim().is_empty())?;
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() < 4 {
            return None;
        }
        let commit_id = parts[0].trim();
        let change_id = parts[1].trim();
        if commit_id.is_empty() || change_id.is_empty() {
            return None;
        }
        let parents = parts[2]
            .split(',')
            .filter(|s| !s.is_empty())
            .map(String::from)
            .collect();
        let description = serde_json::from_str::<String>(parts[3]).unwrap_or_default();
        Some(CommitMeta {
            commit_id: String::from(commit_id),
            change_id: String::from(change_id),
            parents,
            description,
        })
    }

    async fn get_commit_description(&self, rev: &str) -> String {
        let rev_arg = alloc::format!("--revision={rev}");
        self.run_jj(&[
            "log",
            &rev_arg,
            "--limit=1",
            "--no-graph",
            "-T",
            "description",
        ])
        .await
        .unwrap_or_default()
    }

    async fn compute_rebase_isolated_diff(
        &self,
        earlier_parent: &str,
        target_parent: &str,
        earlier_commit: &str,
        target_commit: &str,
    ) -> Option<String> {
        if !is_hex_oid(earlier_parent)
            || !is_hex_oid(target_parent)
            || !is_hex_oid(earlier_commit)
            || !is_hex_oid(target_commit)
        {
            return None;
        }
        let merge_output = self
            .run_git(&[
                "merge-tree",
                "--write-tree",
                "--merge-base",
                earlier_parent,
                target_parent,
                earlier_commit,
            ])
            .await?;
        if !merge_output.status.success() {
            return None;
        }
        let merge_stdout = String::from_utf8_lossy(&merge_output.stdout);
        let virtual_tree = merge_stdout.lines().next()?.trim();
        if !is_hex_oid(virtual_tree) {
            return None;
        }

        let diff_output = self
            .run_git(&[
                "diff",
                "--no-color",
                "--no-ext-diff",
                "--src-prefix=a/",
                "--dst-prefix=b/",
                virtual_tree,
                target_commit,
            ])
            .await?;
        if !diff_output.status.success() {
            return None;
        }
        Some(String::from_utf8_lossy(&diff_output.stdout).into_owned())
    }

    async fn is_clean_rebase_between_commits(
        &self,
        earlier_commit: &str,
        earlier_parent: &str,
        earlier_desc: &str,
        target_commit: &str,
        target_parent: &str,
        target_desc: &str,
    ) -> bool {
        if normalize_desc_for_compare(earlier_desc) != normalize_desc_for_compare(target_desc) {
            return false;
        }
        if !is_hex_oid(earlier_commit) || !is_hex_oid(target_commit) {
            return false;
        }

        if earlier_parent != target_parent {
            return self
                .compute_rebase_isolated_diff(
                    earlier_parent,
                    target_parent,
                    earlier_commit,
                    target_commit,
                )
                .await
                .is_some_and(|diff| diff.trim().is_empty());
        }

        let Some(diff_output) = self
            .run_git(&[
                "diff",
                "--no-color",
                "--no-ext-diff",
                "--src-prefix=a/",
                "--dst-prefix=b/",
                earlier_commit,
                target_commit,
            ])
            .await
        else {
            return false;
        };
        diff_output.status.success()
            && String::from_utf8_lossy(&diff_output.stdout)
                .trim()
                .is_empty()
    }

    async fn run_jj(&self, args: &[&str]) -> Result<String, JujutsuError> {
        let out = self.run_jj_raw(args).await?;
        Ok(out.trim().to_string())
    }

    async fn run_jj_raw(&self, args: &[&str]) -> Result<String, JujutsuError> {
        let mut effective = Vec::with_capacity(args.len() + 2);
        effective.push("--no-pager");
        effective.push("--ignore-working-copy");
        effective.extend_from_slice(args);

        let output = run_command_with_retry("jj", &self.repo_path, &effective, None).await?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            return Err(JujutsuError::CommandFailed {
                command: "jj",
                stderr,
            });
        }
        Ok(String::from_utf8_lossy(&output.stdout)
            .trim_end_matches(['\r', '\n'])
            .to_string())
    }

    async fn run_git(&self, args: &[&str]) -> Option<Output> {
        let git_dir = resolve_git_dir(&self.repo_path);
        let env_pair = git_dir.as_deref().map(|dir| ("GIT_DIR", dir));
        run_command_with_retry("git", &self.repo_path, args, env_pair)
            .await
            .ok()
    }
}

/// Resolves `.jj/repo` and main repository root for both primary and secondary workspaces.
#[must_use]
pub fn resolve_jj_repo_paths(repo_dir: &Path) -> Option<JjRepoPaths> {
    let start_dir = repo_dir
        .canonicalize()
        .unwrap_or_else(|_| repo_dir.to_path_buf());
    let mut cur = start_dir.as_path();
    loop {
        let jj_dir = cur.join(".jj");
        let jj_repo = jj_dir.join("repo");
        if jj_repo.is_file() {
            let target = std::fs::read_to_string(&jj_repo).ok()?;
            let trimmed = target.trim();
            if trimmed.is_empty() {
                return None;
            }
            let target_path = Path::new(trimmed);
            let resolved = if target_path.is_relative() {
                jj_dir.join(target_path)
            } else {
                target_path.to_path_buf()
            };
            let canonical_repo = resolved.canonicalize().unwrap_or(resolved);
            let main_repo_root = canonical_repo.parent()?.parent()?.to_path_buf();
            return Some(JjRepoPaths {
                main_repo_root,
                jj_repo_dir: canonical_repo,
            });
        }
        if jj_repo.is_dir() {
            return Some(JjRepoPaths {
                main_repo_root: cur.to_path_buf(),
                jj_repo_dir: jj_repo,
            });
        }
        cur = cur.parent()?;
    }
}

/// Resolves the backing Git directory when running in a non-colocated or secondary workspace.
#[must_use]
pub fn resolve_git_dir(repo_dir: &Path) -> Option<PathBuf> {
    if repo_dir.join(".git").exists() {
        return None;
    }
    if let Some(paths) = resolve_jj_repo_paths(repo_dir) {
        let store_dir = paths.jj_repo_dir.join("store");
        let direct_git = store_dir.join("git");
        if direct_git.exists() {
            return Some(direct_git);
        }
        let git_target = store_dir.join("git_target");
        if let Ok(raw) = std::fs::read_to_string(&git_target) {
            let trimmed = raw.trim();
            if !trimmed.is_empty() {
                let target_path = Path::new(trimmed);
                let resolved = if target_path.is_relative() {
                    store_dir.join(target_path)
                } else {
                    target_path.to_path_buf()
                };
                if resolved.exists() {
                    return Some(resolved.canonicalize().unwrap_or(resolved));
                }
            }
        }
    }
    None
}

async fn run_command_with_retry(
    program: &'static str,
    dir: &Path,
    args: &[&str],
    env_override: Option<(&str, &Path)>,
) -> Result<Output, JujutsuError> {
    let mut attempt = 0u32;
    loop {
        let mut cmd = Command::new(program);
        cmd.args(args).current_dir(dir).kill_on_drop(true);
        if let Some((key, val)) = env_override {
            cmd.env(key, val);
        }
        match cmd.output().await {
            Ok(output) => return Ok(output),
            Err(err)
                if err.kind() == io::ErrorKind::ExecutableFileBusy
                    && attempt < MAX_ETXTBSY_RETRIES =>
            {
                tokio::time::sleep(Duration::from_millis(1u64 << attempt.min(5))).await;
                attempt += 1;
            }
            Err(err) => {
                return Err(JujutsuError::CommandSpawn {
                    command: program,
                    source: err,
                });
            }
        }
    }
}

/// Parses `jj log` graph output emitted with `JJ_LOG_NODE_MARKER` and `JJ_LOG_EDGE_MARKER`.
#[must_use]
pub fn parse_jj_log_graph(raw_output: &str) -> JjChangesResponse {
    let lines: Vec<&str> = raw_output.lines().collect();
    let mut changes: Vec<JjChange> = Vec::new();
    let mut graph_rows: Vec<JjGraphRow> = Vec::new();
    let mut i = 0usize;

    while i < lines.len() {
        let line = lines[i];
        if line.trim().is_empty() {
            i += 1;
            continue;
        }

        if let Some(node_idx) = line.find(JJ_LOG_NODE_MARKER) {
            let raw_prefix = line[..node_idx].trim_end();
            let graph_cols: Vec<char> = raw_prefix.chars().collect();
            let mut parts_iter = line[node_idx..].split('\t');
            let _marker = parts_iter.next();
            let parts: Vec<&str> = parts_iter.collect();
            let parsed = parse_jj_change_fields(&parts);

            let next_line = lines.get(i + 1).copied().unwrap_or("");
            let edge_idx = next_line.find(JJ_LOG_EDGE_MARKER);

            let Some(parsed) = parsed else {
                if edge_idx.is_some() {
                    i += 1;
                }
                i += 1;
                continue;
            };
            if parsed.is_root {
                if edge_idx.is_some() {
                    i += 1;
                }
                i += 1;
                continue;
            }

            let change = parsed.change;
            let mut down_cols: Vec<usize> = Vec::new();
            let mut edge_connector_cols: Option<Vec<char>> = None;

            if let Some(e_idx) = edge_idx {
                i += 1;
                let edge_prefix = next_line[..e_idx].trim_end();
                let edge_chars: Vec<char> = edge_prefix.chars().collect();
                if edge_chars
                    .iter()
                    .all(|c| matches!(c, '│' | '|' | ':' | ' '))
                {
                    for (col, &ch) in edge_chars.iter().enumerate() {
                        if matches!(ch, '│' | '|' | ':') {
                            down_cols.push(col);
                        }
                    }
                } else {
                    for (col, &ch) in edge_chars.iter().enumerate() {
                        if UPWARD_CONNECTING_GLYPHS.contains(ch) {
                            down_cols.push(col);
                        }
                    }
                    edge_connector_cols = Some(edge_chars);
                }
            }

            changes.push(change.clone());
            graph_rows.push(JjGraphRow::Commit {
                key: change.commit_id.clone(),
                graph_cols,
                down_cols,
                change: alloc::boxed::Box::new(change),
            });

            if let Some(conn_cols) = edge_connector_cols {
                let row_idx = graph_rows.len();
                graph_rows.push(JjGraphRow::Connector {
                    key: alloc::format!("connector-{row_idx}"),
                    graph_cols: conn_cols,
                    is_elided: false,
                    bridges_elided: false,
                });
            }

            i += 1;
            continue;
        }

        if line.contains(JJ_LOG_EDGE_MARKER) {
            i += 1;
            continue;
        }

        let bridges_elided = line.contains("(elided revisions)");
        let without_elided = if let Some(idx) = line.find("(elided revisions)") {
            &line[..idx]
        } else {
            line
        };
        let trimmed_right = without_elided.trim_end();
        let is_pure_spacer = !trimmed_right.is_empty()
            && trimmed_right
                .chars()
                .all(|c| matches!(c, '│' | '|' | ':' | ' '));

        if trimmed_right.is_empty() || (!bridges_elided && is_pure_spacer) {
            i += 1;
            continue;
        }

        let row_idx = graph_rows.len();
        let graph_cols: Vec<char> = trimmed_right.chars().collect();
        let is_elided = graph_cols.contains(&'~');
        graph_rows.push(JjGraphRow::Connector {
            key: alloc::format!("connector-{row_idx}"),
            graph_cols,
            is_elided,
            bridges_elided,
        });
        i += 1;
    }

    // Prune downward stems when the following row has no glyph in that column.
    let len = graph_rows.len();
    for r in 0..len {
        let next_cols: Option<Vec<char>> = if r + 1 < len {
            Some(match &graph_rows[r + 1] {
                JjGraphRow::Commit { graph_cols, .. }
                | JjGraphRow::Connector { graph_cols, .. } => graph_cols.clone(),
            })
        } else {
            None
        };
        if let JjGraphRow::Commit { down_cols, .. } = &mut graph_rows[r] {
            match next_cols {
                Some(cols) => {
                    down_cols.retain(|&col| cols.get(col).copied().unwrap_or(' ') != ' ');
                }
                None => down_cols.clear(),
            }
        }
    }

    JjChangesResponse {
        changes,
        graph_rows,
    }
}

fn parse_jj_change_fields(parts: &[&str]) -> Option<ParsedChangeRow> {
    if parts.len() < 12 {
        return None;
    }
    let change_id = parts[0].trim();
    let shortest_change_id = parts[1].trim();
    let commit_id = parts[2].trim();
    if change_id.is_empty() || commit_id.is_empty() {
        return None;
    }

    let is_working_copy = parts[3] == "1";
    let is_empty = parts[4] == "1";
    let is_immutable = parts[5] == "1";
    let is_conflict = parts[6] == "1";
    let is_root = parts[7] == "1" || change_id == ROOT_CHANGE_ID;

    let bookmarks: Vec<String> = parts[8]
        .split(',')
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect();
    let parent_commit_ids: Vec<String> = parts[9]
        .split(',')
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect();
    let author = String::from(parts[10].trim());
    let timestamp = String::from(parts[11].trim());
    let raw_desc = parts[12..].join("\t");
    let trimmed_desc = raw_desc.trim();
    let has_description = !trimmed_desc.is_empty();

    let change = JjChange {
        change_id: String::from(change_id),
        short_change_id: short_id(change_id),
        shortest_change_id: if shortest_change_id.is_empty() {
            change_id.chars().take(1).collect()
        } else {
            String::from(shortest_change_id)
        },
        commit_id: String::from(commit_id),
        short_commit_id: short_id(commit_id),
        is_working_copy,
        is_empty,
        has_description,
        is_immutable,
        is_conflict,
        bookmarks,
        parent_commit_ids,
        author,
        timestamp,
        description: if has_description {
            String::from(trimmed_desc)
        } else {
            String::from("(no description set)")
        },
        comment_count: 0,
        unresolved_comment_count: 0,
        approved: false,
        clean_rebase_approved: false,
    };

    Some(ParsedChangeRow { change, is_root })
}

fn parse_evolog_line(line: &str) -> Option<RawEvologEntry> {
    let parts: Vec<&str> = line.split('\t').collect();
    if parts.len() < 5 {
        return None;
    }
    let commit_id = parts[0].trim();
    if commit_id.is_empty() {
        return None;
    }
    let timestamp = String::from(parts[1].trim());
    let is_working_copy = parts[2] == "1";
    let parents: Vec<String> = parts[3]
        .split(',')
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect();
    let description = serde_json::from_str::<String>(parts[4]).unwrap_or_default();
    let first_line = parts[5..].join("\t").trim().to_string();

    Some(RawEvologEntry {
        commit_id: String::from(commit_id),
        timestamp,
        is_working_copy,
        parents,
        description,
        first_line,
    })
}

fn insert_thread(
    ordered_ids: &mut Vec<String>,
    by_id: &mut HashMap<String, JjCommentThread>,
    thread: ThreadInfo,
    side: CommentSide,
    overwrite: bool,
) {
    let exists = by_id.contains_key(&thread.thread_id);
    if exists && !overwrite {
        return;
    }
    if !exists {
        ordered_ids.push(thread.thread_id.clone());
    }
    by_id.insert(thread.thread_id.clone(), thread_info_to_wire(thread, side));
}

fn map_threads_for_diff(
    target_commit_id: &str,
    base_commit_id: Option<&str>,
    change_threads: Vec<ThreadInfo>,
    target_only_threads: Vec<ThreadInfo>,
    base_only_threads: Vec<ThreadInfo>,
) -> Vec<JjCommentThread> {
    let mut ordered_ids: Vec<String> = Vec::new();
    let mut by_id: HashMap<String, JjCommentThread> = HashMap::new();

    if let Some(base_cid) = base_commit_id {
        if change_threads.is_empty() {
            for t in base_only_threads {
                insert_thread(&mut ordered_ids, &mut by_id, t, CommentSide::Old, true);
            }
        } else {
            for t in &change_threads {
                if commit_id_matches(&t.commit_id, base_cid) {
                    insert_thread(
                        &mut ordered_ids,
                        &mut by_id,
                        t.clone(),
                        CommentSide::Old,
                        true,
                    );
                }
            }
            for t in base_only_threads {
                insert_thread(&mut ordered_ids, &mut by_id, t, CommentSide::Old, false);
            }
        }
    }

    if change_threads.is_empty() {
        for t in target_only_threads {
            insert_thread(&mut ordered_ids, &mut by_id, t, CommentSide::New, true);
        }
    } else {
        for t in &change_threads {
            if commit_id_matches(&t.commit_id, target_commit_id) {
                insert_thread(
                    &mut ordered_ids,
                    &mut by_id,
                    t.clone(),
                    CommentSide::New,
                    true,
                );
            }
        }
        for t in change_threads {
            insert_thread(&mut ordered_ids, &mut by_id, t, CommentSide::New, false);
        }
    }

    ordered_ids
        .into_iter()
        .filter_map(|id| by_id.remove(&id))
        .collect()
}

fn thread_info_to_wire(thread: ThreadInfo, side: CommentSide) -> JjCommentThread {
    JjCommentThread {
        id: thread.thread_id,
        change_id: thread.change_id,
        commit_id: thread.commit_id,
        file_path: thread.filepath,
        side,
        line_range: thread.line_range.map(|lr| LineRangeSpec {
            start: lr.start,
            end: lr.end,
        }),
        comments: thread.comments,
        resolved: thread.resolved,
    }
}

/// Validates and normalizes a repository-relative path, rejecting traversal, absolute paths, and `.git`/`.jj` metadata.
///
/// # Errors
///
/// Returns [`JujutsuError::InvalidInput`] if `raw` is empty, contains NUL bytes, is absolute,
/// traverses upward with `..`, or targets internal `.git` or `.jj` directories.
pub fn validate_repo_relative_path(raw: &str) -> Result<PathBuf, JujutsuError> {
    if raw.contains('\0') {
        return Err(JujutsuError::InvalidInput(String::from(
            "filePath cannot contain NUL bytes",
        )));
    }
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(JujutsuError::InvalidInput(String::from(
            "filePath cannot be empty",
        )));
    }
    if trimmed == COMMIT_MESSAGE_PATH {
        return Ok(PathBuf::from(COMMIT_MESSAGE_PATH));
    }
    let normalized = trimmed.replace('\\', "/");
    let path = Path::new(&normalized);
    if path.is_absolute() || normalized.starts_with('/') {
        return Err(JujutsuError::InvalidInput(String::from(
            "filePath must be repository-relative",
        )));
    }
    let mut clean = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(seg) => {
                if seg == ".git" || seg == ".jj" {
                    return Err(JujutsuError::InvalidInput(String::from(
                        "filePath cannot target repository metadata directories",
                    )));
                }
                clean.push(seg);
            }
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(JujutsuError::InvalidInput(String::from(
                    "filePath cannot escape the repository root",
                )));
            }
        }
    }
    if clean.as_os_str().is_empty() {
        return Err(JujutsuError::InvalidInput(String::from(
            "filePath cannot be empty",
        )));
    }
    Ok(clean)
}

fn normalize_blob_rev(raw_rev: Option<&str>) -> Result<String, JujutsuError> {
    let Some(trimmed) = raw_rev.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(String::from("@"));
    };
    if trimmed.starts_with('-') {
        return Err(JujutsuError::InvalidInput(String::from(
            "revision parameter cannot start with '-'",
        )));
    }
    if trimmed.chars().any(|c| c.is_control() || c == '\0') {
        return Err(JujutsuError::InvalidInput(String::from(
            "revision parameter contains invalid control characters",
        )));
    }
    let trailing_carets = trimmed.chars().rev().take_while(|&c| c == '^').count();
    if trailing_carets > 0 {
        let base = &trimmed[..trimmed.len() - trailing_carets];
        let mut out = String::with_capacity(trimmed.len());
        out.push_str(base);
        for _ in 0..trailing_carets {
            out.push('-');
        }
        return Ok(out);
    }
    Ok(String::from(trimmed))
}

fn count_blob_lines(bytes: &[u8]) -> usize {
    if bytes.is_empty() {
        return 0;
    }
    let newlines = bytes.iter().filter(|&&b| b == b'\n').count();
    if bytes.ends_with(b"\n") {
        newlines
    } else {
        newlines + 1
    }
}

fn read_working_file(repo_path: &Path, clean_path: &Path) -> Result<Vec<u8>, JujutsuError> {
    let repo_root = repo_path.canonicalize().map_err(JujutsuError::Io)?;
    let target_path = repo_root.join(clean_path);
    let canonical_file = target_path.canonicalize().map_err(|err| {
        if err.kind() == io::ErrorKind::NotFound {
            JujutsuError::NotFound(alloc::format!(
                "file '{}' not found in working copy",
                clean_path.display()
            ))
        } else {
            JujutsuError::Io(err)
        }
    })?;
    if !canonical_file.starts_with(&repo_root) {
        return Err(JujutsuError::InvalidInput(String::from(
            "filePath cannot escape the repository root",
        )));
    }
    let meta = std::fs::metadata(&canonical_file)?;
    if !meta.is_file() {
        return Err(JujutsuError::NotFound(alloc::format!(
            "'{}' is not a regular file",
            clean_path.display()
        )));
    }
    if meta.len() > MAX_BLOB_BYTES as u64 {
        return Err(JujutsuError::InvalidInput(String::from(
            "file exceeds 16 MiB display limit",
        )));
    }
    Ok(std::fs::read(&canonical_file)?)
}

fn blob_content_type(path: &Path) -> &'static str {
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .map(str::to_ascii_lowercase);
    match ext.as_deref() {
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("png") => "image/png",
        Some("gif") => "image/gif",
        Some("bmp") => "image/bmp",
        Some("svg") => "image/svg+xml",
        Some("webp") => "image/webp",
        Some("ico") => "image/x-icon",
        Some("tiff" | "tif") => "image/tiff",
        Some("avif") => "image/avif",
        Some("heic") => "image/heic",
        Some("heif") => "image/heif",
        _ => "application/octet-stream",
    }
}

fn map_store_lookup_error(err: anyhow::Error) -> JujutsuError {
    let msg = err.to_string();
    if msg.contains("No comment thread found") || msg.contains("Store directory does not exist") {
        JujutsuError::NotFound(msg)
    } else {
        JujutsuError::Store(msg)
    }
}

fn is_direct_parent_expr(trimmed: &str, target: &str) -> bool {
    !target.is_empty()
        && trimmed.len() == target.len() + 1
        && trimmed.starts_with(target)
        && matches!(trimmed.as_bytes().last(), Some(b'^' | b'-'))
}

fn normalize_interdiff_base<'a>(
    base: Option<&'a str>,
    raw_target: &str,
    resolved_target: &str,
) -> Option<&'a str> {
    let trimmed = base.map(str::trim).filter(|s| !s.is_empty())?;
    if matches!(trimmed, "parent" | "@-" | "@^")
        || is_direct_parent_expr(trimmed, raw_target)
        || is_direct_parent_expr(trimmed, resolved_target)
    {
        return None;
    }
    Some(trimmed)
}

fn normalize_desc_for_compare(desc: &str) -> String {
    desc.replace("\r\n", "\n").trim_end().to_owned()
}

fn is_plain_id_token(raw: &str) -> bool {
    !raw.is_empty() && raw.bytes().all(|b| b.is_ascii_alphanumeric())
}

fn is_hex_oid(raw: &str) -> bool {
    (raw.len() == 40 || raw.len() == 64) && raw.bytes().all(|b| b.is_ascii_hexdigit())
}

fn short_id(id: &str) -> String {
    id.chars().take(8).collect()
}

fn commit_id_matches(a: &str, b: &str) -> bool {
    a == b || (a.len() >= 6 && b.len() >= 6 && (a.starts_with(b) || b.starts_with(a)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_jj_log_graph_with_elision_branches_and_root_removal() {
        let raw = format!(
            "\
@  {node}\tkkmpptxz\tk\t11111111aaaa\t1\t0\t0\t0\t0\tmain\t22222222bbbb\tAlice\t2026-11-17 10:00:00\tfeat: tip
│  {edge}
├─╮
│ ○  {node}\tzzzzzzzz\tz\t33333333cccc\t0\t1\t0\t0\t0\t\t22222222bbbb\tBob\t2026-11-17 09:00:00\t
│ │  {edge}
├─╯
~  (elided revisions)
◆  {node}\t00000000000000000000000000000000\t0\t000000000000\t0\t1\t1\t0\t1\t\t\t\t1970-01-01 00:00:00\t
   {edge}
",
            node = JJ_LOG_NODE_MARKER,
            edge = JJ_LOG_EDGE_MARKER,
        );

        let parsed = parse_jj_log_graph(&raw);
        assert_eq!(parsed.changes.len(), 2);
        assert_eq!(parsed.changes[0].change_id, "kkmpptxz");
        assert_eq!(parsed.changes[0].shortest_change_id, "k");
        assert!(parsed.changes[0].is_working_copy);
        assert_eq!(parsed.changes[0].bookmarks, vec!["main"]);
        assert_eq!(parsed.changes[0].author, "Alice");

        assert_eq!(parsed.changes[1].change_id, "zzzzzzzz");
        assert!(!parsed.changes[1].has_description);
        assert_eq!(parsed.changes[1].description, "(no description set)");

        // Root commit must be stripped and elision row preserved.
        assert!(parsed.graph_rows.iter().any(|r| matches!(
            r,
            JjGraphRow::Connector {
                is_elided: true,
                bridges_elided: true,
                ..
            }
        )));
    }

    #[test]
    fn maps_threads_onto_old_and_new_sides_for_interdiff() {
        let base_thread = ThreadInfo {
            thread_id: String::from("t-base"),
            change_id: String::from("chg1"),
            commit_id: String::from("base1111"),
            git_commit_hash: String::from("base1111"),
            filepath: String::from("src/lib.rs"),
            line_range: Some(LineRange { start: 4, end: 6 }),
            comments: alloc::vec![String::from("Old note")],
            resolved: false,
        };
        let target_thread = ThreadInfo {
            thread_id: String::from("t-target"),
            change_id: String::from("chg1"),
            commit_id: String::from("targ2222"),
            git_commit_hash: String::from("targ2222"),
            filepath: String::from("src/lib.rs"),
            line_range: Some(LineRange { start: 10, end: 10 }),
            comments: alloc::vec![String::from("New note")],
            resolved: true,
        };

        let mapped = map_threads_for_diff(
            "targ2222",
            Some("base1111"),
            alloc::vec![base_thread, target_thread],
            Vec::new(),
            Vec::new(),
        );
        assert_eq!(mapped.len(), 2);
        assert_eq!(mapped[0].id, "t-base");
        assert_eq!(mapped[0].side, CommentSide::Old);
        assert_eq!(mapped[1].id, "t-target");
        assert_eq!(mapped[1].side, CommentSide::New);
    }

    #[test]
    fn validates_repo_relative_comment_paths() {
        assert_eq!(
            validate_repo_relative_path("src/main.rs").unwrap(),
            PathBuf::from("src/main.rs")
        );
        assert_eq!(
            validate_repo_relative_path(COMMIT_MESSAGE_PATH).unwrap(),
            PathBuf::from(COMMIT_MESSAGE_PATH)
        );
        assert!(validate_repo_relative_path("../secret").is_err());
        assert!(validate_repo_relative_path("/etc/passwd").is_err());
        assert!(validate_repo_relative_path("   ").is_err());
    }

    #[test]
    fn normalizes_only_explicit_parent_interdiff_bases() {
        assert_eq!(
            normalize_interdiff_base(Some("parent"), "abc", "abcdef"),
            None
        );
        assert_eq!(normalize_interdiff_base(Some("@-"), "abc", "abcdef"), None);
        assert_eq!(
            normalize_interdiff_base(Some("abc^"), "abc", "abcdef"),
            None
        );
        assert_eq!(
            normalize_interdiff_base(Some("abcdef-"), "abc", "abcdef"),
            None
        );
        assert_eq!(
            normalize_interdiff_base(Some("@--"), "@", "abcdef"),
            Some("@--")
        );
        assert_eq!(
            normalize_interdiff_base(Some("feature-"), "abc", "abcdef"),
            Some("feature-")
        );
    }
}
