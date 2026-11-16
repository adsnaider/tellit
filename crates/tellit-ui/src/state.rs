//! Client-side connection, repository status, and `localStorage` persistence helpers.

extern crate alloc;

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use std::collections::HashSet;

use tellit_types::{HeartbeatEvent, JjStatusResponse};

/// `localStorage` key controlling whether the left sidebar starts open.
pub const SIDEBAR_OPEN_STORAGE_KEY: &str = "difit.sidebarOpen";
/// `localStorage` key storing the desktop sidebar width in pixels.
pub const SIDEBAR_WIDTH_STORAGE_KEY: &str = "difit.sidebarWidth";
/// `localStorage` key controlling whether the revision graph panel is expanded.
pub const REVISIONS_TREE_STORAGE_KEY: &str = "tellit.revisionsTreeOpen";
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

/// Builds a stable DOM element ID for scrolling to a file diff card.
#[must_use]
pub fn file_element_id(path: &str) -> String {
    let mut out = String::from("file-");
    for ch in path.chars() {
        if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
            out.push(ch);
        } else {
            out.push('_');
        }
    }
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
