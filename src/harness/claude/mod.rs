//! Claude Code adapter. Hooks live in `~/.claude/settings.json`
//! (or `$CLAUDE_CONFIG_DIR/settings.json`).

use std::path::{Path, PathBuf};

use anyhow::Result;

mod transcript;

use super::protocol::{hook, hooks_json};
use super::{Harness, InstallReport};
use crate::core::bench::ShellCall;
use crate::core::paths::home;

pub struct Claude;

const MARKER: &str = " hook claude";

fn config_dir() -> PathBuf {
    std::env::var_os("CLAUDE_CONFIG_DIR").map_or_else(|| home().join(".claude"), PathBuf::from)
}

fn target() -> hooks_json::Target {
    hooks_json::Target { path: config_dir().join("settings.json"), marker: MARKER }
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
        hook::run(self.id())
    }

    fn resume_args(&self, session_id: &str) -> Vec<String> {
        vec!["--resume".into(), session_id.into()]
    }

    fn shell_history(&self, dir: Option<&Path>) -> Result<Vec<ShellCall>> {
        let dir = dir.map_or_else(|| config_dir().join("projects"), Path::to_path_buf);
        anyhow::ensure!(dir.is_dir(), "no Claude Code transcripts at {}", dir.display());
        Ok(transcript::shell_calls(&dir))
    }
}
