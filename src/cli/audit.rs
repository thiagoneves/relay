use std::path::Path;
use std::time::SystemTime;

use crate::core::audit::run::{Scope, Source};
use crate::core::audit::{self, Finding, Origin, Report, SessionAudit, Severity, Transcript};
use crate::core::paths::Paths;
use crate::harness::{self, Harness, HarnessId};
use crate::helpers::term::{self, Color, Paint};
use crate::helpers::{human_tokens, parse_since, truncate_chars};
use crate::limits;

pub struct Options {
    pub only: Option<HarnessId>,
    pub sessions: usize,
    pub all_projects: bool,
    pub since: Option<String>,
    pub json: bool,
}

pub fn run(o: &Options) -> anyhow::Result<i32> {
    let root = if o.all_projects { None } else { Some(Paths::from_cwd()?.root) };
    let since = match &o.since {
        Some(s) => Some(
            parse_since(s, SystemTime::now())
                .ok_or_else(|| anyhow::anyhow!("--since: use 30m, 12h, 7d or 2026-09-22"))?,
        ),
        None => None,
    };
    let harnesses = match &o.only {
        Some(id) => vec![id.adapter()],
        None => harness::all(),
    };
    let mut scope = if o.all_projects { "all projects".to_string() } else { "this project".to_string() };
    if let Some(s) = &o.since {
        scope.push_str(&format!(", since {s}"));
    }
    let audited = Scope { root: root.as_deref(), since, sessions: o.sessions };
    let mut reports = Vec::new();
    for h in harnesses {
        if let Some(r) = audit::run::run(&Adapter(h.as_ref()), &audited) {
            reports.push((h.id(), r));
        }
    }
    if o.json {
        let out: serde_json::Map<String, serde_json::Value> =
            reports.iter().map(|(id, r)| ((*id).to_string(), serde_json::to_value(r).unwrap_or_default())).collect();
        println!("{}", serde_json::to_string_pretty(&out)?);
        return Ok(0);
    }
    if reports.is_empty() {
        println!("relay audit: no transcripts found for {scope}");
        return Ok(0);
    }
    let p = Paint::stdout();
    for (id, r) in &reports {
        print_report(p, *id, r, &scope);
    }
    println!("{}", p.dim("Sizes are estimates; calls and totals are exact, from the transcripts."));
    println!("{}", p.dim("Share = size × API calls after it entered the context: what it cost on your quota."));
    Ok(0)
}

/// The audit reads a harness through `audit::run::Source`; this is that
/// view of an adapter.
struct Adapter<'a>(&'a dyn Harness);

impl Source for Adapter<'_> {
    fn transcripts(&self, root: Option<&Path>) -> Vec<Transcript> {
        self.0.transcripts(root)
    }
    fn audit_session(&self, transcript: &Path) -> Option<SessionAudit> {
        self.0.audit_session(transcript)
    }
    fn configured_hooks(&self, root: Option<&Path>) -> Option<Vec<String>> {
        self.0.configured_hooks(root)
    }
    fn settled(&self, f: &Finding) -> Option<String> {
        self.0.settled(f)
    }
    fn advise(&self, f: &Finding, r: &Report) -> Vec<String> {
        self.0.advise(f, r)
    }
}

fn print_report(p: Paint, id: HarnessId, r: &Report, scope: &str) {
    let subs = if r.subagents > 0 { format!(" + {} subagents", r.subagents) } else { String::new() };
    println!("{} {}", p.bold(&format!("relay audit · {id}")), p.dim(&format!("· {scope}")));
    println!(
        "{}{subs} · {} calls · {} tokens sent ({:.0}% from cache)\n",
        plural(r.sessions, "session"),
        r.calls,
        human_tokens(r.context_sent),
        r.percent(r.cached)
    );
    print_split(p, r);

    let (history, live): (Vec<&Finding>, Vec<&Finding>) = r.findings.iter().partition(|f| f.is_history());
    if !live.is_empty() {
        let total: usize = live.iter().map(|f| f.resent).sum();
        println!(
            "{} {}",
            p.bold(&format!("Worth a look ({})", live.len())),
            p.dim(&format!("· {:.0}% of what was sent · costs, not verdicts: keep what you need", r.percent(total)))
        );
        let mut shown = Vec::new();
        for f in live {
            print_live(p, f, r, &mut shown);
        }
        println!();
    }
    if !history.is_empty() {
        println!(
            "{} {}",
            p.bold(&format!("Already fixed ({})", history.len())),
            p.dim("· still in sessions that started before the change")
        );
        for f in history {
            let share = if f.resent > 0 { format!(" · {:.1}%", r.percent(f.resent)) } else { String::new() };
            println!(
                "  {} {}{}",
                p.color(Color::Green, "✓"),
                truncate_chars(&f.headline, 100),
                p.dim(&format!("{share} · {}", f.fix.trim_start_matches("Already removed: ")))
            );
        }
        println!();
    }
    print_sources(p, r);
}

/// One stacked bar: who the context was spent on.
fn print_split(p: Paint, r: &Report) {
    let parts = [
        (r.spent_by(Origin::Work), Color::Cyan, "work"),
        (r.spent_by(Origin::Config), Color::Yellow, "your config"),
        (r.spent_by(Origin::Harness), Color::Magenta, "harness"),
        (r.unexplained(), Color::Blue, "unexplained"),
    ];
    #[allow(clippy::cast_precision_loss)]
    let shares: Vec<f64> = parts.iter().map(|(n, ..)| *n as f64 / r.context_sent.max(1) as f64).collect();
    let cells = term::split(&shares, limits::audit::BAR_WIDTH);
    let bar: String = parts.iter().zip(&cells).map(|((_, c, _), n)| p.color(*c, &"█".repeat(*n))).collect();
    println!("  {bar}");
    let legend: Vec<String> =
        parts.iter().map(|(n, c, label)| format!("{} {label} {:.0}%", p.color(*c, "■"), r.percent(*n))).collect();
    println!("  {}\n", legend.join("   "));
}

/// Steps already printed for an earlier finding (MCP servers behind both
/// tool names and server instructions) are referred to, not repeated.
fn print_live(p: Paint, f: &Finding, r: &Report, shown: &mut Vec<String>) {
    let mark = match f.severity {
        Severity::Broken => p.color(Color::Red, "✗"),
        Severity::Waste => p.color(Color::Yellow, "!"),
    };
    let share = if f.resent > 0 { format!("  {:.1}%", r.percent(f.resent)) } else { String::new() };
    println!("  {mark} {}{}", truncate_chars(&f.headline, 90), p.bold(&share));
    if let Some(s) = f.subject.as_deref().filter(|s| !f.headline.contains(*s)) {
        println!("    {}", p.dim(s));
    }
    println!("    → {}", f.fix);
    if !f.steps.is_empty() && f.steps.iter().all(|s| shown.contains(s)) {
        println!("      {} {}", p.dim("·"), p.dim("same steps as above"));
        return;
    }
    for s in &f.steps {
        println!("      {} {s}", p.dim("·"));
        shown.push(s.clone());
    }
}

fn print_sources(p: Paint, r: &Report) {
    println!("{}", p.bold("Where the context went"));
    let top = r.costs.first().map_or(1, |c| c.resent.max(1));
    for c in r.costs.iter().take(limits::audit::SOURCES_SHOWN) {
        let (color, who) = match c.origin {
            Origin::Config => (Color::Yellow, "you"),
            Origin::Harness => (Color::Magenta, "harness"),
            Origin::Work => (Color::Cyan, "work"),
        };
        #[allow(clippy::cast_precision_loss)]
        let bar = term::bar(c.resent as f64 / top as f64, 16);
        println!(
            "  {:<46} {} {:>5.1}% {:>7}  {}",
            truncate_chars(&c.source, 46),
            p.color(color, &format!("{bar:<16}")),
            r.percent(c.resent),
            human_tokens(c.resent),
            p.dim(who)
        );
    }
    let rest = r.unexplained();
    println!(
        "  {} {:<16} {:>5.1}% {:>7}",
        p.dim(&format!("{:<46}", "unexplained: thinking, estimate error")),
        "",
        r.percent(rest),
        human_tokens(rest)
    );
    println!();
}

fn plural(n: usize, word: &str) -> String {
    if n == 1 { format!("1 {word}") } else { format!("{n} {word}s") }
}
