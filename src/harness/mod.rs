//! Harness adapters. One directory per coding agent. An adapter knows
//! three things: how to detect the harness, where its hooks live, and
//! how to map its events onto relay's spool. Adding a harness must not
//! require changes in `core`.
//!
//! Hooks deliver, never work: append one event, exit 0, under 50 ms.
//! Any failure is logged locally and swallowed (fail-open).

pub mod claude;
pub mod codex;
pub mod protocol;

use std::io::Read;
use std::path::Path;

use anyhow::{Result, bail};
use serde_json::Value;

use crate::core::paths::Paths;

pub struct InstallReport {
    pub settings_path: std::path::PathBuf,
    pub backup_path: Option<std::path::PathBuf>,
    pub events: Vec<String>,
    pub changed: bool,
}

pub trait Harness {
    /// Stable id used in spool events and handoffs, e.g. `claude-code`.
    fn id(&self) -> &'static str;
    fn command(&self) -> &'static str;
    fn detect(&self) -> bool {
        which(self.command()).is_some()
    }
    /// Write hooks pointing at `exe`. Idempotent.
    fn install(&self, exe: &Path) -> Result<InstallReport>;
    fn uninstall(&self) -> Result<InstallReport>;
    /// Handle one hook event delivered on stdin.
    fn handle_hook(&self) -> Result<()>;
    /// Extra arguments to resume a native session by id.
    fn resume_args(&self, session_id: &str) -> Vec<String>;
}

pub fn by_name(name: &str) -> Result<Box<dyn Harness>> {
    match name {
        "claude" | "claude-code" => Ok(Box::new(claude::Claude)),
        "codex" => Ok(Box::new(codex::Codex)),
        other => bail!("unknown harness `{other}` (available: claude, codex)"),
    }
}

pub fn which(cmd: &str) -> Option<std::path::PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).map(|d| d.join(cmd)).find(|p| p.is_file())
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
    if std::env::var_os("RELAY_DISABLE").is_some() {
        return;
    }
    if let Err(e) = f()
        && let Ok(p) = Paths::from_cwd()
    {
        crate::core::paths::log(&p, &format!("hook {name} error: {e:#}"));
    }
}
