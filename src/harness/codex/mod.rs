//! Codex CLI adapter. Codex speaks the same hook dialect as Claude Code;
//! hooks live in `$CODEX_HOME/hooks.json` (default `~/.codex/hooks.json`)
//! and must be enabled with `[features] hooks = true` in `config.toml`.
//!
//! Known constraint: Codex runs tool commands inside a sandbox where
//! `.git/` is read-only, so `relay x` cannot keep originals there. See
//! `core::exec` for the degraded path.

mod audit;
mod config_toml;
mod transcript;

use std::path::{Path, PathBuf};

use anyhow::Result;

use super::protocol::{hook, hooks_json};
use super::{Harness, InstallReport};
use crate::core::audit::{Finding, Kind, Report, SessionAudit, Transcript};
use crate::core::paths::{home, tilde};

pub struct Codex;

const MARKER: &str = " hook codex";

pub fn codex_home() -> PathBuf {
    std::env::var_os("CODEX_HOME").map_or_else(|| home().join(".codex"), PathBuf::from)
}

fn target() -> hooks_json::Target {
    hooks_json::Target { path: codex_home().join("hooks.json"), marker: MARKER }
}

impl Harness for Codex {
    fn id(&self) -> &'static str {
        "codex"
    }

    fn command(&self) -> &'static str {
        "codex"
    }

    fn install(&self, exe: &Path) -> Result<InstallReport> {
        let mut report = hooks_json::install(&target(), exe)?;
        if config_toml::enable_hooks(&codex_home().join("config.toml"))? {
            report.changed = true;
            report.events.push("features.hooks=true".into());
        }
        Ok(report)
    }

    fn uninstall(&self) -> Result<InstallReport> {
        hooks_json::uninstall(&target())
    }

    fn handle_hook(&self) -> Result<()> {
        hook::run(self.id())
    }

    fn resume_args(&self, session_id: &str) -> Vec<String> {
        vec!["resume".into(), session_id.into()]
    }

    fn transcripts(&self, root: Option<&Path>) -> Vec<Transcript> {
        transcript::rollouts(&codex_home().join("sessions"), root)
    }

    fn audit_session(&self, path: &Path) -> Option<SessionAudit> {
        audit::session(path)
    }

    /// The desktop app can still pick auto-review for a single session.
    fn settled(&self, f: &Finding) -> Option<String> {
        let text = std::fs::read_to_string(codex_home().join("config.toml")).ok()?;
        (f.kind == Kind::AutoReview && config_toml::top_level(&text, "approvals_reviewer")? == "user")
            .then(|| "config.toml now sets approvals_reviewer = \"user\"".to_string())
    }

    fn advise(&self, f: &Finding, _r: &Report) -> Vec<String> {
        if f.is_history() {
            return Vec::new();
        }
        let home = codex_home();
        let config = tilde(&home.join("config.toml"));
        let step = match f.kind {
            Kind::Skills => format!(
                "Skills load from {} and ~/.agents/skills; for a plugin's skills set `enabled = false` under its [plugins.\"…\"] in {config}",
                tilde(&home.join("skills"))
            ),
            Kind::AutoReview => format!("Set `approvals_reviewer = \"user\"` in {config}"),
            Kind::HookOutput | Kind::FailingHooks => {
                format!("Hooks live in {} and in plugins listed in {config}", tilde(&home.join("hooks.json")))
            }
            Kind::Mcp | Kind::ToolNames => format!("MCP servers are the [mcp_servers.*] tables in {config}"),
            Kind::Instructions | Kind::Agents | Kind::Other => return Vec::new(),
        };
        vec![step]
    }
}
