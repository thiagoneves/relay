use std::collections::BTreeMap;
use std::path::Path;
use std::time::SystemTime;

use crate::core::paths::Paths;
use crate::core::scoreboard::{self, Tally};
use crate::core::{budget, outputs, spool};
use crate::harness::HarnessId;
use crate::helpers::env::tilde;
use crate::helpers::text::count;
use crate::helpers::{human_tokens, parse_since, truncate_chars};
use crate::limits::usage::{CONSUMERS, READS};

use super::ui::{Ui, problem};

/// One row of the scoreboard: a session's main thread or a subagent.
struct Row {
    who: String,
    /// Context sent and output, exact, when the transcript is still there.
    exact: Option<(usize, usize)>,
    /// Tool output relay saw, estimated.
    tool: usize,
}

impl Row {
    fn weight(&self) -> usize {
        self.exact.map_or(self.tool, |(sent, _)| sent)
    }
}

pub fn run(since: &str, history: bool) -> anyhow::Result<i32> {
    if history {
        return print_history();
    }
    let from = parse_since(since, SystemTime::now()).ok_or_else(|| {
        problem(
            format!("`--since {since}` is not a time relay understands."),
            "Use 30m, 12h, 7d or a date like 2026-09-22.",
        )
    })?;
    let paths = Paths::from_cwd()?;
    let ui = Ui::stdout();
    ui.heading("relay usage", &format!("{}, since {since}", tilde(&paths.root)));
    let tallies: Vec<Tally> = spool::sessions(&paths)
        .into_iter()
        .filter(|(_, mtime)| *mtime >= from)
        .map(|(s, _)| scoreboard::tally(&s, &spool::read(&paths, &s)))
        .collect();
    if tallies.is_empty() {
        ui.ok("No sessions recorded in that window.");
        ui.next("Widen it with `--since 30d`, or start one with `relay claude`.");
        return Ok(0);
    }
    let mut rows: Vec<Row> = tallies.iter().flat_map(rows_of).collect();
    rows.sort_by_key(|r| std::cmp::Reverse(r.weight()));
    let sent: usize = rows.iter().filter_map(|r| r.exact.map(|(s, _)| s)).sum();
    let subagents = tallies.iter().map(|t| t.agents.len() - 1).sum();
    let kept = print_saved(ui, &paths, &tallies, true);
    ui.headline(&format!(
        "{} tokens sent in {} and {} · {} kept out by relay",
        human_tokens(sent),
        count(tallies.len(), "session"),
        count(subagents, "subagent"),
        human_tokens(kept)
    ));
    print_consumers(ui, &rows);
    print_reads(ui, &paths, &tallies);
    print_saved(ui, &paths, &tallies, false);
    ui.blank();
    ui.next("Point an agent at one task's plan section instead of the whole doc: `relay brief <task id>`.");
    Ok(0)
}

/// Per day: tokens and agents.
type Days = BTreeMap<String, (usize, usize)>;

/// What each task cost, per day, from the agents that ended.
fn print_history() -> anyhow::Result<i32> {
    let paths = Paths::from_cwd()?;
    let ui = Ui::stdout();
    ui.heading("relay usage --history", &tilde(&paths.root));
    let spent = budget::history(&paths);
    if spent.is_empty() {
        ui.ok("No agent has ended here since relay started keeping totals.");
        return Ok(0);
    }
    let by = per_task(&spent);
    let mut rows: Vec<(&String, &Days)> = by.iter().collect();
    rows.sort_by_key(|(_, days)| std::cmp::Reverse(days.keys().next_back().cloned()));
    ui.field("Per task", "tool output per day, estimated; a session naming two tasks counts for both");
    for (task, days) in rows.iter().take(CONSUMERS) {
        let total: usize = days.values().map(|d| d.0).sum();
        let agents: usize = days.values().map(|d| d.1).sum();
        let trend: Vec<String> =
            days.iter().map(|(d, (t, _))| format!("{} {}", d.get(5..).unwrap_or(d), human_tokens(*t))).collect();
        ui.field(
            "",
            &format!("{task:<12} {:>7} over {} · {}", human_tokens(total), count(agents, "agent"), trend.join(" → ")),
        );
    }
    Ok(0)
}

/// Task → day → cost; agents without a task id go under "no task id".
fn per_task(spent: &[budget::Spent]) -> BTreeMap<String, Days> {
    let mut by: BTreeMap<String, Days> = BTreeMap::new();
    for s in spent {
        let day = s.ts.get(..10).unwrap_or("").to_string();
        let tasks = if s.tasks.is_empty() { vec!["no task id".to_string()] } else { s.tasks.clone() };
        for t in tasks {
            let e = by.entry(t).or_default().entry(day.clone()).or_default();
            e.0 += s.tokens;
            e.1 += 1;
        }
    }
    by
}

fn print_consumers(ui: Ui, rows: &[Row]) {
    ui.field("Top consumers", "sent and output exact, from the harness transcript; tool output estimated");
    for r in rows.iter().take(CONSUMERS) {
        let cost = match r.exact {
            Some((s, o)) => format!("{:>7} sent · {:>6} out", human_tokens(s), human_tokens(o)),
            None => format!("{:>7} tool output, estimated", human_tokens(r.tool)),
        };
        ui.field("", &format!("{cost}  {}", r.who));
    }
}

fn rows_of(t: &Tally) -> Vec<Row> {
    let short = &t.session[..8.min(t.session.len())];
    let label = if t.label.is_empty() { String::new() } else { format!("  \"{}\"", truncate_chars(&t.label, 40)) };
    let harness = HarnessId::parse(&t.harness).map(HarnessId::adapter);
    t.agents
        .iter()
        .filter(|a| a.calls > 0 || a.transcript.is_some())
        .map(|a| {
            let exact = harness.as_ref().zip(a.transcript.as_deref()).and_then(|(h, p)| {
                let u = h.audit_session(Path::new(p))?.usage;
                (u.calls > 0).then_some((u.context_sent, u.output))
            });
            let who = if a.id.is_empty() {
                format!("{short} main{label}")
            } else {
                let kind = if a.kind.is_empty() { "subagent" } else { a.kind.as_str() };
                format!("{short} › {kind} {}", &a.id[..8.min(a.id.len())])
            };
            Row { who, exact, tool: a.tool_tokens }
        })
        .collect()
}

fn print_reads(ui: Ui, paths: &Paths, tallies: &[Tally]) {
    let mut reads: Vec<(&str, &scoreboard::Read)> =
        tallies.iter().flat_map(|t| t.reads.iter().map(move |r| (t.session.as_str(), r))).collect();
    if reads.is_empty() {
        return;
    }
    reads.sort_by_key(|(_, r)| std::cmp::Reverse(r.tokens));
    ui.blank();
    ui.field("Biggest reads", "tokens each read cost, estimated");
    for (session, r) in reads.iter().take(READS) {
        let by = if r.agent.is_empty() {
            "main".to_string()
        } else {
            format!("subagent {}", &r.agent[..8.min(r.agent.len())])
        };
        let file = paths.rel_file(&r.file);
        ui.field("", &format!("{:>7}  {file}  ({}, {by})", human_tokens(r.tokens), &session[..8.min(session.len())]));
    }
}

/// What relay kept out of the context in these sessions; printed unless
/// `quiet`. Returns the total, estimated.
fn print_saved(ui: Ui, paths: &Paths, tallies: &[Tally], quiet: bool) -> usize {
    let outs: Vec<outputs::OutputMeta> = tallies.iter().flat_map(|t| outputs::for_session(paths, &t.session)).collect();
    let (c_in, c_out) = outs.iter().fold((0, 0), |(i, o), m| (i + m.tokens_in, o + m.tokens_out));
    let guarded: Vec<(usize, usize)> = tallies.iter().flat_map(|t| t.guarded.iter().copied()).collect();
    let (g_in, g_out) = guarded.iter().fold((0, 0), |(i, o), (a, b)| (i + a, o + b));
    if !quiet {
        ui.blank();
        ui.field("Saved", "estimated");
        ui.field(
            "",
            &format!(
                "compression  {} → {} tokens over {}",
                human_tokens(c_in),
                human_tokens(c_out),
                count(outs.len(), "output")
            ),
        );
        ui.field(
            "",
            &format!(
                "read guard   {} → {} tokens over {} turned down",
                human_tokens(g_in),
                human_tokens(g_out),
                count(guarded.len(), "whole-file read")
            ),
        );
    }
    c_in.saturating_sub(c_out) + g_in.saturating_sub(g_out)
}
