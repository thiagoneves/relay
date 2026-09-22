use std::path::PathBuf;
use std::time::SystemTime;

use crate::core::audit::findings::Status;
use crate::core::audit::{self, Finding, Now, Origin, Report, Severity};
use crate::core::paths::Paths;
use crate::harness::{self, Harness};
use crate::helpers::term::{self, Color, Paint};
use crate::helpers::{human_tokens, parse_since, truncate_chars};

const SOURCES_SHOWN: usize = 12;
const BAR: usize = 40;

pub struct Options {
    pub only: Option<String>,
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
        Some(name) => vec![harness::by_name(name)?],
        None => harness::all(),
    };
    let mut scope = if o.all_projects { "all projects".to_string() } else { "this project".to_string() };
    if let Some(s) = &o.since {
        scope.push_str(&format!(", since {s}"));
    }
    let mut reports = Vec::new();
    for h in harnesses {
        if let Some(r) = audit_one(h.as_ref(), root.as_ref(), since, o.sessions) {
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
        print_report(p, id, r, &scope);
    }
    println!("{}", p.dim("Sizes are estimates; calls and totals are exact, from the transcripts."));
    println!("{}", p.dim("Share = size × API calls after it entered the context: what it cost on your quota."));
    Ok(0)
}

fn audit_one(h: &dyn Harness, root: Option<&PathBuf>, since: Option<SystemTime>, limit: usize) -> Option<Report> {
    let all = h.transcripts(root.map(PathBuf::as_path));
    // The newest session of the audited scope shows what its config loads
    // today. Another project's newest session would not list this
    // project's skills or MCP servers, and mark them removed.
    let newest = audit::newest(&all);
    let recent: Vec<_> = all.into_iter().filter(|t| since.is_none_or(|s| t.started >= s)).collect();
    let audits: Vec<_> = audit::select(recent, limit)
        .iter()
        .filter_map(|t| {
            let mut a = h.audit_session(&t.path)?;
            a.seen = Some(t.modified);
            Some(a)
        })
        .collect();
    if audits.is_empty() {
        return None;
    }
    let now = Now {
        hooks: h.configured_hooks(root.map(PathBuf::as_path)),
        newest: newest.as_ref().and_then(|t| h.audit_session(&t.path)),
        newest_started: newest.map(|t| t.started),
        root: root.cloned(),
    };
    let mut r = audit::report(audits, &now);
    for f in &mut r.findings {
        if let Some(why) = h.settled(f) {
            f.status = Status::Gone;
            f.fix = format!("Already removed: {why}");
        }
    }
    let steps: Vec<Vec<String>> = r.findings.iter().map(|f| h.advise(f, &r)).collect();
    for (f, s) in r.findings.iter_mut().zip(steps) {
        f.steps = s;
    }
    Some(r)
}

fn pct(n: usize, of: usize) -> f64 {
    if of == 0 { 0.0 } else { n as f64 * 100.0 / of as f64 }
}

fn print_report(p: Paint, id: &str, r: &Report, scope: &str) {
    let subs = if r.subagents > 0 { format!(" + {} subagents", r.subagents) } else { String::new() };
    println!("{} {}", p.bold(&format!("relay audit · {id}")), p.dim(&format!("· {scope}")));
    println!(
        "{}{subs} · {} calls · {} tokens sent ({:.0}% from cache)\n",
        plural(r.sessions, "session"),
        r.calls,
        human_tokens(r.context_sent),
        pct(r.cached, r.context_sent)
    );
    print_split(p, r);

    let (history, live): (Vec<&Finding>, Vec<&Finding>) = r.findings.iter().partition(|f| f.is_history());
    if !live.is_empty() {
        let total: usize = live.iter().map(|f| f.resent).sum();
        println!(
            "{} {}",
            p.bold(&format!("Worth a look ({})", live.len())),
            p.dim(&format!(
                "· {:.0}% of what was sent · costs, not verdicts: keep what you need",
                pct(total, r.context_sent)
            ))
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
            let share = if f.resent > 0 { format!(" · {:.1}%", pct(f.resent, r.context_sent)) } else { String::new() };
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
    let by = |o: Origin| r.costs.iter().filter(|c| c.origin == o).map(|c| c.resent).sum::<usize>();
    let (work, config, harness) = (by(Origin::Work), by(Origin::Config), by(Origin::Harness));
    let rest = r.context_sent.saturating_sub(work + config + harness);
    let parts = [
        (work, Color::Cyan, "work"),
        (config, Color::Yellow, "your config"),
        (harness, Color::Magenta, "harness"),
        (rest, Color::Blue, "unexplained"),
    ];
    #[allow(clippy::cast_precision_loss)]
    let shares: Vec<f64> = parts.iter().map(|(n, ..)| *n as f64 / r.context_sent.max(1) as f64).collect();
    let cells = term::split(&shares, BAR);
    let bar: String = parts.iter().zip(&cells).map(|((_, c, _), n)| p.color(*c, &"█".repeat(*n))).collect();
    println!("  {bar}");
    let legend: Vec<String> = parts
        .iter()
        .map(|(n, c, label)| format!("{} {label} {:.0}%", p.color(*c, "■"), pct(*n, r.context_sent)))
        .collect();
    println!("  {}\n", legend.join("   "));
}

/// Steps already printed for an earlier finding (MCP servers behind both
/// tool names and server instructions) are referred to, not repeated.
fn print_live(p: Paint, f: &Finding, r: &Report, shown: &mut Vec<String>) {
    let mark = match f.severity {
        Severity::Broken => p.color(Color::Red, "✗"),
        Severity::Waste => p.color(Color::Yellow, "!"),
    };
    let share = if f.resent > 0 { format!("  {:.1}%", pct(f.resent, r.context_sent)) } else { String::new() };
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
    for c in r.costs.iter().take(SOURCES_SHOWN) {
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
            pct(c.resent, r.context_sent),
            human_tokens(c.resent),
            p.dim(who)
        );
    }
    let itemized: usize = r.costs.iter().map(|c| c.resent).sum();
    let rest = r.context_sent.saturating_sub(itemized);
    println!(
        "  {} {:<16} {:>5.1}% {:>7}",
        p.dim(&format!("{:<46}", "unexplained: thinking, estimate error")),
        "",
        pct(rest, r.context_sent),
        human_tokens(rest)
    );
    println!();
}

fn plural(n: usize, word: &str) -> String {
    if n == 1 { format!("1 {word}") } else { format!("{n} {word}s") }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::path::Path;
    use std::time::{Duration, UNIX_EPOCH};

    use super::*;
    use crate::core::audit::{SessionAudit, Transcript};
    use crate::harness::InstallReport;

    /// This project's session, and a newer one in another project.
    struct Fake {
        audited: RefCell<Vec<PathBuf>>,
    }

    fn t(name: &str, secs: u64) -> Transcript {
        let at = UNIX_EPOCH + Duration::from_secs(secs);
        Transcript { path: PathBuf::from(name), id: name.into(), parent: None, started: at, modified: at }
    }

    impl Harness for Fake {
        fn id(&self) -> &'static str {
            "fake"
        }
        fn command(&self) -> &'static str {
            "fake"
        }
        fn install(&self, _: &Path) -> anyhow::Result<InstallReport> {
            unimplemented!()
        }
        fn uninstall(&self) -> anyhow::Result<InstallReport> {
            unimplemented!()
        }
        fn handle_hook(&self) -> anyhow::Result<()> {
            unimplemented!()
        }
        fn resume_args(&self, _: &str) -> Vec<String> {
            Vec::new()
        }
        fn transcripts(&self, root: Option<&Path>) -> Vec<Transcript> {
            match root {
                Some(_) => vec![t("this", 10)],
                None => vec![t("this", 10), t("other-project", 20)],
            }
        }
        fn audit_session(&self, path: &Path) -> Option<SessionAudit> {
            self.audited.borrow_mut().push(path.to_path_buf());
            Some(SessionAudit::default())
        }
    }

    #[test]
    fn project_audit_judges_findings_against_its_own_newest_session() {
        let h = Fake { audited: RefCell::new(Vec::new()) };
        audit_one(&h, Some(&PathBuf::from("/repo")), None, 5);
        assert!(!h.audited.borrow().contains(&PathBuf::from("other-project")), "{:?}", h.audited.borrow());
    }
}
