//! Claude Code adapter. Hooks live in `~/.claude/settings.json`
//! (or `$CLAUDE_CONFIG_DIR/settings.json`).

use std::path::{Path, PathBuf};

use anyhow::Result;

use super::protocol::{hook, hooks_json};
use super::{Harness, InstallReport};
use crate::core::paths::home;

pub struct Claude;

const MARKER: &str = " hook claude";

fn target() -> hooks_json::Target {
    let dir = std::env::var_os("CLAUDE_CONFIG_DIR").map(PathBuf::from).unwrap_or_else(|| home().join(".claude"));
    hooks_json::Target { path: dir.join("settings.json"), marker: MARKER }
}

impl Harness for Claude {
    fn id(&self) -> &'static str {
        "claude-code"
    }

    fn command(&self) -> &'static str {
        "claude"
    }

    fn install(&self, exe: &Path) -> Result<InstallReport> {
        hooks_json::install(&target(), exe)
    }

    fn uninstall(&self) -> Result<InstallReport> {
        hooks_json::uninstall(&target())
    }

    fn handle_hook(&self) -> Result<()> {
        hook::run(self.id(), MARKER)
    }

    fn resume_args(&self, session_id: &str) -> Vec<String> {
        vec!["--resume".into(), session_id.into()]
    }
}
