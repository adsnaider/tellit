use std::{path::PathBuf, range::Range};

use clap::{Args, Parser, Subcommand};

#[derive(Subcommand, Debug)]
pub enum Commenter {
    Add(Add),
    Resolve(Resolve),
    Reply(Reply),
}

#[derive(Parser, Debug)]
pub struct Cli {
    #[arg(long, short)]
    store: Option<PathBuf>,
    #[arg(long, short)]
    change: Option<String>,
    #[command(subcommand)]
    action: Commenter,
}

#[derive(Debug, Args)]
pub struct Add {
    #[arg(long, short)]
    file: PathBuf,
    #[arg(long, short)]
    at: Option<Range<u32>>,
    #[arg(long, short)]
    content: String,
}

#[derive(Debug, Args)]
pub struct Resolve {
    #[arg(long, short)]
    thread: String,
}

#[derive(Debug, Args)]
pub struct Reply {
    #[arg(long, short)]
    thread: String,
    #[arg(long, short)]
    content: String,
}

fn main() {
    let cmd = Cli::parse();
}
