//! `relay setup`: everything a fresh machine needs, once. Installs relay
//! at its stable path, on PATH, with hooks in every harness found.
//! `relay claude|codex` and `relay install` do the same for one harness.

use crate::core::machine::{self, Installed};
use crate::harness;
use crate::helpers::profile::PathChange;

pub fn run() -> anyhow::Result<i32> {
    let inst = machine::install_self()?;
    report(&inst);

    let mut found = Vec::new();
    for h in harness::all() {
        if !h.detect() {
            println!("relay: {} not found, skipped", h.command());
            continue;
        }
        let r = h.install(&inst.exe)?;
        let state = if r.changed { "installed" } else { "already current" };
        println!("relay: {} hooks {state} in {}", h.id(), r.settings_path.display());
        found.push(h.command());
    }

    println!();
    if found.is_empty() {
        println!("Install Claude Code or Codex, then run `relay setup` again.");
        return Ok(0);
    }
    match &inst.path {
        PathChange::Added(f) | PathChange::InProfile(f) => {
            println!("Open a new terminal (or `source {}`), then in any git repo:", f.display());
        }
        PathChange::Present | PathChange::Manual => println!("Ready. In any git repo:"),
    }
    for c in found {
        println!("  relay {c}");
    }
    Ok(0)
}

pub fn report(inst: &Installed) {
    let dir = machine::bin_dir();
    if inst.updated {
        eprintln!("relay: installed {}", inst.exe.display());
    }
    match &inst.path {
        PathChange::Present => {}
        PathChange::InProfile(f) => eprintln!("relay: {} adds {} to PATH (new terminals)", f.display(), dir.display()),
        PathChange::Added(f) => eprintln!("relay: added {} to PATH in {}", dir.display(), f.display()),
        PathChange::Manual => eprintln!("relay: add {} to your PATH to type `relay`", dir.display()),
    }
}
