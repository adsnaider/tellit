//! Wire types shared between the `tellit` HTTP server and the Leptos browser client.
//!
//! All types in this crate are `#![no_std]` compatible and serialize to the JSON and
//! Server-Sent Events (SSE) payloads exchanged over `/api/*` endpoints.
//!
//! # Examples
//!
//! ```
//! use core::str::FromStr;
//! use tellit_types::{HeartbeatEvent, JjStatusResponse};
//!
//! let status = JjStatusResponse::jj("/workspace/repo");
//! assert!(status.is_jj);
//! assert_eq!(status.repo_root.as_deref(), Some("/workspace/repo"));
//!
//! let event = HeartbeatEvent::from_str("connected")?;
//! assert_eq!(event, HeartbeatEvent::Connected);
//! # Ok::<(), tellit_types::UnknownHeartbeatEvent>(())
//! ```

#![no_std]

extern crate alloc;

use alloc::string::String;
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
        }
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

#[cfg(test)]
mod tests {
    use alloc::string::ToString;

    use super::*;

    #[test]
    fn jj_status_serializes_camel_case_and_omits_missing_root() {
        let jj = JjStatusResponse::jj("/repo/root");
        let jj_json = serde_json::to_string(&jj).unwrap();
        assert_eq!(jj_json, r#"{"isJj":true,"repoRoot":"/repo/root"}"#);
        assert_eq!(
            serde_json::from_str::<JjStatusResponse>(&jj_json).unwrap(),
            jj
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
}
