//! Local `axum` HTTP and Server-Sent Events server backing `tellit launch`.
//!
//! Serves the embedded Leptos WebAssembly bundle (or an external asset directory specified via
//! `TELLIT_WEB_DIR`), reports Jujutsu workspace detection at `GET /api/jj/status`, and streams
//! browser connection heartbeats at `GET /api/heartbeat`.
//!
//! # Examples
//!
//! ```no_run
//! use std::path::PathBuf;
//! use tellit_cli::server::{ServerConfig, start_server};
//!
//! # async fn demo() -> Result<(), tellit_cli::server::ServerError> {
//! let server = start_server(ServerConfig::new(PathBuf::from("."))).await?;
//! println!("Listening on {}", server.url);
//! server.shutdown();
//! server.wait().await?;
//! # Ok(())
//! # }
//! ```

extern crate alloc;

mod assets;
mod background;
mod compression;
mod diff_parser;
mod editor;
mod jujutsu;
mod user_config;
mod watch;

pub use background::{BACKGROUND_CHILD_ENV, BackgroundServerInfo, spawn_background_child};

use alloc::boxed::Box;
use alloc::string::{String, ToString};
use alloc::sync::Arc;
use core::convert::Infallible;
use core::future::Future;
use core::net::{IpAddr, Ipv4Addr, SocketAddr};
use core::pin::Pin;
use core::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use core::task::{Context, Poll};
use core::time::Duration;
use std::io;
use std::path::PathBuf;

use axum::body::Body;
use axum::extract::{Path as AxumPath, Query, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::sse::{Event, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use editor::{EditorError, open_in_editor};
use jujutsu::{JujutsuError, JujutsuService};
use serde::Deserialize;
use tellit_types::{
    ApiErrorResponse, ApproveCommitRequest, ApproveCommitResponse, CreateCommentRequest,
    CreateCommentResponse, HeartbeatEvent, JjChangesResponse, JjCommentsResponse,
    JjCommitsResponse, JjDiffResponse, JjStatusResponse, OpenInEditorRequest, OpenInEditorResponse,
    ReplyCommentRequest, ReplyCommentResponse, ResolveCommentRequest, ResolveCommentResponse,
};
use thiserror::Error;
use tokio::net::TcpListener;
use tokio::sync::watch as tokio_watch;
use tokio::task::JoinHandle;
use tokio_stream::Stream;
use user_config::{UserConfig, UserConfigError, UserConfigStore};
use watch::WatchHub;

/// Default preferred TCP port when `--port` is not specified.
pub const DEFAULT_PORT: u16 = 4966;

/// Default loopback address bound when `--host` is omitted.
pub const DEFAULT_HOST: &str = "127.0.0.1";

const DEFAULT_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(5);
const DEFAULT_DISCONNECT_GRACE: Duration = Duration::from_millis(250);
const IMMUTABLE_PRIVATE_CACHE: &str = "private, max-age=31536000, immutable";
const API_NO_STORE_CACHE: &str = "no-cache, no-store";

/// Errors returned when configuring, binding, or running the `tellit` HTTP server.
#[derive(Debug, Error)]
pub enum ServerError {
    /// Hostname or IP literal passed to `--host` could not be resolved.
    #[error("failed to resolve bind host '{host}': {source}")]
    InvalidHost {
        /// Unresolvable host value supplied by the caller.
        host: String,
        /// Underlying name resolution error.
        #[source]
        source: io::Error,
    },
    /// Every port from `start_port` through `65535` was already in use.
    #[error("no available TCP port found starting from {start_port}")]
    PortExhausted {
        /// Initial port where the fallback scan began.
        start_port: u16,
    },
    /// Socket, process, or filesystem I/O error.
    #[error(transparent)]
    Io(#[from] io::Error),
    /// JSON serialization or deserialization failure during background startup.
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    /// Detached background server exited with an error before completing startup.
    #[error("{0}")]
    BackgroundStartup(String),
    /// Detached background server did not report readiness before the timeout elapsed.
    #[error("timed out waiting for background tellit server to start")]
    BackgroundTimeout,
}

/// Configuration for launching an in-process `tellit` HTTP server.
///
/// # Examples
///
/// ```
/// use std::path::PathBuf;
/// use tellit_cli::server::{DEFAULT_HOST, ServerConfig};
///
/// let config = ServerConfig::new(PathBuf::from("/tmp/repo")).with_port(Some(0));
/// assert_eq!(config.host, DEFAULT_HOST);
/// assert_eq!(config.preferred_port, Some(0));
/// ```
#[derive(Debug, Clone)]
pub struct ServerConfig {
    /// Repository or working directory inspected by Jujutsu endpoints.
    pub repo_path: PathBuf,
    /// Optional custom `.tellit/` review store directory override.
    pub store_path: Option<PathBuf>,
    /// Optional initial target revision passed to `tellit launch`.
    pub commit_ish: Option<String>,
    /// Optional comparison base revision passed to `tellit launch`.
    pub compare_with: Option<String>,
    /// Host address or hostname to bind.
    pub host: String,
    /// Preferred TCP port (`Some(0)` binds an OS-assigned ephemeral port; `None` starts at [`DEFAULT_PORT`]).
    pub preferred_port: Option<u16>,
    /// Keep the server alive after all `/api/heartbeat` browser clients disconnect.
    pub keep_alive: bool,
    /// Optional filesystem directory overriding embedded WebAssembly UI assets.
    pub web_dir: Option<PathBuf>,
    /// Optional path overriding `~/.tellit/config.json` for user settings persistence.
    pub user_config_path: Option<PathBuf>,
    /// Interval between SSE ping frames on `/api/heartbeat`.
    pub heartbeat_interval: Duration,
    /// Grace window before shutting down after the last `/api/heartbeat` client disconnects.
    pub disconnect_grace: Duration,
    /// Suppress port-fallback diagnostic messages on `stderr`.
    pub quiet: bool,
}

impl ServerConfig {
    /// Creates a server configuration rooted at `repo_path` with standard defaults.
    ///
    /// Reads `TELLIT_WEB_DIR` from the environment if set to an existing directory.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::path::PathBuf;
    /// use tellit_cli::server::ServerConfig;
    ///
    /// let config = ServerConfig::new(PathBuf::from("."));
    /// assert!(!config.keep_alive);
    /// ```
    #[must_use]
    pub fn new(repo_path: PathBuf) -> Self {
        let web_dir = std::env::var_os("TELLIT_WEB_DIR")
            .map(PathBuf::from)
            .filter(|path| path.is_dir());

        Self {
            repo_path,
            store_path: None,
            commit_ish: None,
            compare_with: None,
            host: String::from(DEFAULT_HOST),
            preferred_port: None,
            keep_alive: false,
            web_dir,
            user_config_path: None,
            heartbeat_interval: DEFAULT_HEARTBEAT_INTERVAL,
            disconnect_grace: DEFAULT_DISCONNECT_GRACE,
            quiet: false,
        }
    }

    /// Overrides the `.tellit/` review store directory.
    #[must_use]
    pub fn with_store_path(mut self, store_path: Option<PathBuf>) -> Self {
        self.store_path = store_path;
        self
    }

    /// Sets the initial target and comparison revisions resolved by `GET /api/jj/status`.
    #[must_use]
    pub fn with_initial_revisions(
        mut self,
        commit_ish: Option<String>,
        compare_with: Option<String>,
    ) -> Self {
        self.commit_ish = commit_ish;
        self.compare_with = compare_with;
        self
    }

    /// Sets the preferred TCP port.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::path::PathBuf;
    /// use tellit_cli::server::ServerConfig;
    ///
    /// let config = ServerConfig::new(PathBuf::from(".")).with_port(Some(5000));
    /// assert_eq!(config.preferred_port, Some(5000));
    /// ```
    #[must_use]
    pub const fn with_port(mut self, port: Option<u16>) -> Self {
        self.preferred_port = port;
        self
    }

    /// Overrides the bind host when `host` is non-empty.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::path::PathBuf;
    /// use tellit_cli::server::ServerConfig;
    ///
    /// let config = ServerConfig::new(PathBuf::from(".")).with_host(Some("localhost"));
    /// assert_eq!(config.host, "localhost");
    /// ```
    #[must_use]
    pub fn with_host(mut self, host: Option<&str>) -> Self {
        if let Some(trimmed) = host.map(str::trim).filter(|s| !s.is_empty()) {
            self.host = String::from(trimmed);
        }
        self
    }

    /// Configures whether the server stays running after browser tabs close.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::path::PathBuf;
    /// use tellit_cli::server::ServerConfig;
    ///
    /// let config = ServerConfig::new(PathBuf::from(".")).with_keep_alive(true);
    /// assert!(config.keep_alive);
    /// ```
    #[must_use]
    pub const fn with_keep_alive(mut self, keep_alive: bool) -> Self {
        self.keep_alive = keep_alive;
        self
    }

    /// Overrides the static web asset directory on disk.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::path::PathBuf;
    /// use tellit_cli::server::ServerConfig;
    ///
    /// let config = ServerConfig::new(PathBuf::from(".")).with_web_dir(Some(PathBuf::from("/tmp/dist")));
    /// assert_eq!(config.web_dir, Some(PathBuf::from("/tmp/dist")));
    /// ```
    #[must_use]
    pub fn with_web_dir(mut self, web_dir: Option<PathBuf>) -> Self {
        self.web_dir = web_dir;
        self
    }

    /// Overrides the JSON configuration file path for `/api/user-settings`.
    #[must_use]
    pub fn with_user_config_path(mut self, user_config_path: Option<PathBuf>) -> Self {
        self.user_config_path = user_config_path;
        self
    }

    /// Sets the `/api/heartbeat` ping interval and post-disconnect shutdown grace period.
    ///
    /// # Examples
    ///
    /// ```
    /// use core::time::Duration;
    /// use std::path::PathBuf;
    /// use tellit_cli::server::ServerConfig;
    ///
    /// let config = ServerConfig::new(PathBuf::from("."))
    ///     .with_heartbeat_timings(Duration::from_millis(50), Duration::from_millis(25));
    /// assert_eq!(config.heartbeat_interval, Duration::from_millis(50));
    /// ```
    #[must_use]
    pub const fn with_heartbeat_timings(
        mut self,
        heartbeat_interval: Duration,
        disconnect_grace: Duration,
    ) -> Self {
        self.heartbeat_interval = heartbeat_interval;
        self.disconnect_grace = disconnect_grace;
        self
    }

    /// Silences stderr notices during port fallback scanning.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::path::PathBuf;
    /// use tellit_cli::server::ServerConfig;
    ///
    /// let config = ServerConfig::new(PathBuf::from(".")).with_quiet(true);
    /// assert!(config.quiet);
    /// ```
    #[must_use]
    pub const fn with_quiet(mut self, quiet: bool) -> Self {
        self.quiet = quiet;
        self
    }
}

/// Cloneable handle that triggers graceful shutdown on a [`RunningServer`].
#[derive(Clone)]
pub struct ShutdownHandle {
    tx: tokio_watch::Sender<bool>,
}

impl ShutdownHandle {
    /// Signals the associated [`RunningServer`] to stop accepting connections and exit.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # async fn demo(server: tellit_cli::server::RunningServer) {
    /// let handle = server.shutdown_handle();
    /// handle.shutdown();
    /// # }
    /// ```
    pub fn shutdown(&self) {
        let _ = self.tx.send(true);
    }
}

/// Handle to a bound, actively serving `tellit` HTTP server task.
pub struct RunningServer {
    /// Local socket address bound by the server.
    pub addr: SocketAddr,
    /// Bound TCP port.
    pub port: u16,
    /// User-facing HTTP URL for opening or querying the server.
    pub url: String,
    shutdown_tx: tokio_watch::Sender<bool>,
    task: JoinHandle<io::Result<()>>,
}

impl RunningServer {
    /// Returns a cloneable [`ShutdownHandle`] that can stop this server from another task.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # async fn demo(server: tellit_cli::server::RunningServer) {
    /// let handle = server.shutdown_handle();
    /// handle.shutdown();
    /// # }
    /// ```
    #[must_use]
    pub fn shutdown_handle(&self) -> ShutdownHandle {
        ShutdownHandle {
            tx: self.shutdown_tx.clone(),
        }
    }

    /// Returns `true` when the server is bound to a non-loopback address reachable externally.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # async fn demo(server: tellit_cli::server::RunningServer) {
    /// if server.is_external_bind() {
    ///     eprintln!("bound externally on {}", server.addr);
    /// }
    /// # }
    /// ```
    #[must_use]
    pub const fn is_external_bind(&self) -> bool {
        is_external_bind_addr(self.addr)
    }

    /// Signals the HTTP server to stop accepting connections and shut down gracefully.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # async fn demo(server: tellit_cli::server::RunningServer) {
    /// server.shutdown();
    /// # }
    /// ```
    pub fn shutdown(&self) {
        let _ = self.shutdown_tx.send(true);
    }

    /// Waits until the server finishes shutting down.
    ///
    /// # Errors
    ///
    /// Returns [`ServerError::Io`] if the underlying `axum::serve` loop fails or panics.
    pub async fn wait(self) -> Result<(), ServerError> {
        match self.task.await {
            Ok(Ok(())) => Ok(()),
            Ok(Err(err)) => Err(ServerError::Io(err)),
            Err(join_err) => Err(ServerError::Io(io::Error::other(join_err))),
        }
    }
}

struct AppState {
    jj: JujutsuService,
    watch: Arc<WatchHub>,
    user_config: UserConfigStore,
    commit_ish: Option<String>,
    compare_with: Option<String>,
    web_dir: Option<PathBuf>,
    keep_alive: bool,
    heartbeat_interval: Duration,
    disconnect_grace: Duration,
    active_clients: AtomicUsize,
    disconnect_epoch: AtomicU64,
    shutdown_tx: tokio_watch::Sender<bool>,
}

struct BoundListener {
    listener: TcpListener,
    addr: SocketAddr,
    url: String,
}

fn format_display_url(raw_host: &str, bound_addr: SocketAddr) -> String {
    let port = bound_addr.port();
    let trimmed = raw_host.trim();
    if bound_addr.ip().is_unspecified() || trimmed.eq_ignore_ascii_case("localhost") {
        return alloc::format!("http://localhost:{port}");
    }

    match bound_addr.ip() {
        IpAddr::V4(ip) if trimmed.parse::<IpAddr>().is_ok() => {
            alloc::format!("http://{ip}:{port}")
        }
        IpAddr::V6(ip) if trimmed.trim_matches(['[', ']']).parse::<IpAddr>().is_ok() => {
            alloc::format!("http://[{ip}]:{port}")
        }
        _ => alloc::format!("http://{trimmed}:{port}"),
    }
}

async fn resolve_bind_ip(raw_host: &str) -> Result<IpAddr, ServerError> {
    let trimmed = raw_host.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("localhost") {
        return Ok(IpAddr::V4(Ipv4Addr::LOCALHOST));
    }

    let unbracketed = trimmed.trim_matches(['[', ']']);
    if let Ok(ip) = unbracketed.parse::<IpAddr>() {
        return Ok(ip);
    }

    let mut addrs = tokio::net::lookup_host((trimmed, 0))
        .await
        .map_err(|source| ServerError::InvalidHost {
            host: String::from(trimmed),
            source,
        })?;

    addrs
        .next()
        .map(|sock| sock.ip())
        .ok_or_else(|| ServerError::InvalidHost {
            host: String::from(trimmed),
            source: io::Error::new(io::ErrorKind::AddrNotAvailable, "no addresses resolved"),
        })
}

async fn bind_with_fallback(
    raw_host: &str,
    preferred_port: Option<u16>,
    quiet: bool,
) -> Result<BoundListener, ServerError> {
    let ip = resolve_bind_ip(raw_host).await?;

    if preferred_port == Some(0) {
        let listener = TcpListener::bind(SocketAddr::new(ip, 0)).await?;
        let addr = listener.local_addr()?;
        let url = format_display_url(raw_host, addr);
        return Ok(BoundListener {
            listener,
            addr,
            url,
        });
    }

    let start_port = preferred_port.unwrap_or(DEFAULT_PORT);
    for port in start_port..=u16::MAX {
        match TcpListener::bind(SocketAddr::new(ip, port)).await {
            Ok(listener) => {
                let addr = listener.local_addr()?;
                let url = format_display_url(raw_host, addr);
                return Ok(BoundListener {
                    listener,
                    addr,
                    url,
                });
            }
            Err(err) if err.kind() == io::ErrorKind::AddrInUse => {
                if !quiet && port < u16::MAX {
                    eprintln!("Port {port} is busy, trying {}...", port + 1);
                }
            }
            Err(err) => return Err(ServerError::Io(err)),
        }
    }

    Err(ServerError::PortExhausted { start_port })
}

enum ApiError {
    Jujutsu(JujutsuError),
    Editor(EditorError),
    UserConfig(UserConfigError),
}

impl From<JujutsuError> for ApiError {
    fn from(err: JujutsuError) -> Self {
        Self::Jujutsu(err)
    }
}

impl From<EditorError> for ApiError {
    fn from(err: EditorError) -> Self {
        Self::Editor(err)
    }
}

impl From<UserConfigError> for ApiError {
    fn from(err: UserConfigError) -> Self {
        Self::UserConfig(err)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            Self::Jujutsu(err) => {
                let status = match &err {
                    JujutsuError::InvalidInput(_) => StatusCode::BAD_REQUEST,
                    JujutsuError::NotFound(_) => StatusCode::NOT_FOUND,
                    JujutsuError::CommandSpawn { .. }
                    | JujutsuError::CommandFailed { .. }
                    | JujutsuError::Store(_)
                    | JujutsuError::Io(_) => StatusCode::INTERNAL_SERVER_ERROR,
                };
                (status, err.to_string())
            }
            Self::Editor(err) => {
                let status = match &err {
                    EditorError::Disabled
                    | EditorError::NotConfigured { .. }
                    | EditorError::InvalidInput(_)
                    | EditorError::PathEscapesRepo => StatusCode::BAD_REQUEST,
                    EditorError::SpawnFailed { .. } => StatusCode::INTERNAL_SERVER_ERROR,
                };
                (status, err.to_string())
            }
            Self::UserConfig(err) => {
                let status = match &err {
                    UserConfigError::InvalidPatch(_) => StatusCode::BAD_REQUEST,
                    UserConfigError::TooLarge => StatusCode::PAYLOAD_TOO_LARGE,
                    UserConfigError::Io(_) | UserConfigError::Serialize(_) => {
                        StatusCode::INTERNAL_SERVER_ERROR
                    }
                };
                (status, err.to_string())
            }
        };
        (status, Json(ApiErrorResponse { error: message })).into_response()
    }
}

#[derive(Deserialize)]
struct ChangesQuery {
    query: Option<String>,
}

#[derive(Deserialize)]
struct DiffQuery {
    target: Option<String>,
    commit: Option<String>,
    base: Option<String>,
    #[serde(rename = "ignoreRebase")]
    ignore_rebase: Option<bool>,
    eager: Option<bool>,
    #[serde(rename = "summaryThreshold")]
    summary_threshold: Option<usize>,
}

#[derive(Deserialize)]
struct DiffFileQuery {
    target: Option<String>,
    commit: Option<String>,
    base: Option<String>,
    #[serde(rename = "ignoreRebase")]
    ignore_rebase: Option<bool>,
    path: String,
}

#[derive(Deserialize)]
struct CommentsQuery {
    target: Option<String>,
    commit: Option<String>,
    base: Option<String>,
}

#[derive(Deserialize)]
struct BlobQuery {
    #[serde(rename = "ref")]
    rev: Option<String>,
}

#[derive(Deserialize)]
struct HighlightedLinesQuery {
    #[serde(rename = "ref")]
    rev: Option<String>,
    start: Option<u32>,
    end: Option<u32>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LineCountQuery {
    old_ref: Option<String>,
    new_ref: Option<String>,
    old_path: Option<String>,
}

async fn get_jj_status(State(state): State<Arc<AppState>>) -> Json<JjStatusResponse> {
    Json(
        state
            .jj
            .detect_status(state.commit_ish.as_deref(), state.compare_with.as_deref())
            .await,
    )
}

async fn get_jj_changes(
    State(state): State<Arc<AppState>>,
    Query(query): Query<ChangesQuery>,
) -> Result<Json<JjChangesResponse>, ApiError> {
    Ok(Json(state.jj.get_changes(query.query.as_deref()).await?))
}

async fn get_jj_change_commits(
    State(state): State<Arc<AppState>>,
    AxumPath(change_id): AxumPath<String>,
) -> Result<Json<JjCommitsResponse>, ApiError> {
    let commits = state.jj.get_commits_for_change(&change_id).await?;
    Ok(Json(JjCommitsResponse { commits }))
}

/// Returns `true` when `rev` is a full 40-character hexadecimal commit ID.
fn is_immutable_commit_oid(rev: &str) -> bool {
    let trimmed = rev.trim();
    trimmed.len() == 40 && trimmed.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Returns `true` when a diff request addresses full 40-character commit IDs and matches the
/// resolved `commit` and `base_commit` IDs.
fn is_immutable_diff_commits(
    target: &str,
    base: Option<&str>,
    resolved_commit: &str,
    resolved_base_commit: &str,
) -> bool {
    let trimmed_target = target.trim();
    if !is_immutable_commit_oid(trimmed_target)
        || !resolved_commit.eq_ignore_ascii_case(trimmed_target)
    {
        return false;
    }
    match base.map(str::trim).filter(|s| !s.is_empty()) {
        None => is_immutable_commit_oid(resolved_base_commit),
        Some(base_rev) => {
            is_immutable_commit_oid(base_rev) && resolved_base_commit.eq_ignore_ascii_case(base_rev)
        }
    }
}

/// Returns `true` when a diff request addresses full 40-character commit IDs and matches the
/// resolved commit IDs in `diff`.
fn is_immutable_diff_query(target: &str, base: Option<&str>, diff: &JjDiffResponse) -> bool {
    is_immutable_diff_commits(target, base, &diff.commit, &diff.base_commit)
}

/// Returns `true` when a line-count query specifies at least one revision and every provided
/// revision is a full 40-character hexadecimal commit ID.
fn is_immutable_line_count_query(old_ref: Option<&str>, new_ref: Option<&str>) -> bool {
    let old = old_ref.map(str::trim).filter(|s| !s.is_empty());
    let new = new_ref.map(str::trim).filter(|s| !s.is_empty());
    if old.is_none() && new.is_none() {
        return false;
    }
    old.is_none_or(is_immutable_commit_oid) && new.is_none_or(is_immutable_commit_oid)
}

async fn get_jj_diff(
    State(state): State<Arc<AppState>>,
    Query(query): Query<DiffQuery>,
) -> Result<Response, ApiError> {
    let target = query.target.or(query.commit).unwrap_or_default();
    let ignore_rebase = query.ignore_rebase.unwrap_or(true);
    let eager = query.eager.unwrap_or(false);
    let outcome = state
        .jj
        .get_diff(
            &target,
            query.base.as_deref(),
            ignore_rebase,
            eager,
            query.summary_threshold,
        )
        .await?;
    if outcome.is_working_copy_parent_diff {
        let rel_paths = outcome
            .response
            .files
            .iter()
            .filter(|file| file.path != diff_parser::COMMIT_MESSAGE_PATH)
            .flat_map(|file| {
                core::iter::once(PathBuf::from(&file.path))
                    .chain(file.old_path.as_deref().map(PathBuf::from))
            })
            .collect();
        state.watch.update_working_copy_files(rel_paths);
    }
    let cache_control = if outcome.cacheable
        && is_immutable_diff_query(&target, query.base.as_deref(), &outcome.response)
    {
        IMMUTABLE_PRIVATE_CACHE
    } else {
        API_NO_STORE_CACHE
    };
    let response_arc = outcome.response;
    let body = if response_arc.files.len() > 4 {
        tokio::task::spawn_blocking(move || serde_json::to_vec(response_arc.as_ref()))
            .await
            .map_err(|err| JujutsuError::Io(io::Error::other(err)))?
            .map_err(|err| JujutsuError::Io(io::Error::other(err)))?
    } else {
        serde_json::to_vec(response_arc.as_ref())
            .map_err(|err| JujutsuError::Io(io::Error::other(err)))?
    };
    let mut response = Response::new(Body::from(body));
    *response.status_mut() = StatusCode::OK;
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static(cache_control),
    );
    Ok(response)
}

async fn get_jj_diff_file(
    State(state): State<Arc<AppState>>,
    Query(query): Query<DiffFileQuery>,
) -> Result<Response, ApiError> {
    let target = query.target.or(query.commit).unwrap_or_default();
    let ignore_rebase = query.ignore_rebase.unwrap_or(true);
    let outcome = state
        .jj
        .get_diff_file(&target, query.base.as_deref(), ignore_rebase, &query.path)
        .await?;
    let cache_control = if outcome.cacheable
        && is_immutable_diff_commits(
            &target,
            query.base.as_deref(),
            &outcome.commit,
            &outcome.base_commit,
        ) {
        IMMUTABLE_PRIVATE_CACHE
    } else {
        API_NO_STORE_CACHE
    };
    let file_arc = outcome.file;
    let body = if file_arc.chunks.len() > 2 {
        tokio::task::spawn_blocking(move || serde_json::to_vec(file_arc.as_ref()))
            .await
            .map_err(|err| JujutsuError::Io(io::Error::other(err)))?
            .map_err(|err| JujutsuError::Io(io::Error::other(err)))?
    } else {
        serde_json::to_vec(file_arc.as_ref())
            .map_err(|err| JujutsuError::Io(io::Error::other(err)))?
    };
    let mut response = Response::new(Body::from(body));
    *response.status_mut() = StatusCode::OK;
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static(cache_control),
    );
    Ok(response)
}

async fn get_jj_comments(
    State(state): State<Arc<AppState>>,
    Query(query): Query<CommentsQuery>,
) -> Result<Json<JjCommentsResponse>, ApiError> {
    let target = query.target.or(query.commit).unwrap_or_default();
    let threads = state
        .jj
        .get_comments_for_diff(&target, query.base.as_deref())
        .await?;
    Ok(Json(JjCommentsResponse { threads }))
}

async fn post_jj_comment(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateCommentRequest>,
) -> Result<Json<CreateCommentResponse>, ApiError> {
    let thread_id = state
        .jj
        .add_comment(&req.commit_id, &req.file_path, req.line_range, &req.content)
        .await?;
    state.watch.broadcast_comments_changed();
    Ok(Json(CreateCommentResponse { thread_id }))
}

async fn post_jj_comment_reply(
    State(state): State<Arc<AppState>>,
    AxumPath(thread_id): AxumPath<String>,
    Json(req): Json<ReplyCommentRequest>,
) -> Result<Json<ReplyCommentResponse>, ApiError> {
    let canonical_id = state.jj.reply_comment(&thread_id, &req.content).await?;
    state.watch.broadcast_comments_changed();
    Ok(Json(ReplyCommentResponse {
        thread_id: canonical_id,
    }))
}

async fn post_jj_comment_resolve(
    State(state): State<Arc<AppState>>,
    AxumPath(thread_id): AxumPath<String>,
    Json(req): Json<ResolveCommentRequest>,
) -> Result<Json<ResolveCommentResponse>, ApiError> {
    let response = state.jj.resolve_comment(&thread_id, req.unresolve).await?;
    state.watch.broadcast_comments_changed();
    Ok(Json(response))
}

async fn post_jj_approve(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ApproveCommitRequest>,
) -> Result<Json<ApproveCommitResponse>, ApiError> {
    let response = state
        .jj
        .set_approval(&req.commit_id, req.disapprove)
        .await?;
    state.watch.broadcast_comments_changed();
    Ok(Json(response))
}

async fn get_blob(
    State(state): State<Arc<AppState>>,
    AxumPath(path): AxumPath<String>,
    Query(query): Query<BlobQuery>,
) -> Result<Response, ApiError> {
    let blob = state
        .jj
        .get_blob_content(&path, query.rev.as_deref())
        .await?;
    let content_len = blob.bytes.len();
    let mut response = Response::new(Body::from(blob.bytes));
    *response.status_mut() = StatusCode::OK;
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static(blob.content_type),
    );
    headers.insert(header::CONTENT_LENGTH, HeaderValue::from(content_len));
    let cache_control = if query.rev.as_deref().is_some_and(is_immutable_commit_oid) {
        IMMUTABLE_PRIVATE_CACHE
    } else {
        API_NO_STORE_CACHE
    };
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static(cache_control),
    );
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    if blob.content_type == "image/svg+xml" {
        headers.insert(
            header::CONTENT_SECURITY_POLICY,
            HeaderValue::from_static("default-src 'none'; style-src 'unsafe-inline'"),
        );
    }
    Ok(response)
}

fn validate_highlighted_line_range(
    start: Option<u32>,
    end: Option<u32>,
) -> Result<(), JujutsuError> {
    if start == Some(0) || end == Some(0) {
        return Err(JujutsuError::InvalidInput(String::from(
            "invalid line range: start and end must be 1-based",
        )));
    }
    if let (Some(s), Some(e)) = (start, end)
        && s > e
    {
        return Err(JujutsuError::InvalidInput(String::from(
            "invalid line range: start must be <= end",
        )));
    }
    Ok(())
}

fn is_binary_blob(bytes: &[u8]) -> bool {
    bytes[..bytes.len().min(8192)].contains(&0)
}

async fn get_highlighted_lines(
    State(state): State<Arc<AppState>>,
    AxumPath(path): AxumPath<String>,
    Query(query): Query<HighlightedLinesQuery>,
) -> Result<Response, ApiError> {
    validate_highlighted_line_range(query.start, query.end)?;
    let blob = state
        .jj
        .get_blob_content(&path, query.rev.as_deref())
        .await?;
    if is_binary_blob(&blob.bytes) {
        return Err(ApiError::from(JujutsuError::InvalidInput(String::from(
            "cannot return highlighted lines for binary file",
        ))));
    }
    let cache_control = if query.rev.as_deref().is_some_and(is_immutable_commit_oid) {
        IMMUTABLE_PRIVATE_CACHE
    } else {
        API_NO_STORE_CACHE
    };
    let start = query.start;
    let end = query.end;
    let body = tokio::task::spawn_blocking(move || {
        let text = String::from_utf8_lossy(&blob.bytes);
        let highlighted = diff_parser::highlight_file_lines(&path, &text, start, end);
        serde_json::to_vec(&highlighted)
    })
    .await
    .map_err(|err| JujutsuError::Io(io::Error::other(err)))?
    .map_err(|err| JujutsuError::Io(io::Error::other(err)))?;
    let mut response = Response::new(Body::from(body));
    *response.status_mut() = StatusCode::OK;
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static(cache_control),
    );
    Ok(response)
}

async fn get_line_count(
    State(state): State<Arc<AppState>>,
    AxumPath(path): AxumPath<String>,
    Query(query): Query<LineCountQuery>,
) -> Result<Response, ApiError> {
    let outcome = state
        .jj
        .get_line_count(
            &path,
            query.old_ref.as_deref(),
            query.new_ref.as_deref(),
            query.old_path.as_deref(),
        )
        .await?;
    let cache_control = if outcome.all_lookups_succeeded
        && is_immutable_line_count_query(query.old_ref.as_deref(), query.new_ref.as_deref())
    {
        IMMUTABLE_PRIVATE_CACHE
    } else {
        API_NO_STORE_CACHE
    };
    let mut response = Json(outcome.counts).into_response();
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static(cache_control),
    );
    Ok(response)
}

async fn post_open_in_editor(
    State(state): State<Arc<AppState>>,
    Json(req): Json<OpenInEditorRequest>,
) -> Result<Json<OpenInEditorResponse>, ApiError> {
    Ok(Json(open_in_editor(state.jj.repo_path(), &req)?))
}

async fn get_user_settings(State(state): State<Arc<AppState>>) -> Json<UserConfig> {
    Json(state.user_config.read().await)
}

async fn put_user_settings(
    State(state): State<Arc<AppState>>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<UserConfig>, ApiError> {
    let updated = state.user_config.update_client(body).await?;
    Ok(Json(updated))
}

/// Stream emitting an initial `connected` event followed by periodic `heartbeat` events,
/// triggering server auto-shutdown on drop when `keep_alive` is disabled and no tabs remain.
struct HeartbeatStream {
    state: Arc<AppState>,
    sent_initial: bool,
    closed: bool,
    interval: tokio::time::Interval,
    shutdown_fut: Pin<Box<dyn Future<Output = ()> + Send>>,
}

impl HeartbeatStream {
    fn new(state: Arc<AppState>) -> Self {
        state.disconnect_epoch.fetch_add(1, Ordering::SeqCst);
        state.active_clients.fetch_add(1, Ordering::SeqCst);
        let mut interval = tokio::time::interval(state.heartbeat_interval);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let mut shutdown_rx = state.shutdown_tx.subscribe();
        let shutdown_fut = Box::pin(async move {
            while !*shutdown_rx.borrow_and_update() {
                if shutdown_rx.changed().await.is_err() {
                    break;
                }
            }
        });
        Self {
            state,
            sent_initial: false,
            closed: false,
            interval,
            shutdown_fut,
        }
    }
}

impl Stream for HeartbeatStream {
    type Item = Result<Event, Infallible>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if self.closed {
            return Poll::Ready(None);
        }

        if self.shutdown_fut.as_mut().poll(cx).is_ready() {
            self.closed = true;
            return Poll::Ready(None);
        }

        if !self.sent_initial {
            self.sent_initial = true;
            // Consume the immediate first tick of `tokio::time::interval` so the first
            // periodic ping waits a full `heartbeat_interval`.
            let _ = self.interval.poll_tick(cx);
            return Poll::Ready(Some(Ok(
                Event::default().data(HeartbeatEvent::Connected.as_str())
            )));
        }

        match self.interval.poll_tick(cx) {
            Poll::Ready(_) => Poll::Ready(Some(Ok(
                Event::default().data(HeartbeatEvent::Ping.as_str())
            ))),
            Poll::Pending => Poll::Pending,
        }
    }
}

impl Drop for HeartbeatStream {
    fn drop(&mut self) {
        let previous = self.state.active_clients.fetch_sub(1, Ordering::SeqCst);
        if !self.state.keep_alive && previous == 1 {
            let epoch = self
                .state
                .disconnect_epoch
                .fetch_add(1, Ordering::SeqCst)
                .wrapping_add(1);
            let state = Arc::clone(&self.state);
            if let Ok(handle) = tokio::runtime::Handle::try_current() {
                handle.spawn(async move {
                    tokio::time::sleep(state.disconnect_grace).await;
                    if state.active_clients.load(Ordering::SeqCst) == 0
                        && state.disconnect_epoch.load(Ordering::SeqCst) == epoch
                    {
                        let _ = state.shutdown_tx.send(true);
                    }
                });
            }
        }
    }
}

async fn get_heartbeat(State(state): State<Arc<AppState>>) -> Response {
    let stream = HeartbeatStream::new(state);
    let mut response = Sse::new(stream).into_response();
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("no-cache, no-transform"),
    );
    response
}

async fn get_watch(State(state): State<Arc<AppState>>) -> Response {
    let stream = state.watch.subscribe();
    let mut response = Sse::new(stream).into_response();
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("no-cache, no-transform"),
    );
    response
}

async fn get_index(State(state): State<Arc<AppState>>) -> Response {
    let asset = assets::load_asset("index.html", state.web_dir.as_deref()).await;
    assets::into_http_response(asset)
}

async fn get_static_asset(
    State(state): State<Arc<AppState>>,
    AxumPath(path): AxumPath<String>,
) -> Response {
    let asset = assets::load_asset(&path, state.web_dir.as_deref()).await;
    assets::into_http_response(asset)
}

async fn default_api_no_store_cache(
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> Response {
    let mut response = next.run(request).await;
    if !response.headers().contains_key(header::CACHE_CONTROL) {
        response.headers_mut().insert(
            header::CACHE_CONTROL,
            HeaderValue::from_static(API_NO_STORE_CACHE),
        );
    }
    response
}

fn build_router(state: Arc<AppState>) -> Router {
    let api_routes = Router::new()
        .route("/api/jj/status", get(get_jj_status))
        .route("/api/jj/changes", get(get_jj_changes))
        .route(
            "/api/jj/changes/{change_id}/commits",
            get(get_jj_change_commits),
        )
        .route("/api/jj/diff", get(get_jj_diff))
        .route("/api/jj/diff/file", get(get_jj_diff_file))
        .route(
            "/api/jj/comments",
            get(get_jj_comments).post(post_jj_comment),
        )
        .route(
            "/api/jj/comments/{thread_id}/reply",
            post(post_jj_comment_reply),
        )
        .route(
            "/api/jj/comments/{thread_id}/resolve",
            post(post_jj_comment_resolve),
        )
        .route("/api/jj/approve", post(post_jj_approve))
        .route("/api/blob/{*path}", get(get_blob))
        .route("/api/lines/{*path}", get(get_highlighted_lines))
        .route("/api/line-count/{*path}", get(get_line_count))
        .route("/api/open-in-editor", post(post_open_in_editor))
        .route(
            "/api/user-settings",
            get(get_user_settings).put(put_user_settings),
        )
        .layer(axum::middleware::from_fn(default_api_no_store_cache));

    let compressed_routes = Router::new()
        .route("/", get(get_index))
        .route("/index.html", get(get_index))
        .route("/c", get(get_index))
        .route("/c/", get(get_index))
        .route("/c/{*rest}", get(get_index))
        .merge(api_routes)
        .route("/{*path}", get(get_static_asset))
        .layer(axum::middleware::from_fn(compression::compress_response));

    // Keep SSE routes outside the compression layer so event frames flush immediately.
    Router::new()
        .route("/api/heartbeat", get(get_heartbeat))
        .route("/api/watch", get(get_watch))
        .merge(compressed_routes)
        .with_state(state)
}

/// Binds the TCP listener (with `AddrInUse` fallback) and spawns the `axum` server task.
///
/// # Errors
///
/// Returns [`ServerError`] if host resolution fails, all candidate ports are occupied,
/// or socket binding fails with a non-recoverable I/O error.
pub async fn start_server(config: ServerConfig) -> Result<RunningServer, ServerError> {
    let bound = bind_with_fallback(&config.host, config.preferred_port, config.quiet).await?;
    let port = bound.addr.port();

    let (shutdown_tx, mut shutdown_rx) = tokio_watch::channel(false);
    let jj = JujutsuService::new(config.repo_path.clone(), config.store_path);
    let store_dir = jj.store_dir();
    let watch = WatchHub::start(
        config.repo_path,
        store_dir,
        shutdown_tx.clone(),
        config.quiet,
    );
    let state = Arc::new(AppState {
        jj,
        watch,
        user_config: UserConfigStore::new(config.user_config_path),
        commit_ish: config.commit_ish,
        compare_with: config.compare_with,
        web_dir: config.web_dir,
        keep_alive: config.keep_alive,
        heartbeat_interval: config.heartbeat_interval,
        disconnect_grace: config.disconnect_grace,
        active_clients: AtomicUsize::new(0),
        disconnect_epoch: AtomicU64::new(0),
        shutdown_tx: shutdown_tx.clone(),
    });

    let router = build_router(state);
    let listener = bound.listener;

    let task = tokio::spawn(async move {
        axum::serve(listener, router)
            .with_graceful_shutdown(async move {
                while !*shutdown_rx.borrow_and_update() {
                    if shutdown_rx.changed().await.is_err() {
                        break;
                    }
                }
            })
            .await
    });

    Ok(RunningServer {
        addr: bound.addr,
        port,
        url: bound.url,
        shutdown_tx,
        task,
    })
}

/// Returns `true` when `addr` binds a non-loopback interface reachable over the network.
///
/// # Examples
///
/// ```
/// use core::net::{IpAddr, Ipv4Addr, SocketAddr};
/// use tellit_cli::server::is_external_bind_addr;
///
/// let loopback = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 4966);
/// let wildcard = SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 4966);
/// assert!(!is_external_bind_addr(loopback));
/// assert!(is_external_bind_addr(wildcard));
/// ```
#[must_use]
pub const fn is_external_bind_addr(addr: SocketAddr) -> bool {
    !addr.ip().is_loopback()
}

/// Waits for an operating system termination signal (`SIGINT` or Unix `SIGTERM`).
///
/// # Examples
///
/// ```no_run
/// # async fn demo() {
/// tellit_cli::server::wait_for_shutdown_signal().await;
/// # }
/// ```
pub async fn wait_for_shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        if let Ok(mut sigterm) = signal(SignalKind::terminate()) {
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {}
                _ = sigterm.recv() => {}
            }
            return;
        }
    }
    let _ = tokio::signal::ctrl_c().await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_display_url_handles_ipv4_ipv6_and_wildcards() {
        let v4 = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 4966);
        assert_eq!(format_display_url("127.0.0.1", v4), "http://127.0.0.1:4966");
        assert_eq!(format_display_url("localhost", v4), "http://localhost:4966");
        assert_eq!(
            format_display_url(" LOCALHOST ", v4),
            "http://localhost:4966"
        );

        let any_v4 = SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 4966);
        assert_eq!(
            format_display_url("0.0.0.0", any_v4),
            "http://localhost:4966"
        );

        let any_v6 = SocketAddr::new(IpAddr::V6(core::net::Ipv6Addr::UNSPECIFIED), 4966);
        assert_eq!(format_display_url("::", any_v6), "http://localhost:4966");
        assert_eq!(format_display_url("::0", any_v6), "http://localhost:4966");
        assert_eq!(format_display_url("[::0]", any_v6), "http://localhost:4966");

        let v6 = SocketAddr::new(IpAddr::V6(core::net::Ipv6Addr::LOCALHOST), 4966);
        assert_eq!(format_display_url("::1", v6), "http://[::1]:4966");
        assert_eq!(format_display_url("[::1]", v6), "http://[::1]:4966");
    }

    #[test]
    fn external_bind_addr_detection() {
        let v4_loop = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 4966);
        let v6_loop = SocketAddr::new(IpAddr::V6(core::net::Ipv6Addr::LOCALHOST), 4966);
        let v4_any = SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 4966);
        let v6_any = SocketAddr::new(IpAddr::V6(core::net::Ipv6Addr::UNSPECIFIED), 4966);

        assert!(!is_external_bind_addr(v4_loop));
        assert!(!is_external_bind_addr(v6_loop));
        assert!(is_external_bind_addr(v4_any));
        assert!(is_external_bind_addr(v6_any));
    }

    #[test]
    fn immutable_commit_oid_requires_full_40_char_hex() {
        let oid = "0123456789abcdef0123456789abcdef01234567";
        assert!(is_immutable_commit_oid(oid));
        assert!(is_immutable_commit_oid(&alloc::format!("  {oid}  ")));
        assert!(is_immutable_commit_oid(&oid.to_ascii_uppercase()));

        assert!(!is_immutable_commit_oid(""));
        assert!(!is_immutable_commit_oid("@"));
        assert!(!is_immutable_commit_oid("@-"));
        assert!(!is_immutable_commit_oid("main"));
        assert!(!is_immutable_commit_oid("working"));
        assert!(!is_immutable_commit_oid(&oid[..12]));
        assert!(!is_immutable_commit_oid(&oid[..39]));
        assert!(!is_immutable_commit_oid(&alloc::format!("{oid}0")));
        assert!(!is_immutable_commit_oid(
            "ysxkykzrroro0123456789abcdef0123456789ab"
        ));
    }

    #[test]
    fn immutable_diff_and_line_count_queries_reject_mutable_refs() {
        let oid1 = "0123456789abcdef0123456789abcdef01234567";
        let oid2 = "fedcba9876543210fedcba9876543210fedcba98";
        let diff = JjDiffResponse {
            commit: String::from(oid2),
            base_commit: String::from(oid1),
            files: alloc::vec![],
            is_empty: true,
            is_rebase_diff: false,
            rebase_ignored: false,
            rebase_conflict: false,
        };

        assert!(is_immutable_diff_query(oid2, None, &diff));
        assert!(is_immutable_diff_query(oid2, Some(""), &diff));
        assert!(is_immutable_diff_query(oid2, Some(oid1), &diff));
        assert!(!is_immutable_diff_query("@", None, &diff));
        assert!(!is_immutable_diff_query(oid2, Some("@-"), &diff));
        assert!(!is_immutable_diff_query(oid2, Some("main"), &diff));
        // A 40-hex string that resolved to a different commit ID (e.g. bookmark collision) is mutable.
        assert!(!is_immutable_diff_query(oid1, None, &diff));

        // A degraded parent diff whose `fetch_commit_meta` failed falls back to `{oid}-` and must not cache.
        let degraded_diff = JjDiffResponse {
            base_commit: alloc::format!("{oid2}-"),
            ..diff
        };
        assert!(!is_immutable_diff_query(oid2, None, &degraded_diff));

        assert!(is_immutable_line_count_query(Some(oid1), Some(oid2)));
        assert!(is_immutable_line_count_query(Some(oid1), None));
        assert!(is_immutable_line_count_query(None, Some(oid2)));
        assert!(!is_immutable_line_count_query(None, None));
        assert!(!is_immutable_line_count_query(Some(""), Some("")));
        assert!(!is_immutable_line_count_query(Some("@-"), Some(oid2)));
        assert!(!is_immutable_line_count_query(Some(oid1), Some("working")));
    }

    #[test]
    fn highlighted_line_range_validation_rejects_zero_and_inverted_bounds() {
        assert!(validate_highlighted_line_range(None, None).is_ok());
        assert!(validate_highlighted_line_range(Some(1), None).is_ok());
        assert!(validate_highlighted_line_range(None, Some(20)).is_ok());
        assert!(validate_highlighted_line_range(Some(5), Some(5)).is_ok());
        assert!(validate_highlighted_line_range(Some(5), Some(20)).is_ok());
        assert!(validate_highlighted_line_range(Some(0), Some(10)).is_err());
        assert!(validate_highlighted_line_range(Some(1), Some(0)).is_err());
        assert!(validate_highlighted_line_range(Some(12), Some(5)).is_err());

        assert!(!is_binary_blob(b"fn main() {}\n"));
        assert!(is_binary_blob(b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR"));
        let mut nul_after_prefix = alloc::vec![b'a'; 8192];
        nul_after_prefix.push(0);
        assert!(!is_binary_blob(&nul_after_prefix));
    }
}
