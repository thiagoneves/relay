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
use crate::core::audit::{SessionAudit, Transcript};
use crate::core::paths::home;

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
}
