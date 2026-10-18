use std::path::{Path, PathBuf};
use std::str::FromStr;

use anyhow::{Context, Result};
use clap::{Args, Parser, Subcommand};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use walkdir::WalkDir;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct LineRange {
    pub start: u32,
    pub end: u32,
}

impl FromStr for LineRange {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self> {
        let s = s.trim();
        if let Ok(single) = s.parse::<u32>() {
            return Ok(LineRange {
                start: single,
                end: single,
            });
        }
        let delims = ["..=", "..", "-", ":", ","];
        for delim in delims {
            if let Some((start_str, end_str)) = s.split_once(delim) {
                let start: u32 = start_str
                    .trim()
                    .parse()
                    .map_err(|_| anyhow::anyhow!("Invalid start line in range: '{}'", start_str))?;
                let end: u32 = end_str
                    .trim()
                    .parse()
                    .map_err(|_| anyhow::anyhow!("Invalid end line in range: '{}'", end_str))?;
                if start > end {
                    anyhow::bail!(
                        "Start line ({}) cannot be greater than end line ({})",
                        start,
                        end
                    );
                }
                return Ok(LineRange { start, end });
            }
        }
        anyhow::bail!(
            "Invalid line range format: '{}'. Expected e.g. '10', '10..20', '10-20', '10:20'",
            s
        );
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThreadData {
    pub filepath: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line_range: Option<LineRange>,
    pub comments: Vec<String>,
    pub resolved: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThreadInfo {
    pub thread_id: String,
    pub change_id: String,
    pub commit_id: String,
    pub git_commit_hash: String,
    pub filepath: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line_range: Option<LineRange>,
    pub comments: Vec<String>,
    pub resolved: bool,
}

#[derive(Parser, Debug)]
#[command(name = "commenter", about = "Code review commenting tool for Jujutsu (jj)")]
pub struct Cli {
    #[arg(
        long,
        short,
        global = true,
        help = "Path to comments store directory (defaults to <main-repo>/.comments)"
    )]
    pub store: Option<PathBuf>,

    #[arg(
        long,
        short = 'C',
        help = "Jujutsu change/revision to target (defaults to @)"
    )]
    pub change: Option<String>,

    #[command(subcommand)]
    pub action: Commenter,
}

#[derive(Subcommand, Debug)]
pub enum Commenter {
    #[command(about = "Add a new comment thread")]
    Add(Add),
    #[command(about = "Resolve or unresolve a comment thread")]
    Resolve(Resolve),
    #[command(about = "Reply to an existing comment thread")]
    Reply(Reply),
    #[command(about = "Retrieve comments, commit hashes, or thread IDs")]
    Get(Get),
}

#[derive(Debug, Args)]
pub struct Add {
    #[arg(long, short, help = "Filepath from repo root")]
    pub file: PathBuf,

    #[arg(long, short, help = "Line range (e.g. 10, 10..20, 10-20, 10:20)")]
    pub at: Option<LineRange>,

    #[arg(long, short, help = "Comment content")]
    pub content: String,

    #[arg(long, short = 'r', help = "Target revision (overrides global --change)")]
    pub rev: Option<String>,
}

#[derive(Debug, Args)]
pub struct Resolve {
    #[arg(long, short, help = "Thread ID to resolve/unresolve")]
    pub thread: Option<String>,

    #[arg(value_name = "THREAD_ID")]
    pub thread_pos: Option<String>,

    #[arg(long, help = "Unresolve the comment thread instead of resolving")]
    pub unresolve: bool,
}

impl Resolve {
    pub fn get_thread_id(&self) -> Result<&str> {
        self.thread
            .as_deref()
            .or(self.thread_pos.as_deref())
            .ok_or_else(|| anyhow::anyhow!("Thread ID is required (pass via -t/--thread or positional argument)"))
    }
}

#[derive(Debug, Args)]
pub struct Reply {
    #[arg(long, short, help = "Thread ID to reply to")]
    pub thread: Option<String>,

    #[arg(value_name = "THREAD_ID")]
    pub thread_pos: Option<String>,

    #[arg(long, short, help = "Content to reply")]
    pub content: Option<String>,

    #[arg(value_name = "CONTENT")]
    pub content_pos: Option<String>,
}

impl Reply {
    pub fn get_thread_id(&self) -> Result<&str> {
        self.thread
            .as_deref()
            .or(self.thread_pos.as_deref())
            .ok_or_else(|| anyhow::anyhow!("Thread ID is required (pass via -t/--thread or positional argument)"))
    }

    pub fn get_content(&self) -> Result<&str> {
        self.content
            .as_deref()
            .or(self.content_pos.as_deref())
            .ok_or_else(|| anyhow::anyhow!("Content is required (pass via -c/--content or positional argument)"))
    }
}

#[derive(Debug, Args)]
pub struct Get {
    #[command(subcommand)]
    pub target: GetTarget,
}

#[derive(Subcommand, Debug)]
pub enum GetTarget {
    #[command(about = "Get information about a thread ID as JSON")]
    Thread(GetThreadArgs),

    #[command(about = "Get git commit hashes for a Jujutsu change ID")]
    Change(GetChangeArgs),

    #[command(about = "Get thread IDs for a git commit hash")]
    Commit(GetCommitArgs),
}

#[derive(Debug, Args)]
pub struct GetThreadArgs {
    #[arg(value_name = "THREAD_ID", help = "Thread ID (UUID)")]
    pub thread_id: String,
}

#[derive(Debug, Args)]
pub struct GetChangeArgs {
    #[arg(value_name = "CHANGE_ID", help = "Jujutsu change ID")]
    pub change_id: String,

    #[arg(long, help = "Output as JSON array")]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct GetCommitArgs {
    #[arg(value_name = "COMMIT_HASH", help = "Git commit hash")]
    pub commit_hash: String,

    #[arg(long, help = "Output as JSON array")]
    pub json: bool,
}

pub fn run_jj(args: &[&str]) -> Result<String> {
    let output = std::process::Command::new("jj")
        .args(args)
        .output()
        .with_context(|| "Failed to execute 'jj'. Is jj installed and available in PATH?")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("jj error: {}", stderr.trim());
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

pub fn get_workspace_root() -> Result<PathBuf> {
    let root = run_jj(&["root"])?;
    Ok(PathBuf::from(root))
}

pub fn resolve_store(custom_store: Option<&Path>) -> Result<PathBuf> {
    if let Some(store) = custom_store {
        return Ok(store.to_path_buf());
    }

    let workspace_root = get_workspace_root()?;
    let jj_repo = workspace_root.join(".jj").join("repo");

    if jj_repo.is_file() {
        let content = std::fs::read_to_string(&jj_repo)
            .with_context(|| format!("Failed to read {}", jj_repo.display()))?;
        let trimmed = content.trim();
        let target_repo = if Path::new(trimmed).is_relative() {
            workspace_root.join(".jj").join(trimmed)
        } else {
            PathBuf::from(trimmed)
        };
        let canonical_repo = target_repo
            .canonicalize()
            .with_context(|| format!("Failed to canonicalize {}", target_repo.display()))?;
        let main_repo = canonical_repo
            .parent()
            .and_then(|p| p.parent())
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Could not determine main repo root from {}",
                    canonical_repo.display()
                )
            })?;
        Ok(main_repo.join(".comments"))
    } else {
        Ok(workspace_root.join(".comments"))
    }
}

pub fn get_rev_info(rev: &str) -> Result<(String, String)> {
    let stdout = run_jj(&[
        "log",
        "-r",
        rev,
        "--no-graph",
        "-T",
        "change_id ++ \"\n\" ++ commit_id",
    ])?;
    let lines: Vec<&str> = stdout
        .lines()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();
    if lines.len() < 2 {
        anyhow::bail!(
            "Failed to get change ID and commit ID from jj for revision '{}'. Output was: {:?}",
            rev,
            stdout
        );
    }
    Ok((lines[0].to_string(), lines[1].to_string()))
}

#[derive(Debug)]
pub struct FoundThread {
    pub path: PathBuf,
    pub thread_id: String,
    pub change_id: String,
    pub git_commit_hash: String,
}

pub fn find_thread_file(store: &Path, thread_query: &str) -> Result<FoundThread> {
    if !store.exists() {
        anyhow::bail!("Store directory does not exist: {}", store.display());
    }
    let query_clean = thread_query.trim().trim_end_matches(".json");
    let mut exact_match: Option<FoundThread> = None;
    let mut prefix_matches: Vec<FoundThread> = Vec::new();

    for entry in WalkDir::new(store)
        .min_depth(3)
        .max_depth(3)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if path.is_file() && path.extension().and_then(|ext| ext.to_str()) == Some("json") {
            let stem = match path.file_stem().and_then(|s| s.to_str()) {
                Some(s) => s,
                None => continue,
            };
            let parent_commit = match path.parent().and_then(|p| p.file_name()).and_then(|s| s.to_str()) {
                Some(s) => s,
                None => continue,
            };
            let grandparent_change = match path
                .parent()
                .and_then(|p| p.parent())
                .and_then(|p| p.file_name())
                .and_then(|s| s.to_str())
            {
                Some(s) => s,
                None => continue,
            };

            let found = FoundThread {
                path: path.to_path_buf(),
                thread_id: stem.to_string(),
                change_id: grandparent_change.to_string(),
                git_commit_hash: parent_commit.to_string(),
            };

            if stem == query_clean {
                exact_match = Some(found);
                break;
            } else if stem.starts_with(query_clean) {
                prefix_matches.push(found);
            }
        }
    }

    if let Some(found) = exact_match {
        return Ok(found);
    }

    match prefix_matches.len() {
        0 => anyhow::bail!("No comment thread found matching ID '{}'", thread_query),
        1 => Ok(prefix_matches.remove(0)),
        n => {
            let matches: Vec<String> = prefix_matches.into_iter().map(|m| m.thread_id).collect();
            anyhow::bail!(
                "Ambiguous thread ID '{}' matches {} threads: {}",
                thread_query,
                n,
                matches.join(", ")
            )
        }
    }
}

pub fn add_comment(
    store: &Path,
    workspace_root: &Path,
    rev: &str,
    file: &Path,
    at: Option<LineRange>,
    content: String,
) -> Result<String> {
    let (change_id, commit_id) = get_rev_info(rev)?;

    let rel_filepath = if file.is_absolute() {
        file.strip_prefix(workspace_root)
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| file.to_string_lossy().to_string())
    } else {
        let current_dir = std::env::current_dir().unwrap_or_else(|_| workspace_root.to_path_buf());
        let abs = current_dir.join(file);
        if let Ok(rel) = abs.strip_prefix(workspace_root) {
            rel.to_string_lossy().to_string()
        } else {
            file.to_string_lossy().to_string()
        }
    };

    let thread_id = Uuid::new_v4().to_string();
    let thread_dir = store.join(&change_id).join(&commit_id);
    std::fs::create_dir_all(&thread_dir)
        .with_context(|| format!("Failed to create thread directory: {}", thread_dir.display()))?;

    let thread_file = thread_dir.join(format!("{}.json", thread_id));
    let thread_data = ThreadData {
        filepath: rel_filepath,
        line_range: at,
        comments: vec![content],
        resolved: false,
    };

    let json_bytes = serde_json::to_vec_pretty(&thread_data)
        .context("Failed to serialize thread data to JSON")?;
    std::fs::write(&thread_file, json_bytes)
        .with_context(|| format!("Failed to write thread file: {}", thread_file.display()))?;

    Ok(thread_id)
}

pub fn reply_comment(store: &Path, thread_id_query: &str, content: String) -> Result<String> {
    let found = find_thread_file(store, thread_id_query)?;
    let content_bytes = std::fs::read(&found.path)
        .with_context(|| format!("Failed to read thread file: {}", found.path.display()))?;
    let mut thread_data: ThreadData = serde_json::from_slice(&content_bytes)
        .with_context(|| format!("Failed to parse thread file: {}", found.path.display()))?;

    thread_data.comments.push(content);

    let updated_bytes = serde_json::to_vec_pretty(&thread_data)
        .context("Failed to serialize updated thread data")?;
    std::fs::write(&found.path, updated_bytes)
        .with_context(|| format!("Failed to write updated thread file: {}", found.path.display()))?;

    Ok(found.thread_id)
}

pub fn resolve_comment(
    store: &Path,
    thread_id_query: &str,
    unresolve: bool,
) -> Result<(String, bool)> {
    let found = find_thread_file(store, thread_id_query)?;
    let content_bytes = std::fs::read(&found.path)
        .with_context(|| format!("Failed to read thread file: {}", found.path.display()))?;
    let mut thread_data: ThreadData = serde_json::from_slice(&content_bytes)
        .with_context(|| format!("Failed to parse thread file: {}", found.path.display()))?;

    let new_resolved = !unresolve;
    thread_data.resolved = new_resolved;

    let updated_bytes = serde_json::to_vec_pretty(&thread_data)
        .context("Failed to serialize updated thread data")?;
    std::fs::write(&found.path, updated_bytes)
        .with_context(|| format!("Failed to write updated thread file: {}", found.path.display()))?;

    Ok((found.thread_id, new_resolved))
}

pub fn get_thread(store: &Path, thread_id_query: &str) -> Result<ThreadInfo> {
    let found = find_thread_file(store, thread_id_query)?;
    let content_bytes = std::fs::read(&found.path)
        .with_context(|| format!("Failed to read thread file: {}", found.path.display()))?;
    let thread_data: ThreadData = serde_json::from_slice(&content_bytes)
        .with_context(|| format!("Failed to parse thread file: {}", found.path.display()))?;

    Ok(ThreadInfo {
        thread_id: found.thread_id,
        change_id: found.change_id,
        commit_id: found.git_commit_hash.clone(),
        git_commit_hash: found.git_commit_hash,
        filepath: thread_data.filepath,
        line_range: thread_data.line_range,
        comments: thread_data.comments,
        resolved: thread_data.resolved,
    })
}

pub fn get_change_commits(store: &Path, change_query: &str) -> Result<Vec<String>> {
    if !store.exists() {
        return Ok(Vec::new());
    }

    let mut matched_dirs = Vec::new();
    for entry in std::fs::read_dir(store)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name == change_query {
                matched_dirs = vec![entry.path()];
                break;
            } else if name.starts_with(change_query) {
                matched_dirs.push(entry.path());
            }
        }
    }

    let mut commit_hashes = Vec::new();
    for change_dir in matched_dirs {
        for entry in std::fs::read_dir(&change_dir)? {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                let has_json = std::fs::read_dir(entry.path())?
                    .filter_map(|e| e.ok())
                    .any(|e| e.path().extension().and_then(|s| s.to_str()) == Some("json"));
                if has_json {
                    commit_hashes.push(entry.file_name().to_string_lossy().to_string());
                }
            }
        }
    }
    commit_hashes.sort();
    commit_hashes.dedup();
    Ok(commit_hashes)
}

pub fn get_commit_threads(store: &Path, commit_query: &str) -> Result<Vec<String>> {
    if !store.exists() {
        return Ok(Vec::new());
    }

    let mut thread_ids = Vec::new();
    for entry in WalkDir::new(store)
        .min_depth(2)
        .max_depth(2)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if entry.file_type().is_dir() {
            let commit_dir_name = entry.file_name().to_string_lossy();
            if commit_dir_name == commit_query || commit_dir_name.starts_with(commit_query) {
                for file_entry in std::fs::read_dir(entry.path())?.filter_map(|e| e.ok()) {
                    let path = file_entry.path();
                    if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("json") {
                        if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                            thread_ids.push(stem.to_string());
                        }
                    }
                }
            }
        }
    }
    thread_ids.sort();
    thread_ids.dedup();
    Ok(thread_ids)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_line_range_parsing() {
        assert_eq!(LineRange::from_str("10").unwrap(), LineRange { start: 10, end: 10 });
        assert_eq!(LineRange::from_str("10..20").unwrap(), LineRange { start: 10, end: 20 });
        assert_eq!(LineRange::from_str("10..=20").unwrap(), LineRange { start: 10, end: 20 });
        assert_eq!(LineRange::from_str("10-20").unwrap(), LineRange { start: 10, end: 20 });
        assert_eq!(LineRange::from_str("10:20").unwrap(), LineRange { start: 10, end: 20 });
        assert_eq!(LineRange::from_str("10,20").unwrap(), LineRange { start: 10, end: 20 });
        assert!(LineRange::from_str("20..10").is_err());
        assert!(LineRange::from_str("abc").is_err());
    }

    #[test]
    fn test_store_operations() {
        let dir = tempdir().unwrap();
        let store = dir.path().join(".comments");

        let change_id = "change123";
        let commit_hash = "gitcommithash456";
        let target_dir = store.join(change_id).join(commit_hash);
        std::fs::create_dir_all(&target_dir).unwrap();

        let thread_id = "11111111-2222-3333-4444-555555555555";
        let thread_file = target_dir.join(format!("{}.json", thread_id));

        let initial_data = ThreadData {
            filepath: "src/main.rs".to_string(),
            line_range: Some(LineRange { start: 1, end: 5 }),
            comments: vec!["Initial comment".to_string()],
            resolved: false,
        };
        std::fs::write(&thread_file, serde_json::to_vec_pretty(&initial_data).unwrap()).unwrap();

        // Test find exact
        let found = find_thread_file(&store, thread_id).unwrap();
        assert_eq!(found.thread_id, thread_id);
        assert_eq!(found.change_id, change_id);
        assert_eq!(found.git_commit_hash, commit_hash);

        // Test find prefix
        let found_prefix = find_thread_file(&store, "11111111").unwrap();
        assert_eq!(found_prefix.thread_id, thread_id);

        // Test reply
        reply_comment(&store, "11111111", "My reply".to_string()).unwrap();
        let thread_info = get_thread(&store, thread_id).unwrap();
        assert_eq!(thread_info.comments, vec!["Initial comment", "My reply"]);
        assert!(!thread_info.resolved);

        // Test resolve
        resolve_comment(&store, thread_id, false).unwrap();
        let thread_info = get_thread(&store, thread_id).unwrap();
        assert!(thread_info.resolved);

        // Test unresolve
        resolve_comment(&store, thread_id, true).unwrap();
        let thread_info = get_thread(&store, thread_id).unwrap();
        assert!(!thread_info.resolved);

        // Test get_change_commits
        let commits = get_change_commits(&store, "change123").unwrap();
        assert_eq!(commits, vec![commit_hash]);

        // Test get_commit_threads
        let threads = get_commit_threads(&store, commit_hash).unwrap();
        assert_eq!(threads, vec![thread_id]);
    }
}
