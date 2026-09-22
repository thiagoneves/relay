//! Gemini CLI adapter. Hooks live under `hooks` in `~/.gemini/settings.json`
//! in Gemini's own dialect (see `dialect`). Gemini can rewrite a shell
//! command but not replace its output, so, as on Codex, relay compresses
//! what it can approve.

mod dialect;
mod transcript;

use std::path::Path;

use anyhow::Result;
use serde_json::Value;

use super::protocol::hooks_json::{self, Layout};
use super::protocol::reply::Reply;
use super::{Harness, HarnessId, InstallReport, RewriteSupport};
use crate::core::handoff::Tail;
use crate::helpers::env;

pub struct Gemini;

const MARKER: &str = " hook gemini";

fn target() -> hooks_json::Target {
    hooks_json::Target {
        path: env::home().join(".gemini").join("settings.json"),
        marker: MARKER,
        events: dialect::EVENTS,
        layout: Layout::Grouped,
    }
}

impl Harness for Gemini {
    fn id(&self) -> HarnessId {
        HarnessId::Gemini
    }

    fn command(&self) -> &'static str {
        "gemini"
    }

    /// A rewrite goes through only with `allow`; on Windows Gemini runs
    /// commands in `PowerShell`, which `relay x` does not speak.
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

    fn resume_args(&self, session_id: &str) -> Vec<String> {
        vec!["--resume".into(), session_id.into()]
    }

    fn session_tail(&self, transcript: &Path) -> Option<Tail> {
        transcript::tail(transcript)
    }
}
