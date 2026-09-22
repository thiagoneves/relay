//! Cursor adapter, for the editor's agent and the `agent` CLI alike.
//! Hooks live in `~/.cursor/hooks.json` in Cursor's own dialect (see
//! `dialect`). Cursor can rewrite a shell command but not replace its
//! output, so, as on Codex, relay compresses what it can approve.
//!
//! Cursor also runs Claude Code's hooks by default; the Claude adapter
//! leaves events that come from Cursor to this one, so nothing fires twice.

mod dialect;

use std::path::Path;

use anyhow::Result;
use serde_json::Value;

use super::protocol::hooks_json::{self, Layout};
use super::protocol::reply::Reply;
use super::{Harness, HarnessId, InstallReport, RewriteSupport};
use crate::helpers::env;

pub struct Cursor;

const MARKER: &str = " hook cursor";

fn target() -> hooks_json::Target {
    hooks_json::Target {
        path: env::home().join(".cursor").join("hooks.json"),
        marker: MARKER,
        events: dialect::EVENTS,
        layout: Layout::Flat,
    }
}

impl Harness for Cursor {
    fn id(&self) -> HarnessId {
        HarnessId::Cursor
    }

    fn command(&self) -> &'static str {
        "cursor"
    }

    /// Cursor only takes a rewrite it is told to allow; on Windows its
    /// shell is not the POSIX one `relay x` speaks.
    fn rewrites(&self) -> RewriteSupport {
        if cfg!(windows) { RewriteSupport::Never } else { RewriteSupport::ApprovedOnly }
    }

    fn normalize(&self, raw: Value) -> Option<Value> {
        dialect::normalize(&raw)
    }

    fn render(&self, event: &str, reply: &Reply) -> Option<String> {
        dialect::render(event, reply)
    }

    fn install(&self, exe: &Path) -> Result<InstallReport> {
        hooks_json::install(&target(), exe)
    }

    fn uninstall(&self) -> Result<InstallReport> {
        hooks_json::uninstall(&target())
    }

    /// An editor: its agent picks the hooks up, nothing to launch.
    fn launcher(&self) -> Option<String> {
        None
    }

    /// The editor has no command to resume a conversation by id.
    fn resume_args(&self, _: &str) -> Vec<String> {
        Vec::new()
    }
}

/// Whether an event came from Cursor running Claude Code's hooks.
pub fn is_cursor_event(raw: &Value) -> bool {
    raw.get("cursor_version").is_some()
}
