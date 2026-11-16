//! Detached background process spawning and JSON startup handshake for `tellit launch --background`.

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use core::time::Duration;
use std::io::{BufRead, BufReader, Read};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc;

use serde::{Deserialize, Serialize};

use super::ServerError;
use crate::LaunchArgs;

/// Environment variable set on the detached child spawned by `tellit launch --background`.
pub const BACKGROUND_CHILD_ENV: &str = "TELLIT_BACKGROUND_CHILD";

const STARTUP_TIMEOUT: Duration = Duration::from_secs(10);

/// JSON metadata emitted to `stdout` when `tellit launch --background` finishes binding.
///
/// # Examples
///
/// ```
/// use tellit_cli::server::BackgroundServerInfo;
///
/// let info = BackgroundServerInfo {
///     port: 4966,
///     url: String::from("http://127.0.0.1:4966"),
///     pid: 4242,
/// };
/// let json = serde_json::to_string(&info)?;
/// assert!(json.contains("\"port\":4966"));
/// # Ok::<(), serde_json::Error>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackgroundServerInfo {
    /// Bound TCP port of the background server.
    pub port: u16,
    /// Full HTTP URL where the web UI is reachable.
    pub url: String,
    /// Operating system process ID of the detached server process.
    pub pid: u32,
}

fn build_child_args(launch: &LaunchArgs, store: Option<&Path>) -> Vec<String> {
    let mut args = Vec::new();
    if let Some(store_path) = store {
        args.push(alloc::format!("--store={}", store_path.to_string_lossy()));
    }
    args.push(String::from("launch"));
    if let Some(port) = launch.port {
        args.push(alloc::format!("--port={port}"));
    }
    if let Some(ref host) = launch.host {
        args.push(alloc::format!("--host={host}"));
    }
    args.push(String::from("--no-open"));
    args.push(String::from("--keep-alive"));
    if launch.commit_ish.is_some() || launch.compare_with.is_some() {
        args.push(String::from("--"));
        if let Some(ref commit_ish) = launch.commit_ish {
            args.push(commit_ish.clone());
        }
        if let Some(ref compare_with) = launch.compare_with {
            args.push(compare_with.clone());
        }
    }
    args
}

/// Spawns a detached `tellit launch` daemon and waits for its [`BackgroundServerInfo`] handshake.
///
/// # Errors
///
/// Returns [`ServerError`] if spawning the current executable fails, the child exits before
/// emitting valid JSON handshake data, or startup exceeds 10 seconds.
pub fn spawn_background_child(
    launch: &LaunchArgs,
    store: Option<&Path>,
) -> Result<BackgroundServerInfo, ServerError> {
    let current_exe = std::env::current_exe()?;
    let child_args = build_child_args(launch, store);

    let mut command = Command::new(current_exe);
    command
        .args(&child_args)
        .env(BACKGROUND_CHILD_ENV, "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }

    let mut child = command.spawn()?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| ServerError::BackgroundStartup(String::from("missing child stdout pipe")))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| ServerError::BackgroundStartup(String::from("missing child stderr pipe")))?;

    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        let mut line = String::new();
        let read_res = reader.read_line(&mut line);
        let _ = tx.send((read_res, line));
    });

    match rx.recv_timeout(STARTUP_TIMEOUT) {
        Ok((Ok(bytes_read), line)) if bytes_read > 0 => {
            match serde_json::from_str::<BackgroundServerInfo>(line.trim()) {
                Ok(info) => Ok(info),
                Err(err) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    Err(ServerError::Json(err))
                }
            }
        }
        Ok((Ok(_), _)) => {
            let mut err_buf = String::new();
            let _ = BufReader::new(stderr).read_to_string(&mut err_buf);
            let status = child.wait().ok();
            let detail = err_buf.trim();
            if !detail.is_empty() {
                Err(ServerError::BackgroundStartup(String::from(detail)))
            } else {
                let code = status
                    .and_then(|s| s.code())
                    .map_or_else(|| String::from("signal"), |c| c.to_string());
                Err(ServerError::BackgroundStartup(alloc::format!(
                    "background server exited before handshake (status {code})"
                )))
            }
        }
        Ok((Err(io_err), _)) => {
            let _ = child.kill();
            let _ = child.wait();
            Err(ServerError::Io(io_err))
        }
        Err(_) => {
            let _ = child.kill();
            let _ = child.wait();
            Err(ServerError::BackgroundTimeout)
        }
    }
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::*;
    use crate::{Cli, Command as CliCommand};

    #[test]
    fn child_args_strip_background_and_enforce_daemon_flags() {
        let launch = LaunchArgs {
            commit_ish: Some(String::from("@-")),
            compare_with: None,
            port: Some(5100),
            host: Some(String::from("127.0.0.1")),
            no_open: false,
            keep_alive: false,
            background: true,
        };
        let args = build_child_args(&launch, Some(Path::new("/tmp/store")));
        assert_eq!(
            args,
            vec![
                "--store=/tmp/store",
                "launch",
                "--port=5100",
                "--host=127.0.0.1",
                "--no-open",
                "--keep-alive",
                "--",
                "@-",
            ]
        );
    }

    #[test]
    fn child_args_roundtrip_leading_dash_revisions_through_clap() {
        let launch = LaunchArgs {
            commit_ish: Some(String::from("-my-bookmark")),
            compare_with: Some(String::from("-compare-branch")),
            port: Some(4970),
            host: Some(String::from("::1")),
            no_open: false,
            keep_alive: false,
            background: true,
        };
        let mut argv = vec![String::from("tellit")];
        argv.extend(build_child_args(&launch, Some(Path::new("/tmp/store"))));

        let parsed = Cli::try_parse_from(argv).expect("child args must parse cleanly");
        assert_eq!(parsed.store.as_deref(), Some(Path::new("/tmp/store")));
        let CliCommand::Launch(parsed_launch) = parsed.command else {
            panic!("expected launch subcommand");
        };
        assert_eq!(parsed_launch.commit_ish.as_deref(), Some("-my-bookmark"));
        assert_eq!(
            parsed_launch.compare_with.as_deref(),
            Some("-compare-branch")
        );
        assert_eq!(parsed_launch.port, Some(4970));
        assert_eq!(parsed_launch.host.as_deref(), Some("::1"));
        assert!(parsed_launch.no_open);
        assert!(parsed_launch.keep_alive);
        assert!(!parsed_launch.background);
    }
}
