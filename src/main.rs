use std::process::ExitCode;

use clap::Parser;
use commenter::{
    add_comment, get_change_commits, get_commit_threads, get_thread, get_workspace_root,
    reply_comment, resolve_comment, resolve_store, Cli, Commenter, GetTarget,
};

fn run() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let store = resolve_store(cli.store.as_deref())?;

    match cli.action {
        Commenter::Add(add) => {
            let rev = add
                .rev
                .as_deref()
                .or(cli.change.as_deref())
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
            // "adding a comment will only write the thread-id to stdout and anything else is logged to stderr."
            println!("{thread_id}");
            eprintln!("Comment thread created: {thread_id}");
        }
        Commenter::Reply(reply) => {
            let thread_id = reply.get_thread_id()?;
            let content = reply.get_content()?;
            let updated_thread = reply_comment(&store, thread_id, content.to_string())?;
            eprintln!("Replied to thread {updated_thread}");
        }
        Commenter::Resolve(resolve) => {
            let thread_id = resolve.get_thread_id()?;
            let (id, resolved) = resolve_comment(&store, thread_id, resolve.unresolve)?;
            eprintln!(
                "Thread {id} {}",
                if resolved { "resolved" } else { "unresolved" }
            );
        }
        Commenter::Get(get) => match get.target {
            GetTarget::Thread(t) => {
                let info = get_thread(&store, &t.thread_id)?;
                println!("{}", serde_json::to_string_pretty(&info)?);
            }
            GetTarget::Change(c) => {
                let commits = get_change_commits(&store, &c.change_id)?;
                if c.json {
                    println!("{}", serde_json::to_string_pretty(&commits)?);
                } else {
                    for commit in commits {
                        println!("{commit}");
                    }
                }
            }
            GetTarget::Commit(co) => {
                let threads = get_commit_threads(&store, &co.commit_hash)?;
                if co.json {
                    println!("{}", serde_json::to_string_pretty(&threads)?);
                } else {
                    for thread in threads {
                        println!("{thread}");
                    }
                }
            }
        },
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
