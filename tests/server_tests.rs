//! Integration tests for the `tellit` `axum` HTTP server and `tellit launch --background`.

extern crate alloc;

use core::net::{IpAddr, Ipv4Addr, SocketAddr};
use core::str::FromStr;
use core::time::Duration;
use std::fs;
use std::io::Read;
use std::path::PathBuf;
use std::process::Command;

use flate2::read::GzDecoder;
use reqwest::StatusCode;
use tellit_cli::server::{BackgroundServerInfo, ServerConfig, start_server};
use tellit_types::{
    ApproveCommitRequest, ApproveCommitResponse, CommentSide, CreateCommentRequest,
    CreateCommentResponse, HeartbeatEvent, JjChangesResponse, JjCommentsResponse,
    JjCommitsResponse, JjDiffResponse, JjStatusResponse, LineRangeSpec, ReplyCommentRequest,
    ReplyCommentResponse, ResolveCommentRequest, ResolveCommentResponse, WatchEvent, WatchSource,
};
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

fn run_jj_test(repo_dir: &std::path::Path, args: &[&str]) -> String {
    let out = Command::new("jj")
        .env("JJ_CONFIG", "/dev/null")
        .args(["--no-pager"])
        .args(args)
        .current_dir(repo_dir)
        .output()
        .expect("jj must run in integration test");
    assert!(
        out.status.success(),
        "jj {:?} failed: stderr={}",
        args,
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn setup_jj_test_repo() -> tempfile::TempDir {
    let dir = tempdir().unwrap();
    run_jj_test(dir.path(), &["git", "init"]);
    run_jj_test(
        dir.path(),
        &["config", "set", "--repo", "user.name", "Test User"],
    );
    run_jj_test(
        dir.path(),
        &["config", "set", "--repo", "user.email", "test@example.com"],
    );
    dir
}

fn extract_watch_events(raw: &str) -> Vec<WatchEvent> {
    raw.lines()
        .filter_map(|line| line.strip_prefix("data:"))
        .filter_map(|payload| serde_json::from_str::<WatchEvent>(payload.trim()).ok())
        .collect()
}

#[tokio::test]
async fn jj_changes_evolog_rebase_isolation_and_clean_rebase_approval_endpoints() {
    let repo = setup_jj_test_repo();
    let root = repo.path();

    // Base commit A on trunk.
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/upstream.txt"), "v1\n").unwrap();
    fs::write(root.join("src/lib.rs"), "pub fn greet() -> i32 { 1 }\n").unwrap();
    run_jj_test(root, &["describe", "-m", "base"]);
    let base_a_commit = run_jj_test(root, &["log", "-r", "@", "--no-graph", "-T", "commit_id"]);

    // Feature change B (revision #1 -> #2).
    run_jj_test(root, &["new", "-m", "feat: greet"]);
    fs::write(root.join("src/lib.rs"), "pub fn greet() -> i32 { 2 }\n").unwrap();
    let b_change_id = run_jj_test(root, &["log", "-r", "@", "--no-graph", "-T", "change_id"]);
    let b_v1_commit = run_jj_test(root, &["log", "-r", "@", "--no-graph", "-T", "commit_id"]);

    fs::write(root.join("src/lib.rs"), "pub fn greet() -> i32 { 42 }\n").unwrap();
    run_jj_test(root, &["status"]);
    let b_v2_commit = run_jj_test(root, &["log", "-r", "@", "--no-graph", "-T", "commit_id"]);
    assert_ne!(b_v1_commit, b_v2_commit);

    // Advance upstream with an unrelated file change, then rebase B onto it cleanly.
    run_jj_test(root, &["new", &base_a_commit, "-m", "upstream: bump"]);
    fs::write(root.join("src/upstream.txt"), "v2\n").unwrap();
    run_jj_test(root, &["status"]);
    let upstream_a2_commit =
        run_jj_test(root, &["log", "-r", "@", "--no-graph", "-T", "commit_id"]);

    run_jj_test(
        root,
        &["rebase", "-r", &b_change_id, "-d", &upstream_a2_commit],
    );
    let b_v3_commit = run_jj_test(
        root,
        &["log", "-r", &b_change_id, "--no-graph", "-T", "commit_id"],
    );
    assert_ne!(b_v2_commit, b_v3_commit);

    let server = start_server(
        ServerConfig::new(root.to_path_buf())
            .with_port(Some(0))
            .with_keep_alive(true),
    )
    .await
    .unwrap();
    let client = reqwest::Client::new();

    // 1. Query single change via ?query=
    let single_resp: JjChangesResponse = client
        .get(format!("{}/api/jj/changes?query={b_change_id}", server.url))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(single_resp.changes.len(), 1);
    assert_eq!(single_resp.changes[0].change_id, b_change_id);

    // 2. Approve revision #2 (`b_v2_commit`) and verify clean-rebase approval carries onto #3 (`b_v3_commit`).
    let approve_resp: ApproveCommitResponse = client
        .post(format!("{}/api/jj/approve", server.url))
        .json(&ApproveCommitRequest {
            commit_id: b_v2_commit.clone(),
            disapprove: false,
        })
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(approve_resp.approved);

    let commits_resp: JjCommitsResponse = client
        .get(format!(
            "{}/api/jj/changes/{b_change_id}/commits",
            server.url
        ))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let head_commit = commits_resp
        .commits
        .iter()
        .find(|c| c.commit_id == b_v3_commit)
        .expect("rebased commit must be in evolog");
    assert!(!head_commit.approved);
    assert!(head_commit.clean_rebase_approved);
    assert_eq!(
        head_commit.clean_rebase_source_commit_id.as_deref(),
        Some(b_v2_commit.as_str())
    );

    // `GET /api/jj/changes` reflects clean_rebase_approved on the change graph row too.
    let graph_resp: JjChangesResponse = client
        .get(format!("{}/api/jj/changes", server.url))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let graph_change = graph_resp
        .changes
        .iter()
        .find(|c| c.change_id == b_change_id)
        .expect("feature change must appear in jj log graph");
    assert!(graph_change.clean_rebase_approved);

    // 3. Rebase-isolated interdiff between `b_v2_commit` and `b_v3_commit` hides `src/upstream.txt`.
    let isolated_diff: JjDiffResponse = client
        .get(format!(
            "{}/api/jj/diff?target={b_v3_commit}&base={b_v2_commit}&ignoreRebase=true",
            server.url
        ))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(isolated_diff.is_rebase_diff);
    assert!(isolated_diff.rebase_ignored);
    assert!(!isolated_diff.rebase_conflict);
    assert_eq!(isolated_diff.files[0].path, "Commit Message");
    assert!(
        !isolated_diff
            .files
            .iter()
            .any(|f| f.path == "src/upstream.txt")
    );
    assert!(isolated_diff.is_empty);

    // With `ignoreRebase=false`, the upstream rebase changes appear.
    let full_interdiff: JjDiffResponse = client
        .get(format!(
            "{}/api/jj/diff?target={b_v3_commit}&base={b_v2_commit}&ignoreRebase=false",
            server.url
        ))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(full_interdiff.is_rebase_diff);
    assert!(!full_interdiff.rebase_ignored);
    assert!(
        full_interdiff
            .files
            .iter()
            .any(|f| f.path == "src/upstream.txt")
    );

    // Parent diff includes virtual Commit Message and highlighted `src/lib.rs`.
    let parent_diff: JjDiffResponse = client
        .get(format!("{}/api/jj/diff?target={b_v3_commit}", server.url))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(parent_diff.files[0].path, "Commit Message");
    let lib_file = parent_diff
        .files
        .iter()
        .find(|f| f.path == "src/lib.rs")
        .expect("src/lib.rs must appear in parent diff");
    assert!(!lib_file.chunks[0].lines[0].syntax_spans.is_empty());

    server.shutdown();
    server.wait().await.unwrap();
}

#[tokio::test]
async fn jj_comments_crud_and_watch_sse_stream_work_end_to_end() {
    let repo = setup_jj_test_repo();
    let root = repo.path();

    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/main.rs"), "fn main() {}\n").unwrap();
    run_jj_test(root, &["describe", "-m", "init"]);
    let base_commit = run_jj_test(root, &["log", "-r", "@", "--no-graph", "-T", "commit_id"]);

    run_jj_test(root, &["new", "-m", "feat: edit main"]);
    fs::write(
        root.join("src/main.rs"),
        "fn main() {\n    println!(\"hi\");\n}\n",
    )
    .unwrap();
    let target_commit_1 = run_jj_test(root, &["log", "-r", "@", "--no-graph", "-T", "commit_id"]);

    fs::write(
        root.join("src/main.rs"),
        "fn main() {\n    println!(\"hello\");\n}\n",
    )
    .unwrap();
    let target_commit_2 = run_jj_test(root, &["log", "-r", "@", "--no-graph", "-T", "commit_id"]);
    assert_ne!(base_commit, target_commit_1);
    assert_ne!(target_commit_1, target_commit_2);

    let server = start_server(
        ServerConfig::new(root.to_path_buf())
            .with_port(Some(0))
            .with_keep_alive(true),
    )
    .await
    .unwrap();
    let client = reqwest::Client::new();

    // Connect `/api/watch` and confirm initial Connected event.
    let mut watch_resp = client
        .get(format!("{}/api/watch", server.url))
        .send()
        .await
        .unwrap();
    assert_eq!(watch_resp.status(), StatusCode::OK);
    let first_chunk = tokio::time::timeout(Duration::from_secs(2), watch_resp.chunk())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(
        extract_watch_events(&String::from_utf8_lossy(&first_chunk)),
        vec![WatchEvent::Connected]
    );

    // Create thread on `target_commit_1` (will map to Old when comparing target_commit_1 -> target_commit_2).
    let created_old: CreateCommentResponse = client
        .post(format!("{}/api/jj/comments", server.url))
        .json(&CreateCommentRequest {
            commit_id: target_commit_1.clone(),
            file_path: String::from("src/main.rs"),
            line_range: Some(LineRangeSpec { start: 2, end: 2 }),
            content: String::from("nit on v1"),
        })
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    // Watch SSE stream must receive CommentsChanged.
    let watch_chunk = tokio::time::timeout(Duration::from_secs(2), watch_resp.chunk())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(
        extract_watch_events(&String::from_utf8_lossy(&watch_chunk))
            .contains(&WatchEvent::CommentsChanged)
    );

    // Reply and resolve.
    let reply_resp: ReplyCommentResponse = client
        .post(format!(
            "{}/api/jj/comments/{}/reply",
            server.url, created_old.thread_id
        ))
        .json(&ReplyCommentRequest {
            content: String::from("Fixed in v2"),
        })
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(reply_resp.thread_id, created_old.thread_id);

    let resolve_resp: ResolveCommentResponse = client
        .post(format!(
            "{}/api/jj/comments/{}/resolve",
            server.url, created_old.thread_id
        ))
        .json(&ResolveCommentRequest { unresolve: false })
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(resolve_resp.resolved);

    let comments_resp: JjCommentsResponse = client
        .get(format!(
            "{}/api/jj/comments?target={target_commit_2}&base={target_commit_1}",
            server.url
        ))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(comments_resp.threads.len(), 1);
    assert_eq!(comments_resp.threads[0].side, CommentSide::Old);
    assert!(comments_resp.threads[0].resolved);
    assert_eq!(
        comments_resp.threads[0].comments,
        vec!["nit on v1", "Fixed in v2"]
    );

    // Touch a working tree file and confirm WatchEvent::Reload(WorkingTree) arrives on `/api/watch`.
    fs::write(root.join("src/main.rs"), "fn main() { /* edited */ }\n").unwrap();
    let mut saw_working_reload = false;
    let mut seen_events = Vec::new();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
    while tokio::time::Instant::now() < deadline {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        match tokio::time::timeout(remaining, watch_resp.chunk()).await {
            Ok(Ok(Some(chunk))) => {
                let events = extract_watch_events(&String::from_utf8_lossy(&chunk));
                if events.contains(&WatchEvent::Reload {
                    source: WatchSource::WorkingTree,
                }) {
                    saw_working_reload = true;
                    break;
                }
                seen_events.extend(events);
            }
            Ok(Ok(None)) => break,
            Ok(Err(err)) => panic!("watch stream read error: {err}"),
            Err(_) => break,
        }
    }
    assert!(
        saw_working_reload,
        "did not observe WorkingTree reload; seen events: {seen_events:?}"
    );

    drop(watch_resp);
    server.shutdown();
    server.wait().await.unwrap();
}

#[tokio::test]
async fn blob_line_count_and_open_in_editor_enforce_safety_and_serve_content() {
    let repo = setup_jj_test_repo();
    let root = repo.path();

    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("src/lib.rs"),
        "pub fn one() {}\npub fn two() {}\n",
    )
    .unwrap();
    run_jj_test(root, &["describe", "-m", "v1"]);
    let rev1 = run_jj_test(root, &["log", "-r", "@", "--no-graph", "-T", "commit_id"]);

    run_jj_test(root, &["new", "-m", "v2"]);
    fs::write(
        root.join("src/lib.rs"),
        "pub fn one() {}\npub fn two() {}\npub fn three() {}\n",
    )
    .unwrap();
    fs::write(
        root.join("logo.svg"),
        "<svg xmlns=\"http://www.w3.org/2000/svg\"/>\n",
    )
    .unwrap();
    let rev2 = run_jj_test(root, &["log", "-r", "@", "--no-graph", "-T", "commit_id"]);

    let server = start_server(
        ServerConfig::new(root.to_path_buf())
            .with_port(Some(0))
            .with_keep_alive(true),
    )
    .await
    .unwrap();
    let client = reqwest::Client::new();

    // Blob serves exact revision contents and strict headers for SVG.
    let blob_v1 = client
        .get(format!("{}/api/blob/src/lib.rs?ref={rev1}", server.url))
        .send()
        .await
        .unwrap();
    assert_eq!(blob_v1.status(), StatusCode::OK);
    assert_eq!(
        blob_v1.text().await.unwrap(),
        "pub fn one() {}\npub fn two() {}\n"
    );

    let svg_resp = client
        .get(format!("{}/api/blob/logo.svg?ref={rev2}", server.url))
        .send()
        .await
        .unwrap();
    assert_eq!(svg_resp.status(), StatusCode::OK);
    assert_eq!(svg_resp.headers()["content-type"], "image/svg+xml");
    assert_eq!(svg_resp.headers()["x-content-type-options"], "nosniff");
    assert!(svg_resp.headers().contains_key("content-security-policy"));

    // Path traversal (backslash-normalized, symlink escape, flag-like ref) and VCS metadata files are rejected.
    #[cfg(unix)]
    std::os::unix::fs::symlink("/etc/hosts", root.join("outside.txt")).unwrap();
    #[cfg(unix)]
    {
        let symlink_blob = client
            .get(format!("{}/api/blob/outside.txt?ref=working", server.url))
            .send()
            .await
            .unwrap();
        assert_eq!(symlink_blob.status(), StatusCode::BAD_REQUEST);
    }

    let bad_blob = client
        .get(format!(
            "{}/api/blob/..%5Cetc%5Cpasswd?ref=working",
            server.url
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(bad_blob.status(), StatusCode::BAD_REQUEST);

    let flag_ref_blob = client
        .get(format!("{}/api/blob/src/lib.rs?ref=--help", server.url))
        .send()
        .await
        .unwrap();
    assert_eq!(flag_ref_blob.status(), StatusCode::BAD_REQUEST);

    let bad_git = client
        .get(format!("{}/api/blob/.git/config?ref=working", server.url))
        .send()
        .await
        .unwrap();
    assert_eq!(bad_git.status(), StatusCode::BAD_REQUEST);

    // Line-count returns exact old and new counts and handles added files gracefully.
    let counts: tellit_types::LineCountResponse = client
        .get(format!(
            "{}/api/line-count/src/lib.rs?oldRef={rev1}&newRef={rev2}",
            server.url
        ))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(counts.old_line_count, Some(2));
    assert_eq!(counts.new_line_count, Some(3));

    let added_counts: tellit_types::LineCountResponse = client
        .get(format!(
            "{}/api/line-count/logo.svg?oldRef={rev1}&newRef={rev2}",
            server.url
        ))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(added_counts.old_line_count, Some(0));
    assert_eq!(added_counts.new_line_count, Some(1));

    // Editor endpoint blocks traversal and shell interpreters.
    let traversal_editor = client
        .post(format!("{}/api/open-in-editor", server.url))
        .json(&tellit_types::OpenInEditorRequest {
            file_path: String::from("../secret.rs"),
            line: Some(1),
            editor: None,
        })
        .send()
        .await
        .unwrap();
    assert_eq!(traversal_editor.status(), StatusCode::BAD_REQUEST);

    let shell_editor = client
        .post(format!("{}/api/open-in-editor", server.url))
        .json(&tellit_types::OpenInEditorRequest {
            file_path: String::from("src/lib.rs"),
            line: Some(2),
            editor: Some(tellit_types::EditorSpecRequest {
                id: Some(String::from("custom")),
                command: Some(String::from("bash")),
                args_template: Some(String::from("-c echo")),
            }),
        })
        .send()
        .await
        .unwrap();
    assert_eq!(shell_editor.status(), StatusCode::BAD_REQUEST);

    server.shutdown();
    server.wait().await.unwrap();
}

#[tokio::test]
async fn user_settings_get_and_put_persist_and_validate_payloads() {
    let dir = tempfile::tempdir().unwrap();
    let cfg_path = dir.path().join("tellit").join("config.json");
    let config = ServerConfig::new(dir.path().to_path_buf())
        .with_port(Some(0))
        .with_keep_alive(true)
        .with_quiet(true)
        .with_user_config_path(Some(cfg_path.clone()));
    let server = start_server(config).await.unwrap();
    let client = reqwest::Client::new();

    // Missing config file yields version 1 with empty client object.
    let initial: serde_json::Value = client
        .get(format!("{}/api/user-settings", server.url))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(initial, serde_json::json!({ "version": 1, "client": {} }));

    // PUT persists and shallow-merges client keys onto disk.
    let put_resp: serde_json::Value = client
        .put(format!("{}/api/user-settings", server.url))
        .json(&serde_json::json!({
            "client": {
                "appearance": { "theme": "light", "fontSize": 15 },
                "preserved": "yes"
            }
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(put_resp["client"]["preserved"], "yes");
    assert_eq!(put_resp["client"]["appearance"]["theme"], "light");

    let merge_resp: serde_json::Value = client
        .put(format!("{}/api/user-settings", server.url))
        .json(&serde_json::json!({
            "client": {
                "appearance": { "theme": "dark", "fontSize": 16 }
            }
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(merge_resp["client"]["preserved"], "yes");
    assert_eq!(merge_resp["client"]["appearance"]["fontSize"], 16);

    let on_disk: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&cfg_path).unwrap()).unwrap();
    assert_eq!(on_disk, merge_resp);

    // Non-object client patch returns 400 Bad Request.
    let bad = client
        .put(format!("{}/api/user-settings", server.url))
        .json(&serde_json::json!({ "client": "invalid" }))
        .send()
        .await
        .unwrap();
    assert_eq!(bad.status(), StatusCode::BAD_REQUEST);

    server.shutdown();
    server.wait().await.unwrap();
}

fn gunzip_bytes(compressed: &[u8]) -> Vec<u8> {
    let mut decoder = GzDecoder::new(compressed);
    let mut out = Vec::new();
    decoder
        .read_to_end(&mut out)
        .expect("response body must be valid gzip");
    out
}

fn header_str(resp: &reqwest::Response, name: reqwest::header::HeaderName) -> Option<&str> {
    resp.headers().get(name).and_then(|v| v.to_str().ok())
}

#[tokio::test]
async fn gzip_compression_applies_to_json_and_static_assets_and_respects_accept_encoding() {
    let repo = setup_jj_test_repo();
    let root = repo.path();

    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("src/lib.rs"),
        "pub fn alpha() -> i32 { 1 }\npub fn beta() -> i32 { 2 }\n".repeat(40),
    )
    .unwrap();
    run_jj_test(root, &["describe", "-m", "feat: add compressible source"]);
    let commit_oid = run_jj_test(root, &["log", "-r", "@", "--no-graph", "-T", "commit_id"]);

    let web_dir = root.join("web");
    fs::create_dir_all(&web_dir).unwrap();
    let wasm_payload = [b"\0asm\x01\x00\x00\x00".as_slice(), &vec![b'A'; 32 * 1024]].concat();
    let js_payload = "export function renderDiff() { return 'tellit'; }\n".repeat(20);
    let css_payload = ".diff-card { contain: content; color: #eee; }\n".repeat(20);
    let png_payload = [b"\x89PNG\r\n\x1a\n".as_slice(), &vec![0x42u8; 4096]].concat();

    fs::write(
        web_dir.join("tellit-ui-8aaef9e7a356ea33_bg.wasm"),
        &wasm_payload,
    )
    .unwrap();
    fs::write(web_dir.join("tellit-ui-8aaef9e7a356ea33.js"), &js_payload).unwrap();
    fs::write(web_dir.join("styles-0123456789abcdef.css"), &css_payload).unwrap();
    fs::write(web_dir.join("preview.png"), &png_payload).unwrap();

    let server = start_server(
        ServerConfig::new(root.to_path_buf())
            .with_port(Some(0))
            .with_web_dir(Some(web_dir))
            .with_keep_alive(true),
    )
    .await
    .unwrap();
    let client = reqwest::Client::new();

    // Large WASM (> 16 KiB spawn_blocking threshold), JS, CSS, and diff JSON compress with gzip.
    let wasm_resp = client
        .get(format!("{}/tellit-ui-8aaef9e7a356ea33_bg.wasm", server.url))
        .header(reqwest::header::ACCEPT_ENCODING, "br, gzip")
        .send()
        .await
        .unwrap();
    assert_eq!(wasm_resp.status(), StatusCode::OK);
    assert_eq!(
        header_str(&wasm_resp, reqwest::header::CONTENT_ENCODING),
        Some("gzip")
    );
    assert_eq!(
        header_str(&wasm_resp, reqwest::header::VARY),
        Some("accept-encoding")
    );
    let wasm_wire = wasm_resp.bytes().await.unwrap();
    assert!(wasm_wire.len() < wasm_payload.len());
    assert_eq!(gunzip_bytes(&wasm_wire), wasm_payload);

    let js_resp = client
        .get(format!("{}/tellit-ui-8aaef9e7a356ea33.js", server.url))
        .header(reqwest::header::ACCEPT_ENCODING, "gzip")
        .send()
        .await
        .unwrap();
    assert_eq!(
        header_str(&js_resp, reqwest::header::CONTENT_ENCODING),
        Some("gzip")
    );
    let js_wire = js_resp.bytes().await.unwrap();
    assert_eq!(
        String::from_utf8(gunzip_bytes(&js_wire)).unwrap(),
        js_payload
    );

    let css_resp = client
        .get(format!("{}/styles-0123456789abcdef.css", server.url))
        .header(reqwest::header::ACCEPT_ENCODING, "gzip")
        .send()
        .await
        .unwrap();
    assert_eq!(
        header_str(&css_resp, reqwest::header::CONTENT_ENCODING),
        Some("gzip")
    );
    let css_wire = css_resp.bytes().await.unwrap();
    assert_eq!(
        String::from_utf8(gunzip_bytes(&css_wire)).unwrap(),
        css_payload
    );

    let diff_resp = client
        .get(format!("{}/api/jj/diff?target={commit_oid}", server.url))
        .header(reqwest::header::ACCEPT_ENCODING, "gzip, deflate")
        .send()
        .await
        .unwrap();
    assert_eq!(diff_resp.status(), StatusCode::OK);
    assert_eq!(
        header_str(&diff_resp, reqwest::header::CONTENT_ENCODING),
        Some("gzip")
    );
    assert_eq!(
        header_str(&diff_resp, reqwest::header::VARY),
        Some("accept-encoding")
    );
    let diff_wire = diff_resp.bytes().await.unwrap();
    let diff_json = gunzip_bytes(&diff_wire);
    assert!(diff_wire.len() < diff_json.len());
    let parsed_diff: JjDiffResponse = serde_json::from_slice(&diff_json).unwrap();
    assert_eq!(parsed_diff.commit, commit_oid);

    // Without Accept-Encoding or with gzip;q=0, responses stay uncompressed while advertising Vary.
    let plain_js = client
        .get(format!("{}/tellit-ui-8aaef9e7a356ea33.js", server.url))
        .send()
        .await
        .unwrap();
    assert_eq!(
        header_str(&plain_js, reqwest::header::CONTENT_ENCODING),
        None
    );
    assert_eq!(
        header_str(&plain_js, reqwest::header::VARY),
        Some("accept-encoding")
    );
    assert_eq!(plain_js.text().await.unwrap(), js_payload);

    let rejected_gzip = client
        .get(format!("{}/tellit-ui-8aaef9e7a356ea33.js", server.url))
        .header(reqwest::header::ACCEPT_ENCODING, "gzip;q=0, *;q=1")
        .send()
        .await
        .unwrap();
    assert_eq!(
        header_str(&rejected_gzip, reqwest::header::CONTENT_ENCODING),
        None
    );
    assert_eq!(rejected_gzip.text().await.unwrap(), js_payload);

    // HEAD responses on compressible routes preserve `Vary: accept-encoding` without compressing.
    let head_js = client
        .head(format!("{}/tellit-ui-8aaef9e7a356ea33.js", server.url))
        .header(reqwest::header::ACCEPT_ENCODING, "gzip")
        .send()
        .await
        .unwrap();
    assert_eq!(head_js.status(), StatusCode::OK);
    assert_eq!(
        header_str(&head_js, reqwest::header::CONTENT_ENCODING),
        None
    );
    assert_eq!(
        header_str(&head_js, reqwest::header::VARY),
        Some("accept-encoding")
    );

    // Already-compressed image formats must never be gzipped.
    let png_resp = client
        .get(format!("{}/preview.png", server.url))
        .header(reqwest::header::ACCEPT_ENCODING, "gzip")
        .send()
        .await
        .unwrap();
    assert_eq!(
        header_str(&png_resp, reqwest::header::CONTENT_ENCODING),
        None
    );
    assert_eq!(
        png_resp.bytes().await.unwrap().as_ref(),
        png_payload.as_slice()
    );

    server.shutdown();
    server.wait().await.unwrap();
}

#[tokio::test]
async fn sse_endpoints_stream_uncompressed_when_client_accepts_gzip() {
    let repo = setup_jj_test_repo();
    let root = repo.path();

    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/lib.rs"), "pub fn init() {}\n").unwrap();
    run_jj_test(root, &["describe", "-m", "init"]);
    let commit_oid = run_jj_test(root, &["log", "-r", "@", "--no-graph", "-T", "commit_id"]);

    let server = start_server(
        ServerConfig::new(root.to_path_buf())
            .with_port(Some(0))
            .with_keep_alive(true)
            .with_heartbeat_timings(Duration::from_millis(25), Duration::from_millis(25)),
    )
    .await
    .unwrap();
    let client = reqwest::Client::new();

    // `/api/heartbeat` must not compress or buffer even when `Accept-Encoding: gzip` is requested.
    let mut hb_resp = client
        .get(format!("{}/api/heartbeat", server.url))
        .header(reqwest::header::ACCEPT_ENCODING, "gzip, deflate, br")
        .send()
        .await
        .unwrap();
    assert_eq!(hb_resp.status(), StatusCode::OK);
    assert_eq!(
        header_str(&hb_resp, reqwest::header::CONTENT_ENCODING),
        None
    );
    assert_eq!(
        header_str(&hb_resp, reqwest::header::CACHE_CONTROL),
        Some("no-cache, no-transform")
    );
    let mut hb_events = Vec::new();
    while hb_events.len() < 2 {
        let chunk = tokio::time::timeout(Duration::from_secs(2), hb_resp.chunk())
            .await
            .expect("heartbeat SSE stream timed out under Accept-Encoding: gzip")
            .unwrap()
            .expect("heartbeat SSE stream closed prematurely");
        hb_events.extend(extract_sse_data_lines(&String::from_utf8_lossy(&chunk)));
    }
    assert_eq!(hb_events[0], HeartbeatEvent::Connected);
    assert_eq!(hb_events[1], HeartbeatEvent::Ping);

    // `/api/watch` must also stream uncompressed and deliver live events immediately.
    let mut watch_resp = client
        .get(format!("{}/api/watch", server.url))
        .header(reqwest::header::ACCEPT_ENCODING, "gzip, deflate, br")
        .send()
        .await
        .unwrap();
    assert_eq!(watch_resp.status(), StatusCode::OK);
    assert_eq!(
        header_str(&watch_resp, reqwest::header::CONTENT_ENCODING),
        None
    );
    assert_eq!(
        header_str(&watch_resp, reqwest::header::CACHE_CONTROL),
        Some("no-cache, no-transform")
    );
    let first_watch = tokio::time::timeout(Duration::from_secs(2), watch_resp.chunk())
        .await
        .expect("watch SSE stream timed out under Accept-Encoding: gzip")
        .unwrap()
        .unwrap();
    assert_eq!(
        extract_watch_events(&String::from_utf8_lossy(&first_watch)),
        vec![WatchEvent::Connected]
    );

    let comment_status = client
        .post(format!("{}/api/jj/comments", server.url))
        .json(&CreateCommentRequest {
            commit_id: commit_oid,
            file_path: String::from("src/lib.rs"),
            line_range: Some(LineRangeSpec { start: 1, end: 1 }),
            content: String::from("stream check"),
        })
        .send()
        .await
        .unwrap()
        .status();
    assert_eq!(comment_status, StatusCode::OK);

    let second_watch = tokio::time::timeout(Duration::from_secs(2), watch_resp.chunk())
        .await
        .expect("watch CommentsChanged timed out under Accept-Encoding: gzip")
        .unwrap()
        .unwrap();
    assert!(
        extract_watch_events(&String::from_utf8_lossy(&second_watch))
            .contains(&WatchEvent::CommentsChanged)
    );

    server.shutdown();
    server.wait().await.unwrap();
}

#[tokio::test]
async fn cache_control_headers_distinguish_hashed_assets_commit_oids_and_mutable_refs() {
    let repo = setup_jj_test_repo();
    let root = repo.path();

    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/lib.rs"), "pub fn v1() {}\n").unwrap();
    run_jj_test(root, &["describe", "-m", "v1"]);
    let rev1 = run_jj_test(root, &["log", "-r", "@", "--no-graph", "-T", "commit_id"]);

    run_jj_test(root, &["new", "-m", "v2"]);
    fs::write(root.join("src/lib.rs"), "pub fn v2() {}\n").unwrap();
    run_jj_test(root, &["bookmark", "create", "feat-branch", "-r", "@"]);
    let rev2 = run_jj_test(root, &["log", "-r", "@", "--no-graph", "-T", "commit_id"]);
    let change2 = run_jj_test(root, &["log", "-r", "@", "--no-graph", "-T", "change_id"]);
    assert_eq!(rev1.len(), 40);
    assert_eq!(rev2.len(), 40);

    let web_dir = root.join("web");
    fs::create_dir_all(&web_dir).unwrap();
    fs::write(web_dir.join("index.html"), "<!doctype html><html></html>").unwrap();
    fs::write(
        web_dir.join("tellit-ui-8aaef9e7a356ea33_bg.wasm"),
        b"\0asm\x01\x00\x00\x00",
    )
    .unwrap();
    fs::write(web_dir.join("tellit-ui-8aaef9e7a356ea33.js"), "export {};").unwrap();
    fs::write(web_dir.join("styles-0123456789abcdef.css"), "body{}").unwrap();
    fs::write(web_dir.join("favicon.svg"), "<svg/>").unwrap();
    fs::write(web_dir.join("app.js"), "export {};").unwrap();

    let server = start_server(
        ServerConfig::new(root.to_path_buf())
            .with_port(Some(0))
            .with_web_dir(Some(web_dir))
            .with_keep_alive(true),
    )
    .await
    .unwrap();
    let client = reqwest::Client::new();

    // Static assets: HTML is no-store, content-hashed assets are immutable, unhashed assets are no-cache.
    let index_resp = client
        .get(format!("{}/index.html", server.url))
        .send()
        .await
        .unwrap();
    assert_eq!(
        header_str(&index_resp, reqwest::header::CACHE_CONTROL),
        Some("no-cache, no-store, must-revalidate")
    );
    for hashed in [
        "/tellit-ui-8aaef9e7a356ea33_bg.wasm",
        "/tellit-ui-8aaef9e7a356ea33.js",
        "/styles-0123456789abcdef.css",
    ] {
        let resp = client
            .get(format!("{}{hashed}", server.url))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            header_str(&resp, reqwest::header::CACHE_CONTROL),
            Some("public, max-age=31536000, immutable"),
            "expected immutable cache header for {hashed}"
        );
    }
    for unhashed in ["/favicon.svg", "/app.js"] {
        let resp = client
            .get(format!("{}{unhashed}", server.url))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            header_str(&resp, reqwest::header::CACHE_CONTROL),
            Some("no-cache"),
            "expected no-cache header for {unhashed}"
        );
    }

    // Immutable commit-OID-addressed diff, blob, and line-count responses get private immutable caching.
    for immutable_api in [
        format!("/api/jj/diff?target={rev2}"),
        format!("/api/jj/diff?target={rev2}&base={rev1}"),
        format!("/api/blob/src/lib.rs?ref={rev2}"),
        format!("/api/line-count/src/lib.rs?oldRef={rev1}&newRef={rev2}"),
    ] {
        let resp = client
            .get(format!("{}{immutable_api}", server.url))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK, "failed {immutable_api}");
        assert_eq!(
            header_str(&resp, reqwest::header::CACHE_CONTROL),
            Some("private, max-age=31536000, immutable"),
            "expected private immutable cache header for {immutable_api}"
        );
    }

    // Mutable refs (`@`, bookmarks, short prefixes, `working`), fallback line counts on
    // non-existent 40-hex commit OIDs, and mutable endpoints stay no-cache, no-store.
    let short_rev2 = &rev2[..12];
    let missing_oid = "ffffffffffffffffffffffffffffffffffffffff";
    for mutable_api in [
        String::from("/api/jj/diff?target=@"),
        String::from("/api/jj/diff?target=feat-branch"),
        format!("/api/jj/diff?target={short_rev2}"),
        format!("/api/jj/diff?target={rev2}&base=@-"),
        String::from("/api/blob/src/lib.rs?ref=@"),
        String::from("/api/blob/src/lib.rs?ref=working"),
        String::from("/api/blob/src/lib.rs?ref=feat-branch"),
        format!("/api/line-count/src/lib.rs?oldRef=@-&newRef={rev2}"),
        format!("/api/line-count/src/lib.rs?oldRef={missing_oid}&newRef={rev2}"),
        String::from("/api/jj/status"),
        String::from("/api/jj/changes"),
        format!("/api/jj/changes/{change2}/commits"),
        format!("/api/jj/comments?target={rev2}"),
        String::from("/api/user-settings"),
    ] {
        let resp = client
            .get(format!("{}{mutable_api}", server.url))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK, "failed {mutable_api}");
        assert_eq!(
            header_str(&resp, reqwest::header::CACHE_CONTROL),
            Some("no-cache, no-store"),
            "expected no-cache, no-store header for {mutable_api}"
        );
    }

    server.shutdown();
    server.wait().await.unwrap();
}

async fn wait_for_watch_event(
    resp: &mut reqwest::Response,
    expected: WatchEvent,
    timeout: Duration,
) -> bool {
    let deadline = tokio::time::Instant::now() + timeout;
    while tokio::time::Instant::now() < deadline {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        match tokio::time::timeout(remaining, resp.chunk()).await {
            Ok(Ok(Some(chunk))) => {
                if extract_watch_events(&String::from_utf8_lossy(&chunk)).contains(&expected) {
                    return true;
                }
            }
            Ok(Ok(None)) | Err(_) => return false,
            Ok(Err(err)) => panic!("watch stream read error: {err}"),
        }
    }
    false
}

#[tokio::test]
async fn watch_targets_only_diffed_files_handles_atomic_saves_store_and_jj_op_refresh() {
    let repo = setup_jj_test_repo();
    let root = repo.path();

    fs::create_dir_all(root.join("src")).unwrap();
    fs::create_dir_all(root.join("unrelated/deep")).unwrap();
    fs::create_dir_all(root.join("target/debug")).unwrap();
    fs::write(root.join("src/main.rs"), "fn main() {}\n").unwrap();
    fs::write(root.join("src/unrelated.rs"), "pub fn untouched() {}\n").unwrap();
    fs::write(root.join("unrelated/deep/file.txt"), "base\n").unwrap();
    run_jj_test(root, &["describe", "-m", "base"]);

    // Working copy (`@`) modifies only `src/main.rs`.
    run_jj_test(root, &["new", "-m", "feat: edit main only"]);
    fs::write(
        root.join("src/main.rs"),
        "fn main() { println!(\"v1\"); }\n",
    )
    .unwrap();
    run_jj_test(root, &["status"]);

    let server = start_server(
        ServerConfig::new(root.to_path_buf())
            .with_port(Some(0))
            .with_keep_alive(true),
    )
    .await
    .unwrap();
    let client = reqwest::Client::new();

    let mut watch_resp = client
        .get(format!("{}/api/watch", server.url))
        .send()
        .await
        .unwrap();
    assert_eq!(watch_resp.status(), StatusCode::OK);
    assert!(
        wait_for_watch_event(
            &mut watch_resp,
            WatchEvent::Connected,
            Duration::from_secs(2)
        )
        .await,
        "expected initial Connected event on /api/watch"
    );

    // Fetch working-copy diff so the targeted watch set is synchronized.
    let diff_status = client
        .get(format!("{}/api/jj/diff?target=@", server.url))
        .send()
        .await
        .unwrap()
        .status();
    assert_eq!(diff_status, StatusCode::OK);
    // Allow the watch task a moment to apply the updated watch set.
    tokio::time::sleep(Duration::from_millis(50)).await;

    // 1. Edits to `target/`, untouched nested directories, and untracked siblings inside the
    //    watched `src/` directory must NOT emit `WorkingTree` reload events.
    fs::write(root.join("target/debug/build.out"), "artifact\n").unwrap();
    fs::write(root.join("unrelated/deep/file.txt"), "changed\n").unwrap();
    fs::write(
        root.join("src/unrelated.rs"),
        "pub fn untouched() { /* edit */ }\n",
    )
    .unwrap();
    let unexpected = tokio::time::timeout(Duration::from_millis(600), watch_resp.chunk()).await;
    assert!(
        unexpected.is_err(),
        "edits to unrelated dirs or sibling files must not trigger watch events"
    );

    // 2. Atomic editor save (write temp file + rename) on diffed `src/main.rs` emits WorkingTree.
    let tmp_path = root.join("src/.main.rs.tmp");
    fs::write(&tmp_path, "fn main() { println!(\"atomic\"); }\n").unwrap();
    fs::rename(&tmp_path, root.join("src/main.rs")).unwrap();
    assert!(
        wait_for_watch_event(
            &mut watch_resp,
            WatchEvent::Reload {
                source: WatchSource::WorkingTree,
            },
            Duration::from_secs(3),
        )
        .await,
        "atomic rename save on diffed file must emit WorkingTree reload"
    );

    // 3. External `.tellit/` creation and subsequent nested edits emit CommentsChanged.
    let ext_thread = root.join(".tellit/ext/commit/thread.json");
    fs::create_dir_all(ext_thread.parent().unwrap()).unwrap();
    fs::write(&ext_thread, "{}\n").unwrap();
    assert!(
        wait_for_watch_event(
            &mut watch_resp,
            WatchEvent::CommentsChanged,
            Duration::from_secs(3),
        )
        .await,
        "creating .tellit/ store on disk must emit CommentsChanged"
    );

    fs::write(&ext_thread, "{\"updated\":true}\n").unwrap();
    assert!(
        wait_for_watch_event(
            &mut watch_resp,
            WatchEvent::CommentsChanged,
            Duration::from_secs(3),
        )
        .await,
        "modifying nested file inside .tellit/ must emit CommentsChanged"
    );

    // 4. Adding a new file in a new directory and snapshotting via `jj` emits JjOp AND refreshes
    //    the watched diffed-file set without any `/api/jj/diff` call.
    fs::create_dir_all(root.join("extra")).unwrap();
    fs::write(root.join("extra/added.rs"), "pub fn added() {}\n").unwrap();
    run_jj_test(root, &["status"]);
    assert!(
        wait_for_watch_event(
            &mut watch_resp,
            WatchEvent::Reload {
                source: WatchSource::JjOp,
            },
            Duration::from_secs(3),
        )
        .await,
        "jj op_heads update must emit JjOp reload"
    );

    // Wait briefly for the post-JjOp background `jj diff --name-only` refresh, then edit the newly
    // diffed file and confirm WorkingTree fires.
    let mut saw_refreshed_working = false;
    for attempt in 0..5 {
        tokio::time::sleep(Duration::from_millis(150)).await;
        fs::write(
            root.join("extra/added.rs"),
            format!("pub fn added() -> usize {{ {attempt} }}\n"),
        )
        .unwrap();
        if wait_for_watch_event(
            &mut watch_resp,
            WatchEvent::Reload {
                source: WatchSource::WorkingTree,
            },
            Duration::from_millis(700),
        )
        .await
        {
            saw_refreshed_working = true;
            break;
        }
    }
    assert!(
        saw_refreshed_working,
        "post-JjOp refresh must watch newly added working-copy diffed files"
    );

    drop(watch_resp);
    server.shutdown();
    server.wait().await.unwrap();
}
