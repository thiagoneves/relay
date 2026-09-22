use crate::core::memory::{self, Kind};
use crate::core::paths::Paths;
use crate::core::{handoff, log, outputs, spool, timings, usage};
use crate::helpers::env::tilde;
use crate::helpers::text::count;
use crate::helpers::{dir_size, human_bytes, human_tokens, truncate_chars};

use super::ui::Ui;

pub fn run() -> anyhow::Result<i32> {
    let paths = Paths::from_cwd()?;
    let ui = Ui::stdout();
    ui.heading("relay status", &tilde(&paths.root));
    let outs = outputs::list(&paths);
    print_compression(ui, &paths, &outs);
    print_last_session_context(ui, &paths);
    print_orientation(ui, &paths);
    let failures = print_failures(ui, &paths);
    print_hook_latency(ui, &paths);
    let sessions = spool::sessions(&paths).len();
    ui.field("Sessions", &format!("{} recorded · {}", sessions, count(handoff::count(&paths), "handoff")));
    let remembered = print_remembered(ui, &paths);
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
    ui.next(next_step(failures, sessions, remembered));
    Ok(0)
}

/// Items per kind, and how many may be stale. Returns how many there are.
fn print_remembered(ui: Ui, paths: &Paths) -> usize {
    let now = crate::helpers::now_iso();
    let (expired, items): (Vec<memory::Item>, Vec<memory::Item>) =
        memory::list(paths).into_iter().partition(|i| i.expired(&now));
    let of_kind = |k: Kind| items.iter().filter(|i| i.kind == k).count();
    let remembered = [Kind::Rule, Kind::Gotcha, Kind::Decision].map(of_kind);
    let stale = memory::staleness(&paths.root, &items, crate::limits::brief::STALE_COMMITS)
        .iter()
        .filter(|changed| !changed.is_empty())
        .count();
    let mut notes = String::new();
    if stale > 0 {
        notes.push_str(&format!(" · {stale} may be stale (see `relay brief`)"));
    }
    if !expired.is_empty() {
        notes.push_str(&format!(" · {} expired, delete or renew", expired.len()));
    }
    ui.field(
        "Remembered",
        &format!(
            "{} · {} · {}{notes}",
            count(remembered[0], "rule"),
            count(remembered[1], "gotcha"),
            count(remembered[2], "decision")
        ),
    );
    remembered.iter().sum()
}

fn next_step(failures: usize, sessions: usize, remembered: usize) -> &'static str {
    match (sessions, remembered) {
        _ if failures > 0 => "See what failed and when: `relay log`.",
        (0, _) => {
            "Start a session with `relay claude`, `relay codex` or `relay gemini`, or open the project in Cursor."
        }
        (_, 0) => "Save what a new session should know: `relay remember rule \"<one line>\"`.",
        _ => "See what fills your context and how to trim it: `relay audit`.",
    }
}

/// Hooks never fail in front of the harness; this is where the user
/// finds out that one did. Returns how many failed in the window.
fn print_failures(ui: Ui, paths: &Paths) -> usize {
    let window = crate::limits::status::FAILURE_WINDOW;
    let recent = log::since(paths, std::time::SystemTime::now() - window);
    match recent.last() {
        None => ui.field("Failures", "none in the last 7 days"),
        Some(last) => ui.field(
            "Failures",
            &format!("{} in the last 7 days · last: {}", recent.len(), truncate_chars(&last.what, 80)),
        ),
    }
    recent.len()
}

/// p95 per budget group over the failure window; one that runs over its
/// budget says so.
fn print_hook_latency(ui: Ui, paths: &Paths) {
    let stats = timings::since(paths, std::time::SystemTime::now() - crate::limits::status::FAILURE_WINDOW);
    if stats.is_empty() {
        return;
    }
    let parts: Vec<String> = stats
        .iter()
        .map(|s| {
            let over = if s.p95 > s.group.budget() {
                format!(", over its {:?} budget", s.group.budget())
            } else {
                String::new()
            };
            format!("{:.0?} {}{over}", s.p95, label(s.group))
        })
        .collect();
    let calls: usize = stats.iter().map(|s| s.calls).sum();
    ui.field("Hook time", &format!("p95 {} · {} in the last 7 days", parts.join(" · "), count(calls, "call")));
}

fn label(group: timings::Group) -> &'static str {
    match group {
        timings::Group::PerCall => "per tool call",
        timings::Group::SessionStart => "at session start",
        timings::Group::SessionEnd => "at session end",
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
            "{} → {} tokens · {}% saved over {} (estimate)",
            human_tokens(tokens_in),
            human_tokens(tokens_out),
            saved * 100 / tokens_in.max(1),
            count(outs.len(), "output")
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
