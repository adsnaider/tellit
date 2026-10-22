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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApprovalData {
    pub approved: bool,
    pub timestamp: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commit_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub change_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommitGetResponse {
    pub threads: Vec<ThreadInfo>,
    pub approval: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommitIdsGetResponse {
    pub threads: Vec<String>,
    pub approval: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChangeThreadsGetResponse {
    pub threads: Vec<ThreadInfo>,
    pub approval: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChangeCommitsGetResponse {
    pub commits: Vec<String>,
    pub approval: bool,
}

#[derive(Parser, Debug)]
#[command(name = "tellit", about = "Code review diff viewer and commenting tool for Jujutsu (jj)")]
pub struct Cli {
    #[arg(
        long,
        short,
        global = true,
        help = "Path to comments store directory (defaults to <main-repo>/.tellit)"
    )]
    pub store: Option<PathBuf>,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    #[command(about = "Launch the web diff viewer")]
    Launch(LaunchArgs),

    #[command(about = "Manage code review comments")]
    Comment(CommentArgs),
}

#[derive(Debug, Args)]
pub struct LaunchArgs {
    #[arg(help = "Commit-ish, change ID, or revision to review (default: @)")]
    pub commit_ish: Option<String>,

    #[arg(help = "Optional: compare with this commit/branch")]
    pub compare_with: Option<String>,

    #[arg(long, help = "Port to listen on")]
    pub port: Option<u16>,

    #[arg(long, help = "Host address to bind")]
    pub host: Option<String>,

    #[arg(long, help = "Do not automatically open browser")]
    pub no_open: bool,

    #[arg(long, help = "Keep server running even after browser disconnects")]
    pub keep_alive: bool,

    #[arg(long, help = "Run in background and print JSON info")]
    pub background: bool,

    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    pub extra_args: Vec<String>,
}

#[derive(Debug, Args)]
pub struct CommentArgs {
    #[arg(
        long,
        short = 'C',
        help = "Jujutsu change/revision to target (defaults to @)"
    )]
    pub change: Option<String>,

    #[command(subcommand)]
    pub action: CommentAction,
}

#[derive(Subcommand, Debug)]
pub enum CommentAction {
    #[command(about = "Add a new comment thread")]
    Add(Add),
    #[command(about = "Resolve or unresolve a comment thread")]
    Resolve(Resolve),
    #[command(about = "Reply to an existing comment thread")]
    Reply(Reply),
    #[command(about = "Retrieve comments, commit hashes, or thread IDs")]
    Get(Get),
    #[command(name = "get-all", about = "Get thread IDs for a change, commit, or revision as a JSON array")]
    GetAll(GetAllArgs),
    #[command(about = "Approve or disapprove a commit or change")]
    Approve(ApproveArgs),
}

#[derive(Debug, Args)]
pub struct ApproveArgs {
    #[arg(help = "Target revision, change ID, or commit hash (defaults to @)")]
    pub target: Option<String>,

    #[arg(long, help = "Disapprove the commit/change instead of approving")]
    pub disapprove: bool,
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

    #[command(name = "all", about = "Get thread IDs for a change, commit, or revision as a JSON array")]
    All(GetAllArgs),
}

#[derive(Debug, Args, Clone)]
pub struct GetAllArgs {
    #[arg(
        value_name = "TARGET",
        default_value = "@",
        help = "Change ID, commit hash, or revision (defaults to @)"
    )]
    pub target: String,

    #[arg(
        long,
        alias = "unresovled",
        conflicts_with = "resolved",
        help = "Only return unresolved comment thread IDs"
    )]
    pub unresolved: bool,

    #[arg(
        long,
        conflicts_with = "unresolved",
        help = "Only return resolved comment thread IDs"
    )]
    pub resolved: bool,
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

    #[arg(long, help = "Output all threads for the change as JSON")]
    pub threads: bool,
}

#[derive(Debug, Args)]
pub struct GetCommitArgs {
    #[arg(value_name = "COMMIT_HASH", help = "Git commit hash")]
    pub commit_hash: String,

    #[arg(long, help = "Output as JSON array")]
    pub json: bool,

    #[arg(long, help = "Output all threads for the commit as JSON")]
    pub threads: bool,
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

    let repo_root = if jj_repo.is_file() {
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
        canonical_repo
            .parent()
            .and_then(|p| p.parent())
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Could not determine main repo root from {}",
                    canonical_repo.display()
                )
            })?
            .to_path_buf()
    } else {
        workspace_root
    };

    let tellit_store = repo_root.join(".tellit");
    Ok(tellit_store)
}

pub fn locate_web_cli() -> Result<PathBuf> {
    if let Ok(path) = std::env::var("TELLIT_WEB_CLI") {
        let p = PathBuf::from(path);
        if p.exists() {
            return Ok(p);
        }
    }
    if let Ok(dir) = std::env::var("TELLIT_WEB_DIR") {
        let p = PathBuf::from(&dir).join("dist/cli/index.js");
        if p.exists() {
            return Ok(p);
        }
        let p2 = PathBuf::from(&dir).join("cli/index.js");
        if p2.exists() {
            return Ok(p2);
        }
        let p3 = PathBuf::from(&dir).join("index.js");
        if p3.exists() {
            return Ok(p3);
        }
    }

    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(exe_dir) = exe_path.parent() {
            let candidates = [
                exe_dir.join("../share/tellit/web/dist/cli/index.js"),
                exe_dir.join("../share/tellit/dist/cli/index.js"),
                exe_dir.join("../dist/cli/index.js"),
            ];
            for cand in candidates {
                if cand.exists() {
                    return Ok(cand);
                }
            }
        }
    }

    // Check development paths (walk up from current directory)
    if let Ok(cur) = std::env::current_dir() {
        let mut d = cur.as_path();
        for _ in 0..6 {
            let candidate = d.join("frontend/dist/cli/index.js");
            if candidate.exists() {
                return Ok(candidate);
            }
            let candidate2 = d.join("dist/cli/index.js");
            if candidate2.exists() {
                return Ok(candidate2);
            }
            if let Some(parent) = d.parent() {
                d = parent;
            } else {
                break;
            }
        }
    }

    anyhow::bail!("Could not locate tellit web CLI (dist/cli/index.js). Try setting TELLIT_WEB_DIR or TELLIT_WEB_CLI.")
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
            if stem == "approval" {
                continue;
            }
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
                    .any(|e| {
                        let p = e.path();
                        p.extension().and_then(|s| s.to_str()) == Some("json")
                            && p.file_stem().and_then(|s| s.to_str()) != Some("approval")
                    });
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
                            if stem != "approval" {
                                thread_ids.push(stem.to_string());
                            }
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

pub fn get_change_threads(store: &Path, change_query: &str) -> Result<Vec<ThreadInfo>> {
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

    let mut threads = Vec::new();
    for change_dir in matched_dirs {
        let change_id = change_dir
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        for commit_entry in std::fs::read_dir(&change_dir)? {
            let commit_entry = commit_entry?;
            if commit_entry.file_type()?.is_dir() {
                let commit_id = commit_entry.file_name().to_string_lossy().to_string();
                for file_entry in std::fs::read_dir(commit_entry.path())? {
                    let file_entry = file_entry?;
                    let path = file_entry.path();
                    if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("json") {
                        let thread_id = path
                            .file_stem()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .to_string();
                        if thread_id == "approval" {
                            continue;
                        }
                        if let Ok(bytes) = std::fs::read(&path) {
                            if let Ok(thread_data) = serde_json::from_slice::<ThreadData>(&bytes) {
                                threads.push(ThreadInfo {
                                    thread_id,
                                    change_id: change_id.clone(),
                                    commit_id: commit_id.clone(),
                                    git_commit_hash: commit_id.clone(),
                                    filepath: thread_data.filepath,
                                    line_range: thread_data.line_range,
                                    comments: thread_data.comments,
                                    resolved: thread_data.resolved,
                                });
                            }
                        }
                    }
                }
            }
        }
    }
    threads.sort_by(|a, b| a.thread_id.cmp(&b.thread_id));
    Ok(threads)
}

pub fn get_commit_thread_infos(store: &Path, commit_query: &str) -> Result<Vec<ThreadInfo>> {
    if !store.exists() {
        return Ok(Vec::new());
    }

    let mut threads = Vec::new();
    for entry in WalkDir::new(store)
        .min_depth(2)
        .max_depth(2)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if entry.file_type().is_dir() {
            let commit_dir_name = entry.file_name().to_string_lossy();
            if commit_dir_name == commit_query || commit_dir_name.starts_with(commit_query) {
                let change_id = entry
                    .path()
                    .parent()
                    .and_then(|p| p.file_name())
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_default();
                let commit_id = commit_dir_name.to_string();

                for file_entry in std::fs::read_dir(entry.path())?.filter_map(|e| e.ok()) {
                    let path = file_entry.path();
                    if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("json") {
                        let thread_id = path
                            .file_stem()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .to_string();
                        if thread_id == "approval" {
                            continue;
                        }
                        if let Ok(bytes) = std::fs::read(&path) {
                            if let Ok(thread_data) = serde_json::from_slice::<ThreadData>(&bytes) {
                                threads.push(ThreadInfo {
                                    thread_id,
                                    change_id: change_id.clone(),
                                    commit_id: commit_id.clone(),
                                    git_commit_hash: commit_id.clone(),
                                    filepath: thread_data.filepath,
                                    line_range: thread_data.line_range,
                                    comments: thread_data.comments,
                                    resolved: thread_data.resolved,
                                });
                            }
                        }
                    }
                }
            }
        }
    }
    threads.sort_by(|a, b| a.thread_id.cmp(&b.thread_id));
    Ok(threads)
}

pub fn get_all_threads(
    store: &Path,
    target: &str,
    resolved_filter: Option<bool>,
) -> Result<Vec<String>> {
    if !store.exists() {
        return Ok(Vec::new());
    }

    let mut threads = Vec::new();

    // 1. Check if target matches any change directory in store
    let change_threads = get_change_threads(store, target).unwrap_or_default();
    if !change_threads.is_empty() {
        threads = change_threads;
    } else {
        // 2. Check if target matches any commit directory in store
        let commit_threads = get_commit_thread_infos(store, target).unwrap_or_default();
        if !commit_threads.is_empty() {
            threads = commit_threads;
        } else {
            // 3. Try resolving target as a Jujutsu revision (e.g. '@', '@-', bookmark, or revset)
            if let Ok((change_id, commit_id)) = get_rev_info(target) {
                let change_threads = get_change_threads(store, &change_id).unwrap_or_default();
                if !change_threads.is_empty() {
                    threads = change_threads;
                } else {
                    threads = get_commit_thread_infos(store, &commit_id).unwrap_or_default();
                }
            }
        }
    }

    let mut thread_ids: Vec<String> = threads
        .into_iter()
        .filter(|t| match resolved_filter {
            Some(expected) => t.resolved == expected,
            None => true,
        })
        .map(|t| t.thread_id)
        .collect();

    thread_ids.sort();
    thread_ids.dedup();
    Ok(thread_ids)
}

pub fn is_commit_approved(store: &Path, commit_query: &str) -> bool {
    if !store.exists() {
        return false;
    }
    for entry in WalkDir::new(store)
        .min_depth(2)
        .max_depth(2)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if entry.file_type().is_dir() {
            let commit_name = entry.file_name().to_string_lossy();
            if commit_name == commit_query
                || (commit_query.len() >= 6 && commit_name.starts_with(commit_query))
                || (commit_name.len() >= 6 && commit_query.starts_with(&*commit_name))
            {
                let approval_file = entry.path().join("approval.json");
                if approval_file.exists() {
                    if let Ok(bytes) = std::fs::read(&approval_file) {
                        if let Ok(data) = serde_json::from_slice::<ApprovalData>(&bytes) {
                            return data.approved;
                        }
                    }
                    return true;
                }
            }
        }
    }
    false
}

pub fn is_change_approved(store: &Path, change_query: &str) -> bool {
    // 1. Ask Jujutsu for the latest commit of the change
    let latest_commit = match run_jj(&[
        "log",
        "-r",
        &format!("change_id({})", change_query),
        "-n",
        "1",
        "--no-graph",
        "-T",
        "commit_id",
    ]) {
        Ok(out) if !out.trim().is_empty() => Some(out.trim().to_string()),
        _ => match run_jj(&[
            "log",
            "-r",
            change_query,
            "-n",
            "1",
            "--no-graph",
            "-T",
            "commit_id",
        ]) {
            Ok(out) if !out.trim().is_empty() => Some(out.trim().to_string()),
            _ => None,
        },
    };

    if let Some(commit_id) = latest_commit {
        return is_commit_approved(store, &commit_id);
    }

    false
}

pub fn set_commit_approval(
    store: &Path,
    target: Option<&str>,
    disapprove: bool,
) -> Result<(String, String, bool)> {
    let rev = target.unwrap_or("@");

    // 1. Resolve rev using Jujutsu log
    let (change_id, commit_id) = match get_rev_info(rev) {
        Ok(info) => info,
        Err(_) => match run_jj(&[
            "log",
            "-r",
            &format!("change_id({})", rev),
            "-n",
            "1",
            "--no-graph",
            "-T",
            "change_id ++ \"\n\" ++ commit_id",
        ]) {
            Ok(out) => {
                let lines: Vec<&str> = out.lines().map(|s| s.trim()).filter(|s| !s.is_empty()).collect();
                if lines.len() >= 2 {
                    (lines[0].to_string(), lines[1].to_string())
                } else {
                    anyhow::bail!("Failed to resolve revision or change '{}'", rev);
                }
            }
            Err(_) => {
                // Search store as fallback
                let mut found: Option<(String, String)> = None;
                if store.exists() {
                    for entry in WalkDir::new(store)
                        .min_depth(2)
                        .max_depth(2)
                        .into_iter()
                        .filter_map(|e| e.ok())
                    {
                        if entry.file_type().is_dir() {
                            let commit_name = entry.file_name().to_string_lossy();
                            let change_name = entry
                                .path()
                                .parent()
                                .and_then(|p| p.file_name())
                                .map(|s| s.to_string_lossy().to_string())
                                .unwrap_or_default();
                            if commit_name == rev
                                || (rev.len() >= 6 && commit_name.starts_with(rev))
                                || change_name == rev
                                || (rev.len() >= 6 && change_name.starts_with(rev))
                            {
                                found = Some((change_name, commit_name.to_string()));
                                break;
                            }
                        }
                    }
                }
                found.ok_or_else(|| {
                    anyhow::anyhow!(
                        "Could not resolve target '{}' to a Jujutsu revision or store commit",
                        rev
                    )
                })?
            }
        },
    };

    let commit_dir = store.join(&change_id).join(&commit_id);
    let approval_file = commit_dir.join("approval.json");

    if disapprove {
        if approval_file.exists() {
            let _ = std::fs::remove_file(&approval_file);
        }
        Ok((change_id, commit_id, false))
    } else {
        std::fs::create_dir_all(&commit_dir)?;
        let now_secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let data = ApprovalData {
            approved: true,
            timestamp: now_secs.to_string(),
            commit_id: Some(commit_id.clone()),
            change_id: Some(change_id.clone()),
        };
        let bytes = serde_json::to_vec_pretty(&data)?;
        std::fs::write(&approval_file, bytes)?;
        Ok((change_id, commit_id, true))
    }
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
        let store = dir.path().join(".tellit");

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

        // Test get_change_threads
        let change_threads = get_change_threads(&store, "change123").unwrap();
        assert_eq!(change_threads.len(), 1);
        assert_eq!(change_threads[0].thread_id, thread_id);
        assert_eq!(change_threads[0].commit_id, commit_hash);

        // Test get_commit_thread_infos
        let commit_threads = get_commit_thread_infos(&store, commit_hash).unwrap();
        assert_eq!(commit_threads.len(), 1);
        assert_eq!(commit_threads[0].thread_id, thread_id);

        // Test approval operations
        assert!(!is_commit_approved(&store, commit_hash));
        set_commit_approval(&store, Some(commit_hash), false).unwrap();
        assert!(is_commit_approved(&store, commit_hash));

        // Test disapprove
        set_commit_approval(&store, Some(commit_hash), true).unwrap();
        assert!(!is_commit_approved(&store, commit_hash));
    }
}
