//! Claude Code adapter.
//!
//! - `hook.rs`     event handling (stdin JSON → spool, PreToolUse rewrite)
//! - `settings.rs` `~/.claude/settings.json` hook registration

pub mod hook;
pub mod settings;

use std::path::Path;

use anyhow::Result;

use super::{Harness, InstallReport};

pub struct Claude;

impl Harness for Claude {
    fn id(&self) -> &'static str {
        hook::HARNESS
    }

    fn command(&self) -> &'static str {
        "claude"
    }

    fn install(&self, exe: &Path) -> Result<InstallReport> {
        settings::install(exe)
    }

    fn uninstall(&self) -> Result<InstallReport> {
        settings::uninstall()
    }

    fn handle_hook(&self) -> Result<()> {
        hook::run()
    }

    fn resume_args(&self, session_id: &str) -> Vec<String> {
        vec!["--resume".into(), session_id.into()]
    }
}
