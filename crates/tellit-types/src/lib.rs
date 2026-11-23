//! Wire types shared between the `tellit` HTTP server and the Leptos browser client.
//!
//! All types in this crate are `#![no_std]` compatible and serialize to the JSON and
//! Server-Sent Events (SSE) payloads exchanged over `/api/*` endpoints.
//!
//! # Examples
//!
//! ```
//! use core::str::FromStr;
//! use tellit_types::{HeartbeatEvent, JjStatusResponse, ReviewStatus};
//!
//! let status = JjStatusResponse::jj("/workspace/repo");
//! assert!(status.is_jj);
//! assert_eq!(status.repo_root.as_deref(), Some("/workspace/repo"));
//!
//! let event = HeartbeatEvent::from_str("connected")?;
//! assert_eq!(event, HeartbeatEvent::Connected);
//!
//! assert_eq!(
//!     ReviewStatus::compute(2, 1, true, false),
//!     ReviewStatus::HasUnresolved
//! );
//! # Ok::<(), tellit_types::UnknownHeartbeatEvent>(())
//! ```

#![no_std]

extern crate alloc;

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;
use core::str::FromStr;

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Response payload returned by `GET /api/jj/status`.
///
/// # Examples
///
/// ```
/// use tellit_types::JjStatusResponse;
///
/// let status = JjStatusResponse::jj("/tmp/repo");
/// let json = serde_json::to_string(&status)?;
/// assert_eq!(json, r#"{"isJj":true,"repoRoot":"/tmp/repo"}"#);
/// # Ok::<(), serde_json::Error>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JjStatusResponse {
    /// Whether the server's active directory belongs to a Jujutsu workspace.
    pub is_jj: bool,
    /// Root path of the Jujutsu workspace when `is_jj` is `true`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repo_root: Option<String>,
    /// Initial target commit ID resolved from `tellit launch [commit_ish]`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub initial_target: Option<String>,
    /// Initial comparison base commit ID resolved from `tellit launch <commit_ish> <compare_with>`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub initial_base: Option<String>,
}

impl JjStatusResponse {
    /// Creates a status response for a detected Jujutsu workspace at `repo_root`.
    ///
    /// # Examples
    ///
    /// ```
    /// use tellit_types::JjStatusResponse;
    ///
    /// let status = JjStatusResponse::jj("/home/user/repo");
    /// assert!(status.is_jj);
    /// assert_eq!(status.repo_root.as_deref(), Some("/home/user/repo"));
    /// ```
    #[must_use]
    pub fn jj(repo_root: impl Into<String>) -> Self {
        Self {
            is_jj: true,
            repo_root: Some(repo_root.into()),
            initial_target: None,
            initial_base: None,
        }
    }

    /// Attaches resolved initial target and comparison commit IDs from CLI launch arguments.
    ///
    /// # Examples
    ///
    /// ```
    /// use tellit_types::JjStatusResponse;
    ///
    /// let status = JjStatusResponse::jj("/repo")
    ///     .with_initial_revisions(Some(String::from("abc123")), None);
    /// assert_eq!(status.initial_target.as_deref(), Some("abc123"));
    /// ```
    #[must_use]
    pub fn with_initial_revisions(
        mut self,
        initial_target: Option<String>,
        initial_base: Option<String>,
    ) -> Self {
        self.initial_target = initial_target;
        self.initial_base = initial_base;
        self
    }

    /// Creates a status response indicating that the active directory is not a Jujutsu workspace.
    ///
    /// # Examples
    ///
    /// ```
    /// use tellit_types::JjStatusResponse;
    ///
    /// let status = JjStatusResponse::not_jj();
    /// assert!(!status.is_jj);
    /// assert!(status.repo_root.is_none());
    /// ```
    #[must_use]
    pub const fn not_jj() -> Self {
        Self {
            is_jj: false,
            repo_root: None,
            initial_target: None,
            initial_base: None,
        }
    }
}

/// Server-Sent Events data payload emitted by `GET /api/heartbeat`.
///
/// # Examples
///
/// ```
/// use core::str::FromStr;
/// use tellit_types::HeartbeatEvent;
///
/// assert_eq!(HeartbeatEvent::Connected.as_str(), "connected");
/// assert_eq!(HeartbeatEvent::from_str("heartbeat")?, HeartbeatEvent::Ping);
/// # Ok::<(), tellit_types::UnknownHeartbeatEvent>(())
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeartbeatEvent {
    /// Initial event sent immediately when a browser tab opens `/api/heartbeat`.
    Connected,
    /// Periodic keep-alive event sent while the `/api/heartbeat` stream stays open.
    Ping,
}

impl HeartbeatEvent {
    /// Returns the wire-format SSE `data:` payload string for this event.
    ///
    /// # Examples
    ///
    /// ```
    /// use tellit_types::HeartbeatEvent;
    ///
    /// assert_eq!(HeartbeatEvent::Connected.as_str(), "connected");
    /// assert_eq!(HeartbeatEvent::Ping.as_str(), "heartbeat");
    /// ```
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Connected => "connected",
            Self::Ping => "heartbeat",
        }
    }
}

impl fmt::Display for HeartbeatEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Error returned when parsing an unrecognized `/api/heartbeat` event payload.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("unrecognized heartbeat event payload: {payload:?}")]
pub struct UnknownHeartbeatEvent {
    /// Raw SSE `data:` string that did not match any known [`HeartbeatEvent`].
    pub payload: String,
}

impl FromStr for HeartbeatEvent {
    type Err = UnknownHeartbeatEvent;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim() {
            "connected" => Ok(Self::Connected),
            "heartbeat" => Ok(Self::Ping),
            other => Err(UnknownHeartbeatEvent {
                payload: String::from(other),
            }),
        }
    }
}

/// Combined review indicator state for a Jujutsu change or commit.
///
/// Unresolved comments always take precedence over approval indicators.
///
/// # Examples
///
/// ```
/// use tellit_types::ReviewStatus;
///
/// assert_eq!(ReviewStatus::compute(0, 0, false, false), ReviewStatus::None);
/// assert_eq!(ReviewStatus::compute(2, 0, false, false), ReviewStatus::AllResolved);
/// assert_eq!(ReviewStatus::compute(2, 0, false, true), ReviewStatus::CleanRebaseApproved);
/// assert_eq!(ReviewStatus::compute(2, 0, true, false), ReviewStatus::Approved);
/// assert_eq!(ReviewStatus::compute(2, 1, true, true), ReviewStatus::HasUnresolved);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewStatus {
    /// No comments and no approval recorded.
    None,
    /// At least one comment thread exists, all are resolved, and the commit is not approved.
    AllResolved,
    /// Zero unresolved threads and the commit is a clean rebase of an approved revision.
    CleanRebaseApproved,
    /// Zero unresolved threads and the commit is directly approved.
    Approved,
    /// One or more unresolved comment threads exist.
    HasUnresolved,
}

impl ReviewStatus {
    /// Computes the effective [`ReviewStatus`] from comment counts and approval flags.
    ///
    /// # Examples
    ///
    /// ```
    /// use tellit_types::ReviewStatus;
    ///
    /// assert_eq!(ReviewStatus::compute(1, 0, true, false), ReviewStatus::Approved);
    /// ```
    #[must_use]
    pub const fn compute(
        comment_count: usize,
        unresolved_comment_count: usize,
        approved: bool,
        clean_rebase_approved: bool,
    ) -> Self {
        if unresolved_comment_count > 0 {
            Self::HasUnresolved
        } else if approved {
            Self::Approved
        } else if clean_rebase_approved {
            Self::CleanRebaseApproved
        } else if comment_count > 0 {
            Self::AllResolved
        } else {
            Self::None
        }
    }
}

/// Summary of a Jujutsu change entry in the revision graph (`GET /api/jj/changes`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JjChange {
    /// Full Jujutsu change ID.
    pub change_id: String,
    /// Eight-character change ID prefix.
    pub short_change_id: String,
    /// Shortest unique change ID prefix reported by Jujutsu.
    pub shortest_change_id: String,
    /// Full head commit hash for this change.
    pub commit_id: String,
    /// Eight-character commit hash prefix.
    pub short_commit_id: String,
    /// Whether this commit is the active working-copy commit (`@`).
    pub is_working_copy: bool,
    /// Whether the commit introduces no tree modifications relative to its parent.
    pub is_empty: bool,
    /// Whether the commit has a non-empty description.
    pub has_description: bool,
    /// Whether Jujutsu marks this revision immutable (`◆`).
    pub is_immutable: bool,
    /// Whether the commit tree contains unresolved merge conflicts (`×`).
    pub is_conflict: bool,
    /// Local bookmark names pointing to this change.
    #[serde(default)]
    pub bookmarks: Vec<String>,
    /// Parent commit hashes of this revision.
    #[serde(default)]
    pub parent_commit_ids: Vec<String>,
    /// Commit author name or email.
    pub author: String,
    /// Formatted committer timestamp (`YYYY-MM-DD HH:MM:SS`).
    pub timestamp: String,
    /// First line of the commit description, or `"(no description set)"`.
    pub description: String,
    /// Total `.tellit/` comment threads attached to commits under this change.
    pub comment_count: usize,
    /// Unresolved `.tellit/` comment threads under this change.
    pub unresolved_comment_count: usize,
    /// Whether the current head commit of this change is directly approved.
    pub approved: bool,
    /// Whether the head commit is an unmodified rebase of an earlier approved commit.
    pub clean_rebase_approved: bool,
}

impl JjChange {
    /// Computes the combined [`ReviewStatus`] dot indicator for this change.
    ///
    /// # Examples
    ///
    /// ```
    /// use tellit_types::{JjChange, ReviewStatus};
    ///
    /// let change = JjChange {
    ///     change_id: String::from("kkmpptxz"),
    ///     short_change_id: String::from("kkmpptxz"),
    ///     shortest_change_id: String::from("k"),
    ///     commit_id: String::from("12345678"),
    ///     short_commit_id: String::from("12345678"),
    ///     is_working_copy: true,
    ///     is_empty: false,
    ///     has_description: true,
    ///     is_immutable: false,
    ///     is_conflict: false,
    ///     bookmarks: Vec::new(),
    ///     parent_commit_ids: Vec::new(),
    ///     author: String::from("dev@example.com"),
    ///     timestamp: String::from("2026-11-16 12:00:00"),
    ///     description: String::from("feat: add parser"),
    ///     comment_count: 0,
    ///     unresolved_comment_count: 0,
    ///     approved: true,
    ///     clean_rebase_approved: false,
    /// };
    /// assert_eq!(change.review_status(), ReviewStatus::Approved);
    /// ```
    #[must_use]
    pub const fn review_status(&self) -> ReviewStatus {
        ReviewStatus::compute(
            self.comment_count,
            self.unresolved_comment_count,
            self.approved,
            self.clean_rebase_approved,
        )
    }

    /// Returns the Jujutsu DAG node glyph (`@`, `◆`, `×`, or `○`) for this change.
    ///
    /// # Examples
    ///
    /// ```
    /// use tellit_types::JjChange;
    ///
    /// let change = JjChange {
    ///     change_id: String::from("kkmpptxz"),
    ///     short_change_id: String::from("kkmpptxz"),
    ///     shortest_change_id: String::from("k"),
    ///     commit_id: String::from("12345678"),
    ///     short_commit_id: String::from("12345678"),
    ///     is_working_copy: true,
    ///     is_empty: false,
    ///     has_description: true,
    ///     is_immutable: false,
    ///     is_conflict: false,
    ///     bookmarks: Vec::new(),
    ///     parent_commit_ids: Vec::new(),
    ///     author: String::from("dev@example.com"),
    ///     timestamp: String::from("2026-11-16 12:00:00"),
    ///     description: String::from("feat: add parser"),
    ///     comment_count: 0,
    ///     unresolved_comment_count: 0,
    ///     approved: false,
    ///     clean_rebase_approved: false,
    /// };
    /// assert_eq!(change.node_glyph(), '@');
    /// ```
    #[must_use]
    pub const fn node_glyph(&self) -> char {
        if self.is_working_copy {
            '@'
        } else if self.is_immutable {
            '◆'
        } else if self.is_conflict {
            '×'
        } else {
            '○'
        }
    }
}

/// Row in the parsed `jj log` ASCII/Unicode DAG layout.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum JjGraphRow {
    /// Row carrying a commit node and its associated [`JjChange`].
    #[serde(rename_all = "camelCase")]
    Commit {
        /// Unique row key (commit hash).
        key: String,
        /// Per-column graph characters on the commit line.
        graph_cols: Vec<char>,
        /// Column indices with a vertical stem continuing below the commit line.
        down_cols: Vec<usize>,
        /// Change metadata displayed beside the graph gutter.
        change: Box<JjChange>,
    },
    /// Connector or elision row bridging adjacent commit rows in the DAG.
    #[serde(rename_all = "camelCase")]
    Connector {
        /// Unique row key.
        key: String,
        /// Per-column box-drawing or elision characters.
        graph_cols: Vec<char>,
        /// Whether this row renders an elision marker (`~`).
        is_elided: bool,
        /// Whether this row came from a `(elided revisions)` line.
        bridges_elided: bool,
    },
}

impl JjGraphRow {
    /// Builds a single-column [`JjGraphRow::Commit`] row for a standalone change lookup.
    #[must_use]
    pub fn standalone(change: &JjChange) -> Self {
        Self::Commit {
            key: change.commit_id.clone(),
            graph_cols: alloc::vec![change.node_glyph()],
            down_cols: Vec::new(),
            change: alloc::boxed::Box::new(change.clone()),
        }
    }
}

/// Response payload returned by `GET /api/jj/changes`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JjChangesResponse {
    /// Ordered Jujutsu changes matching the log or query.
    pub changes: Vec<JjChange>,
    /// Graph rows aligned with `changes` for DAG rendering.
    pub graph_rows: Vec<JjGraphRow>,
}

/// Single commit revision in a change's evolution log (`GET /api/jj/changes/{change_id}/commits`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JjCommit {
    /// Full Git commit hash.
    pub commit_id: String,
    /// Eight-character commit hash prefix.
    pub short_commit_id: String,
    /// Parent commit hashes of this revision.
    #[serde(default)]
    pub parent_commit_ids: Vec<String>,
    /// Formatted committer timestamp (`YYYY-MM-DD HH:MM:SS`).
    pub timestamp: String,
    /// First line of the commit description, or `"(no description set)"`.
    pub description: String,
    /// Whether this commit is currently checked out as `@`.
    pub is_working_copy: bool,
    /// Total `.tellit/` threads recorded directly against this commit hash.
    pub comment_count: usize,
    /// Unresolved `.tellit/` threads recorded against this commit hash.
    pub unresolved_comment_count: usize,
    /// Whether this commit has a direct `approval.json` record.
    pub approved: bool,
    /// Whether this commit is a clean rebase of an earlier approved commit in the change.
    pub clean_rebase_approved: bool,
    /// Commit hash of the earlier approved revision carried forward across clean rebase.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clean_rebase_source_commit_id: Option<String>,
    /// 1-based revision number in causal evolog order of the source approved commit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clean_rebase_source_commit_number: Option<usize>,
}

impl JjCommit {
    /// Computes the combined [`ReviewStatus`] dot indicator for this commit.
    ///
    /// # Examples
    ///
    /// ```
    /// use tellit_types::{JjCommit, ReviewStatus};
    ///
    /// let commit = JjCommit {
    ///     commit_id: String::from("abcdef12"),
    ///     short_commit_id: String::from("abcdef12"),
    ///     parent_commit_ids: Vec::new(),
    ///     timestamp: String::from("2026-11-16 12:00:00"),
    ///     description: String::from("refactor"),
    ///     is_working_copy: false,
    ///     comment_count: 1,
    ///     unresolved_comment_count: 0,
    ///     approved: false,
    ///     clean_rebase_approved: true,
    ///     clean_rebase_source_commit_id: Some(String::from("00112233")),
    ///     clean_rebase_source_commit_number: Some(1),
    /// };
    /// assert_eq!(commit.review_status(), ReviewStatus::CleanRebaseApproved);
    /// ```
    #[must_use]
    pub const fn review_status(&self) -> ReviewStatus {
        ReviewStatus::compute(
            self.comment_count,
            self.unresolved_comment_count,
            self.approved,
            self.clean_rebase_approved,
        )
    }
}

/// Response payload returned by `GET /api/jj/changes/{change_id}/commits`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JjCommitsResponse {
    /// Commits in causal evolution order (oldest `#1` first, latest head commit last).
    pub commits: Vec<JjCommit>,
}

/// Modification status of a file in a diff.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FileStatus {
    /// Newly added file.
    Added,
    /// Modified existing file.
    Modified,
    /// Deleted file.
    Deleted,
    /// Renamed file (with optional modifications).
    Renamed,
}

impl FileStatus {
    /// Short single-letter badge text (`A`, `M`, `D`, `R`) displayed in the file tree.
    ///
    /// # Examples
    ///
    /// ```
    /// use tellit_types::FileStatus;
    ///
    /// assert_eq!(FileStatus::Added.short_code(), "A");
    /// assert_eq!(FileStatus::Renamed.short_code(), "R");
    /// ```
    #[must_use]
    pub const fn short_code(self) -> &'static str {
        match self {
            Self::Added => "A",
            Self::Modified => "M",
            Self::Deleted => "D",
            Self::Renamed => "R",
        }
    }
}

/// Kind of line within a unified diff hunk.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DiffLineKind {
    /// Unchanged context line present in both base and target.
    Context,
    /// Line added in the target revision.
    Add,
    /// Line removed from the base revision.
    Delete,
}

/// Maximum line length in bytes that receives syntax highlighting, word-level diffing, and
/// token splitting; longer lines render as plain text.
pub const MAX_HIGHLIGHT_LINE_BYTES: usize = 1_024;

/// Number of non-default syntax palette classes (`.syn-1`..`.syn-19`) defined by the UI CSS.
pub const SYNTAX_PALETTE_SIZE: u8 = 19;

/// Syntax-highlighted byte-length span into [`DiffLine::content`].
///
/// `style == 0` uses the theme's default foreground; `1..=SYNTAX_PALETTE_SIZE` maps to CSS class
/// `.syn-<style>`. Serializes compactly as `[len, style]` to avoid duplicating line text on the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "(u32, u8)", into = "(u32, u8)")]
pub struct SyntaxSpan {
    /// Byte length of this slice inside [`DiffLine::content`].
    pub len: u32,
    /// Palette style index (`0` = default foreground, `1..=19` = `.syn-<style>`).
    pub style: u8,
}

impl From<(u32, u8)> for SyntaxSpan {
    fn from((len, style): (u32, u8)) -> Self {
        Self { len, style }
    }
}

impl From<SyntaxSpan> for (u32, u8) {
    fn from(span: SyntaxSpan) -> Self {
        (span.len, span.style)
    }
}

/// Inline word-level diff byte-length segment into [`DiffLine::content`].
///
/// Serializes compactly as `[len, emphasized]` to avoid duplicating line text on the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "(u32, bool)", into = "(u32, bool)")]
pub struct WordDiffSegment {
    /// Byte length of this slice inside [`DiffLine::content`].
    pub len: u32,
    /// `true` when this word/token was inserted or deleted relative to the paired line.
    pub emphasized: bool,
}

impl From<(u32, bool)> for WordDiffSegment {
    fn from((len, emphasized): (u32, bool)) -> Self {
        Self { len, emphasized }
    }
}

impl From<WordDiffSegment> for (u32, bool) {
    fn from(seg: WordDiffSegment) -> Self {
        (seg.len, seg.emphasized)
    }
}

/// Single line inside a [`DiffChunk`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffLine {
    /// Whether the line is context, added, or deleted.
    pub kind: DiffLineKind,
    /// Raw line text without the leading `+`, `-`, or space marker.
    pub content: String,
    /// 1-based line number in the base file (`None` for added lines).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub old_line_number: Option<u32>,
    /// 1-based line number in the target file (`None` for deleted lines).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new_line_number: Option<u32>,
    /// Syntax-highlighted spans covering `content`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub syntax_spans: Vec<SyntaxSpan>,
    /// Optional word-level inline diff segments when paired with an adjacent edit line.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub word_diff: Vec<WordDiffSegment>,
}

/// Contiguous unified diff hunk (`@@ -old_start,old_lines +new_start,new_lines @@`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffChunk {
    /// Raw `@@ ... @@` hunk header line.
    pub header: String,
    /// Starting 1-based line number in the old file.
    pub old_start: u32,
    /// Line count in the old file covered by this hunk.
    pub old_lines: u32,
    /// Starting 1-based line number in the new file.
    pub new_start: u32,
    /// Line count in the new file covered by this hunk.
    pub new_lines: u32,
    /// Diff lines belonging to this hunk.
    pub lines: Vec<DiffLine>,
}

const fn default_true() -> bool {
    true
}

/// Changed file entry in [`JjDiffResponse`], including the virtual `"Commit Message"` entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffFile {
    /// Repository-relative path (or `"Commit Message"` for the virtual description diff).
    pub path: String,
    /// Previous repository-relative path when `status` is [`FileStatus::Renamed`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub old_path: Option<String>,
    /// File modification status.
    pub status: FileStatus,
    /// Number of added lines across all hunks.
    pub additions: u32,
    /// Number of deleted lines across all hunks.
    pub deletions: u32,
    /// Whether lockfile/codegen path or header rules classify this file as generated.
    pub is_generated: bool,
    /// Parsed diff hunks for this file.
    pub chunks: Vec<DiffChunk>,
    /// `true` when `chunks` contains full parsed hunks; `false` for summary-only entries
    /// whose chunks are loaded on demand via `GET /api/jj/diff/file`.
    #[serde(default = "default_true")]
    pub chunks_loaded: bool,
}

/// Response payload returned by `GET /api/jj/diff`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JjDiffResponse {
    /// Resolved target commit ID.
    pub commit: String,
    /// Effective base revision label (resolved parent commit ID, `"{commit}-"`, or compared commit ID).
    pub base_commit: String,
    /// Ordered changed files, beginning with `"Commit Message"`.
    pub files: Vec<DiffFile>,
    /// `true` when `files` has no non-empty changes.
    pub is_empty: bool,
    /// `true` when comparing two revisions of the same change across different parent commits.
    pub is_rebase_diff: bool,
    /// `true` when upstream rebase changes were isolated out via `git merge-tree`.
    pub rebase_ignored: bool,
    /// `true` when in-memory `git merge-tree` encountered a conflict and fell back to raw diff.
    pub rebase_conflict: bool,
}

/// Inclusive 1-based line range attached to a review comment thread.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct LineRangeSpec {
    /// 1-based starting line number (inclusive).
    pub start: u32,
    /// 1-based ending line number (inclusive).
    pub end: u32,
}

/// Which side of a comparison a review comment thread anchors to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CommentSide {
    /// Anchored to the base (`From`) revision in an inter-commit comparison.
    Old,
    /// Anchored to the target (`To`) revision.
    New,
}

/// Review comment thread returned by `GET /api/jj/comments`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JjCommentThread {
    /// Thread UUID.
    pub id: String,
    /// Jujutsu change ID directory under `.tellit/`.
    pub change_id: String,
    /// Git commit hash directory under `.tellit/<change_id>/`.
    pub commit_id: String,
    /// Repository-relative file path.
    pub file_path: String,
    /// Diff side (`old` or `new`) for this comparison.
    pub side: CommentSide,
    /// Optional 1-based inclusive line range (`None` for whole-file threads).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line_range: Option<LineRangeSpec>,
    /// Ordered comment messages in the thread.
    pub comments: Vec<String>,
    /// Whether the thread is marked resolved.
    pub resolved: bool,
}

/// Response payload returned by `GET /api/jj/comments`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JjCommentsResponse {
    /// Comment threads belonging to the active change/commit comparison.
    pub threads: Vec<JjCommentThread>,
}

/// Request body accepted by `POST /api/jj/comments`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateCommentRequest {
    /// Target commit hash or revision on which to record the thread.
    pub commit_id: String,
    /// Repository-relative file path.
    pub file_path: String,
    /// Optional 1-based inclusive line range.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line_range: Option<LineRangeSpec>,
    /// Initial comment message text.
    pub content: String,
}

/// Response payload returned by `POST /api/jj/comments`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateCommentResponse {
    /// Newly created UUID v4 thread identifier.
    pub thread_id: String,
}

/// Request body accepted by `POST /api/jj/comments/{thread_id}/reply`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplyCommentRequest {
    /// Reply message text to append.
    pub content: String,
}

/// Response payload returned by `POST /api/jj/comments/{thread_id}/reply`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplyCommentResponse {
    /// Canonical thread UUID that received the reply.
    pub thread_id: String,
}

/// Request body accepted by `POST /api/jj/comments/{thread_id}/resolve`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolveCommentRequest {
    /// `true` to reopen the thread as unresolved, `false` to mark it resolved.
    #[serde(default)]
    pub unresolve: bool,
}

/// Response payload returned by `POST /api/jj/comments/{thread_id}/resolve`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolveCommentResponse {
    /// Canonical thread UUID updated.
    pub thread_id: String,
    /// Resulting resolved state (`true` when resolved).
    pub resolved: bool,
}

/// Request body accepted by `POST /api/jj/approve`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApproveCommitRequest {
    /// Commit hash or Jujutsu revision to approve or disapprove.
    pub commit_id: String,
    /// `true` to revoke existing approval, `false` to approve.
    #[serde(default)]
    pub disapprove: bool,
}

/// Response payload returned by `POST /api/jj/approve`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApproveCommitResponse {
    /// Resolved Jujutsu change ID.
    pub change_id: String,
    /// Resolved Git commit hash.
    pub commit_id: String,
    /// Resulting approval state (`true` when approved).
    pub approved: bool,
}

/// Origin category for a live filesystem reload notification on `GET /api/watch`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WatchSource {
    /// Working-tree file changed on disk.
    WorkingTree,
    /// Jujutsu `.jj/repo/op_heads` operation head advanced.
    JjOp,
}

/// JSON event streamed over `GET /api/watch` Server-Sent Events.
///
/// # Examples
///
/// ```
/// use tellit_types::{WatchEvent, WatchSource};
///
/// let evt = WatchEvent::Reload { source: WatchSource::JjOp };
/// let json = serde_json::to_string(&evt)?;
/// assert_eq!(json, r#"{"type":"reload","source":"jjOp"}"#);
/// # Ok::<(), serde_json::Error>(())
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum WatchEvent {
    /// Sent immediately when a client subscribes to `/api/watch`.
    Connected,
    /// Working tree or Jujutsu operation state changed; client should reload revisions and diff.
    Reload {
        /// Subsystem that triggered the reload notification.
        source: WatchSource,
    },
    /// `.tellit/` comment threads or approvals changed on disk or via API.
    CommentsChanged,
}

/// Response payload returned by `GET /api/line-count/{*path}`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LineCountResponse {
    /// Total lines at `oldRef` (when requested).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub old_line_count: Option<usize>,
    /// Total lines at `newRef` (when requested).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new_line_count: Option<usize>,
}

/// Special editor preset identifier representing disabled editor launching.
pub const NONE_EDITOR_ID: &str = "none";

/// Special editor preset identifier representing user-defined command and args template.
pub const CUSTOM_EDITOR_ID: &str = "custom";

/// Built-in editor CLI preset recognized by both the server and client settings UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EditorPreset {
    /// Stable lowercase identifier (`vscode`, `cursor`, `zed`, etc.).
    pub id: &'static str,
    /// Human-readable label shown in settings.
    pub label: &'static str,
    /// Executable name launched directly without a shell.
    pub command: &'static str,
    /// Whitespace-separated argument template supporting `%file` and `%line`.
    pub args_template: &'static str,
}

/// Supported built-in editor presets matching the `tellit` CLI defaults.
pub const EDITOR_PRESETS: &[EditorPreset] = &[
    EditorPreset {
        id: "vscode",
        label: "VS Code",
        command: "code",
        args_template: "-g %file:%line",
    },
    EditorPreset {
        id: "cursor",
        label: "Cursor",
        command: "cursor",
        args_template: "-g %file:%line",
    },
    EditorPreset {
        id: "zed",
        label: "Zed",
        command: "zed",
        args_template: "%file:%line",
    },
    EditorPreset {
        id: "textmate",
        label: "TextMate",
        command: "mate",
        args_template: "-l %line %file",
    },
    EditorPreset {
        id: "bbedit",
        label: "BBEdit",
        command: "bbedit",
        args_template: "+%line %file",
    },
    EditorPreset {
        id: "emacs",
        label: "Emacs",
        command: "emacsclient",
        args_template: "--no-wait +%line %file",
    },
    EditorPreset {
        id: "macvim",
        label: "MacVim",
        command: "mvim",
        args_template: "--remote-silent +%line %file",
    },
    EditorPreset {
        id: "sublime",
        label: "Sublime Text",
        command: "subl",
        args_template: "%file:%line",
    },
    EditorPreset {
        id: "nova",
        label: "Nova",
        command: "nova",
        args_template: "open %file -l %line",
    },
    EditorPreset {
        id: "vim",
        label: "Vim / Neovim",
        command: "nvim",
        args_template: "+%line %file",
    },
];

/// Looks up a built-in [`EditorPreset`] by case-insensitive identifier.
#[must_use]
pub fn resolve_editor_preset(id: &str) -> Option<&'static EditorPreset> {
    let normalized = id.trim();
    EDITOR_PRESETS
        .iter()
        .find(|preset| preset.id.eq_ignore_ascii_case(normalized))
}

/// Client editor configuration sent with `POST /api/open-in-editor`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorSpecRequest {
    /// Preset or special identifier (`vscode`, `zed`, `custom`, `none`, etc.).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Custom executable name when `id` is `"custom"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    /// Custom argument template (`%file`, `%line`) when `id` is `"custom"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub args_template: Option<String>,
}

impl EditorSpecRequest {
    /// Constructs an [`EditorSpecRequest`] prefilled from a built-in [`EditorPreset`].
    #[must_use]
    pub fn from_preset(preset: EditorPreset) -> Self {
        Self {
            id: Some(String::from(preset.id)),
            command: Some(String::from(preset.command)),
            args_template: Some(String::from(preset.args_template)),
        }
    }
}

/// Request body accepted by `POST /api/open-in-editor`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenInEditorRequest {
    /// Repository-relative path of the file to open.
    pub file_path: String,
    /// Optional 1-based line number to jump to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<usize>,
    /// Optional editor override from client settings.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub editor: Option<EditorSpecRequest>,
}

/// Response payload returned by `POST /api/open-in-editor`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenInEditorResponse {
    /// `true` once the editor process was spawned.
    pub success: bool,
}

/// Default UI font-family CSS stack when no custom font is selected.
pub const DEFAULT_FONT_FAMILY: &str =
    r#"-apple-system, BlinkMacSystemFont, "Segoe UI", "Noto Sans", Helvetica, Arial, sans-serif"#;

/// User theme selection for light, dark, or OS-matched color schemes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThemePreference {
    /// GitHub-style dark theme.
    #[default]
    Dark,
    /// GitHub-style light theme.
    Light,
    /// Follow the browser's `prefers-color-scheme` media query.
    Auto,
}

impl ThemePreference {
    /// Returns the canonical lowercase string identifier for storage and DOM attributes.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Dark => "dark",
            Self::Light => "light",
            Self::Auto => "auto",
        }
    }
}

/// Color palette preference for diff addition and deletion highlights.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ColorVisionMode {
    /// Standard green addition and red deletion highlights.
    #[default]
    Normal,
    /// Accessible blue addition and orange deletion highlights.
    Deuteranopia,
}

impl ColorVisionMode {
    /// Returns the canonical lowercase string identifier for DOM attributes.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Deuteranopia => "deuteranopia",
        }
    }
}

/// Client appearance, editor preset, and auto-viewed pattern preferences persisted in settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AppearanceSettings {
    /// Base UI font size in pixels, clamped to `10..=20`.
    pub font_size: u8,
    /// CSS font-family stack applied across the interface.
    pub font_family: String,
    /// Theme mode (`dark`, `light`, or `auto`).
    pub theme: ThemePreference,
    /// Color vision palette (`normal` or `deuteranopia`).
    pub color_vision: ColorVisionMode,
    /// Preferred external editor preset or custom command used by `POST /api/open-in-editor`.
    pub editor: EditorSpecRequest,
    /// Glob patterns for files that should start marked as viewed automatically.
    pub auto_viewed_patterns: Vec<String>,
}

impl AppearanceSettings {
    /// Default VS Code editor preset matching `src/server/editor.rs`.
    #[must_use]
    pub fn default_editor() -> EditorSpecRequest {
        EditorSpecRequest {
            id: Some(String::from("vscode")),
            command: Some(String::from("code")),
            args_template: Some(String::from("-g %file:%line")),
        }
    }

    /// Clamps `font_size` into the supported `10..=20` pixel range.
    #[must_use]
    pub fn normalized(mut self) -> Self {
        self.font_size = self.font_size.clamp(10, 20);
        if self.font_family.trim().is_empty() {
            self.font_family = String::from(DEFAULT_FONT_FAMILY);
        }
        self
    }
}

impl Default for AppearanceSettings {
    fn default() -> Self {
        Self {
            font_size: 14,
            font_family: String::from(DEFAULT_FONT_FAMILY),
            theme: ThemePreference::Dark,
            color_vision: ColorVisionMode::Normal,
            editor: Self::default_editor(),
            auto_viewed_patterns: Vec::new(),
        }
    }
}

/// Structured error payload returned by `/api/*` endpoints on failure.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiErrorResponse {
    /// Human-readable error message.
    pub error: String,
}

#[cfg(test)]
mod tests {
    use alloc::string::ToString;

    use super::*;

    #[test]
    fn jj_status_serializes_camel_case_and_omits_missing_fields() {
        let jj = JjStatusResponse::jj("/repo/root");
        let jj_json = serde_json::to_string(&jj).unwrap();
        assert_eq!(jj_json, r#"{"isJj":true,"repoRoot":"/repo/root"}"#);
        assert_eq!(
            serde_json::from_str::<JjStatusResponse>(&jj_json).unwrap(),
            jj
        );

        let with_revs = JjStatusResponse::jj("/repo/root").with_initial_revisions(
            Some(String::from("commit_b")),
            Some(String::from("commit_a")),
        );
        let rev_json = serde_json::to_string(&with_revs).unwrap();
        assert_eq!(
            serde_json::from_str::<JjStatusResponse>(&rev_json).unwrap(),
            with_revs
        );

        let non_jj = JjStatusResponse::not_jj();
        let non_jj_json = serde_json::to_string(&non_jj).unwrap();
        assert_eq!(non_jj_json, r#"{"isJj":false}"#);
        assert_eq!(
            serde_json::from_str::<JjStatusResponse>(&non_jj_json).unwrap(),
            non_jj
        );
    }

    #[test]
    fn heartbeat_event_roundtrips_wire_strings() {
        for event in [HeartbeatEvent::Connected, HeartbeatEvent::Ping] {
            let text = event.to_string();
            assert_eq!(HeartbeatEvent::from_str(&text).unwrap(), event);
        }

        let err = HeartbeatEvent::from_str("unexpected").unwrap_err();
        assert_eq!(err.payload, "unexpected");
        assert_eq!(
            err.to_string(),
            r#"unrecognized heartbeat event payload: "unexpected""#
        );
    }

    #[test]
    fn review_status_precedence_orders_unresolved_above_approvals() {
        assert_eq!(
            ReviewStatus::compute(3, 1, true, true),
            ReviewStatus::HasUnresolved
        );
        assert_eq!(
            ReviewStatus::compute(3, 0, true, true),
            ReviewStatus::Approved
        );
        assert_eq!(
            ReviewStatus::compute(3, 0, false, true),
            ReviewStatus::CleanRebaseApproved
        );
        assert_eq!(
            ReviewStatus::compute(3, 0, false, false),
            ReviewStatus::AllResolved
        );
        assert_eq!(
            ReviewStatus::compute(0, 0, false, false),
            ReviewStatus::None
        );
    }

    #[test]
    fn watch_event_tagged_serialization_roundtrips() {
        let events = [
            WatchEvent::Connected,
            WatchEvent::Reload {
                source: WatchSource::WorkingTree,
            },
            WatchEvent::Reload {
                source: WatchSource::JjOp,
            },
            WatchEvent::CommentsChanged,
        ];
        for event in events {
            let encoded = serde_json::to_string(&event).unwrap();
            let decoded: WatchEvent = serde_json::from_str(&encoded).unwrap();
            assert_eq!(decoded, event);
        }
    }

    #[test]
    fn appearance_settings_defaults_and_roundtrips() {
        let defaults = AppearanceSettings::default();
        let json = serde_json::to_string(&defaults).unwrap();
        let decoded: AppearanceSettings = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, defaults);

        let partial: AppearanceSettings = serde_json::from_str(r#"{"fontSize":99}"#).unwrap();
        let normalized = partial.normalized();
        assert_eq!(normalized.font_size, 20);
        assert_eq!(normalized.theme, ThemePreference::Dark);
    }
}
