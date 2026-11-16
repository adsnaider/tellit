//! Browser HTTP and Server-Sent Events client calls to the local `tellit` server.

extern crate alloc;

use alloc::string::String;
use core::str::FromStr;

use tellit_types::{HeartbeatEvent, JjStatusResponse};
use thiserror::Error;
use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;
use wasm_bindgen_futures::JsFuture;
use web_sys::{Event, EventSource, MessageEvent, Response};

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
    #[error("request failed with HTTP {status} {status_text}")]
    HttpStatus {
        /// Numeric HTTP status code.
        status: u16,
        /// Status reason phrase reported by the browser response.
        status_text: String,
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

/// Fetches `GET /api/jj/status` from the local `tellit` server.
///
/// # Errors
///
/// Returns [`ApiError`] if no browser window exists, the HTTP request fails,
/// the server returns a non-2xx status, or the body fails JSON deserialization.
pub async fn fetch_jj_status() -> Result<JjStatusResponse, ApiError> {
    let window = web_sys::window().ok_or(ApiError::MissingWindow)?;
    let resp_value = JsFuture::from(window.fetch_with_str("/api/jj/status"))
        .await
        .map_err(|err| ApiError::Transport(format_js_error(&err)))?;
    let response: Response = resp_value
        .dyn_into()
        .map_err(|err| ApiError::Transport(format_js_error(&err)))?;

    if !response.ok() {
        return Err(ApiError::HttpStatus {
            status: response.status(),
            status_text: response.status_text(),
        });
    }

    let text_promise = response
        .text()
        .map_err(|err| ApiError::Transport(format_js_error(&err)))?;
    let text_value = JsFuture::from(text_promise)
        .await
        .map_err(|err| ApiError::Transport(format_js_error(&err)))?;
    let body = text_value.as_string().unwrap_or_default();

    Ok(serde_json::from_str(&body)?)
}

/// Active `EventSource` subscription to `GET /api/heartbeat` that closes cleanly on drop.
pub struct HeartbeatSubscription {
    source: EventSource,
    _on_message: Closure<dyn FnMut(MessageEvent)>,
    _on_error: Closure<dyn FnMut(Event)>,
}

impl Drop for HeartbeatSubscription {
    fn drop(&mut self) {
        self.source.set_onmessage(None);
        self.source.set_onerror(None);
        self.source.close();
    }
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
) -> Result<HeartbeatSubscription, ApiError> {
    if web_sys::window().is_none() {
        return Err(ApiError::MissingWindow);
    }

    let source = EventSource::new("/api/heartbeat")
        .map_err(|err| ApiError::Transport(format_js_error(&err)))?;

    let on_message = Closure::<dyn FnMut(MessageEvent)>::new(move |msg: MessageEvent| {
        if let Some(payload) = msg.data().as_string()
            && let Ok(event) = HeartbeatEvent::from_str(&payload)
        {
            on_event(event);
        }
    });

    let error_source = source.clone();
    let on_error = Closure::<dyn FnMut(Event)>::new(move |_err: Event| {
        error_source.close();
        on_disconnect();
    });

    source.set_onmessage(Some(on_message.as_ref().unchecked_ref()));
    source.set_onerror(Some(on_error.as_ref().unchecked_ref()));

    Ok(HeartbeatSubscription {
        source,
        _on_message: on_message,
        _on_error: on_error,
    })
}
