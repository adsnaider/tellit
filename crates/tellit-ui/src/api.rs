//! Browser HTTP and Server-Sent Events client calls to the local `tellit` server.

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use core::str::FromStr;

use serde::Serialize;
use serde::de::DeserializeOwned;
use tellit_types::{
    ApiErrorResponse, AppearanceSettings, ApproveCommitRequest, ApproveCommitResponse,
    CreateCommentRequest, CreateCommentResponse, DiffFile, EditorSpecRequest, HeartbeatEvent,
    HighlightedLinesResponse, JjChangesResponse, JjCommentsResponse, JjCommitsResponse,
    JjDiffResponse, JjStatusResponse, LineCountResponse, OpenInEditorRequest, OpenInEditorResponse,
    ReplyCommentRequest, ReplyCommentResponse, ResolveCommentRequest, ResolveCommentResponse,
    WatchEvent,
};
use thiserror::Error;
use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;
use wasm_bindgen_futures::JsFuture;
use web_sys::{Event, EventSource, Headers, MessageEvent, RequestInit, Response};

/// Errors produced when communicating with the local `tellit` HTTP server.
#[derive(Debug, Error)]
pub enum ApiError {
    /// Browser `window` object is unavailable in the current execution environment.
    #[error("browser window is unavailable")]
    MissingWindow,
    /// Fetch or `EventSource` failed at the browser transport layer.
    #[error("network request failed: {0}")]
    Transport(String),
    /// Server responded with a non-success HTTP status code.
    #[error("{message} (HTTP {status})")]
    HttpStatus {
        /// Numeric HTTP status code.
        status: u16,
        /// Server-supplied error message or browser status text.
        message: String,
    },
    /// Server response body was not valid JSON matching the expected type.
    #[error("invalid JSON response: {0}")]
    Json(#[from] serde_json::Error),
}

fn format_js_error(value: &wasm_bindgen::JsValue) -> String {
    js_sys::Error::from(value.clone())
        .message()
        .as_string()
        .or_else(|| value.as_string())
        .unwrap_or_else(|| String::from("unknown browser error"))
}

fn encode_uri_component(raw: &str) -> String {
    js_sys::encode_uri_component(raw)
        .as_string()
        .unwrap_or_else(|| String::from(raw))
}

fn encode_uri_path(path: &str) -> String {
    path.split('/')
        .map(encode_uri_component)
        .collect::<Vec<_>>()
        .join("/")
}

async fn fetch_response(url: &str, init: Option<&RequestInit>) -> Result<Response, ApiError> {
    let window = web_sys::window().ok_or(ApiError::MissingWindow)?;
    let promise = match init {
        Some(opts) => window.fetch_with_str_and_init(url, opts),
        None => window.fetch_with_str(url),
    };
    let resp_value = JsFuture::from(promise)
        .await
        .map_err(|err| ApiError::Transport(format_js_error(&err)))?;
    resp_value
        .dyn_into()
        .map_err(|err| ApiError::Transport(format_js_error(&err)))
}

fn http_status_error(status: u16, status_text: String, parsed_msg: Option<String>) -> ApiError {
    let message = parsed_msg.unwrap_or_else(|| {
        if status_text.is_empty() {
            String::from("request failed")
        } else {
            status_text
        }
    });
    ApiError::HttpStatus { status, message }
}

async fn send_text_request(url: &str, init: Option<&RequestInit>) -> Result<String, ApiError> {
    let response = fetch_response(url, init).await?;
    let status = response.status();
    let status_text = response.status_text();

    let text_promise = response
        .text()
        .map_err(|err| ApiError::Transport(format_js_error(&err)))?;
    let text_value = JsFuture::from(text_promise)
        .await
        .map_err(|err| ApiError::Transport(format_js_error(&err)))?;
    let body = text_value.as_string().unwrap_or_default();

    if !response.ok() {
        let parsed_msg = serde_json::from_str::<ApiErrorResponse>(&body)
            .ok()
            .map(|e| e.error)
            .filter(|s| !s.trim().is_empty());
        return Err(http_status_error(status, status_text, parsed_msg));
    }

    Ok(body)
}

async fn send_bytes_request(url: &str, init: Option<&RequestInit>) -> Result<Vec<u8>, ApiError> {
    let response = fetch_response(url, init).await?;
    let status = response.status();
    let status_text = response.status_text();

    let buf_promise = response
        .array_buffer()
        .map_err(|err| ApiError::Transport(format_js_error(&err)))?;
    let buf_value = JsFuture::from(buf_promise)
        .await
        .map_err(|err| ApiError::Transport(format_js_error(&err)))?;
    let bytes = js_sys::Uint8Array::new(&buf_value).to_vec();

    if !response.ok() {
        let parsed_msg = serde_json::from_slice::<ApiErrorResponse>(&bytes)
            .ok()
            .map(|e| e.error)
            .filter(|s| !s.trim().is_empty());
        return Err(http_status_error(status, status_text, parsed_msg));
    }

    Ok(bytes)
}

async fn send_request<T: DeserializeOwned>(
    url: &str,
    init: Option<&RequestInit>,
) -> Result<T, ApiError> {
    let bytes = send_bytes_request(url, init).await?;
    Ok(serde_json::from_slice(&bytes)?)
}

async fn fetch_json<T: DeserializeOwned>(url: &str) -> Result<T, ApiError> {
    send_request(url, None).await
}

async fn send_json_with_method<Req: Serialize, Resp: DeserializeOwned>(
    method: &str,
    url: &str,
    payload: &Req,
) -> Result<Resp, ApiError> {
    let json_body = serde_json::to_string(payload)?;
    let headers = Headers::new().map_err(|err| ApiError::Transport(format_js_error(&err)))?;
    headers
        .set("Content-Type", "application/json")
        .map_err(|err| ApiError::Transport(format_js_error(&err)))?;

    let init = RequestInit::new();
    init.set_method(method);
    init.set_headers(&headers);
    init.set_body(&wasm_bindgen::JsValue::from_str(&json_body));

    send_request(url, Some(&init)).await
}

async fn post_json<Req: Serialize, Resp: DeserializeOwned>(
    url: &str,
    payload: &Req,
) -> Result<Resp, ApiError> {
    send_json_with_method("POST", url, payload).await
}

async fn put_json<Req: Serialize, Resp: DeserializeOwned>(
    url: &str,
    payload: &Req,
) -> Result<Resp, ApiError> {
    send_json_with_method("PUT", url, payload).await
}

/// Fetches `GET /api/jj/status` from the local `tellit` server.
///
/// # Errors
///
/// Returns [`ApiError`] if no browser window exists, the HTTP request fails,
/// the server returns a non-2xx status, or the body fails JSON deserialization.
pub async fn fetch_jj_status() -> Result<JjStatusResponse, ApiError> {
    fetch_json("/api/jj/status").await
}

/// Fetches `GET /api/jj/changes` (or `GET /api/jj/changes?query=...` when `query` is provided).
///
/// # Errors
///
/// Returns [`ApiError`] if the HTTP request fails or the response JSON is invalid.
pub async fn fetch_jj_changes(query: Option<&str>) -> Result<JjChangesResponse, ApiError> {
    let url = match query.map(str::trim).filter(|s| !s.is_empty()) {
        Some(q) => alloc::format!("/api/jj/changes?query={}", encode_uri_component(q)),
        None => String::from("/api/jj/changes"),
    };
    fetch_json(&url).await
}

/// Fetches `GET /api/jj/changes/{change_id}/commits` for a Jujutsu change.
///
/// # Errors
///
/// Returns [`ApiError`] if the HTTP request fails or the response JSON is invalid.
pub async fn fetch_jj_commits(change_id: &str) -> Result<JjCommitsResponse, ApiError> {
    let url = alloc::format!(
        "/api/jj/changes/{}/commits",
        encode_uri_component(change_id)
    );
    fetch_json(&url).await
}

/// Fetches `GET /api/jj/diff` for `target`, optionally compared against `base`.
///
/// # Errors
///
/// Returns [`ApiError`] if the HTTP request fails or the response JSON is invalid.
pub async fn fetch_jj_diff(
    target: &str,
    base: Option<&str>,
    ignore_rebase: bool,
) -> Result<JjDiffResponse, ApiError> {
    let mut params: Vec<String> = alloc::vec![
        alloc::format!("target={}", encode_uri_component(target)),
        alloc::format!("ignoreRebase={ignore_rebase}"),
    ];
    if let Some(base_ref) = normalize_interdiff_base(base, target) {
        params.push(alloc::format!("base={}", encode_uri_component(base_ref)));
    }
    let url = alloc::format!("/api/jj/diff?{}", params.join("&"));
    fetch_json(&url).await
}

/// Fetches a single parsed [`DiffFile`] from `GET /api/jj/diff/file` for lazy hydration.
///
/// # Errors
///
/// Returns [`ApiError`] if the HTTP request fails, the file is missing (404), or JSON is invalid.
pub async fn fetch_jj_diff_file(
    target: &str,
    base: Option<&str>,
    ignore_rebase: bool,
    path: &str,
) -> Result<DiffFile, ApiError> {
    let mut params: Vec<String> = alloc::vec![
        alloc::format!("target={}", encode_uri_component(target)),
        alloc::format!("ignoreRebase={ignore_rebase}"),
        alloc::format!("path={}", encode_uri_component(path)),
    ];
    if let Some(base_ref) = normalize_interdiff_base(base, target) {
        params.push(alloc::format!("base={}", encode_uri_component(base_ref)));
    }
    let url = alloc::format!("/api/jj/diff/file?{}", params.join("&"));
    fetch_json(&url).await
}

/// Fetches `GET /api/jj/comments` for `target` and optional `base` commit.
///
/// # Errors
///
/// Returns [`ApiError`] if the HTTP request fails or the response JSON is invalid.
pub async fn fetch_jj_comments(
    target: &str,
    base: Option<&str>,
) -> Result<JjCommentsResponse, ApiError> {
    let mut params: Vec<String> =
        alloc::vec![alloc::format!("target={}", encode_uri_component(target))];
    if let Some(base_ref) = normalize_interdiff_base(base, target) {
        params.push(alloc::format!("base={}", encode_uri_component(base_ref)));
    }
    let url = alloc::format!("/api/jj/comments?{}", params.join("&"));
    fetch_json(&url).await
}

/// Sends `POST /api/jj/approve` to approve or revoke approval on `commit_id`.
///
/// # Errors
///
/// Returns [`ApiError`] if the HTTP request fails or the response JSON is invalid.
pub async fn post_jj_approve(
    commit_id: &str,
    disapprove: bool,
) -> Result<ApproveCommitResponse, ApiError> {
    let req = ApproveCommitRequest {
        commit_id: String::from(commit_id),
        disapprove,
    };
    post_json("/api/jj/approve", &req).await
}

/// Creates a new `.tellit/` comment thread via `POST /api/jj/comments`.
///
/// # Errors
///
/// Returns [`ApiError`] if the HTTP request fails or the server returns a non-2xx response.
pub async fn post_jj_comment(
    req: &CreateCommentRequest,
) -> Result<CreateCommentResponse, ApiError> {
    post_json("/api/jj/comments", req).await
}

/// Appends a reply message to an existing `.tellit/` review thread.
///
/// # Errors
///
/// Returns [`ApiError`] if the HTTP request fails or the thread is missing.
pub async fn post_jj_comment_reply(
    thread_id: &str,
    content: &str,
) -> Result<ReplyCommentResponse, ApiError> {
    let url = alloc::format!("/api/jj/comments/{}/reply", encode_uri_component(thread_id));
    post_json(
        &url,
        &ReplyCommentRequest {
            content: String::from(content),
        },
    )
    .await
}

/// Resolves or reopens an existing `.tellit/` review thread.
///
/// # Errors
///
/// Returns [`ApiError`] if the HTTP request fails or the thread is missing.
pub async fn post_jj_comment_resolve(
    thread_id: &str,
    unresolve: bool,
) -> Result<ResolveCommentResponse, ApiError> {
    let url = alloc::format!(
        "/api/jj/comments/{}/resolve",
        encode_uri_component(thread_id)
    );
    post_json(&url, &ResolveCommentRequest { unresolve }).await
}

/// Constructs a `/api/blob/{*path}?ref=...` URL for raw image or blob fetching.
#[must_use]
pub fn blob_url(file_path: &str, rev: &str) -> String {
    alloc::format!(
        "/api/blob/{}?ref={}",
        encode_uri_path(file_path),
        encode_uri_component(rev)
    )
}

/// Fetches UTF-8 text content for `file_path` at Jujutsu revision `rev` via `GET /api/blob/{*path}`.
///
/// # Errors
///
/// Returns [`ApiError`] if the HTTP request fails or the server returns 4xx/5xx.
pub async fn fetch_blob_text(file_path: &str, rev: &str) -> Result<String, ApiError> {
    let url = blob_url(file_path, rev);
    send_text_request(&url, None).await
}

/// Builds the `/api/lines/{*path}` URL for syntax-highlighted source lines at `rev`.
#[must_use]
pub fn highlighted_lines_url(
    file_path: &str,
    rev: &str,
    start: Option<u32>,
    end: Option<u32>,
) -> String {
    let mut url = alloc::format!(
        "/api/lines/{}?ref={}",
        encode_uri_path(file_path),
        encode_uri_component(rev),
    );
    if let Some(s) = start {
        url.push_str(&alloc::format!("&start={s}"));
    }
    if let Some(e) = end {
        url.push_str(&alloc::format!("&end={e}"));
    }
    url
}

/// Fetches syntax-highlighted lines for `file_path` at `rev` via `GET /api/lines/{*path}`.
///
/// # Errors
///
/// Returns [`ApiError`] if the HTTP request fails or the JSON payload is invalid.
pub async fn fetch_highlighted_lines(
    file_path: &str,
    rev: &str,
    start: Option<u32>,
    end: Option<u32>,
) -> Result<HighlightedLinesResponse, ApiError> {
    let url = highlighted_lines_url(file_path, rev, start, end);
    fetch_json(&url).await
}

/// Fetches a blob resource's byte length via `HEAD` and `Content-Length` without downloading the body.
///
/// # Errors
///
/// Returns [`ApiError`] if the browser request fails or returns a non-success HTTP status.
pub async fn fetch_blob_byte_size(url: &str) -> Result<u64, ApiError> {
    let init = RequestInit::new();
    init.set_method("HEAD");
    let response = fetch_response(url, Some(&init)).await?;
    if !response.ok() {
        return Err(ApiError::HttpStatus {
            status: response.status(),
            message: response.status_text(),
        });
    }
    let len = response
        .headers()
        .get("content-length")
        .ok()
        .flatten()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .unwrap_or(0);
    Ok(len)
}

/// Fetches line counts for `file_path` at `old_ref` (`old_path` if renamed) and/or `new_ref`.
///
/// # Errors
///
/// Returns [`ApiError`] if the HTTP request fails or the JSON payload is invalid.
pub async fn fetch_line_count(
    file_path: &str,
    old_ref: Option<&str>,
    new_ref: Option<&str>,
    old_path: Option<&str>,
) -> Result<LineCountResponse, ApiError> {
    let mut params = Vec::new();
    if let Some(old_r) = old_ref.map(str::trim).filter(|s| !s.is_empty()) {
        params.push(alloc::format!("oldRef={}", encode_uri_component(old_r)));
    }
    if let Some(new_r) = new_ref.map(str::trim).filter(|s| !s.is_empty()) {
        params.push(alloc::format!("newRef={}", encode_uri_component(new_r)));
    }
    if let Some(old_p) = old_path
        .map(str::trim)
        .filter(|s| !s.is_empty() && *s != file_path)
    {
        params.push(alloc::format!("oldPath={}", encode_uri_component(old_p)));
    }

    let query = params.join("&");
    let encoded_path = encode_uri_path(file_path);
    let url = if query.is_empty() {
        alloc::format!("/api/line-count/{encoded_path}")
    } else {
        alloc::format!("/api/line-count/{encoded_path}?{query}")
    };
    fetch_json(&url).await
}

/// Requests the backend to open `file_path` at `line` in the configured external editor.
///
/// # Errors
///
/// Returns [`ApiError`] if the HTTP request fails or the editor could not be launched.
pub async fn post_open_in_editor(
    file_path: &str,
    line: Option<usize>,
    editor: Option<EditorSpecRequest>,
) -> Result<OpenInEditorResponse, ApiError> {
    post_json(
        "/api/open-in-editor",
        &OpenInEditorRequest {
            file_path: String::from(file_path),
            line,
            editor,
        },
    )
    .await
}

/// Fetches server-persisted client appearance settings from `GET /api/user-settings`.
///
/// # Errors
///
/// Returns [`ApiError`] if the HTTP request fails or returns malformed JSON.
pub async fn fetch_user_settings() -> Result<Option<AppearanceSettings>, ApiError> {
    let payload: serde_json::Value = fetch_json("/api/user-settings").await?;
    let Some(appearance) = payload
        .get("client")
        .and_then(|c| c.get("appearance"))
        .cloned()
    else {
        return Ok(None);
    };
    let parsed: AppearanceSettings = serde_json::from_value(appearance)?;
    Ok(Some(parsed.normalized()))
}

/// Persists `settings` under `client.appearance` via `PUT /api/user-settings`.
///
/// # Errors
///
/// Returns [`ApiError`] if serialization fails or the HTTP request fails.
pub async fn save_user_settings(settings: &AppearanceSettings) -> Result<(), ApiError> {
    let body = serde_json::json!({
        "client": {
            "appearance": settings,
        }
    });
    let _: serde_json::Value = put_json("/api/user-settings", &body).await?;
    Ok(())
}

/// Copies `text` to the browser clipboard via `navigator.clipboard.writeText`.
///
/// # Errors
///
/// Returns [`ApiError`] if the browser window is missing or clipboard write fails.
pub async fn copy_to_clipboard(text: &str) -> Result<(), ApiError> {
    let window = web_sys::window().ok_or(ApiError::MissingWindow)?;
    let clipboard = window.navigator().clipboard();
    let promise = clipboard.write_text(text);
    JsFuture::from(promise)
        .await
        .map_err(|err| ApiError::Transport(format_js_error(&err)))?;
    Ok(())
}

fn normalize_interdiff_base<'a>(base: Option<&'a str>, target: &str) -> Option<&'a str> {
    let trimmed = base.map(str::trim).filter(|s| !s.is_empty())?;
    let trimmed_target = target.trim();
    if trimmed == "parent"
        || (!trimmed_target.is_empty()
            && (trimmed == alloc::format!("{trimmed_target}^")
                || trimmed == alloc::format!("{trimmed_target}-")))
    {
        return None;
    }
    Some(trimmed)
}

/// Active `EventSource` subscription that unregisters handlers and closes on drop.
pub struct SseSubscription {
    source: EventSource,
    _on_message: Closure<dyn FnMut(MessageEvent)>,
    _on_error: Closure<dyn FnMut(Event)>,
}

impl Drop for SseSubscription {
    fn drop(&mut self) {
        self.source.set_onmessage(None);
        self.source.set_onerror(None);
        self.source.close();
    }
}

fn subscribe_sse(
    url: &str,
    mut on_payload: impl FnMut(&str) + 'static,
    mut on_error_cb: impl FnMut(&EventSource) + 'static,
) -> Result<SseSubscription, ApiError> {
    if web_sys::window().is_none() {
        return Err(ApiError::MissingWindow);
    }

    let source = EventSource::new(url).map_err(|err| ApiError::Transport(format_js_error(&err)))?;

    let on_message = Closure::<dyn FnMut(MessageEvent)>::new(move |msg: MessageEvent| {
        if let Some(payload) = msg.data().as_string() {
            on_payload(&payload);
        }
    });

    let error_source = source.clone();
    let on_error = Closure::<dyn FnMut(Event)>::new(move |_err: Event| {
        on_error_cb(&error_source);
    });

    source.set_onmessage(Some(on_message.as_ref().unchecked_ref()));
    source.set_onerror(Some(on_error.as_ref().unchecked_ref()));

    Ok(SseSubscription {
        source,
        _on_message: on_message,
        _on_error: on_error,
    })
}

/// Opens an `EventSource` connection to `/api/heartbeat` and invokes callbacks on state changes.
///
/// # Errors
///
/// Returns [`ApiError::MissingWindow`] when running outside a browser window or
/// [`ApiError::Transport`] if constructing the `EventSource` fails.
pub fn subscribe_heartbeat(
    mut on_event: impl FnMut(HeartbeatEvent) + 'static,
    mut on_disconnect: impl FnMut() + 'static,
) -> Result<SseSubscription, ApiError> {
    subscribe_sse(
        "/api/heartbeat",
        move |payload| {
            if let Ok(event) = HeartbeatEvent::from_str(payload) {
                on_event(event);
            }
        },
        move |source| {
            source.close();
            on_disconnect();
        },
    )
}

/// Opens an `EventSource` connection to `/api/watch` for live repository and comment updates.
///
/// # Errors
///
/// Returns [`ApiError::MissingWindow`] when running outside a browser window or
/// [`ApiError::Transport`] if constructing the `EventSource` fails.
pub fn subscribe_watch(
    mut on_event: impl FnMut(WatchEvent) + 'static,
    mut on_error: impl FnMut() + 'static,
) -> Result<SseSubscription, ApiError> {
    subscribe_sse(
        "/api/watch",
        move |payload| {
            if let Ok(event) = serde_json::from_str::<WatchEvent>(payload) {
                on_event(event);
            }
        },
        move |_source| {
            // Allow EventSource to reconnect automatically on transient drops while surfacing error state.
            on_error();
        },
    )
}
