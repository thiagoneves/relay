//! Harness adapters. One directory per coding agent. An adapter knows
//! three things: how to detect the harness, where its hooks live, and
//! how to map its events onto relay's spool. Adding a harness must not
//! require changes in `core`.
//!
//! Hooks deliver, never work: append one event, exit 0, under 50 ms.
//! Any failure is logged locally and swallowed (fail-open).

pub mod claude;
pub mod codex;
pub mod jsonl;
pub mod protocol;

use std::io::Read;
use std::path::Path;

use anyhow::{Result, bail};
use serde_json::Value;

use crate::core::audit::{Finding, Report, SessionAudit, Transcript};
use crate::core::bench::ShellCall;
use crate::core::handoff::Tail;
use crate::core::paths::Paths;
use crate::helpers::env::{self, Var};

/// The harnesses relay has an adapter for. The CLI name is what users
/// type and what installed hooks call (`relay hook claude`); the stored
/// name is what spool events and handoffs record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum HarnessId {
    #[value(alias = "claude-code")]
    Claude,
    Codex,
}

impl HarnessId {
    pub fn stored(self) -> &'static str {
        match self {
            Self::Claude => "claude-code",
            Self::Codex => "codex",
        }
    }

    /// Accepts both the CLI and the stored name.
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "claude" | "claude-code" => Some(Self::Claude),
            "codex" => Some(Self::Codex),
            _ => None,
        }
    }

    pub fn adapter(self) -> Box<dyn Harness> {
        match self {
            Self::Claude => Box::new(claude::Claude),
            Self::Codex => Box::new(codex::Codex),
        }
    }
}

impl std::fmt::Display for HarnessId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.stored())
    }
}

/// How far a harness lets a hook rewrite a shell command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RewriteSupport {
    /// Rewrites with or without a permission decision.
    Any,
    /// Only rewrites the hook also approves.
    ApprovedOnly,
    /// Commands are left alone.
    Never,
}

pub struct InstallReport {
    pub settings_path: std::path::PathBuf,
    pub backup_path: Option<std::path::PathBuf>,
    pub events: Vec<String>,
    pub changed: bool,
}

pub trait Harness {
    fn id(&self) -> HarnessId;
    fn command(&self) -> &'static str;
    fn detect(&self) -> bool {
        crate::helpers::shell::which(self.command()).is_some()
    }
    /// Write hooks pointing at `exe`. Idempotent.
    fn install(&self, exe: &Path) -> Result<InstallReport>;
    fn uninstall(&self) -> Result<InstallReport>;
    fn rewrites(&self) -> RewriteSupport {
        RewriteSupport::Any
    }
    /// Whether a `PostToolUse` hook may replace the shell output the model
    /// sees, so relay can shrink it without rewriting the command.
    fn replaces_output(&self) -> bool {
        false
    }
    /// How long the harness lets this shell call run before killing it,
    /// when it says. `relay x` stops just before, so the output so far
    /// still reaches the model.
    fn command_timeout(&self, tool_input: &Value) -> Option<std::time::Duration> {
        let _ = tool_input;
        None
    }
    /// Extra arguments to resume a native session by id.
    fn resume_args(&self, session_id: &str) -> Vec<String>;
    /// Session transcripts for the project at `root`, or for every
    /// project when `None`, subagents included.
    fn transcripts(&self, root: Option<&Path>) -> Vec<Transcript> {
        let _ = root;
        Vec::new()
    }
    /// Hook commands configured today for the project at `root`, plugins
    /// included. `None` when the adapter cannot tell.
    fn configured_hooks(&self, root: Option<&Path>) -> Option<Vec<String>> {
        let _ = root;
        None
    }
    /// What the handoff needs from a session transcript that hooks never
    /// see: how each turn ended, the user's answers, the approved plan.
    fn session_tail(&self, transcript: &Path) -> Option<Tail> {
        let _ = transcript;
        None
    }
    /// The text the model received for each of `ids`, as the transcript
    /// recorded it; ids it no longer holds are left out.
    fn tool_results(&self, transcript: &Path, ids: &[&str]) -> std::collections::HashMap<String, String> {
        let _ = (transcript, ids);
        std::collections::HashMap::new()
    }
    /// Where one session's context went; see `core::audit`.
    fn audit_session(&self, transcript: &Path) -> Option<SessionAudit> {
        let _ = transcript;
        None
    }
    /// Concrete steps to fix `f` in this harness: commands, files, menus.
    fn advise(&self, f: &Finding, r: &Report) -> Vec<String> {
        let _ = (f, r);
        Vec::new()
    }
    /// Why `f` no longer applies, when the adapter can tell from today's
    /// config and the audited sessions cannot (they predate the change).
    fn settled(&self, f: &Finding) -> Option<String> {
        let _ = f;
        None
    }
    /// Shell calls from the harness's own transcripts, for benchmarking
    /// against real history. `dir` overrides the default location.
    fn shell_history(&self, dir: Option<&Path>) -> Result<Vec<ShellCall>> {
        let _ = dir;
        bail!("reading {} history is not supported yet", self.id())
    }
}

/// Every adapter relay has, for commands that look across harnesses.
pub fn all() -> Vec<Box<dyn Harness>> {
    [HarnessId::Claude, HarnessId::Codex].into_iter().map(HarnessId::adapter).collect()
}

/// The transcript tail of a recorded session, through the harness and
/// transcript its `session_start` event names.
pub fn tail_for(paths: &Paths, session: &str) -> Option<Tail> {
    let events = crate::core::spool::read(paths, session);
    let start = events.iter().find(|e| e.event == "session_start")?;
    let h = HarnessId::parse(start.data["harness"].as_str()?)?.adapter();
    h.session_tail(Path::new(start.data["transcript_path"].as_str()?))
}

/// Read the whole stdin as JSON. Empty stdin is not an error.
pub fn read_stdin_json() -> Result<Option<Value>> {
    let mut buf = String::new();
    std::io::stdin().read_to_string(&mut buf)?;
    if buf.trim().is_empty() {
        return Ok(None);
    }
    Ok(Some(serde_json::from_str(&buf)?))
}

/// Run a hook handler fail-open: errors go to the local log, exit 0.
pub fn run_fail_open(name: &str, f: impl FnOnce() -> Result<()>) {
    if env::is_set(Var::RelayDisable) {
        return;
    }
    if let Err(e) = f()
        && let Ok(p) = Paths::from_cwd()
    {
        crate::core::log::write(&p, &format!("hook {name} error: {e:#}"));
    }
}
