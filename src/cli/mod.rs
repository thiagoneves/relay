//! Command-line surface. Each command is a thin file that parses flags,
//! calls into `core` or `harness`, and prints. No domain logic here.

mod audit;
mod bench;
mod brief;
mod compile;
mod get;
mod handoff;
mod hook;
mod init;
mod install;
mod log;
mod pipe;
mod purge;
mod remember;
mod setup;
mod status;
mod ui;
mod usage;
mod wrap;
mod x;

use clap::{Parser, Subcommand};

use crate::core::exec;
use crate::harness::HarnessId;

#[derive(Parser)]
#[command(
    name = "relay",
    version,
    about = "Context layer for coding agents: compresses what goes in, remembers what matters, lives in your repo."
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// One-time machine setup: put `relay` on PATH, register hooks in every harness found
    Setup,
    /// Set up this repo: `.relay/` (shared) and the local store
    Init {
        /// Keep memory in the local store instead of `.relay/` (a repo you cannot commit to)
        #[arg(long)]
        local: bool,
        /// Ask the agent for shorter replies (an Answers section in project.md)
        #[arg(long)]
        terse: bool,
        /// Leave AGENTS.md and CLAUDE.md alone (by default they get a pointer to relay's memory)
        #[arg(long)]
        no_agents_md: bool,
    },
    /// Register hooks for a harness (claude, codex)
    Install { harness: HarnessId },
    /// Remove relay hooks from a harness (claude, codex)
    Uninstall { harness: HarnessId },
    /// Hook entry point used by harnesses; reads JSON on stdin
    #[command(hide = true)]
    Hook { harness: HarnessId },
    /// Run a command, print a compressed view, keep the original
    #[command(name = "x", trailing_var_arg = true, allow_hyphen_values = true)]
    Exec {
        /// Print the raw output (still strips ANSI)
        #[arg(long)]
        raw: bool,
        /// Session the output belongs to; set by the hook that rewrote the command
        #[arg(long, hide = true)]
        session: Option<String>,
        /// Stop the command after this many milliseconds; set by the hook, just under the harness's timeout
        #[arg(long, hide = true, value_name = "MS")]
        stop_after_ms: Option<u64>,
        #[arg(required = true, num_args = 1..)]
        cmd: Vec<String>,
    },
    /// Compress stdin as `relay x` would for `--cmd`, without running or storing anything
    #[command(hide = true)]
    Pipe {
        #[arg(long)]
        cmd: String,
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
        /// Copy the handoff into `.relay/handoffs/` (credentials masked) for the team
        #[arg(long, conflicts_with = "show")]
        share: bool,
    },
    /// Promote decisions from recent sessions into `.relay/`
    Compile {
        /// Which candidates to keep: numbers like 1,3 or `all`
        #[arg(long, value_name = "WHICH")]
        save: Option<String>,
    },
    /// Save a rule, gotcha or decision to `.relay/` (committed, one file per item)
    Remember {
        #[arg(value_enum)]
        kind: crate::core::memory::Kind,
        /// The item; first line becomes the title
        #[arg(required = true, num_args = 1..)]
        text: Vec<String>,
        /// Paths the item is about (repeatable)
        #[arg(long = "path")]
        paths: Vec<String>,
        /// Session the item came from; set by the hook that rewrote the command
        #[arg(long, hide = true)]
        session: Option<String>,
        /// When the item stops applying: 30d, 12h or a date like 2026-12-01
        #[arg(long, value_name = "WHEN")]
        until: Option<String>,
    },
    /// Point out context waste: what fills every call and costs quota, from the harness transcripts
    Audit {
        /// Harness to audit (default: every harness with transcripts)
        #[arg(long)]
        harness: Option<HarnessId>,
        /// Newest top-level sessions to read, per harness (their subagents are included)
        #[arg(long, default_value_t = 20)]
        sessions: usize,
        /// Every project, not only this one
        #[arg(long)]
        all_projects: bool,
        /// Only sessions started since then: `30m`, `12h`, `7d` or `2026-09-22`
        #[arg(long)]
        since: Option<String>,
        /// Machine-readable output
        #[arg(long)]
        json: bool,
    },
    /// Measure compression: tokens saved and signal kept, per filter
    Bench {
        /// Fixture dir with <name>.cmd, <name>.out and optional <name>.keep
        /// (default: this worktree's stored originals)
        #[arg(long)]
        corpus: Option<std::path::PathBuf>,
        /// Replay the shell calls in a harness's own transcripts (claude)
        #[arg(long, value_name = "HARNESS", conflicts_with = "corpus")]
        history: Option<HarnessId>,
        /// Transcript directory for --history (default: the harness's own)
        #[arg(long, value_name = "DIR", requires = "history")]
        history_dir: Option<std::path::PathBuf>,
        /// Machine-readable output
        #[arg(long)]
        json: bool,
        /// Exit 1 when any `.keep` line or more than this share of signal lines is lost
        #[arg(long, value_name = "RECALL")]
        min_recall: Option<f64>,
    },
    /// Print the brief a new session would receive, or with a query the one page for a task
    Brief {
        /// A task id or words: the plan sections, memory and files for it
        query: Vec<String>,
    },
    /// What relay saved, what it stores, and where
    Status,
    /// Who spent the context: sessions and subagents, the biggest reads, what relay kept out
    Usage {
        /// Sessions active since then: `30m`, `12h`, `7d` or `2026-09-22`
        #[arg(long, default_value = "7d")]
        since: String,
    },
    /// What failed in relay's hooks, newest last
    Log {
        /// How many lines to show
        #[arg(short = 'n', long, default_value_t = crate::limits::status::LOG_LINES)]
        lines: usize,
    },
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
    /// Launch Gemini CLI supervised by relay (installs hooks, writes handoff on exit)
    #[command(trailing_var_arg = true, allow_hyphen_values = true)]
    Gemini {
        /// Resume the last relay-supervised session on this branch
        #[arg(long)]
        last: bool,
        args: Vec<String>,
    },
}

/// Run the command line and return the process exit code. Errors are
/// reported here, with a next step when relay knows one.
pub fn main() -> i32 {
    match run() {
        Ok(code) => code,
        Err(e) => {
            ui::report_error(&e);
            1
        }
    }
}

fn run() -> anyhow::Result<i32> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Setup => setup::run(),
        Commands::Init { local, terse, no_agents_md } => init::run(local, terse, !no_agents_md),
        Commands::Install { harness } => install::install(harness),
        Commands::Uninstall { harness } => install::uninstall(harness),
        Commands::Hook { harness } => hook::run(harness),
        Commands::Exec { raw, session, stop_after_ms, cmd } => {
            let stop_after = stop_after_ms.map(std::time::Duration::from_millis);
            x::run(&x::command_line(&cmd), &exec::Options { raw, session: session.as_deref(), stop_after })
        }
        Commands::Pipe { cmd } => pipe::run(&cmd),
        Commands::Get { id, meta } => get::run(&id, meta),
        Commands::Handoff { session, show, share } => handoff::run(session.as_deref(), show, share),
        Commands::Compile { save } => compile::run(save.as_deref()),
        Commands::Remember { kind, text, paths, session, until } => {
            remember::run(kind, &text.join(" "), &paths, session.as_deref(), until.as_deref())
        }
        Commands::Audit { harness, sessions, all_projects, since, json } => {
            audit::run(&audit::Options { only: harness, sessions, all_projects, since, json })
        }
        Commands::Bench { corpus, history, history_dir, json, min_recall } => {
            let source = match (corpus, history) {
                (Some(dir), _) => bench::Source::Corpus(dir),
                (None, Some(h)) => bench::Source::History(h, history_dir),
                (None, None) => bench::Source::Store,
            };
            bench::run(source, json, min_recall)
        }
        Commands::Brief { query } => brief::run(&query.join(" ")),
        Commands::Log { lines } => log::run(lines),
        Commands::Status => status::run(),
        Commands::Usage { since } => usage::run(&since),
        Commands::Purge { yes } => purge::run(yes),
        Commands::Claude { last, args } => wrap::run(HarnessId::Claude, last, &args),
        Commands::Codex { last, args } => wrap::run(HarnessId::Codex, last, &args),
        Commands::Gemini { last, args } => wrap::run(HarnessId::Gemini, last, &args),
    }
}
