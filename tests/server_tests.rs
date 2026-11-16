//! Integration tests for the `tellit` `axum` HTTP server and `tellit launch --background`.

extern crate alloc;

use core::net::{IpAddr, Ipv4Addr, SocketAddr};
use core::str::FromStr;
use core::time::Duration;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

use reqwest::StatusCode;
use tellit_cli::server::{BackgroundServerInfo, ServerConfig, start_server};
use tellit_types::{HeartbeatEvent, JjStatusResponse};
use tempfile::tempdir;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

#[tokio::test]
async fn serves_index_and_static_wasm_assets_with_expected_headers() {
    let dir = tempdir().unwrap();
    let web_dir = dir.path().join("web");
    fs::create_dir_all(&web_dir).unwrap();
    fs::write(
        web_dir.join("index.html"),
        "<!doctype html><html><body>tellit leptos test</body></html>",
    )
    .unwrap();
    fs::write(web_dir.join("app.wasm"), b"\0asm\x01\x00\x00\x00").unwrap();
    fs::write(web_dir.join("app.js"), "export default {};").unwrap();
    fs::write(web_dir.join("styles.css"), "body { margin: 0; }").unwrap();
    fs::write(web_dir.join("favicon.svg"), "<svg></svg>").unwrap();

    let server = start_server(
        ServerConfig::new(dir.path().to_path_buf())
            .with_port(Some(0))
            .with_web_dir(Some(web_dir))
            .with_keep_alive(true),
    )
    .await
    .unwrap();

    let client = reqwest::Client::new();

    let root_resp = client.get(format!("{}/", server.url)).send().await.unwrap();
    assert_eq!(root_resp.status(), StatusCode::OK);
    assert_eq!(
        root_resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok()),
        Some("text/html; charset=utf-8")
    );
    assert_eq!(
        root_resp
            .headers()
            .get(reqwest::header::CACHE_CONTROL)
            .and_then(|v| v.to_str().ok()),
        Some("no-cache, no-store, must-revalidate")
    );
    let html = root_resp.text().await.unwrap();
    assert!(html.contains("tellit leptos test"));

    let wasm_resp = client
        .get(format!("{}/app.wasm", server.url))
        .send()
        .await
        .unwrap();
    assert_eq!(wasm_resp.status(), StatusCode::OK);
    assert_eq!(
        wasm_resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok()),
        Some("application/wasm")
    );

    let js_resp = client
        .get(format!("{}/app.js", server.url))
        .send()
        .await
        .unwrap();
    assert_eq!(js_resp.status(), StatusCode::OK);
    assert_eq!(
        js_resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok()),
        Some("text/javascript; charset=utf-8")
    );

    let missing_resp = client
        .get(format!("{}/does-not-exist.js", server.url))
        .send()
        .await
        .unwrap();
    assert_eq!(missing_resp.status(), StatusCode::NOT_FOUND);

    server.shutdown();
    server.wait().await.unwrap();
}

#[tokio::test]
async fn serves_embedded_root_html_without_external_web_dir() {
    let dir = tempdir().unwrap();
    let server = start_server(
        ServerConfig::new(dir.path().to_path_buf())
            .with_port(Some(0))
            .with_web_dir(None)
            .with_keep_alive(true),
    )
    .await
    .unwrap();

    let resp = reqwest::get(format!("{}/", server.url)).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let html = resp.text().await.unwrap();
    assert!(html.contains("<title>tellit"));

    server.shutdown();
    server.wait().await.unwrap();
}

#[tokio::test]
async fn jj_status_reports_jj_and_non_jj_directories() {
    let non_jj_dir = tempdir().unwrap();
    let non_jj_server = start_server(
        ServerConfig::new(non_jj_dir.path().to_path_buf())
            .with_port(Some(0))
            .with_keep_alive(true),
    )
    .await
    .unwrap();

    let client = reqwest::Client::new();
    let non_jj_status: JjStatusResponse = client
        .get(format!("{}/api/jj/status", non_jj_server.url))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(non_jj_status, JjStatusResponse::not_jj());

    non_jj_server.shutdown();
    non_jj_server.wait().await.unwrap();

    let jj_dir = tempdir().unwrap();
    let init_status = Command::new("jj")
        .args(["git", "init"])
        .current_dir(jj_dir.path())
        .output()
        .expect("jj must be available in test environment");
    assert!(init_status.status.success());

    let canonical_jj_root = jj_dir
        .path()
        .canonicalize()
        .unwrap()
        .to_string_lossy()
        .into_owned();

    let jj_server = start_server(
        ServerConfig::new(jj_dir.path().to_path_buf())
            .with_port(Some(0))
            .with_keep_alive(true),
    )
    .await
    .unwrap();

    let jj_status: JjStatusResponse = client
        .get(format!("{}/api/jj/status", jj_server.url))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(jj_status.is_jj);
    assert_eq!(
        PathBuf::from(jj_status.repo_root.unwrap())
            .canonicalize()
            .unwrap()
            .to_string_lossy(),
        canonical_jj_root
    );

    jj_server.shutdown();
    jj_server.wait().await.unwrap();
}

fn extract_sse_data_lines(raw: &str) -> Vec<HeartbeatEvent> {
    raw.lines()
        .filter_map(|line| line.strip_prefix("data:"))
        .filter_map(|payload| HeartbeatEvent::from_str(payload.trim()).ok())
        .collect()
}

#[tokio::test]
async fn heartbeat_streams_events_and_shuts_down_after_disconnect_without_keep_alive() {
    let dir = tempdir().unwrap();
    let server = start_server(
        ServerConfig::new(dir.path().to_path_buf())
            .with_port(Some(0))
            .with_keep_alive(false)
            .with_heartbeat_timings(Duration::from_millis(30), Duration::from_millis(30)),
    )
    .await
    .unwrap();

    let client = reqwest::Client::new();
    let mut resp = client
        .get(format!("{}/api/heartbeat", server.url))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let content_type = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();
    assert!(content_type.starts_with("text/event-stream"));

    let mut received = Vec::new();
    while received.len() < 2 {
        let chunk = tokio::time::timeout(Duration::from_secs(2), resp.chunk())
            .await
            .expect("heartbeat stream timed out")
            .unwrap()
            .expect("heartbeat stream ended prematurely");
        let text = String::from_utf8_lossy(&chunk);
        received.extend(extract_sse_data_lines(&text));
    }

    assert_eq!(received[0], HeartbeatEvent::Connected);
    assert_eq!(received[1], HeartbeatEvent::Ping);

    // Dropping the SSE response simulates closing the browser tab; with keep_alive = false,
    // the server must exit on its own once the disconnect grace window elapses.
    drop(resp);
    drop(client);

    tokio::time::timeout(Duration::from_secs(2), server.wait())
        .await
        .expect("server should auto-shutdown after last heartbeat client disconnects")
        .unwrap();
}

#[tokio::test]
async fn heartbeat_disconnect_keeps_server_running_when_keep_alive_is_enabled() {
    let dir = tempdir().unwrap();
    let server = start_server(
        ServerConfig::new(dir.path().to_path_buf())
            .with_port(Some(0))
            .with_keep_alive(true)
            .with_heartbeat_timings(Duration::from_millis(30), Duration::from_millis(20)),
    )
    .await
    .unwrap();

    let client = reqwest::Client::new();
    let mut resp = client
        .get(format!("{}/api/heartbeat", server.url))
        .send()
        .await
        .unwrap();
    let first_chunk = resp.chunk().await.unwrap().unwrap();
    assert_eq!(
        extract_sse_data_lines(&String::from_utf8_lossy(&first_chunk)),
        vec![HeartbeatEvent::Connected]
    );
    drop(resp);

    tokio::time::sleep(Duration::from_millis(80)).await;

    let status_resp = client
        .get(format!("{}/api/jj/status", server.url))
        .send()
        .await
        .unwrap();
    assert_eq!(status_resp.status(), StatusCode::OK);

    server.shutdown();
    server.wait().await.unwrap();
}

#[tokio::test]
async fn port_fallback_advances_when_preferred_port_is_occupied() {
    let occupied = TcpListener::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0))
        .await
        .unwrap();
    let busy_port = occupied.local_addr().unwrap().port();

    let dir = tempdir().unwrap();
    let server = start_server(
        ServerConfig::new(dir.path().to_path_buf())
            .with_port(Some(busy_port))
            .with_keep_alive(true)
            .with_quiet(true),
    )
    .await
    .unwrap();

    assert!(
        server.port > busy_port,
        "expected fallback port > {busy_port}, got {}",
        server.port
    );
    assert_eq!(server.url, format!("http://127.0.0.1:{}", server.port));

    let resp = reqwest::get(format!("{}/api/jj/status", server.url))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    server.shutdown();
    server.wait().await.unwrap();
    drop(occupied);
}

#[cfg(unix)]
struct DaemonKillGuard {
    pid: u32,
}

#[cfg(unix)]
impl Drop for DaemonKillGuard {
    fn drop(&mut self) {
        let _ = Command::new("kill")
            .args(["-TERM", &self.pid.to_string()])
            .status();
    }
}

#[cfg(unix)]
#[tokio::test]
async fn cli_launch_background_emits_json_and_serves_requests() {
    let jj_dir = tempdir().unwrap();
    let init = Command::new("jj")
        .args(["git", "init"])
        .current_dir(jj_dir.path())
        .output()
        .unwrap();
    assert!(init.status.success());

    let occupied = TcpListener::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0))
        .await
        .unwrap();
    let busy_port = occupied.local_addr().unwrap().port();

    let tellit_bin = env!("CARGO_BIN_EXE_tellit");
    let busy_port_arg = busy_port.to_string();
    let repo_dir = jj_dir.path().to_path_buf();

    let output = tokio::task::spawn_blocking(move || {
        Command::new(tellit_bin)
            .args(["launch", "--background", "--port", &busy_port_arg])
            .current_dir(repo_dir)
            .output()
            .unwrap()
    })
    .await
    .unwrap();

    assert!(
        output.status.success(),
        "tellit launch --background failed: stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout_str = String::from_utf8(output.stdout).unwrap();
    let info: BackgroundServerInfo = serde_json::from_str(stdout_str.trim()).unwrap();
    let _guard = DaemonKillGuard { pid: info.pid };

    assert!(info.port > busy_port);
    assert_eq!(info.url, format!("http://127.0.0.1:{}", info.port));
    assert!(info.pid > 0);

    let status: JjStatusResponse = reqwest::get(format!("{}/api/jj/status", info.url))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(status.is_jj);

    let index_resp = reqwest::get(format!("{}/", info.url)).await.unwrap();
    assert_eq!(index_resp.status(), StatusCode::OK);

    drop(occupied);
}

#[tokio::test]
async fn web_dir_isolates_assets_and_blocks_http_path_traversal() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("secret.txt"), "top-secret-outside-web-dir").unwrap();

    let web_dir = dir.path().join("web");
    fs::create_dir_all(web_dir.join("nested")).unwrap();
    fs::write(web_dir.join("custom.txt"), "only-in-web-dir").unwrap();

    let server = start_server(
        ServerConfig::new(dir.path().to_path_buf())
            .with_port(Some(0))
            .with_web_dir(Some(web_dir))
            .with_keep_alive(true),
    )
    .await
    .unwrap();

    let client = reqwest::Client::new();

    // Present in web_dir -> 200 OK.
    let ok_resp = client
        .get(format!("{}/custom.txt", server.url))
        .send()
        .await
        .unwrap();
    assert_eq!(ok_resp.status(), StatusCode::OK);

    // Absent from web_dir (including index.html, directory paths, and embedded-only files)
    // must 404 rather than mixing assets from EmbeddedUiAssets.
    for path in ["/", "/index.html", "/nested", "/favicon-white.svg"] {
        let resp = client
            .get(format!("{}{path}", server.url))
            .send()
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::NOT_FOUND,
            "expected 404 for {path} under isolated web_dir"
        );
    }

    // Percent-encoded parent traversal attempts must return 404 without leaking secret.txt.
    for attempt in [
        "/%2e%2e/secret.txt",
        "/assets/%2e%2e/%2e%2e/secret.txt",
        "/nested/%2e%2e/%2e%2e/secret.txt",
    ] {
        let resp = client
            .get(format!("{}{attempt}", server.url))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND, "traversal {attempt}");
        let body = resp.text().await.unwrap();
        assert!(!body.contains("top-secret"));
    }

    // Also send an unnormalized raw HTTP/1.1 traversal target directly on the socket.
    let mut raw_stream = TcpStream::connect(server.addr).await.unwrap();
    raw_stream
        .write_all(b"GET /../secret.txt HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let mut raw_resp = String::new();
    raw_stream.read_to_string(&mut raw_resp).await.unwrap();
    assert!(
        raw_resp.starts_with("HTTP/1.1 404") || raw_resp.starts_with("HTTP/1.1 400"),
        "unexpected raw response: {raw_resp}"
    );
    assert!(!raw_resp.contains("top-secret"));

    server.shutdown();
    server.wait().await.unwrap();
}

#[tokio::test]
async fn heartbeat_multi_tab_and_reconnect_cancels_stale_grace_timer() {
    let dir = tempdir().unwrap();
    let grace = Duration::from_millis(140);
    let server = start_server(
        ServerConfig::new(dir.path().to_path_buf())
            .with_port(Some(0))
            .with_keep_alive(false)
            .with_heartbeat_timings(Duration::from_secs(5), grace),
    )
    .await
    .unwrap();

    // Use separate no-pool clients so dropping a response immediately closes its TCP socket.
    let make_client = || {
        reqwest::Client::builder()
            .pool_max_idle_per_host(0)
            .build()
            .unwrap()
    };

    let client1 = make_client();
    let mut tab1 = client1
        .get(format!("{}/api/heartbeat", server.url))
        .send()
        .await
        .unwrap();
    let chunk1 = tab1.chunk().await.unwrap().unwrap();
    assert_eq!(
        extract_sse_data_lines(&String::from_utf8_lossy(&chunk1)),
        vec![HeartbeatEvent::Connected]
    );

    let client2 = make_client();
    let mut tab2 = client2
        .get(format!("{}/api/heartbeat", server.url))
        .send()
        .await
        .unwrap();
    let chunk2 = tab2.chunk().await.unwrap().unwrap();
    assert_eq!(
        extract_sse_data_lines(&String::from_utf8_lossy(&chunk2)),
        vec![HeartbeatEvent::Connected]
    );

    // (a) Closing tab 1 while tab 2 stays open must NOT shut down the server past `grace`.
    drop(tab1);
    drop(client1);
    tokio::time::sleep(Duration::from_millis(200)).await;
    let alive = reqwest::get(format!("{}/api/jj/status", server.url))
        .await
        .unwrap();
    assert_eq!(alive.status(), StatusCode::OK);

    // (b) Closing tab 2 starts grace timer #1 (fires at ~140ms). Reconnecting tab 3 at 50ms
    // and closing tab 3 at 100ms starts grace timer #2 (fires at ~240ms). At 165ms, timer #1
    // has already woken up, yet the server must still be running until timer #2's full window.
    drop(tab2);
    drop(client2);
    tokio::time::sleep(Duration::from_millis(50)).await;

    let client3 = make_client();
    let mut tab3 = client3
        .get(format!("{}/api/heartbeat", server.url))
        .send()
        .await
        .unwrap();
    let chunk3 = tab3.chunk().await.unwrap().unwrap();
    assert_eq!(
        extract_sse_data_lines(&String::from_utf8_lossy(&chunk3)),
        vec![HeartbeatEvent::Connected]
    );

    tokio::time::sleep(Duration::from_millis(50)).await;
    drop(tab3);
    drop(client3);

    // 65ms after tab 3 closed (~165ms after tab 2 closed): timer #1 has fired, timer #2 has not.
    tokio::time::sleep(Duration::from_millis(65)).await;
    let still_alive = reqwest::get(format!("{}/api/jj/status", server.url))
        .await
        .unwrap();
    assert_eq!(still_alive.status(), StatusCode::OK);

    // Once timer #2's grace window elapses, the server shuts down cleanly.
    tokio::time::timeout(Duration::from_secs(2), server.wait())
        .await
        .expect("server should shut down once latest disconnect grace elapses")
        .unwrap();
}

#[tokio::test]
async fn cli_launch_background_reports_startup_failure_on_invalid_host() {
    let dir = tempdir().unwrap();
    let tellit_bin = env!("CARGO_BIN_EXE_tellit");
    let repo_dir = dir.path().to_path_buf();

    let output = tokio::task::spawn_blocking(move || {
        Command::new(tellit_bin)
            .args([
                "launch",
                "--background",
                "--host",
                "unresolvable-host.invalid",
            ])
            .current_dir(repo_dir)
            .output()
            .unwrap()
    })
    .await
    .unwrap();

    assert!(
        !output.status.success(),
        "expected non-zero exit when --host cannot resolve"
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).trim().is_empty(),
        "failed background startup must not emit partial JSON on stdout"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("unresolvable-host.invalid"),
        "expected host failure details on stderr, got: {stderr}"
    );
}
