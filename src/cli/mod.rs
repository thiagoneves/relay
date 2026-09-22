//! Command-line surface. Each command is a thin file that parses flags,
//! calls into `core` or `harness`, and prints. No domain logic here.

mod brief;
mod get;
mod handoff;
mod hook;
mod init;
mod install;
mod purge;
mod remember;
mod status;
mod wrap;
mod x;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "relay", version, about = "Context layer for coding agents: compresses what goes in, remembers what matters, lives in your repo.")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Set up this repo: `.relay/` (shared) and the local store
    Init,
    /// Register hooks for a harness (claude, codex)
    Install { harness: String },
    /// Remove relay hooks from a harness (claude, codex)
    Uninstall { harness: String },
    /// Hook entry point used by harnesses; reads JSON on stdin
    Hook { harness: String },
    /// Run a command, print a compressed view, keep the original
    #[command(name = "x", trailing_var_arg = true, allow_hyphen_values = true)]
    Exec {
        /// Print the raw output (still strips ANSI)
        #[arg(long)]
        raw: bool,
        #[arg(required = true, num_args = 1..)]
        cmd: Vec<String>,
    },
    /// Print the original output behind a compressed view
    Get {
        id: String,
        /// Print metadata instead of the output
        #[arg(long)]
        meta: bool,
    },
    /// Build (or show) the rule-based handoff for a session
    Handoff {
        /// Session id (default: most recent)
        #[arg(long)]
        session: Option<String>,
        /// Show the latest handoff without rebuilding
        #[arg(long)]
        show: bool,
    },
    /// Save a rule, gotcha or decision to `.relay/` (committed, one file per item)
    Remember {
        /// rule, gotcha or decision
        kind: String,
        /// The item; first line becomes the title
        #[arg(required = true, num_args = 1..)]
        text: Vec<String>,
        /// Paths the item is about (repeatable)
        #[arg(long = "path")]
        paths: Vec<String>,
    },
    /// Print the brief a new session would receive
    Brief,
    /// What relay saved, what it stores, and where
    Status,
    /// Delete local relay data for this worktree (never touches .relay/)
    Purge {
        #[arg(long)]
        yes: bool,
    },
    /// Launch Claude Code supervised by relay (installs hooks, writes handoff on exit)
    #[command(trailing_var_arg = true, allow_hyphen_values = true)]
    Claude {
        /// Resume the last relay-supervised session on this branch
        #[arg(long)]
        last: bool,
        args: Vec<String>,
    },
    /// Launch Codex supervised by relay (installs hooks, writes handoff on exit)
    #[command(trailing_var_arg = true, allow_hyphen_values = true)]
    Codex {
        /// Resume the last relay-supervised session on this branch
        #[arg(long)]
        last: bool,
        args: Vec<String>,
    },
}

pub fn run() -> anyhow::Result<i32> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Init => init::run(),
        Commands::Install { harness } => install::install(&harness),
        Commands::Uninstall { harness } => install::uninstall(&harness),
        Commands::Hook { harness } => hook::run(&harness),
        Commands::Exec { raw, cmd } => x::run(&cmd.join(" "), raw),
        Commands::Get { id, meta } => get::run(&id, meta),
        Commands::Handoff { session, show } => handoff::run(session.as_deref(), show),
        Commands::Remember { kind, text, paths } => remember::run(&kind, &text.join(" "), &paths),
        Commands::Brief => brief::run(),
        Commands::Status => status::run(),
        Commands::Purge { yes } => purge::run(yes),
        Commands::Claude { last, args } => wrap::run("claude", last, &args),
        Commands::Codex { last, args } => wrap::run("codex", last, &args),
    }
}
