//! `relay claude [args]`: session supervisor. Lives exactly as long as
//! the harness process. Before: install, init, hooks. After: make sure a
//! handoff exists for the session that just ran. No daemon.

use std::process::Command;
use std::time::SystemTime;

use anyhow::{Context, bail};

use crate::core::paths::Paths;
use crate::core::{bootstrap, handoff, machine, outputs, spool};
use crate::harness;
use crate::helpers::{human_tokens, shell};

pub fn run(name: &str, last: bool, args: &[String]) -> anyhow::Result<i32> {
    let h = harness::by_name(name)?;
    let Some(program) = shell::which(h.command()) else {
        bail!("`{}` not found on PATH", h.command());
    };
    let paths = Paths::from_cwd()?;
    paths.ensure_local()?;
    let created = bootstrap::ensure_shared(&paths)?;

    let inst = machine::install_self()?;
    super::setup::report(&inst);
    let report = h.install(&inst.exe)?;
    let mut notes = Vec::new();
    if created {
        notes.push(format!("{} created", paths.rel(&paths.project_file())));
    }
    if report.changed {
        notes.push("hooks installed".to_string());
    }
    if notes.is_empty() {
        notes.push("ready".to_string());
    }
    eprintln!("relay: {}", notes.join(" · "));

    let mut launch: Vec<String> = Vec::new();
    if last {
        match std::fs::read_to_string(paths.local.join("last_session")) {
            Ok(id) if !id.trim().is_empty() => launch.extend(h.resume_args(id.trim())),
            _ => eprintln!("relay: no previous session to resume, starting fresh"),
        }
    }
    launch.extend(args.iter().cloned());

    let started = SystemTime::now();
    let status = Command::new(&program)
        .args(&launch)
        .current_dir(&paths.root)
        .status()
        .with_context(|| format!("failed to launch {}", h.command()))?;
    let code = status.code().unwrap_or(1);

    // Post-exit: the SessionEnd hook normally wrote the handoff. If the
    // harness died without firing it, do it here.
    if let Some((session, mtime)) = spool::sessions(&paths).into_iter().find(|(_, m)| *m >= started) {
        let hp = handoff::path_for(&paths, &session);
        let stale = std::fs::metadata(&hp).and_then(|m| m.modified()).map(|m| m < mtime).unwrap_or(true);
        if stale {
            let _ = handoff::build(&paths, &session, "wrapper-exit", harness::tail_for(&paths, &session).as_ref());
        }
        let outs = outputs::for_session(&paths, &session);
        let saved: usize = outs.iter().map(super::super::core::outputs::OutputMeta::saved).sum();
        eprintln!(
            "relay: session {} · saved ~{} tokens over {} outputs · handoff {}",
            &session[..8.min(session.len())],
            human_tokens(saved),
            outs.len(),
            paths.rel(&hp)
        );
    }
    Ok(code)
}
