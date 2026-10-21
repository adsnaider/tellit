use std::process::ExitCode;

use clap::Parser;
use tellit_cli::{
    add_comment, get_change_commits, get_change_threads, get_commit_thread_infos,
    get_commit_threads, get_thread, get_workspace_root, locate_web_cli, reply_comment,
    resolve_comment, resolve_store, Cli, Command, CommentAction, GetTarget,
};

fn locate_node() -> std::path::PathBuf {
    if let Ok(node_var) = std::env::var("NODE") {
        let p = std::path::PathBuf::from(node_var);
        if p.exists() {
            return p;
        }
    }
    if std::process::Command::new("node").arg("-v").output().is_ok() {
        return std::path::PathBuf::from("node");
    }
    // Search nix store fallback if node is not directly in PATH
    if let Ok(entries) = std::fs::read_dir("/nix/store") {
        for entry in entries.filter_map(|e| e.ok()) {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.contains("nodejs") && !name.contains("npm") && !name.contains("dev") && !name.contains("source") {
                let candidate = entry.path().join("bin/node");
                if candidate.is_file() {
                    return candidate;
                }
            }
        }
    }
    std::path::PathBuf::from("node")
}

fn run() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Launch(launch) => {
            let web_cli = locate_web_cli()?;
            let current_exe = std::env::current_exe()
                .unwrap_or_else(|_| std::path::PathBuf::from("tellit"));
            let node_bin = locate_node();

            let mut cmd = std::process::Command::new(&node_bin);
            cmd.arg(&web_cli);
            cmd.env("TELLIT_BIN", &current_exe);

            if let Some(ref commit) = launch.commit_ish {
                cmd.arg(commit);
            } else {
                cmd.arg("@");
            }

            if let Some(ref compare_with) = launch.compare_with {
                cmd.arg(compare_with);
            }

            if let Some(port) = launch.port {
                cmd.arg("--port").arg(port.to_string());
            }

            if let Some(ref host) = launch.host {
                cmd.arg("--host").arg(host);
            }

            if launch.no_open {
                cmd.arg("--no-open");
            }

            if launch.keep_alive {
                cmd.arg("--keep-alive");
            }

            if launch.background {
                cmd.arg("--background");
            }

            for extra in &launch.extra_args {
                cmd.arg(extra);
            }

            let status = cmd
                .stdin(std::process::Stdio::inherit())
                .stdout(std::process::Stdio::inherit())
                .stderr(std::process::Stdio::inherit())
                .status()?;

            if !status.success() {
                std::process::exit(status.code().unwrap_or(1));
            }
        }
        Command::Comment(comment) => {
            let store = resolve_store(cli.store.as_deref())?;
            match comment.action {
                CommentAction::Add(add) => {
                    let rev = add
                        .rev
                        .as_deref()
                        .or(comment.change.as_deref())
                        .unwrap_or("@");
                    let workspace_root = get_workspace_root()?;
                    let thread_id = add_comment(
                        &store,
                        &workspace_root,
                        rev,
                        &add.file,
                        add.at,
                        add.content,
                    )?;
                    println!("{thread_id}");
                    eprintln!("Comment thread created: {thread_id}");
                }
                CommentAction::Reply(reply) => {
                    let thread_id = reply.get_thread_id()?;
                    let content = reply.get_content()?;
                    let updated_thread = reply_comment(&store, thread_id, content.to_string())?;
                    eprintln!("Replied to thread {updated_thread}");
                }
                CommentAction::Resolve(resolve) => {
                    let thread_id = resolve.get_thread_id()?;
                    let (id, resolved) = resolve_comment(&store, thread_id, resolve.unresolve)?;
                    eprintln!(
                        "Thread {id} {}",
                        if resolved { "resolved" } else { "unresolved" }
                    );
                }
                CommentAction::Get(get) => match get.target {
                    GetTarget::Thread(t) => {
                        let info = get_thread(&store, &t.thread_id)?;
                        println!("{}", serde_json::to_string_pretty(&info)?);
                    }
                    GetTarget::Change(c) => {
                        if c.threads {
                            let threads = get_change_threads(&store, &c.change_id)?;
                            println!("{}", serde_json::to_string_pretty(&threads)?);
                        } else {
                            let commits = get_change_commits(&store, &c.change_id)?;
                            if c.json {
                                println!("{}", serde_json::to_string_pretty(&commits)?);
                            } else {
                                for commit in commits {
                                    println!("{commit}");
                                }
                            }
                        }
                    }
                    GetTarget::Commit(co) => {
                        if co.threads {
                            let threads = get_commit_thread_infos(&store, &co.commit_hash)?;
                            println!("{}", serde_json::to_string_pretty(&threads)?);
                        } else {
                            let threads = get_commit_threads(&store, &co.commit_hash)?;
                            if co.json {
                                println!("{}", serde_json::to_string_pretty(&threads)?);
                            } else {
                                for thread in threads {
                                    println!("{thread}");
                                }
                            }
                        }
                    }
                },
            }
        }
    }

    Ok(())
}

fn main() -> ExitCode {
    if let Err(err) = run() {
        eprintln!("Error: {err:?}");
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}
