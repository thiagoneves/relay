//! Claude Code adapter. Hooks live in `~/.claude/settings.json`
//! (or `$CLAUDE_CONFIG_DIR/settings.json`).

use std::path::{Path, PathBuf};

use anyhow::Result;

mod advice;
mod audit;
mod tail;
mod transcript;

use super::protocol::{hook, hooks_json};
use super::{Harness, InstallReport};
use crate::core::audit::{Finding, Report, SessionAudit, Transcript};
use crate::core::bench::ShellCall;
use crate::core::handoff::Tail;
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

    fn transcripts(&self, root: Option<&Path>) -> Vec<Transcript> {
        let projects = config_dir().join("projects");
        let dirs: Vec<PathBuf> = match root {
            Some(r) => vec![projects.join(transcript::project_slug(r))],
            None => std::fs::read_dir(&projects).map(|rd| rd.flatten().map(|e| e.path()).collect()).unwrap_or_default(),
        };
        dirs.iter().flat_map(|d| transcript::sessions_in(d)).collect()
    }

    fn session_tail(&self, transcript: &Path) -> Option<Tail> {
        tail::read(transcript)
    }

    fn audit_session(&self, path: &Path) -> Option<SessionAudit> {
        audit::session(path)
    }

    fn advise(&self, f: &Finding, r: &Report) -> Vec<String> {
        advice::steps(f, r, &config_dir())
    }

    /// Project settings live in each project, so only a single project can
    /// be checked; across all projects the answer is unknown.
    fn configured_hooks(&self, root: Option<&Path>) -> Option<Vec<String>> {
        let root = root?;
        let dir = config_dir();
        let mut files = vec![
            dir.join("settings.json"),
            dir.join("settings.local.json"),
            root.join(".claude/settings.json"),
            root.join(".claude/settings.local.json"),
        ];
        files.extend(audit::plugin_hook_files(&dir.join("plugins/installed_plugins.json")));
        Some(files.iter().flat_map(|f| audit::hook_commands(f)).collect())
    }

    fn shell_history(&self, dir: Option<&Path>) -> Result<Vec<ShellCall>> {
        let dir = dir.map_or_else(|| config_dir().join("projects"), Path::to_path_buf);
        anyhow::ensure!(dir.is_dir(), "no Claude Code transcripts at {}", dir.display());
        Ok(transcript::shell_calls(&dir))
    }
}
