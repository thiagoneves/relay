use crate::core::memory::{self, Kind};
use crate::core::paths::Paths;
use crate::core::{handoff, outputs, spool, usage};
use crate::helpers::env::tilde;
use crate::helpers::{dir_size, human_bytes, human_tokens};

use super::ui::Ui;

pub fn run() -> anyhow::Result<i32> {
    let paths = Paths::from_cwd()?;
    let ui = Ui::stdout();
    ui.heading("relay status", &tilde(&paths.root));
    let outs = outputs::list(&paths);
    print_compression(ui, &paths, &outs);
    print_last_session_context(ui, &paths);
    print_orientation(ui, &paths);
    let sessions = spool::sessions(&paths).len();
    ui.field("Sessions", &format!("{sessions} recorded · {} handoffs", handoff::count(&paths)));
    let items = memory::list(&paths);
    let count = |k: Kind| items.iter().filter(|i| i.kind == k).count();
    let remembered = [Kind::Rule, Kind::Gotcha, Kind::Decision].map(count);
    ui.field(
        "Remembered",
        &format!("{} rules · {} gotchas · {} decisions", remembered[0], remembered[1], remembered[2]),
    );
    ui.blank();
    ui.field(
        "Shared",
        &format!("{} · {} · committed with the repo", paths.rel(&paths.shared), human_bytes(dir_size(&paths.shared))),
    );
    ui.field(
        "Local",
        &format!(
            "{} · {} · this worktree, never leaves the machine",
            paths.rel(&paths.local),
            human_bytes(dir_size(&paths.local))
        ),
    );
    ui.blank();
    ui.next(next_step(sessions, remembered.iter().sum()));
    Ok(0)
}

fn next_step(sessions: usize, remembered: usize) -> &'static str {
    match (sessions, remembered) {
        (0, _) => "Start a session with `relay claude` or `relay codex`.",
        (_, 0) => "Save what a new session should know: `relay remember rule \"<one line>\"`.",
        _ => "See what fills your context and how to trim it: `relay audit`.",
    }
}

fn print_compression(ui: Ui, paths: &Paths, outs: &[outputs::OutputMeta]) {
    if outs.is_empty() {
        ui.field("Compression", "nothing compressed yet");
        return;
    }
    let tokens_in: usize = outs.iter().map(|m| m.tokens_in).sum();
    let tokens_out: usize = outs.iter().map(|m| m.tokens_out).sum();
    let saved = tokens_in.saturating_sub(tokens_out);
    ui.field(
        "Compression",
        &format!(
            "{} → {} tokens · {}% saved over {} outputs (estimate)",
            human_tokens(tokens_in),
            human_tokens(tokens_out),
            saved * 100 / tokens_in.max(1),
            outs.len()
        ),
    );
    let refetched = outputs::fetched_ids(paths).len();
    ui.field(
        "Refetched",
        &format!("{refetched} of {} originals · how often the compressed view was not enough", outs.len()),
    );
}

/// Median tokens read before the first edit, sessions with a brief against
/// sessions without one. Only sessions that edited count.
fn print_orientation(ui: Ui, paths: &Paths) {
    let (mut with, mut without) = (Vec::new(), Vec::new());
    for (s, _) in spool::sessions(paths) {
        let u = usage::of(&spool::read(paths, &s));
        match u.brief_tokens {
            Some(b) if u.edited && b > 0 => with.push(u.orientation_tokens),
            Some(_) if u.edited => without.push(u.orientation_tokens),
            _ => {}
        }
    }
    let side = |v: Vec<usize>, label: &str| {
        let n = v.len();
        usage::median(v).map_or_else(|| format!("none {label} yet"), |m| format!("{} {label} (n={n})", human_tokens(m)))
    };
    if !with.is_empty() || !without.is_empty() {
        ui.field(
            "Orientation",
            &format!(
                "tokens read before the first edit: {} · {} (median, estimate)",
                side(with, "with a brief"),
                side(without, "without")
            ),
        );
    }
}

/// Exact numbers for the newest session, read from the harness transcript
/// whose path the `SessionStart` hook recorded.
fn print_last_session_context(ui: Ui, paths: &Paths) {
    let Some((session, _)) = spool::sessions(paths).into_iter().next() else { return };
    let events = spool::read(paths, &session);
    let Some(start) = events.iter().find(|e| e.event == "session_start") else { return };
    let (Some(harness), Some(transcript)) = (start.data["harness"].as_str(), start.data["transcript_path"].as_str())
    else {
        return;
    };
    let Some(audit) = crate::harness::HarnessId::parse(harness)
        .and_then(|id| id.adapter().audit_session(std::path::Path::new(transcript)))
    else {
        return;
    };
    let u = audit.usage;
    ui.field(
        "Last session",
        &format!(
            "{} calls · {} tokens sent, {}% cached · {} at the first call",
            u.calls,
            human_tokens(u.context_sent),
            u.cached * 100 / u.context_sent.max(1),
            human_tokens(u.first_context)
        ),
    );
}
