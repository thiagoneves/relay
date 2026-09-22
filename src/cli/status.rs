use crate::core::memory::{self, Kind};
use crate::core::paths::Paths;
use crate::core::{outputs, spool, usage};
use crate::helpers::{dir_size, human_bytes, human_tokens};

pub fn run() -> anyhow::Result<i32> {
    let paths = Paths::from_cwd()?;
    let outs = outputs::list(&paths);
    let tokens_in: usize = outs.iter().map(|m| m.tokens_in).sum();
    let tokens_out: usize = outs.iter().map(|m| m.tokens_out).sum();
    let saved = tokens_in.saturating_sub(tokens_out);
    let pct = if tokens_in > 0 { saved * 100 / tokens_in } else { 0 };
    let refetched = outputs::fetched_ids(&paths).len();
    let sessions = spool::sessions(&paths).len();
    let handoffs = std::fs::read_dir(paths.handoffs()).map(std::iter::Iterator::count).unwrap_or(0);

    println!("relay status · {}", paths.root.display());
    println!();
    println!(
        "Compression   {} → {} tokens, saved {} ({pct}%) over {} outputs  [estimate]",
        human_tokens(tokens_in),
        human_tokens(tokens_out),
        human_tokens(saved),
        outs.len()
    );
    if !outs.is_empty() {
        println!(
            "Refetched     {refetched} of {} originals ({}%): how often the compressed view was not enough",
            outs.len(),
            refetched * 100 / outs.len()
        );
    }
    print_last_session_context(&paths);
    print_orientation(&paths);
    println!("Sessions      {sessions} recorded, {handoffs} handoffs");
    let items = memory::list(&paths);
    let count = |k: Kind| items.iter().filter(|i| i.kind == k).count();
    println!(
        "Remembered    {} rules, {} gotchas, {} decisions",
        count(Kind::Rule),
        count(Kind::Gotcha),
        count(Kind::Decision)
    );
    println!("Layer cost    0 tokens (no LLM calls in the default path)");
    println!();
    println!("Shared (committed)  {}  {}", paths.rel(&paths.shared), human_bytes(dir_size(&paths.shared)));
    println!("Local (this worktree) {}  {}", paths.local.display(), human_bytes(dir_size(&paths.local)));
    println!("Leaves this machine: nothing.");
    Ok(0)
}

/// Median tokens read before the first edit, sessions with a brief against
/// sessions without one. Only sessions that edited count.
fn print_orientation(paths: &Paths) {
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
        usage::median(v)
            .map_or_else(|| format!("{label}: no sessions yet"), |m| format!("{} {label} (n={n})", human_tokens(m)))
    };
    if !with.is_empty() || !without.is_empty() {
        println!(
            "Orientation   median tokens read before the first edit: {} · {}  [estimate]",
            side(with, "with brief"),
            side(without, "without")
        );
    }
}

/// Exact numbers for the newest session, read from the harness transcript
/// whose path the `SessionStart` hook recorded.
fn print_last_session_context(paths: &Paths) {
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
    println!(
        "Context       last session: {} calls, {} tokens sent ({}% cached), {} at the first call  [exact] · `relay audit` for waste",
        u.calls,
        human_tokens(u.context_sent),
        u.cached * 100 / u.context_sent.max(1),
        human_tokens(u.first_context)
    );
}
