//! `relay setup`: everything a fresh machine needs, once. Installs relay
//! at its stable path, on PATH, with hooks in every harness found.
//! `relay claude|codex` and `relay install` do the same for one harness.

use crate::harness;
use crate::helpers::env::tilde;
use crate::helpers::profile::PathChange;
use crate::machine::{self, Installed, Refresh};

use super::ui::Ui;

pub fn run() -> anyhow::Result<i32> {
    let ui = Ui::stdout();
    ui.heading("relay setup", "this machine");
    let inst = machine::install_self()?;
    report(&inst);

    let mut found = Vec::new();
    for h in harness::all() {
        if !h.detect() {
            ui.note(&format!("  {} not found, skipped", h.command()));
            continue;
        }
        let r = h.install(&inst.exe)?;
        let state = if r.changed { "hooks installed in" } else { "hooks already current in" };
        ui.ok(&format!("{} {state} {}", h.command(), tilde(&r.settings_path)));
        found.push(h);
    }
    ui.blank();
    if found.is_empty() {
        ui.next("Install Claude Code, Codex, Gemini CLI or Cursor, then run `relay setup` again.");
        return Ok(0);
    }
    if let PathChange::Added(f) | PathChange::InProfile(f) = &inst.path {
        ui.next(&format!("Open a new terminal (or run `source {}`) so `relay` is on your PATH.", tilde(f)));
    }
    let commands: Vec<String> = found.iter().filter_map(|h| h.launcher()).map(|c| format!("`{c}`")).collect();
    if !commands.is_empty() {
        ui.next(&format!("In any git repo, start a session with {}.", commands.join(" or ")));
    }
    if found.iter().any(|h| h.launcher().is_none()) {
        ui.next("In Cursor, just open a project: its agent uses the hooks.");
    }
    Ok(0)
}

/// What installing relay itself changed, on stderr: `relay claude` and
/// `relay install` print it before their own output.
pub fn report(inst: &Installed) {
    let ui = Ui::stderr();
    let dir = tilde(&machine::bin_dir());
    match &inst.refresh {
        Refresh::Unchanged => {}
        Refresh::Updated => ui.ok(&format!("relay installed at {}", tilde(&inst.exe))),
        Refresh::KeptOld(why) => ui.warn(&format!(
            "Could not replace {} ({why}); hooks keep running the previous build until relay is not running.",
            tilde(&inst.exe)
        )),
    }
    match &inst.path {
        PathChange::Present => {}
        PathChange::InProfile(f) => ui.ok(&format!("{} already adds {dir} to PATH for new terminals", tilde(f))),
        PathChange::Added(f) => ui.ok(&format!("Added {dir} to PATH in {}", tilde(f))),
        PathChange::Manual => ui.warn(&format!("Add {dir} to your PATH to type `relay` directly.")),
    }
}
