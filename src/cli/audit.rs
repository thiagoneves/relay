use crate::core::audit::{self, Origin, Report, Severity};
use crate::core::paths::Paths;
use crate::harness;
use crate::helpers::{human_tokens, truncate_chars};

const SOURCES_SHOWN: usize = 15;

pub fn run(only: Option<&str>, sessions: usize, all_projects: bool, json: bool) -> anyhow::Result<i32> {
    let root = if all_projects { None } else { Some(Paths::from_cwd()?.root) };
    let harnesses = match only {
        Some(name) => vec![harness::by_name(name)?],
        None => harness::all(),
    };
    let scope = if all_projects { "all projects".to_string() } else { "this project".to_string() };
    let mut reports = Vec::new();
    for h in harnesses {
        let picked = audit::select(h.transcripts(root.as_deref()), sessions);
        let audits: Vec<_> = picked.iter().filter_map(|t| h.audit_session(&t.path)).collect();
        if !audits.is_empty() {
            reports.push((h.id(), audit::report(audits)));
        }
    }
    if json {
        let out: serde_json::Map<String, serde_json::Value> =
            reports.iter().map(|(id, r)| ((*id).to_string(), serde_json::to_value(r).unwrap_or_default())).collect();
        println!("{}", serde_json::to_string_pretty(&out)?);
        return Ok(0);
    }
    if reports.is_empty() {
        println!("relay audit: no transcripts found for {scope}");
        return Ok(0);
    }
    for (id, r) in &reports {
        print_report(id, r, &scope);
    }
    println!("Sizes are estimates; calls and totals are exact, from the transcripts.");
    println!("`resent` = size × API calls after it entered the context: what it cost on your quota.");
    Ok(0)
}

fn pct(n: usize, of: usize) -> f64 {
    if of == 0 { 0.0 } else { n as f64 * 100.0 / of as f64 }
}

fn print_report(id: &str, r: &Report, scope: &str) {
    println!("relay audit · {id} · {} sessions + {} subagents in {scope}\n", r.sessions, r.subagents);
    println!(
        "Context sent: {} tokens over {} calls ({:.0}% from cache). Subagents: {} ({:.0}%).\n",
        human_tokens(r.context_sent),
        r.calls,
        pct(r.cached, r.context_sent),
        human_tokens(r.subagent_context_sent),
        pct(r.subagent_context_sent, r.context_sent)
    );
    if !r.findings.is_empty() {
        println!("Findings, worst first:");
        for f in &r.findings {
            let mark = if f.severity == Severity::Broken { "✗" } else { "!" };
            let cost = if f.resent > 0 {
                format!(" · {} resent ({:.1}%)", human_tokens(f.resent), pct(f.resent, r.context_sent))
            } else {
                String::new()
            };
            println!("  {mark} {}{cost}", truncate_chars(&f.text, 110));
            println!("    → {}", f.fix);
        }
        println!();
    }
    println!("Where the context went (largest first):");
    println!("  {:<62} {:>6} {:>9} {:>9} {:>6}", "source", "items", "size", "resent", "share");
    for c in r.costs.iter().take(SOURCES_SHOWN) {
        let who = match c.origin {
            Origin::Config => "you",
            Origin::Harness => "harness",
            Origin::Work => "work",
        };
        println!(
            "  {:<62} {:>6} {:>9} {:>9} {:>5.1}%  [{who}]",
            truncate_chars(&c.source, 62),
            c.items,
            human_tokens(c.tokens),
            human_tokens(c.resent),
            pct(c.resent, r.context_sent)
        );
    }
    let itemized: usize = r.costs.iter().map(|c| c.resent).sum();
    println!(
        "  {:<62} {:>6} {:>9} {:>9} {:>5.1}%",
        "not itemized: tool schemas, replies, reasoning",
        "",
        "",
        human_tokens(r.context_sent.saturating_sub(itemized)),
        pct(r.context_sent.saturating_sub(itemized), r.context_sent)
    );
    println!();
}
