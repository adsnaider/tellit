//! Gzip response compression middleware for JSON API and static web asset responses.

extern crate alloc;

use alloc::vec::Vec;
use std::io::{self, Write};

use axum::body::{Body, Bytes};
use axum::extract::Request;
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use flate2::Compression;
use flate2::write::GzEncoder;

/// Minimum response body size in bytes before attempting gzip compression.
const MIN_COMPRESS_BYTES: usize = 64;

/// Payload size threshold above which gzip compression runs on a blocking worker thread.
const SPAWN_BLOCKING_THRESHOLD: usize = 16 * 1024;

/// Compresses eligible HTTP responses with gzip when the client advertises `Accept-Encoding: gzip`.
///
/// Server-Sent Events (`text/event-stream`), `no-transform` responses, already-encoded bodies,
/// and incompressible binary formats pass through untouched without buffering.
pub async fn compress_response(request: Request, next: Next) -> Response {
    let method = request.method().clone();
    let accepts_gzip = client_accepts_gzip(request.headers());

    let mut response = next.run(request).await;
    if !is_compressible_response(response.status(), response.headers()) {
        return response;
    }

    append_vary_accept_encoding(response.headers_mut());
    if !accepts_gzip || method == Method::HEAD {
        return response;
    }

    let level = compression_level_for(response.headers());
    let (mut parts, body) = response.into_parts();
    let Ok(raw_bytes) = axum::body::to_bytes(body, usize::MAX).await else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };

    if raw_bytes.len() < MIN_COMPRESS_BYTES {
        return Response::from_parts(parts, Body::from(raw_bytes));
    }

    let compressed = match compress_bytes(raw_bytes.clone(), level).await {
        Ok(bytes) if bytes.len() < raw_bytes.len() => bytes,
        _ => return Response::from_parts(parts, Body::from(raw_bytes)),
    };

    parts
        .headers
        .insert(header::CONTENT_ENCODING, HeaderValue::from_static("gzip"));
    parts
        .headers
        .insert(header::CONTENT_LENGTH, HeaderValue::from(compressed.len()));
    parts.headers.remove(header::ACCEPT_RANGES);

    Response::from_parts(parts, Body::from(compressed))
}

/// Parses `Accept-Encoding` headers according to RFC 9110 q-value rules for `gzip`/`x-gzip`.
fn client_accepts_gzip(headers: &HeaderMap) -> bool {
    let mut explicit_gzip: Option<f32> = None;
    let mut wildcard: Option<f32> = None;

    for value in headers.get_all(header::ACCEPT_ENCODING) {
        let Ok(raw) = value.to_str() else {
            continue;
        };
        for item in raw.split(',') {
            let mut parts = item.split(';');
            let coding = parts.next().map_or("", str::trim);
            if coding.is_empty() {
                continue;
            }
            let Some(q) = parse_q_value(parts) else {
                continue;
            };
            if coding.eq_ignore_ascii_case("gzip") || coding.eq_ignore_ascii_case("x-gzip") {
                explicit_gzip = Some(explicit_gzip.map_or(q, |prev| prev.max(q)));
            } else if coding == "*" {
                wildcard = Some(wildcard.map_or(q, |prev| prev.max(q)));
            }
        }
    }

    explicit_gzip.or(wildcard).is_some_and(|q| q > 0.0)
}

fn parse_q_value<'a>(params: impl Iterator<Item = &'a str>) -> Option<f32> {
    let mut q = 1.0_f32;
    for param in params {
        let trimmed = param.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some((key, val)) = trimmed.split_once('=')
            && key.trim().eq_ignore_ascii_case("q")
        {
            let parsed = val.trim().parse::<f32>().ok()?;
            if !(0.0..=1.0).contains(&parsed) {
                return None;
            }
            q = parsed;
        }
    }
    Some(q)
}

fn is_compressible_response(status: StatusCode, headers: &HeaderMap) -> bool {
    if status.is_informational()
        || matches!(
            status,
            StatusCode::NO_CONTENT | StatusCode::PARTIAL_CONTENT | StatusCode::NOT_MODIFIED
        )
    {
        return false;
    }

    if headers.contains_key(header::CONTENT_ENCODING) || headers.contains_key(header::CONTENT_RANGE)
    {
        return false;
    }

    if has_cache_control_directive(headers, "no-transform") {
        return false;
    }

    let Some(content_type) = headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
    else {
        return false;
    };

    is_compressible_content_type(content_type)
}

fn is_compressible_content_type(raw: &str) -> bool {
    let media_type = raw.split(';').next().map_or("", str::trim);
    if media_type.eq_ignore_ascii_case("text/event-stream") {
        return false;
    }

    media_type.eq_ignore_ascii_case("application/json")
        || media_type.eq_ignore_ascii_case("application/wasm")
        || media_type.eq_ignore_ascii_case("application/javascript")
        || media_type.eq_ignore_ascii_case("application/x-javascript")
        || media_type.eq_ignore_ascii_case("application/xml")
        || media_type.eq_ignore_ascii_case("image/svg+xml")
        || starts_with_ignore_ascii_case(media_type, "text/")
        || ends_with_ignore_ascii_case(media_type, "+json")
        || ends_with_ignore_ascii_case(media_type, "+xml")
}

fn starts_with_ignore_ascii_case(haystack: &str, prefix: &str) -> bool {
    haystack
        .get(..prefix.len())
        .is_some_and(|head| head.eq_ignore_ascii_case(prefix))
}

fn ends_with_ignore_ascii_case(haystack: &str, suffix: &str) -> bool {
    haystack
        .len()
        .checked_sub(suffix.len())
        .and_then(|start| haystack.get(start..))
        .is_some_and(|tail| tail.eq_ignore_ascii_case(suffix))
}

fn has_cache_control_directive(headers: &HeaderMap, target: &str) -> bool {
    headers.get_all(header::CACHE_CONTROL).iter().any(|value| {
        value.to_str().is_ok_and(|raw| {
            raw.split(',').any(|directive| {
                let name = directive.split('=').next().map_or("", str::trim);
                name.eq_ignore_ascii_case(target)
            })
        })
    })
}

fn compression_level_for(headers: &HeaderMap) -> Compression {
    // Public immutable bundles (`.wasm`, hashed `.js`/`.css`) are cached for a year, so spending
    // slightly more CPU once for a smaller wire payload pays off. Dynamic API JSON uses fast
    // compression to keep response latency low.
    let is_wasm = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|ct| {
            ct.split(';')
                .next()
                .map_or("", str::trim)
                .eq_ignore_ascii_case("application/wasm")
        });
    if is_wasm
        || (has_cache_control_directive(headers, "public")
            && has_cache_control_directive(headers, "immutable"))
    {
        Compression::default()
    } else {
        Compression::fast()
    }
}

fn append_vary_accept_encoding(headers: &mut HeaderMap) {
    let already_present = headers.get_all(header::VARY).iter().any(|value| {
        value.to_str().is_ok_and(|raw| {
            raw.split(',').any(|token| {
                let trimmed = token.trim();
                trimmed == "*" || trimmed.eq_ignore_ascii_case("accept-encoding")
            })
        })
    });
    if !already_present {
        headers.append(header::VARY, HeaderValue::from_static("accept-encoding"));
    }
}

async fn compress_bytes(bytes: Bytes, level: Compression) -> io::Result<Vec<u8>> {
    if bytes.len() >= SPAWN_BLOCKING_THRESHOLD {
        tokio::task::spawn_blocking(move || gzip_compress(&bytes, level))
            .await
            .map_err(io::Error::other)?
    } else {
        gzip_compress(&bytes, level)
    }
}

fn gzip_compress(bytes: &[u8], level: Compression) -> io::Result<Vec<u8>> {
    let mut encoder = GzEncoder::new(Vec::with_capacity(bytes.len() / 4 + 32), level);
    encoder.write_all(bytes)?;
    encoder.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers_with_accept_encoding(values: &[&str]) -> HeaderMap {
        let mut headers = HeaderMap::new();
        for value in values {
            headers.append(
                header::ACCEPT_ENCODING,
                HeaderValue::from_str(value).unwrap(),
            );
        }
        headers
    }

    #[test]
    fn accept_encoding_respects_gzip_and_q_values() {
        assert!(!client_accepts_gzip(&HeaderMap::new()));
        assert!(client_accepts_gzip(&headers_with_accept_encoding(&[
            "gzip, deflate, br"
        ])));
        assert!(client_accepts_gzip(&headers_with_accept_encoding(&[
            "br, GZIP; q=0.5"
        ])));
        assert!(client_accepts_gzip(&headers_with_accept_encoding(&[
            "x-gzip"
        ])));
        assert!(client_accepts_gzip(&headers_with_accept_encoding(&["*"])));
        assert!(!client_accepts_gzip(&headers_with_accept_encoding(&[
            "br, deflate"
        ])));
        assert!(!client_accepts_gzip(&headers_with_accept_encoding(&[
            "gzip;q=0"
        ])));
        assert!(!client_accepts_gzip(&headers_with_accept_encoding(&[
            "*;q=1, gzip;q=0"
        ])));
        assert!(!client_accepts_gzip(&headers_with_accept_encoding(&[
            "*;q=0"
        ])));
    }

    #[test]
    fn compressible_content_types_exclude_sse_and_binary_images() {
        assert!(is_compressible_content_type("application/json"));
        assert!(is_compressible_content_type("application/wasm"));
        assert!(is_compressible_content_type(
            "text/javascript; charset=utf-8"
        ));
        assert!(is_compressible_content_type("text/css; charset=utf-8"));
        assert!(is_compressible_content_type("text/html; charset=utf-8"));
        assert!(is_compressible_content_type("image/svg+xml"));

        assert!(!is_compressible_content_type("text/event-stream"));
        assert!(!is_compressible_content_type(
            "text/event-stream; charset=utf-8"
        ));
        assert!(!is_compressible_content_type("image/png"));
        assert!(!is_compressible_content_type("image/webp"));
        assert!(!is_compressible_content_type("application/octet-stream"));
    }

    #[test]
    fn no_transform_disables_compression() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/json"),
        );
        headers.insert(
            header::CACHE_CONTROL,
            HeaderValue::from_static("no-cache, no-transform"),
        );
        assert!(!is_compressible_response(StatusCode::OK, &headers));
    }

    #[test]
    fn append_vary_avoids_duplicates_and_respects_wildcard() {
        let mut headers = HeaderMap::new();
        append_vary_accept_encoding(&mut headers);
        append_vary_accept_encoding(&mut headers);
        assert_eq!(headers.get_all(header::VARY).iter().count(), 1);

        let mut wildcard = HeaderMap::new();
        wildcard.insert(header::VARY, HeaderValue::from_static("*"));
        append_vary_accept_encoding(&mut wildcard);
        assert_eq!(wildcard.get_all(header::VARY).iter().count(), 1);
    }
}
