//! Client-side URL routing for Gerrit-style change overview and single-file diff views.

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

use wasm_bindgen::JsValue;

/// Canonical virtual file path for the commit message entry.
pub const COMMIT_MESSAGE_PATH: &str = "Commit Message";
/// Compact URL slug representing [`COMMIT_MESSAGE_PATH`] in single-file URLs.
pub const COMMIT_MESSAGE_URL_SLUG: &str = "COMMIT_MSG";

/// Optional non-default comparison query parameters preserved across change and file URLs.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RouteQuery {
    /// Explicit target commit ID when inspecting a non-head revision of the change.
    pub target: Option<String>,
    /// Explicit base commit ID when viewing an interdiff (omitted when comparing against parent).
    pub base: Option<String>,
    /// Explicitly disabled rebase isolation (`Some(false)` emits `ignoreRebase=false`).
    pub ignore_rebase: Option<bool>,
}

impl RouteQuery {
    /// Builds a clean URL query from UI comparison state, omitting default values.
    #[must_use]
    pub fn from_ui_state(
        target: Option<&str>,
        head_commit: Option<&str>,
        base: &str,
        ignore_rebase: bool,
    ) -> Self {
        let target_param = target
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .filter(|t| !head_commit.is_some_and(|head| commit_ids_equivalent(head, t)))
            .map(String::from);

        let trimmed_base = base.trim();
        let base_param = (!trimmed_base.is_empty() && trimmed_base != "parent")
            .then(|| String::from(trimmed_base));

        let ignore_rebase_param = (!ignore_rebase).then_some(false);

        Self {
            target: target_param,
            base: base_param,
            ignore_rebase: ignore_rebase_param,
        }
    }

    /// Returns `true` when no non-default query parameters are set.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.target.is_none() && self.base.is_none() && !matches!(self.ignore_rebase, Some(false))
    }
}

/// Parsed client-side application route.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppRoute {
    /// Root landing (`/`) or unrecognized path; redirects to the default change overview.
    Root,
    /// Change overview (`/c/<change_id>`) showing commit message and file table with inline expand.
    ChangeOverview {
        /// Jujutsu change ID addressed by the URL.
        change_id: String,
        /// Optional non-default comparison query parameters.
        query: RouteQuery,
    },
    /// Dedicated single-file diff view (`/c/<change_id>/file/<encoded_path>`).
    FileDiff {
        /// Jujutsu change ID addressed by the URL.
        change_id: String,
        /// Decoded repository-relative file path (or [`COMMIT_MESSAGE_PATH`]).
        file_path: String,
        /// Optional non-default comparison query parameters.
        query: RouteQuery,
    },
}

impl AppRoute {
    /// Returns the change ID addressed by this route, if any.
    #[must_use]
    pub fn change_id(&self) -> Option<&str> {
        match self {
            Self::Root => None,
            Self::ChangeOverview { change_id, .. } | Self::FileDiff { change_id, .. } => {
                Some(change_id.as_str())
            }
        }
    }

    /// Returns the active file path when this route is [`Self::FileDiff`].
    #[must_use]
    pub fn file_path(&self) -> Option<&str> {
        match self {
            Self::FileDiff { file_path, .. } => Some(file_path.as_str()),
            Self::Root | Self::ChangeOverview { .. } => None,
        }
    }

    /// Returns the query parameters attached to this route, if any.
    #[must_use]
    pub fn query(&self) -> Option<&RouteQuery> {
        match self {
            Self::Root => None,
            Self::ChangeOverview { query, .. } | Self::FileDiff { query, .. } => Some(query),
        }
    }

    /// Formats this route as a root-absolute URL path plus optional query string.
    #[must_use]
    pub fn to_url(&self) -> String {
        match self {
            Self::Root => String::from("/"),
            Self::ChangeOverview { change_id, query } => {
                let encoded_id = encode_path_segment(change_id);
                let qs = format_query(query);
                alloc::format!("/c/{encoded_id}{qs}")
            }
            Self::FileDiff {
                change_id,
                file_path,
                query,
            } => {
                let encoded_id = encode_path_segment(change_id);
                let encoded_path = encode_file_path_for_url(file_path);
                let qs = format_query(query);
                alloc::format!("/c/{encoded_id}/file/{encoded_path}{qs}")
            }
        }
    }
}

/// Parses browser `pathname` and `search` (`?k=v...`) strings into an [`AppRoute`].
#[must_use]
pub fn parse_route(pathname: &str, search: &str) -> AppRoute {
    let trimmed = pathname.trim_start_matches('/');
    if trimmed.is_empty() {
        return AppRoute::Root;
    }

    let Some(after_c) = trimmed.strip_prefix("c/") else {
        return AppRoute::Root;
    };

    let query = parse_query(search);

    if let Some((raw_change_id, rest)) = after_c.split_once('/') {
        let Some(change_id) = decode_percent_segment(raw_change_id).filter(|s| !s.is_empty())
        else {
            return AppRoute::Root;
        };

        if rest.is_empty() {
            return AppRoute::ChangeOverview { change_id, query };
        }

        if let Some(raw_file) = rest.strip_prefix("file/") {
            if let Some(file_path) = decode_file_path_from_url(raw_file) {
                return AppRoute::FileDiff {
                    change_id,
                    file_path,
                    query,
                };
            }
            return AppRoute::ChangeOverview { change_id, query };
        }

        return AppRoute::Root;
    }

    let Some(change_id) = decode_percent_segment(after_c).filter(|s| !s.is_empty()) else {
        return AppRoute::Root;
    };

    AppRoute::ChangeOverview { change_id, query }
}

/// Encodes a repository-relative file path for `/c/<id>/file/<path>`, keeping `/` readable
/// while percent-encoding reserved characters and mapping `"Commit Message"` to `COMMIT_MSG`.
#[must_use]
pub fn encode_file_path_for_url(file_path: &str) -> String {
    if file_path == COMMIT_MESSAGE_PATH {
        return String::from(COMMIT_MESSAGE_URL_SLUG);
    }
    file_path
        .split('/')
        .map(encode_path_segment)
        .collect::<Vec<_>>()
        .join("/")
}

/// Decodes a file path from `/c/<id>/file/<encoded>`, normalizing `COMMIT_MSG` and
/// `Commit%20Message` back to `"Commit Message"`.
#[must_use]
pub fn decode_file_path_from_url(encoded: &str) -> Option<String> {
    if encoded.is_empty() {
        return None;
    }
    let mut segments = Vec::new();
    for raw_seg in encoded.split('/') {
        if raw_seg.is_empty() {
            return None;
        }
        let decoded = decode_percent_segment(raw_seg)?;
        if decoded.is_empty() || decoded == "." || decoded == ".." {
            return None;
        }
        segments.push(decoded);
    }
    if segments.is_empty() {
        return None;
    }
    let joined = segments.join("/");
    if joined == COMMIT_MESSAGE_URL_SLUG || joined == COMMIT_MESSAGE_PATH {
        return Some(String::from(COMMIT_MESSAGE_PATH));
    }
    Some(joined)
}

/// Percent-encodes a single URL path segment or query value using RFC 3986 unreserved characters.
#[must_use]
pub fn encode_path_segment(raw: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = String::with_capacity(raw.len());
    for byte in raw.bytes() {
        if is_unreserved_byte(byte) {
            out.push(char::from(byte));
        } else {
            out.push('%');
            out.push(char::from(HEX[usize::from(byte >> 4)]));
            out.push(char::from(HEX[usize::from(byte & 0x0F)]));
        }
    }
    out
}

fn is_unreserved_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~')
}

fn hex_nibble(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

fn decode_percent_inner(raw: &str, plus_as_space: bool) -> Option<String> {
    let bytes = raw.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0usize;
    while i < bytes.len() {
        match bytes[i] {
            b'%' => {
                if i + 2 >= bytes.len() {
                    return None;
                }
                let hi = hex_nibble(bytes[i + 1])?;
                let lo = hex_nibble(bytes[i + 2])?;
                out.push((hi << 4) | lo);
                i += 3;
            }
            b'+' if plus_as_space => {
                out.push(b' ');
                i += 1;
            }
            other => {
                out.push(other);
                i += 1;
            }
        }
    }
    String::from_utf8(out).ok()
}

fn decode_percent_segment(raw: &str) -> Option<String> {
    decode_percent_inner(raw, false)
}

fn decode_query_value(raw: &str) -> Option<String> {
    decode_percent_inner(raw, true)
}

fn parse_query(search: &str) -> RouteQuery {
    let trimmed = search.strip_prefix('?').unwrap_or(search);
    if trimmed.is_empty() {
        return RouteQuery::default();
    }

    let mut out = RouteQuery::default();
    for pair in trimmed.split('&') {
        if pair.is_empty() {
            continue;
        }
        let (raw_key, raw_val) = pair.split_once('=').unwrap_or((pair, ""));
        let Some(key) = decode_query_value(raw_key) else {
            continue;
        };
        let Some(val) = decode_query_value(raw_val) else {
            continue;
        };
        let val_trimmed = val.trim();
        match key.as_str() {
            "target" | "commit" if !val_trimmed.is_empty() => {
                out.target = Some(String::from(val_trimmed));
            }
            "base" if !val_trimmed.is_empty() && val_trimmed != "parent" => {
                out.base = Some(String::from(val_trimmed));
            }
            "ignoreRebase" => {
                if val_trimmed.eq_ignore_ascii_case("false") || val_trimmed == "0" {
                    out.ignore_rebase = Some(false);
                } else if val_trimmed.eq_ignore_ascii_case("true") || val_trimmed == "1" {
                    out.ignore_rebase = Some(true);
                }
            }
            _ => {}
        }
    }
    out
}

fn format_query(query: &RouteQuery) -> String {
    if query.is_empty() {
        return String::new();
    }
    let mut parts = Vec::new();
    if let Some(ref target) = query.target {
        let trimmed = target.trim();
        if !trimmed.is_empty() {
            parts.push(alloc::format!("target={}", encode_path_segment(trimmed)));
        }
    }
    if let Some(ref base) = query.base {
        let trimmed = base.trim();
        if !trimmed.is_empty() && trimmed != "parent" {
            parts.push(alloc::format!("base={}", encode_path_segment(trimmed)));
        }
    }
    if query.ignore_rebase == Some(false) {
        parts.push(String::from("ignoreRebase=false"));
    }
    if parts.is_empty() {
        String::new()
    } else {
        alloc::format!("?{}", parts.join("&"))
    }
}

fn commit_ids_equivalent(a: &str, b: &str) -> bool {
    !a.is_empty() && !b.is_empty() && (a == b || a.starts_with(b) || b.starts_with(a))
}

/// Reads and parses the current browser `window.location` into an [`AppRoute`].
#[must_use]
pub fn current_browser_route() -> AppRoute {
    let Some(window) = web_sys::window() else {
        return AppRoute::Root;
    };
    let location = window.location();
    let pathname = location.pathname().unwrap_or_default();
    let search = location.search().unwrap_or_default();
    parse_route(&pathname, &search)
}

fn current_browser_path_and_search() -> Option<String> {
    let window = web_sys::window()?;
    let location = window.location();
    let pathname = location.pathname().ok()?;
    let search = location.search().ok().unwrap_or_default();
    Some(alloc::format!("{pathname}{search}"))
}

/// Navigates the browser URL via `history.pushState` when `route` differs from the current URL.
pub fn push_browser_route(route: &AppRoute) {
    let target_url = route.to_url();
    if current_browser_path_and_search().as_deref() == Some(target_url.as_str()) {
        return;
    }
    if let Some(history) = web_sys::window().and_then(|w| w.history().ok()) {
        let _ = history.push_state_with_url(&JsValue::NULL, "", Some(&target_url));
    }
}

/// Replaces the current browser history entry via `history.replaceState` when `route` differs.
pub fn replace_browser_route(route: &AppRoute) {
    let target_url = route.to_url();
    if current_browser_path_and_search().as_deref() == Some(target_url.as_str()) {
        return;
    }
    if let Some(history) = web_sys::window().and_then(|w| w.history().ok()) {
        let _ = history.replace_state_with_url(&JsValue::NULL, "", Some(&target_url));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_formats_root_and_change_overview_routes() {
        assert_eq!(parse_route("/", ""), AppRoute::Root);
        assert_eq!(parse_route("", ""), AppRoute::Root);
        assert_eq!(parse_route("/unknown/path", ""), AppRoute::Root);
        assert_eq!(parse_route("/c", ""), AppRoute::Root);
        assert_eq!(parse_route("/c/", ""), AppRoute::Root);

        let overview = parse_route("/c/ysxkykzrroro", "");
        assert_eq!(
            overview,
            AppRoute::ChangeOverview {
                change_id: String::from("ysxkykzrroro"),
                query: RouteQuery::default(),
            }
        );
        assert_eq!(overview.to_url(), "/c/ysxkykzrroro");

        // Trailing slash normalizes to canonical overview URL.
        let with_slash = parse_route("/c/ysxkykzrroro/", "");
        assert_eq!(with_slash, overview);
    }

    #[test]
    fn roundtrips_single_file_routes_including_nested_paths_and_special_chars() {
        let route = AppRoute::FileDiff {
            change_id: String::from("ysxkykzrroro"),
            file_path: String::from("src/server.rs"),
            query: RouteQuery::default(),
        };
        let url = route.to_url();
        assert_eq!(url, "/c/ysxkykzrroro/file/src/server.rs");
        assert_eq!(parse_route(&url, ""), route);

        let special = AppRoute::FileDiff {
            change_id: String::from("abc123"),
            file_path: String::from("docs/my file #1?v=2%20(final)/café.md"),
            query: RouteQuery::default(),
        };
        let special_url = special.to_url();
        assert_eq!(
            special_url,
            "/c/abc123/file/docs/my%20file%20%231%3Fv%3D2%2520%28final%29/caf%C3%A9.md"
        );
        assert_eq!(parse_route(&special_url, ""), special);
    }

    #[test]
    fn maps_commit_message_slug_and_encoded_name_both_ways() {
        let cm_route = AppRoute::FileDiff {
            change_id: String::from("ysxkykzrroro"),
            file_path: String::from(COMMIT_MESSAGE_PATH),
            query: RouteQuery::default(),
        };
        let url = cm_route.to_url();
        assert_eq!(url, "/c/ysxkykzrroro/file/COMMIT_MSG");
        assert_eq!(parse_route(&url, ""), cm_route);

        // Both `COMMIT_MSG` and `Commit%20Message` decode back to `"Commit Message"`.
        assert_eq!(
            parse_route("/c/ysxkykzrroro/file/Commit%20Message", ""),
            cm_route
        );
    }

    #[test]
    fn preserves_non_default_query_parameters_and_omits_defaults() {
        let query = RouteQuery {
            target: Some(String::from("50feee44")),
            base: Some(String::from("1234abcd")),
            ignore_rebase: Some(false),
        };
        let route = AppRoute::FileDiff {
            change_id: String::from("ysxkykzrroro"),
            file_path: String::from("src/lib.rs"),
            query: query.clone(),
        };
        let full = route.to_url();
        assert_eq!(
            full,
            "/c/ysxkykzrroro/file/src/lib.rs?target=50feee44&base=1234abcd&ignoreRebase=false"
        );
        let (path, search) = full.split_once('?').unwrap();
        assert_eq!(parse_route(path, search), route);

        // Default UI state (head commit, parent base, ignore_rebase=true) produces empty query.
        let default_q = RouteQuery::from_ui_state(
            Some("50feee441c24bdf517fa1cdd4c444e700f623b37"),
            Some("50feee441c24bdf517fa1cdd4c444e700f623b37"),
            "parent",
            true,
        );
        assert!(default_q.is_empty());
        assert_eq!(
            AppRoute::ChangeOverview {
                change_id: String::from("ysxkykzrroro"),
                query: default_q,
            }
            .to_url(),
            "/c/ysxkykzrroro"
        );

        // Malformed empty file path under `/c/<id>/file/` falls back gracefully to ChangeOverview.
        assert_eq!(
            parse_route("/c/ysxkykzrroro/file/", ""),
            AppRoute::ChangeOverview {
                change_id: String::from("ysxkykzrroro"),
                query: RouteQuery::default(),
            }
        );

        // Dot and parent-traversal segments (raw or percent-encoded) are rejected.
        for bad_path in [
            "/c/ysxkykzrroro/file/./src/lib.rs",
            "/c/ysxkykzrroro/file/../secret.txt",
            "/c/ysxkykzrroro/file/src/%2e%2e/lib.rs",
            "/c/ysxkykzrroro/file/src/%2e/lib.rs",
        ] {
            assert_eq!(
                parse_route(bad_path, ""),
                AppRoute::ChangeOverview {
                    change_id: String::from("ysxkykzrroro"),
                    query: RouteQuery::default(),
                },
                "traversal path {bad_path} must be rejected"
            );
        }
    }
}
