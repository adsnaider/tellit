//! Static and embedded WebAssembly asset lookup for the Leptos browser client.

extern crate alloc;

use alloc::borrow::Cow;
use alloc::string::String;
use std::path::{Component, Path, PathBuf};

use axum::body::Body;
use axum::http::{HeaderValue, Response, StatusCode, header};
use rust_embed::RustEmbed;

const FALLBACK_INDEX_HTML: &[u8] = b"<!doctype html>\
<html lang=\"en\"><head><meta charset=\"utf-8\"><title>tellit</title></head>\
<body><main><p>tellit UI bundle is not built yet. Run <code>trunk build</code> in <code>crates/tellit-ui</code>.</p></main></body></html>";

#[derive(RustEmbed)]
#[folder = "crates/tellit-ui/dist"]
struct EmbeddedUiAssets;

/// HTTP caching policy for a served static asset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssetCachePolicy {
    /// Entry-point HTML that must never be stored by caches.
    NoStore,
    /// Unhashed static asset that must revalidate on each request.
    NoCache,
    /// Content-hashed asset safe to cache indefinitely.
    Immutable,
}

/// Loaded asset bytes and MIME metadata ready for an HTTP response.
pub struct AssetPayload {
    /// Raw asset bytes, borrowed from the binary when embedded or owned when read from disk.
    pub bytes: Cow<'static, [u8]>,
    /// Resolved `Content-Type` header value.
    pub content_type: String,
    /// Caching behavior applied to the HTTP response headers.
    pub cache_policy: AssetCachePolicy,
}

/// Validates and normalizes a URL path into a safe relative asset path without parent traversal.
fn sanitize_relative_path(raw_path: &str) -> Option<PathBuf> {
    let trimmed = raw_path.trim_start_matches('/');
    let candidate = if trimmed.is_empty() {
        "index.html"
    } else {
        trimmed
    };

    let mut clean = PathBuf::new();
    for component in Path::new(candidate).components() {
        match component {
            Component::Normal(segment) => clean.push(segment),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return None,
        }
    }

    if clean.as_os_str().is_empty() {
        clean.push("index.html");
    }
    Some(clean)
}

fn content_type_for(path: &Path) -> String {
    match path.extension().and_then(|ext| ext.to_str()) {
        Some("html") => String::from("text/html; charset=utf-8"),
        Some("wasm") => String::from("application/wasm"),
        Some("js" | "mjs") => String::from("text/javascript; charset=utf-8"),
        Some("css") => String::from("text/css; charset=utf-8"),
        Some("svg") => String::from("image/svg+xml"),
        Some("json") => String::from("application/json"),
        _ => mime_guess::from_path(path)
            .first_or_octet_stream()
            .to_string(),
    }
}

/// Returns `true` when `path` carries a Trunk-style lowercase hexadecimal content hash in its
/// filename (for example `tellit-ui-<hex>_bg.wasm`, `tellit-ui-<hex>.js`, or `styles-<hex>.css`).
fn is_content_hashed_asset(path: &Path) -> bool {
    let Some(ext) = path.extension().and_then(|ext| ext.to_str()) else {
        return false;
    };
    if ext.eq_ignore_ascii_case("html") {
        return false;
    }

    let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
        return false;
    };
    let stem = if ext.eq_ignore_ascii_case("wasm") {
        stem.strip_suffix("_bg").unwrap_or(stem)
    } else {
        stem
    };

    let Some((prefix, hash)) = stem.rsplit_once('-') else {
        return false;
    };
    !prefix.is_empty()
        && hash.len() >= 8
        && hash.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

fn cache_policy_for(path: &Path) -> AssetCachePolicy {
    if path
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("html"))
    {
        AssetCachePolicy::NoStore
    } else if is_content_hashed_asset(path) {
        AssetCachePolicy::Immutable
    } else {
        AssetCachePolicy::NoCache
    }
}

/// Resolves an asset from `web_dir` on disk when configured, or from embedded WASM assets otherwise.
pub async fn load_asset(raw_path: &str, web_dir: Option<&Path>) -> Option<AssetPayload> {
    let rel_path = sanitize_relative_path(raw_path)?;
    let content_type = content_type_for(&rel_path);
    let cache_policy = cache_policy_for(&rel_path);

    if let Some(dir) = web_dir {
        let full_path = dir.join(&rel_path);
        let metadata = tokio::fs::metadata(&full_path).await.ok()?;
        if !metadata.is_file() {
            return None;
        }
        let bytes = tokio::fs::read(&full_path).await.ok()?;
        return Some(AssetPayload {
            bytes: Cow::Owned(bytes),
            content_type,
            cache_policy,
        });
    }

    let key = rel_path.to_string_lossy();
    if let Some(embedded) = EmbeddedUiAssets::get(&key) {
        return Some(AssetPayload {
            bytes: embedded.data,
            content_type,
            cache_policy,
        });
    }

    if rel_path == Path::new("index.html") {
        return Some(AssetPayload {
            bytes: Cow::Borrowed(FALLBACK_INDEX_HTML),
            content_type,
            cache_policy: AssetCachePolicy::NoStore,
        });
    }

    None
}

/// Converts a resolved [`AssetPayload`] into an HTTP response, or `404 Not Found` when absent.
pub fn into_http_response(asset: Option<AssetPayload>) -> Response<Body> {
    let Some(asset) = asset else {
        return Response::builder()
            .status(StatusCode::NOT_FOUND)
            .header(header::CONTENT_TYPE, "text/plain; charset=utf-8")
            .body(Body::from("Not Found"))
            .expect("static 404 response headers are valid");
    };

    let body = match asset.bytes {
        Cow::Borrowed(static_bytes) => Body::from(static_bytes),
        Cow::Owned(owned_bytes) => Body::from(owned_bytes),
    };

    let mut builder = Response::builder().status(StatusCode::OK);
    if let Ok(ct) = HeaderValue::from_str(&asset.content_type) {
        builder = builder.header(header::CONTENT_TYPE, ct);
    }
    match asset.cache_policy {
        AssetCachePolicy::NoStore => {
            builder = builder
                .header(header::CACHE_CONTROL, "no-cache, no-store, must-revalidate")
                .header(header::PRAGMA, "no-cache")
                .header(header::EXPIRES, "0");
        }
        AssetCachePolicy::NoCache => {
            builder = builder.header(header::CACHE_CONTROL, "no-cache");
        }
        AssetCachePolicy::Immutable => {
            builder = builder.header(header::CACHE_CONTROL, "public, max-age=31536000, immutable");
        }
    }
    builder
        .body(body)
        .expect("asset response headers are valid")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_rejects_parent_traversal_and_normalizes_root() {
        assert_eq!(
            sanitize_relative_path("/"),
            Some(PathBuf::from("index.html"))
        );
        assert_eq!(
            sanitize_relative_path(""),
            Some(PathBuf::from("index.html"))
        );
        assert_eq!(
            sanitize_relative_path("/favicon.svg"),
            Some(PathBuf::from("favicon.svg"))
        );
        assert!(sanitize_relative_path("/../Cargo.toml").is_none());
        assert!(sanitize_relative_path("assets/../../secret").is_none());
    }

    #[test]
    fn cache_policy_distinguishes_html_hashed_and_unhashed_assets() {
        assert_eq!(
            cache_policy_for(Path::new("index.html")),
            AssetCachePolicy::NoStore
        );
        assert_eq!(
            cache_policy_for(Path::new("index-8aaef9e7a356ea33.html")),
            AssetCachePolicy::NoStore
        );
        assert_eq!(
            cache_policy_for(Path::new("tellit-ui-8aaef9e7a356ea33_bg.wasm")),
            AssetCachePolicy::Immutable
        );
        assert_eq!(
            cache_policy_for(Path::new("tellit-ui-8aaef9e7a356ea33.js")),
            AssetCachePolicy::Immutable
        );
        assert_eq!(
            cache_policy_for(Path::new("styles-0123456789abcdef.css")),
            AssetCachePolicy::Immutable
        );
        assert_eq!(
            cache_policy_for(Path::new("favicon.svg")),
            AssetCachePolicy::NoCache
        );
        assert_eq!(
            cache_policy_for(Path::new("favicon-white.svg")),
            AssetCachePolicy::NoCache
        );
        assert_eq!(
            cache_policy_for(Path::new("app.wasm")),
            AssetCachePolicy::NoCache
        );
        assert_eq!(
            cache_policy_for(Path::new("styles.css")),
            AssetCachePolicy::NoCache
        );
        assert_eq!(
            cache_policy_for(Path::new("app-1234567.js")),
            AssetCachePolicy::NoCache
        );
    }
}
