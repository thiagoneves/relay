//! `relay claude [args]`: session supervisor. Lives exactly as long as
//! the harness process. Before: install, init, hooks. After: make sure a
//! handoff exists for the session that just ran. No daemon.

use std::process::Command;
use std::time::SystemTime;

use anyhow::Context;

use crate::core::paths::Paths;
use crate::core::{bootstrap, handoff, outputs, spool};
use crate::harness::{self, Harness, HarnessId};
use crate::helpers::env::Var;
use crate::helpers::{human_tokens, new_id, shell};
use crate::machine;

use super::ui::{Ui, problem};

pub fn run(id: HarnessId, last: bool, args: &[String]) -> anyhow::Result<i32> {
    let h = id.adapter();
    let program = shell::which(h.command()).ok_or_else(|| {
        problem(
            format!("`{}` is not installed, or not on your PATH.", h.command()),
            format!("Install it, check that `{}` runs in this terminal, then try again.", h.command()),
        )
    })?;
    let paths = Paths::from_cwd()?;
    prepare(h.as_ref(), &paths)?;
    let mut launch = if last { resume_args(h.as_ref(), &paths) } else { Vec::new() };
    launch.extend(args.iter().cloned());

    let wrapper = new_id("w");
    let started = SystemTime::now();
    let status = Command::new(&program)
        .args(&launch)
        .env(Var::RelayWrapper.name(), &wrapper)
        .current_dir(&paths.root)
        .status()
        .with_context(|| format!("failed to launch {}", h.command()))?;
    if let Some((session, mtime)) = own_session(&paths, &wrapper, started) {
        close(h.command(), &paths, &session, mtime);
    }
    Ok(status.code().unwrap_or(1))
}

/// Local store, project rules, relay on PATH and hooks, before launch.
fn prepare(h: &dyn Harness, paths: &Paths) -> anyhow::Result<()> {
    paths.ensure_local()?;
    let created = bootstrap::ensure_shared(paths)?;
    let inst = machine::install_self()?;
    super::setup::report(&inst);
    let report = h.install(&inst.exe)?;
    let ui = Ui::stderr();
    if created {
        ui.ok(&format!("Created {}: the rules every session reads first.", paths.rel(&paths.project_file())));
    }
    if report.changed {
        ui.ok(&format!("{} hooks installed.", h.command()));
    }
    Ok(())
}

fn resume_args(h: &dyn Harness, paths: &Paths) -> Vec<String> {
    if let Some(id) = spool::last_session(paths) {
        h.resume_args(&id)
    } else {
        Ui::stderr().warn("No previous session to resume; starting a new one.");
        Vec::new()
    }
}

/// After the harness exits: the `SessionEnd` hook normally wrote the
/// handoff; if the harness died without firing it, write it here.
fn close(command: &str, paths: &Paths, session: &str, ended: SystemTime) {
    let hp = handoff::path_for(paths, session);
    let stale = std::fs::metadata(&hp).and_then(|m| m.modified()).map_or(true, |m| m < ended);
    if stale {
        let _ = handoff::build(paths, session, "wrapper-exit", harness::tail_for(paths, session).as_ref());
    }
    outputs::prune(paths, crate::limits::store::KEEP_OUTPUTS);
    let outs = outputs::for_session(paths, session);
    let saved: usize = outs.iter().map(outputs::OutputMeta::saved).sum();
    let ui = Ui::stderr();
    ui.ok(&format!(
        "Session saved for the next one · ~{} tokens saved over {} outputs · {}",
        human_tokens(saved),
        outs.len(),
        paths.rel(&hp)
    ));
    ui.next(&format!("`relay {command}` picks up from here; `relay {command} --last` resumes this session."));
}

/// The session this wrapper launched: the latest one stamped with its id
/// (a `/clear` inside it starts another). Sessions stamped by a different
/// wrapper belong to a parallel `relay claude|codex`; an unstamped one is
/// taken only as a fallback, for a harness that does not pass its
/// environment on to hooks.
fn own_session(paths: &Paths, wrapper: &str, started: SystemTime) -> Option<(String, SystemTime)> {
    let recent: Vec<_> = spool::sessions(paths).into_iter().filter(|(_, m)| *m >= started).collect();
    let stamps: Vec<_> = recent.iter().map(|(s, _)| spool::wrappers(paths, s)).collect();
    let pick = |f: &dyn Fn(&[String]) -> bool| recent.iter().zip(&stamps).find(|(_, w)| f(w)).map(|(s, _)| s.clone());
    pick(&|w| w.iter().any(|x| x == wrapper)).or_else(|| pick(&|w| w.is_empty()))
}
