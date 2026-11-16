//! CLI entrypoint for `tellit`.

use std::io::Write;
use std::path::Path;
use std::process::ExitCode;

use clap::Parser;
use tellit_cli::server::{
    BACKGROUND_CHILD_ENV, BackgroundServerInfo, ServerConfig, spawn_background_child, start_server,
    wait_for_shutdown_signal,
};
use tellit_cli::{
    ChangeCommitsGetResponse, ChangeThreadsGetResponse, Cli, Command, CommentAction,
    CommitGetResponse, CommitIdsGetResponse, GetTarget, LaunchArgs, add_comment, get_all_threads,
    get_change_approved_commits, get_change_commits, get_change_threads, get_commit_thread_infos,
    get_commit_threads, get_thread, get_workspace_root, is_change_approved, is_commit_approved,
    reply_comment, resolve_comment, resolve_store, set_commit_approval,
};

fn run_launch(launch: &LaunchArgs, store: Option<&Path>) -> anyhow::Result<()> {
    let is_background_child = std::env::var_os(BACKGROUND_CHILD_ENV).is_some();

    if launch.background && !is_background_child {
        let info = spawn_background_child(launch, store)?;
        println!("{}", serde_json::to_string(&info)?);
        return Ok(());
    }

    let repo_path = match get_workspace_root() {
        Ok(root) => root,
        Err(_) => std::env::current_dir()?,
    };

    let config = ServerConfig::new(repo_path)
        .with_port(launch.port)
        .with_host(launch.host.as_deref())
        .with_keep_alive(launch.keep_alive || is_background_child)
        .with_quiet(is_background_child);

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;

    runtime.block_on(async move {
        let running = start_server(config).await?;

        if is_background_child {
            let handshake = BackgroundServerInfo {
                port: running.port,
                url: running.url.clone(),
                pid: std::process::id(),
            };
            let line = serde_json::to_string(&handshake)?;
            let mut stdout = std::io::stdout().lock();
            let _ = writeln!(stdout, "{line}");
            let _ = stdout.flush();
        } else {
            if running.is_external_bind() {
                eprintln!(
                    "Warning: server is bound to external interface {}",
                    running.addr
                );
            }
            eprintln!("tellit server listening on {}", running.url);
            let target = launch.commit_ish.as_deref().unwrap_or("@");
            eprintln!("Reviewing: {target}");
            if launch.keep_alive {
                eprintln!("Keep-alive mode: server stays running after browser tabs disconnect.");
            }
            if !launch.no_open
                && let Err(err) = open::that(&running.url)
            {
                eprintln!("Warning: failed to open browser automatically: {err}");
            }
        }

        let shutdown_handle = running.shutdown_handle();
        tokio::spawn(async move {
            wait_for_shutdown_signal().await;
            shutdown_handle.shutdown();
        });

        running.wait().await?;
        Ok(())
    })
}

fn run() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Launch(ref launch) => run_launch(launch, cli.store.as_deref())?,
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
                    let thread_id =
                        add_comment(&store, &workspace_root, rev, &add.file, add.at, add.content)?;
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
                CommentAction::GetAll(args) => {
                    let resolved_filter = if args.unresolved {
                        Some(false)
                    } else if args.resolved {
                        Some(true)
                    } else {
                        None
                    };
                    let thread_ids = get_all_threads(&store, &args.target, resolved_filter)?;
                    println!("{}", serde_json::to_string_pretty(&thread_ids)?);
                }
                CommentAction::Approve(args) => {
                    let (change_id, commit_id, approved) =
                        set_commit_approval(&store, args.target.as_deref(), args.disapprove)?;
                    if approved {
                        eprintln!("Approved commit {commit_id} (change {change_id})");
                    } else {
                        eprintln!("Revoked approval for commit {commit_id} (change {change_id})");
                    }
                }
                CommentAction::Get(get) => match get.target {
                    GetTarget::Thread(t) => {
                        let info = get_thread(&store, &t.thread_id)?;
                        println!("{}", serde_json::to_string_pretty(&info)?);
                    }
                    GetTarget::Change(c) => {
                        let approval = is_change_approved(&store, &c.change_id);
                        let approved_commits = get_change_approved_commits(&store, &c.change_id)?;
                        if c.threads {
                            let threads = get_change_threads(&store, &c.change_id)?;
                            let resp = ChangeThreadsGetResponse {
                                threads,
                                approval,
                                approved_commits,
                            };
                            println!("{}", serde_json::to_string_pretty(&resp)?);
                        } else {
                            let commits = get_change_commits(&store, &c.change_id)?;
                            if c.json {
                                let resp = ChangeCommitsGetResponse {
                                    commits,
                                    approval,
                                    approved_commits,
                                };
                                println!("{}", serde_json::to_string_pretty(&resp)?);
                            } else {
                                for commit in &commits {
                                    println!("{commit}");
                                }
                                println!("approval: {approval}");
                            }
                        }
                    }
                    GetTarget::Commit(co) => {
                        let approval = is_commit_approved(&store, &co.commit_hash);
                        if co.threads {
                            let threads = get_commit_thread_infos(&store, &co.commit_hash)?;
                            let resp = CommitGetResponse { threads, approval };
                            println!("{}", serde_json::to_string_pretty(&resp)?);
                        } else {
                            let threads = get_commit_threads(&store, &co.commit_hash)?;
                            if co.json {
                                let resp = CommitIdsGetResponse { threads, approval };
                                println!("{}", serde_json::to_string_pretty(&resp)?);
                            } else {
                                for thread in &threads {
                                    println!("{thread}");
                                }
                                println!("approval: {approval}");
                            }
                        }
                    }
                    GetTarget::All(args) => {
                        let resolved_filter = if args.unresolved {
                            Some(false)
                        } else if args.resolved {
                            Some(true)
                        } else {
                            None
                        };
                        let thread_ids = get_all_threads(&store, &args.target, resolved_filter)?;
                        println!("{}", serde_json::to_string_pretty(&thread_ids)?);
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
