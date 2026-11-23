//! Client-side connection, repository status, and `localStorage` persistence helpers.

extern crate alloc;

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use std::collections::HashSet;

use tellit_types::{
    AppearanceSettings, CommentSide, DiffFile, FileStatus, HeartbeatEvent, JjCommentThread,
    JjStatusResponse, LineRangeSpec, ThemePreference,
};
use wasm_bindgen::JsCast;

/// Default combined changed-line threshold above which a file card starts auto-collapsed.
pub const LARGE_DIFF_COLLAPSE_THRESHOLD: u32 = 500;

/// Reason a file diff card starts collapsed by default on load.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutoCollapseReason {
    /// Classified as generated or lockfile output.
    Generated,
    /// File was deleted in the target revision.
    Deleted,
    /// Combined additions and deletions exceed [`LARGE_DIFF_COLLAPSE_THRESHOLD`].
    LargeDiff,
}

impl AutoCollapseReason {
    /// Short badge text rendered on the file card header.
    #[must_use]
    pub const fn badge_label(self) -> &'static str {
        match self {
            Self::Generated => "Generated",
            Self::Deleted => "Deleted",
            Self::LargeDiff => "Large diff",
        }
    }

    /// Human-readable prefix used inside collapsed card summary notes.
    #[must_use]
    pub const fn collapsed_prefix(self) -> &'static str {
        match self {
            Self::Generated => "Generated file collapsed",
            Self::Deleted => "Deleted file collapsed",
            Self::LargeDiff => "Large diff collapsed",
        }
    }
}

/// Classifies whether `file` should start collapsed by default and why.
#[must_use]
pub fn auto_collapse_reason(file: &DiffFile) -> Option<AutoCollapseReason> {
    if file.path == "Commit Message" {
        return None;
    }
    if file.is_generated {
        return Some(AutoCollapseReason::Generated);
    }
    if file.status == FileStatus::Deleted {
        return Some(AutoCollapseReason::Deleted);
    }
    if file.additions.saturating_add(file.deletions) > LARGE_DIFF_COLLAPSE_THRESHOLD {
        return Some(AutoCollapseReason::LargeDiff);
    }
    None
}

/// `localStorage` key controlling whether the left sidebar starts open.
pub const SIDEBAR_OPEN_STORAGE_KEY: &str = "difit.sidebarOpen";
/// `localStorage` key storing the desktop sidebar width in pixels.
pub const SIDEBAR_WIDTH_STORAGE_KEY: &str = "difit.sidebarWidth";
/// `localStorage` key controlling whether the revision graph panel is expanded.
pub const REVISIONS_TREE_STORAGE_KEY: &str = "tellit.revisionsTreeOpen";
/// `localStorage` key persisting unified vs. split diff presentation mode.
pub const DIFF_MODE_STORAGE_KEY: &str = "tellit.diffViewMode";
/// `localStorage` key controlling whether resolved comment threads appear inline in diffs.
pub const SHOW_RESOLVED_INLINE_STORAGE_KEY: &str = "tellit.showResolvedInline";
/// `localStorage` key caching client appearance and editor settings.
pub const APPEARANCE_STORAGE_KEY: &str = "tellit.appearanceSettings";
/// Minimum sidebar width in pixels.
pub const SIDEBAR_MIN_WIDTH: u32 = 200;
/// Maximum sidebar width in pixels.
pub const SIDEBAR_MAX_WIDTH: u32 = 600;
/// Default sidebar width in pixels.
pub const SIDEBAR_DEFAULT_WIDTH: u32 = 300;

/// Browser-to-server `/api/heartbeat` SSE connection state.
///
/// # Examples
///
/// ```
/// use tellit_types::HeartbeatEvent;
/// use tellit_ui::ConnectionState;
///
/// let state = ConnectionState::Connecting.on_heartbeat(HeartbeatEvent::Ping);
/// assert_eq!(state, ConnectionState::Live);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionState {
    /// Initial state while opening `/api/heartbeat`.
    Connecting,
    /// Connected to `/api/heartbeat` and receiving server events.
    Live,
    /// Heartbeat stream closed or encountered a transport error.
    Offline,
}

impl ConnectionState {
    /// Transitions connection state upon receiving a [`HeartbeatEvent`].
    ///
    /// # Examples
    ///
    /// ```
    /// use tellit_types::HeartbeatEvent;
    /// use tellit_ui::ConnectionState;
    ///
    /// assert_eq!(
    ///     ConnectionState::Connecting.on_heartbeat(HeartbeatEvent::Connected),
    ///     ConnectionState::Live
    /// );
    /// ```
    #[must_use]
    pub const fn on_heartbeat(self, _event: HeartbeatEvent) -> Self {
        Self::Live
    }

    /// Short human-readable status label for the top bar indicator.
    ///
    /// # Examples
    ///
    /// ```
    /// use tellit_ui::ConnectionState;
    ///
    /// assert_eq!(ConnectionState::Connecting.label(), "Connecting");
    /// assert_eq!(ConnectionState::Live.label(), "Live");
    /// assert_eq!(ConnectionState::Offline.label(), "Offline");
    /// ```
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Connecting => "Connecting",
            Self::Live => "Live",
            Self::Offline => "Offline",
        }
    }

    /// CSS class modifier applied to the connection pill in the header.
    ///
    /// # Examples
    ///
    /// ```
    /// use tellit_ui::ConnectionState;
    ///
    /// assert_eq!(ConnectionState::Live.badge_class(), "badge badge--live");
    /// ```
    #[must_use]
    pub const fn badge_class(self) -> &'static str {
        match self {
            Self::Connecting => "badge badge--connecting",
            Self::Live => "badge badge--live",
            Self::Offline => "badge badge--offline",
        }
    }
}

/// View state for the initial `GET /api/jj/status` probe.
///
/// # Examples
///
/// ```
/// use tellit_types::JjStatusResponse;
/// use tellit_ui::RepoStatusState;
///
/// let state = RepoStatusState::Loaded(JjStatusResponse::not_jj());
/// assert_eq!(state.badge_label(), "non-jj directory");
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RepoStatusState {
    /// Request to `/api/jj/status` is in flight.
    Loading,
    /// Server returned a [`JjStatusResponse`].
    Loaded(JjStatusResponse),
    /// Request to `/api/jj/status` failed with an error message.
    Error(String),
}

impl RepoStatusState {
    /// Summary label shown in the top bar repository badge.
    ///
    /// # Examples
    ///
    /// ```
    /// use tellit_types::JjStatusResponse;
    /// use tellit_ui::RepoStatusState;
    ///
    /// assert_eq!(RepoStatusState::Loading.badge_label(), "Checking workspace...");
    /// assert_eq!(
    ///     RepoStatusState::Loaded(JjStatusResponse::jj("/repo")).badge_label(),
    ///     "jj workspace"
    /// );
    /// ```
    #[must_use]
    pub fn badge_label(&self) -> &'static str {
        match self {
            Self::Loading => "Checking workspace...",
            Self::Loaded(status) if status.is_jj => "jj workspace",
            Self::Loaded(_) => "non-jj directory",
            Self::Error(_) => "Status unavailable",
        }
    }
}

/// Clamps sidebar width into the supported desktop resize bounds.
#[must_use]
pub const fn clamp_sidebar_width(width: u32) -> u32 {
    if width < SIDEBAR_MIN_WIDTH {
        SIDEBAR_MIN_WIDTH
    } else if width > SIDEBAR_MAX_WIDTH {
        SIDEBAR_MAX_WIDTH
    } else {
        width
    }
}

fn push_sanitized_id(out: &mut String, raw: &str) {
    for ch in raw.chars() {
        if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
            out.push(ch);
        } else {
            out.push('_');
        }
    }
}

/// Builds a stable DOM element ID for scrolling to a file diff card.
#[must_use]
pub fn file_element_id(path: &str) -> String {
    let mut out = String::from("file-");
    push_sanitized_id(&mut out, path);
    out
}

/// Builds a stable DOM element ID for scrolling to one line inside a file diff.
#[must_use]
pub fn diff_line_element_id(
    path: &str,
    chunk_index: usize,
    line_index: usize,
    side: CommentSide,
) -> String {
    let side_tag = match side {
        CommentSide::Old => "old",
        CommentSide::New => "new",
    };
    let mut out = String::from("diffline-");
    push_sanitized_id(&mut out, path);
    out.push('-');
    out.push_str(&alloc::format!("{chunk_index}-{line_index}-{side_tag}"));
    out
}

fn local_storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok()?
}

/// Reads a boolean preference from browser `localStorage`, returning `default` if absent.
#[must_use]
pub fn load_storage_bool(key: &str, default: bool) -> bool {
    let Some(storage) = local_storage() else {
        return default;
    };
    match storage.get_item(key).ok().flatten().as_deref() {
        Some("true") => true,
        Some("false") => false,
        _ => default,
    }
}

/// Writes a boolean preference into browser `localStorage`, ignoring storage errors.
pub fn save_storage_bool(key: &str, value: bool) {
    if let Some(storage) = local_storage() {
        let _ = storage.set_item(key, if value { "true" } else { "false" });
    }
}

/// Reads an unsigned integer preference from browser `localStorage`.
#[must_use]
pub fn load_storage_u32(key: &str, default: u32) -> u32 {
    let Some(storage) = local_storage() else {
        return default;
    };
    storage
        .get_item(key)
        .ok()
        .flatten()
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(default)
}

/// Writes an unsigned integer preference into browser `localStorage`.
pub fn save_storage_u32(key: &str, value: u32) {
    if let Some(storage) = local_storage() {
        let _ = storage.set_item(key, &value.to_string());
    }
}

/// Builds the `localStorage` key storing reviewed file paths for a repository and target commit.
#[must_use]
pub fn viewed_files_storage_key(repo_root: &str, target_commit: &str) -> String {
    alloc::format!("tellit.viewedFiles::{repo_root}::{target_commit}")
}

/// Loads the set of reviewed file paths persisted in `localStorage` for `(repo_root, target_commit)`.
#[must_use]
pub fn load_viewed_files(repo_root: &str, target_commit: &str) -> HashSet<String> {
    if target_commit.is_empty() {
        return HashSet::new();
    }
    let Some(storage) = local_storage() else {
        return HashSet::new();
    };
    let key = viewed_files_storage_key(repo_root, target_commit);
    let Some(raw) = storage.get_item(&key).ok().flatten() else {
        return HashSet::new();
    };
    serde_json::from_str::<Vec<String>>(&raw)
        .unwrap_or_default()
        .into_iter()
        .collect()
}

/// Persists the set of reviewed file paths into `localStorage` for `(repo_root, target_commit)`.
pub fn save_viewed_files(repo_root: &str, target_commit: &str, viewed: &HashSet<String>) {
    if target_commit.is_empty() {
        return;
    }
    let Some(storage) = local_storage() else {
        return;
    };
    let mut paths: Vec<&str> = viewed.iter().map(String::as_str).collect();
    paths.sort_unstable();
    if let Ok(json) = serde_json::to_string(&paths) {
        let key = viewed_files_storage_key(repo_root, target_commit);
        let _ = storage.set_item(&key, &json);
    }
}

/// Diff layout mode rendered by [`crate::components::text_diff::TextDiffViewer`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DiffViewMode {
    /// Unified single-column interleaved diff table.
    #[default]
    Unified,
    /// Side-by-side split old/new diff table.
    Split,
}

impl DiffViewMode {
    /// Parses a persisted view mode name, falling back to [`DiffViewMode::Unified`].
    #[must_use]
    pub fn from_storage(raw: &str) -> Self {
        if raw.trim().eq_ignore_ascii_case("split") {
            Self::Split
        } else {
            Self::Unified
        }
    }

    /// Stable string persisted into `localStorage`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Unified => "unified",
            Self::Split => "split",
        }
    }
}

/// Loads the saved [`DiffViewMode`] from `localStorage`.
#[must_use]
pub fn load_diff_view_mode() -> DiffViewMode {
    local_storage()
        .and_then(|s| s.get_item(DIFF_MODE_STORAGE_KEY).ok().flatten())
        .map_or(DiffViewMode::Unified, |raw| {
            DiffViewMode::from_storage(&raw)
        })
}

/// Saves the selected [`DiffViewMode`] to `localStorage`.
pub fn save_diff_view_mode(mode: DiffViewMode) {
    if let Some(storage) = local_storage() {
        let _ = storage.set_item(DIFF_MODE_STORAGE_KEY, mode.as_str());
    }
}

/// Builds a stable DOM element ID for scrolling directly to a review comment thread card.
#[must_use]
pub fn thread_element_id(thread_id: &str) -> String {
    let mut out = String::from("comment-thread-");
    push_sanitized_id(&mut out, thread_id);
    out
}

/// Formats the short line label for a thread (`L12`, `L12-L15`, or `file`).
#[must_use]
pub fn format_thread_line_label(range: Option<LineRangeSpec>) -> String {
    match range {
        Some(lr) if lr.start == lr.end => alloc::format!("L{}", lr.start),
        Some(lr) => alloc::format!("L{}-L{}", lr.start, lr.end),
        None => String::from("file"),
    }
}

/// Formats `file:line (old)?` for display in headers and copied agent prompts.
#[must_use]
pub fn format_thread_location(thread: &JjCommentThread) -> String {
    let file = if thread.file_path.is_empty() {
        "<unknown file>"
    } else {
        thread.file_path.as_str()
    };
    let line_label = format_thread_line_label(thread.line_range);
    let side_suffix = if thread.side == CommentSide::Old {
        " (old)"
    } else {
        ""
    };
    alloc::format!("{file}:{line_label}{side_suffix}")
}

/// Parsed segment inside a Markdown comment body separating prose from `suggestion` code fences.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BodySegment {
    /// Plain prose outside a fenced suggestion block.
    Text(String),
    /// Replacement code lines extracted from a ```` ```suggestion ```` block.
    Suggestion(String),
}

/// Splits a comment body into alternating prose and fenced ```` ```suggestion ```` segments.
#[must_use]
pub fn parse_body_segments(body: &str) -> Vec<BodySegment> {
    if !body.contains("```suggestion") {
        return alloc::vec![BodySegment::Text(String::from(body))];
    }

    let mut segments = Vec::new();
    let mut rest = body;

    while let Some(start) = rest.find("```suggestion") {
        let before = &rest[..start];
        if !before.trim().is_empty() {
            segments.push(BodySegment::Text(String::from(before.trim_end())));
        }

        let after_tag = &rest[start + "```suggestion".len()..];
        let content_start = after_tag.find('\n').map_or(0, |idx| idx + 1);
        let candidate = &after_tag[content_start..];
        let Some(end) = candidate.find("\n```").or_else(|| candidate.find("```")) else {
            segments.push(BodySegment::Text(String::from(&rest[start..])));
            return segments;
        };

        let suggested = candidate[..end].trim_end_matches('\r');
        segments.push(BodySegment::Suggestion(String::from(suggested)));

        let close_len = if candidate[end..].starts_with("\n```") {
            4
        } else {
            3
        };
        rest = &candidate[end + close_len..];
    }

    if !rest.trim().is_empty() {
        segments.push(BodySegment::Text(String::from(rest.trim_start())));
    }
    if segments.is_empty() {
        segments.push(BodySegment::Text(String::from(body)));
    }
    segments
}

fn format_comment_body_for_prompt(body: &str, code_snippet: Option<&str>) -> String {
    if !body.contains("```suggestion") {
        return String::from(body);
    }

    let mut parts = Vec::new();
    for segment in parse_body_segments(body) {
        match segment {
            BodySegment::Text(text) => {
                let trimmed = text.trim();
                if !trimmed.is_empty() {
                    parts.push(String::from(trimmed));
                }
            }
            BodySegment::Suggestion(suggested) => {
                if let Some(orig) = code_snippet.filter(|s| !s.is_empty()) {
                    parts.push(alloc::format!("ORIGINAL:\n```\n{orig}\n```"));
                }
                parts.push(alloc::format!("SUGGESTED:\n```\n{suggested}\n```"));
            }
        }
    }
    parts.join("\n")
}

/// Formats one review thread (location, root message, suggestion blocks, and replies) for agent copying.
#[must_use]
pub fn format_thread_prompt(thread: &JjCommentThread, code_snippet: Option<&str>) -> String {
    let mut sections = alloc::vec![format_thread_location(thread)];
    for (idx, body) in thread.comments.iter().enumerate() {
        if idx == 0 {
            sections.push(format_comment_body_for_prompt(body, code_snippet));
        } else {
            sections.push(alloc::format!("Reply {idx}"));
            sections.push(format_comment_body_for_prompt(body, code_snippet));
        }
    }
    sections
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Formats all provided comment threads with diff revision header for pasting into an AI agent.
#[must_use]
pub fn format_all_comments_prompt(
    threads: &[JjCommentThread],
    target_commit: &str,
    base_commit: &str,
) -> String {
    if threads.is_empty() {
        return String::new();
    }
    let body = threads
        .iter()
        .map(|t| format_thread_prompt(t, None))
        .collect::<Vec<_>>()
        .join("\n=====\n");

    if !target_commit.is_empty() && !base_commit.is_empty() {
        let short_base: String = base_commit.chars().take(12).collect();
        let short_target: String = target_commit.chars().take(12).collect();
        alloc::format!("diff {short_base}..{short_target}\n\n{body}")
    } else {
        body
    }
}

/// Concrete light or dark theme active after resolving [`ThemePreference::Auto`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolvedTheme {
    /// Dark color palette.
    Dark,
    /// Light color palette.
    Light,
}

impl ResolvedTheme {
    /// Returns the attribute value written to `document.documentElement` (`"dark"` or `"light"`).
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Dark => "dark",
            Self::Light => "light",
        }
    }
}

/// Loads cached [`AppearanceSettings`] from `localStorage`, falling back to defaults.
#[must_use]
pub fn load_appearance_settings() -> AppearanceSettings {
    local_storage()
        .and_then(|s| s.get_item(APPEARANCE_STORAGE_KEY).ok().flatten())
        .and_then(|raw| serde_json::from_str::<AppearanceSettings>(&raw).ok())
        .map(AppearanceSettings::normalized)
        .unwrap_or_default()
}

/// Persists `settings` into `localStorage`.
pub fn save_appearance_settings(settings: &AppearanceSettings) {
    if let (Some(storage), Ok(json)) = (local_storage(), serde_json::to_string(settings)) {
        let _ = storage.set_item(APPEARANCE_STORAGE_KEY, &json);
    }
}

/// Resolves `pref` into either [`ResolvedTheme::Dark`] or [`ResolvedTheme::Light`].
#[must_use]
pub fn resolve_theme_preference(pref: ThemePreference) -> ResolvedTheme {
    match pref {
        ThemePreference::Dark => ResolvedTheme::Dark,
        ThemePreference::Light => ResolvedTheme::Light,
        ThemePreference::Auto => {
            let prefers_dark = web_sys::window()
                .and_then(|w| w.match_media("(prefers-color-scheme: dark)").ok().flatten())
                .is_none_or(|mq| mq.matches());
            if prefers_dark {
                ResolvedTheme::Dark
            } else {
                ResolvedTheme::Light
            }
        }
    }
}

/// Applies theme, color vision mode, and font CSS variables onto `document.documentElement`.
pub fn apply_appearance_to_document(settings: &AppearanceSettings) -> ResolvedTheme {
    let resolved = resolve_theme_preference(settings.theme);
    let Some(document) = web_sys::window().and_then(|w| w.document()) else {
        return resolved;
    };
    let Some(root) = document.document_element() else {
        return resolved;
    };
    let _ = root.set_attribute("data-theme", resolved.as_str());
    let _ = root.set_attribute("data-color-vision", settings.color_vision.as_str());

    if let Some(html_el) = root.dyn_ref::<web_sys::HtmlElement>() {
        let style = html_el.style();
        let clamped_size = settings.font_size.clamp(10, 20);
        let _ = style.set_property("--app-font-size", &alloc::format!("{clamped_size}px"));
        if !settings.font_family.trim().is_empty() {
            let _ = style.set_property("--app-font-family", settings.font_family.trim());
        }
    }
    resolved
}

/// Returns `true` when `filename` ends with a Markdown extension (`.md`, `.markdown`).
#[must_use]
pub fn is_markdown_file(filename: &str) -> bool {
    let ext = filename.rsplit('.').next().unwrap_or("");
    filename.contains('.')
        && (ext.eq_ignore_ascii_case("md") || ext.eq_ignore_ascii_case("markdown"))
}

/// Returns `true` when `filename` ends with a recognized raster or vector image extension.
#[must_use]
pub fn is_image_file(filename: &str) -> bool {
    if !filename.contains('.') {
        return false;
    }
    let ext = filename.rsplit('.').next().unwrap_or("");
    matches!(
        ext.to_ascii_lowercase().as_str(),
        "jpg"
            | "jpeg"
            | "png"
            | "gif"
            | "bmp"
            | "svg"
            | "webp"
            | "ico"
            | "tiff"
            | "tif"
            | "avif"
            | "heic"
            | "heif"
    )
}

/// Parses newline-separated auto-viewed glob patterns, ignoring empty lines and `#` comments.
#[must_use]
pub fn parse_auto_viewed_patterns(raw: &str) -> Vec<String> {
    raw.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(String::from)
        .collect()
}

/// Formats auto-viewed glob patterns into newline-delimited editor text.
#[must_use]
pub fn format_auto_viewed_patterns(patterns: &[String]) -> String {
    patterns.join("\n")
}

/// Returns `true` when `file_path` matches at least one glob pattern in `patterns`.
#[must_use]
pub fn matches_auto_viewed_patterns(file_path: &str, patterns: &[String]) -> bool {
    let basename = file_path.rsplit('/').next().unwrap_or(file_path);
    patterns.iter().any(|pattern| {
        let trimmed = pattern.trim();
        if trimmed.is_empty() {
            return false;
        }
        if !trimmed.contains('/') {
            glob_match(trimmed, basename) || glob_match(trimmed, file_path)
        } else {
            glob_match(trimmed, file_path)
        }
    })
}

fn glob_match(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    glob_match_chars(&p, &t)
}

fn glob_match_chars(pattern: &[char], text: &[char]) -> bool {
    match pattern {
        [] => text.is_empty(),
        ['*', '*', '/', rest @ ..] => {
            if glob_match_chars(rest, text) {
                return true;
            }
            for idx in 0..text.len() {
                if text[idx] == '/' && glob_match_chars(rest, &text[idx + 1..]) {
                    return true;
                }
            }
            false
        }
        ['*', '*', rest @ ..] => (0..=text.len()).any(|i| glob_match_chars(rest, &text[i..])),
        ['*', rest @ ..] => {
            for i in 0..=text.len() {
                if glob_match_chars(rest, &text[i..]) {
                    return true;
                }
                if i < text.len() && text[i] == '/' {
                    break;
                }
            }
            false
        }
        ['?', rest @ ..] => {
            !text.is_empty() && text[0] != '/' && glob_match_chars(rest, &text[1..])
        }
        [ch, rest @ ..] => !text.is_empty() && text[0] == *ch && glob_match_chars(rest, &text[1..]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diff_view_mode_parses_and_roundtrips() {
        assert_eq!(DiffViewMode::from_storage("split"), DiffViewMode::Split);
        assert_eq!(DiffViewMode::from_storage("SPLIT"), DiffViewMode::Split);
        assert_eq!(DiffViewMode::from_storage("unified"), DiffViewMode::Unified);
        assert_eq!(DiffViewMode::from_storage("other"), DiffViewMode::Unified);
        assert_eq!(DiffViewMode::Split.as_str(), "split");
    }

    #[test]
    fn formats_thread_and_all_comments_prompts_with_suggestions() {
        let thread = JjCommentThread {
            id: String::from("t1"),
            change_id: String::from("chg1"),
            commit_id: String::from("111122223333"),
            file_path: String::from("src/main.rs"),
            side: CommentSide::New,
            line_range: Some(LineRangeSpec { start: 10, end: 12 }),
            comments: alloc::vec![
                String::from("Please inline this:\n```suggestion\nlet x = 42;\n```"),
                String::from("Done!"),
            ],
            resolved: false,
        };
        let single = format_thread_prompt(&thread, Some("let x = 1;"));
        assert!(single.contains("src/main.rs:L10-L12\n"));
        assert!(single.contains("ORIGINAL:\n```\nlet x = 1;\n```"));
        assert!(single.contains("SUGGESTED:\n```\nlet x = 42;\n```"));
        assert!(single.contains("Reply 1\nDone!"));

        let all = format_all_comments_prompt(
            core::slice::from_ref(&thread),
            "aaaa1111bbbb2222",
            "cccc3333dddd4444",
        );
        assert!(all.starts_with("diff cccc3333dddd..aaaa1111bbbb\n\n"));
    }

    #[test]
    fn detects_markdown_and_image_file_extensions() {
        assert!(is_markdown_file("README.md"));
        assert!(is_markdown_file("docs/GUIDE.MARKDOWN"));
        assert!(!is_markdown_file("src/main.rs"));

        assert!(is_image_file("assets/logo.PNG"));
        assert!(is_image_file("icon.svg"));
        assert!(!is_image_file("README.md"));
    }

    #[test]
    fn matches_auto_viewed_glob_patterns() {
        let raw = "*.lock\n  src/generated/** \n# ignore comment\n*.stories.tsx\n";
        let patterns = parse_auto_viewed_patterns(raw);
        assert_eq!(
            patterns,
            vec!["*.lock", "src/generated/**", "*.stories.tsx"]
        );

        assert!(matches_auto_viewed_patterns("Cargo.lock", &patterns));
        assert!(matches_auto_viewed_patterns("nested/foo.lock", &patterns));
        assert!(matches_auto_viewed_patterns(
            "src/generated/types/api.rs",
            &patterns
        ));
        assert!(matches_auto_viewed_patterns(
            "ui/Button.stories.tsx",
            &patterns
        ));
        assert!(!matches_auto_viewed_patterns("src/main.rs", &patterns));
    }

    #[test]
    fn auto_collapse_rules_classify_generated_deleted_and_large_files() {
        let base_file =
            |path: &str, status: FileStatus, add: u32, del: u32, is_gen: bool| DiffFile {
                path: String::from(path),
                old_path: None,
                status,
                additions: add,
                deletions: del,
                is_generated: is_gen,
                chunks: Vec::new(),
                chunks_loaded: false,
            };

        assert_eq!(
            auto_collapse_reason(&base_file(
                "Commit Message",
                FileStatus::Modified,
                900,
                0,
                false
            )),
            None
        );
        assert_eq!(
            auto_collapse_reason(&base_file("Cargo.lock", FileStatus::Deleted, 0, 900, true)),
            Some(AutoCollapseReason::Generated)
        );
        assert_eq!(
            auto_collapse_reason(&base_file("old.rs", FileStatus::Deleted, 0, 12, false)),
            Some(AutoCollapseReason::Deleted)
        );
        assert_eq!(
            auto_collapse_reason(&base_file("big.rs", FileStatus::Modified, 300, 201, false)),
            Some(AutoCollapseReason::LargeDiff)
        );
        assert_eq!(
            auto_collapse_reason(&base_file(
                "exact.rs",
                FileStatus::Modified,
                250,
                250,
                false
            )),
            None
        );
        assert_eq!(
            auto_collapse_reason(&base_file("small.rs", FileStatus::Added, 40, 0, false)),
            None
        );
    }
}
