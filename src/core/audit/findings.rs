//! Findings: config sources and failing hooks worth acting on, each told
//! apart as live or history against what the harness loads today.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::SystemTime;

use serde::Serialize;

use super::{Cost, HookError, Now, Origin, Report};
use crate::helpers::{human_tokens, short_utc};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub enum Severity {
    Broken,
    Waste,
}

/// What a finding is about, so an adapter can say how to fix it there.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Kind {
    FailingHooks,
    HookOutput,
    Skills,
    Instructions,
    Mcp,
    ToolNames,
    Agents,
    AutoReview,
    Other,
}

/// Whether today's config still produces it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Status {
    Active,
    /// Still there, with fewer entries (skills, servers) than audited.
    Reduced {
        now: usize,
    },
    Gone,
    Unknown,
}

#[derive(Debug, Serialize)]
pub struct Finding {
    pub severity: Severity,
    pub kind: Kind,
    pub status: Status,
    pub headline: String,
    /// The file or hook command it is about, when there is one.
    pub subject: Option<String>,
    pub fix: String,
    /// Concrete steps for this harness; filled by the adapter.
    pub steps: Vec<String>,
    /// Tokens resent across the audited sessions; 0 when not a token cost.
    pub resent: usize,
    /// Newest audited session it appeared in.
    pub last_seen: Option<String>,
}

impl Finding {
    pub fn is_history(&self) -> bool {
        self.status == Status::Gone
    }
}

/// Sources below this share of all context sent are not worth a finding.
const MIN_SHARE: f64 = 0.005;

pub fn findings(r: &Report, seen: &HashMap<String, SystemTime>, now: &Now) -> Vec<Finding> {
    let mut out = failing_hooks(&r.hook_errors, now);
    let share = |n: usize| if r.context_sent == 0 { 0.0 } else { n as f64 / r.context_sent as f64 };
    for c in r.costs.iter().filter(|c| c.origin == Origin::Config && share(c.resent) >= MIN_SHARE) {
        let kind = kind_of(&c.source);
        let (status, why) = status_of(c, kind, now);
        let fix = match why {
            Some(why) => format!("Already removed: {why}"),
            None => fix_for(kind, &c.source),
        };
        out.push(Finding {
            severity: Severity::Waste,
            kind,
            status,
            headline: headline(c, kind, status, r),
            subject: subject_of(c, kind),
            fix,
            steps: Vec::new(),
            resent: c.resent,
            last_seen: seen.get(&c.source).copied().map(short_utc),
        });
    }
    out.sort_by(|a, b| {
        a.is_history().cmp(&b.is_history()).then(a.severity.cmp(&b.severity)).then(b.resent.cmp(&a.resent))
    });
    out
}

fn failing_hooks(errors: &[HookError], now: &Now) -> Vec<Finding> {
    let mut by_cause: Vec<(&str, Vec<&HookError>)> = Vec::new();
    for h in errors {
        match by_cause.iter_mut().find(|(m, _)| *m == h.message) {
            Some((_, v)) => v.push(h),
            None => by_cause.push((&h.message, vec![h])),
        }
    }
    by_cause
        .into_iter()
        .map(|(message, hooks)| {
            let count: usize = hooks.iter().map(|h| h.count).sum();
            let last = hooks.iter().filter_map(|h| h.last_seen).max();
            let status = match &now.hooks {
                Some(cfg) if hooks.iter().any(|h| cfg.iter().any(|c| c == &h.command)) => Status::Active,
                Some(_) => Status::Gone,
                None => Status::Unknown,
            };
            let fix = if status == Status::Gone {
                "Already removed: gone from your hook config".into()
            } else {
                "Fix or remove it: it runs, and fails, on every matching event".into()
            };
            let who = if hooks.len() == 1 { "Hook".to_string() } else { format!("{} hooks", hooks.len()) };
            let why = message.trim_start_matches("Failed with non-blocking status code: ");
            Finding {
                severity: Severity::Broken,
                kind: Kind::FailingHooks,
                status,
                headline: format!("{who} failed {count}× · {}", crate::helpers::truncate_chars(why, 60)),
                subject: Some(ends(&hooks[0].command)),
                fix,
                steps: Vec::new(),
                resent: 0,
                last_seen: last.map(short_utc),
            }
        })
        .collect()
}

fn kind_of(source: &str) -> Kind {
    let starts = |p: &str| source.starts_with(p);
    if source == "Skills listing" {
        Kind::Skills
    } else if starts("Instruction file ") {
        Kind::Instructions
    } else if starts("Hook output") || starts("Hook or plugin output") {
        Kind::HookOutput
    } else if starts("MCP server instructions") {
        Kind::Mcp
    } else if starts("Deferred tool names") {
        Kind::ToolNames
    } else if starts("Agent types") {
        Kind::Agents
    } else if starts("Auto-review") {
        Kind::AutoReview
    } else {
        Kind::Other
    }
}

fn subject_of(c: &Cost, kind: Kind) -> Option<String> {
    match kind {
        Kind::Instructions => c.source.strip_prefix("Instruction file ").map(str::to_string),
        Kind::HookOutput => hook_command(&c.source).map(str::to_string),
        _ => None,
    }
}

/// `Hook output on <event>: <command>` → the command.
fn hook_command(source: &str) -> Option<&str> {
    source.strip_prefix("Hook output on ")?.split_once(": ").map(|(_, cmd)| cmd)
}

/// Live or history, and why it is history. Files are checked on disk,
/// hooks against today's config, everything else against the harness's
/// newest session.
fn status_of(c: &Cost, kind: Kind, now: &Now) -> (Status, Option<String>) {
    if kind == Kind::Instructions
        && let Some(p) = c.source.strip_prefix("Instruction file ").and_then(|p| resolve(p, now))
    {
        return if p.exists() {
            (Status::Active, None)
        } else {
            (Status::Gone, Some("the file no longer exists".into()))
        };
    }
    if let (Some(cmd), Some(cfg)) = (hook_command(&c.source), &now.hooks)
        && cfg.iter().any(|h| same_command(h, cmd))
    {
        return (Status::Active, None);
    }
    let Some(newest) = &now.newest else { return (Status::Unknown, None) };
    let when = now.newest_started.map_or_else(String::new, |t| format!(", started {}", short_utc(t)));
    match newest.costs.iter().find(|x| x.source == c.source) {
        None => (Status::Gone, Some(format!("not in your newest session{when}"))),
        Some(x) if x.items < c.items => (Status::Reduced { now: x.items }, None),
        Some(_) => (Status::Active, None),
    }
}

/// `~/x` against home, a relative path against the project root.
fn resolve(path: &str, now: &Now) -> Option<PathBuf> {
    if let Some(rest) = path.strip_prefix("~/") {
        return Some(crate::core::paths::home().join(rest));
    }
    let p = PathBuf::from(path);
    if p.is_absolute() { Some(p) } else { now.root.as_ref().map(|r| r.join(p)) }
}

/// Adapters shorten long hook commands with a trailing `…`.
fn same_command(configured: &str, label: &str) -> bool {
    let flat = configured.split_whitespace().collect::<Vec<_>>().join(" ");
    match label.strip_suffix('…') {
        Some(prefix) => flat.starts_with(prefix),
        None => flat == label,
    }
}

fn headline(c: &Cost, kind: Kind, status: Status, r: &Report) -> String {
    let size = human_tokens(c.tokens / c.items.max(1));
    let now = match status {
        Status::Reduced { now } => format!(" (now {now})"),
        _ => String::new(),
    };
    match kind {
        Kind::Skills => {
            let used = if r.skills_tracked {
                format!("{} used", r.skills_used.len())
            } else {
                "use not recorded by this harness".into()
            };
            format!("{} skills listed on every call{now}, {used}", r.skills_listed.max(c.items))
        }
        Kind::Instructions => format!("{} · {size} tokens on every call", subject_of(c, kind).unwrap_or_default()),
        Kind::Mcp => format!("{} MCP servers add instructions to every call{now}", c.items),
        Kind::ToolNames => format!("{} tool names listed on every call{now}", c.items),
        Kind::Agents => format!("{} agent types listed on every call{now}", c.items),
        Kind::HookOutput => match hook_command(&c.source) {
            Some(cmd) => format!("Hook `{}` writes into the context", ends(cmd)),
            None => c.source.clone(),
        },
        Kind::AutoReview | Kind::Other | Kind::FailingHooks => c.source.clone(),
    }
}

fn fix_for(kind: Kind, source: &str) -> String {
    match kind {
        Kind::Skills => {
            "The listing loads in every project; a skill you rarely need can live in the projects that use it".into()
        }
        Kind::Instructions if source.contains("/projects/") || source.contains("/memory/") => {
            "Project memory: worth pruning entries that no longer hold".into()
        }
        Kind::Instructions if source.starts_with("Instruction file ~/") => {
            "Applies to every project below it: what only some projects need can move into theirs".into()
        }
        Kind::HookOutput => "If the agent does not need what it prints, the hook can run silently".into(),
        Kind::Mcp | Kind::ToolNames => "Servers you need only in some projects can be enabled just there".into(),
        Kind::Agents => "Agent types you no longer use can go".into(),
        Kind::AutoReview => {
            "Every approval sends the conversation to a reviewer model; reviewing approvals yourself avoids it".into()
        }
        Kind::Instructions | Kind::Other | Kind::FailingHooks => {
            "Worth trimming to what the agent needs on every call".into()
        }
    }
}

/// Long commands keep their start and their end, where the script name is.
pub fn ends(cmd: &str) -> String {
    let flat = cmd.split_whitespace().collect::<Vec<_>>().join(" ");
    let chars: Vec<char> = flat.chars().collect();
    if chars.len() <= 70 {
        return flat;
    }
    let head: String = chars[..25].iter().collect();
    let tail: String = chars[chars.len() - 40..].iter().collect();
    format!("{head} … {tail}")
}

#[cfg(test)]
mod tests {
    use super::super::{ApiUsage, SessionAudit, report};
    use super::*;

    fn cost(source: &str, origin: Origin, items: usize, resent: usize) -> Cost {
        Cost { source: source.into(), origin, items, tokens: 10 * items, resent }
    }

    fn session(costs: Vec<Cost>, hook_errors: Vec<HookError>) -> SessionAudit {
        SessionAudit {
            usage: ApiUsage { calls: 10, context_sent: 1_000_000, ..ApiUsage::default() },
            costs,
            hook_errors,
            ..SessionAudit::default()
        }
    }

    fn hook_error(command: &str, message: &str) -> HookError {
        HookError { command: command.into(), message: message.into(), count: 1, last_seen: None }
    }

    #[test]
    fn hooks_gone_from_config_are_history() {
        let s = session(vec![], vec![hook_error("old", "gone"), hook_error("live", "broken")]);
        let now = Now { hooks: Some(vec!["live".into()]), ..Now::default() };
        let r = report(vec![s], &now);
        let status: Vec<Status> = r.findings.iter().map(|f| f.status).collect();
        assert_eq!(status, [Status::Active, Status::Gone]);
    }

    #[test]
    fn newest_session_tells_removed_and_reduced_sources() {
        let old = session(
            vec![
                cost("Skills listing", Origin::Config, 300, 50_000),
                cost("Agent types listing", Origin::Config, 27, 40_000),
                cost("Instruction file /nonexistent/CLAUDE.md", Origin::Config, 1, 90_000),
            ],
            vec![],
        );
        let newest = session(vec![cost("Skills listing", Origin::Config, 36, 5_000)], vec![]);
        let now = Now { newest: Some(newest), ..Now::default() };
        let r = report(vec![old], &now);
        let by = |k: Kind| r.findings.iter().find(|f| f.kind == k).unwrap();
        assert_eq!(by(Kind::Skills).status, Status::Reduced { now: 36 });
        assert_eq!(by(Kind::Agents).status, Status::Gone);
        assert_eq!(by(Kind::Instructions).status, Status::Gone);
        assert!(!r.findings[0].is_history(), "live findings come first");
    }

    #[test]
    fn live_findings_rank_broken_first_then_by_cost() {
        let s = session(
            vec![
                cost("Skills listing", Origin::Config, 1, 50_000),
                cost("Hook output on UserPromptSubmit: x", Origin::Config, 1, 90_000),
                cost("Tool results: Bash", Origin::Work, 1, 500_000),
                cost("Instruction file ~/CLAUDE.md", Origin::Config, 1, 100),
            ],
            vec![hook_error("rtk hook claude", "not found")],
        );
        let r = report(vec![s], &Now::default());
        let kinds: Vec<Kind> = r.findings.iter().map(|f| f.kind).collect();
        assert_eq!(kinds, [Kind::FailingHooks, Kind::HookOutput, Kind::Skills]);
    }

    #[test]
    fn matches_shortened_hook_commands() {
        assert!(same_command("sh -c  'long   command here'", "sh -c 'long…"));
        assert!(!same_command("other", "sh -c 'long…"));
    }
}
