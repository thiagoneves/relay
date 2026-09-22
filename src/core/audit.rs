//! Context waste across sessions: what each source cost on the quota and
//! which of those costs the user can remove. Harness-agnostic: an adapter
//! walks its own transcript and feeds a `Collector` with API calls and the
//! blocks that entered the context; this module prices every block at its
//! size times the calls that followed (a block stays in the context and is
//! resent on each of them), adds sessions up, and ranks findings.

use std::collections::{BTreeSet, HashMap};

use serde::Serialize;

use crate::core::usage::ApiUsage;

/// Who can change a source.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Origin {
    /// User or project configuration: instructions, skills, MCP, hooks.
    Config,
    /// The harness itself: system prompt.
    Harness,
    /// The work: tool results, conversation.
    #[default]
    Work,
}

#[derive(Debug, Default, Clone, Serialize)]
pub struct Cost {
    pub source: String,
    pub origin: Origin,
    pub items: usize,
    /// Size when it entered the context (estimate).
    pub tokens: usize,
    /// `tokens` times the API calls that followed: its share of the quota.
    pub resent: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct HookError {
    pub command: String,
    pub message: String,
    pub count: usize,
}

#[derive(Debug, Default, Clone)]
pub struct SessionAudit {
    pub usage: ApiUsage,
    pub costs: Vec<Cost>,
    pub hook_errors: Vec<HookError>,
    pub skills_listed: BTreeSet<String>,
    pub skills_invoked: BTreeSet<String>,
    /// False when the harness does not record which skills ran.
    pub skills_tracked: bool,
    pub subagent: bool,
}

#[derive(Debug, Default, Serialize)]
pub struct Report {
    pub sessions: usize,
    pub calls: usize,
    pub context_sent: usize,
    pub cached: usize,
    pub subagents: usize,
    pub subagent_context_sent: usize,
    pub costs: Vec<Cost>,
    pub hook_errors: Vec<HookError>,
    pub skills_listed: usize,
    pub skills_used: Vec<String>,
    pub skills_tracked: bool,
    pub findings: Vec<Finding>,
}

#[derive(Debug, Serialize)]
pub struct Finding {
    pub severity: Severity,
    pub text: String,
    pub fix: String,
    /// Tokens resent across the audited sessions; 0 when not a token cost.
    pub resent: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub enum Severity {
    Broken,
    Waste,
}

/// A session transcript a harness wrote. Subagent transcripts carry the
/// id of the session that spawned them.
#[derive(Debug, Clone)]
pub struct Transcript {
    pub path: std::path::PathBuf,
    pub id: String,
    pub parent: Option<String>,
    pub modified: std::time::SystemTime,
}

/// The `limit` newest top-level sessions plus the subagents they spawned.
pub fn select(mut all: Vec<Transcript>, limit: usize) -> Vec<Transcript> {
    all.sort_by_key(|t| std::cmp::Reverse(t.modified));
    let top: Vec<Transcript> = all.iter().filter(|t| t.parent.is_none()).take(limit).cloned().collect();
    let ids: BTreeSet<&str> = top.iter().map(|t| t.id.as_str()).collect();
    let subs: Vec<Transcript> =
        all.iter().filter(|t| t.parent.as_deref().is_some_and(|p| ids.contains(p))).cloned().collect();
    top.into_iter().chain(subs).collect()
}

struct Block {
    source: String,
    origin: Origin,
    tokens: usize,
    items: usize,
    /// Listings report their entry count; other sources count occurrences.
    listing: bool,
    /// Sent on every call and never compacted away (system prompts).
    pinned: bool,
    /// API calls made before it entered the context.
    from: usize,
    /// Calls made before a compaction dropped it; `None` while still in.
    until: Option<usize>,
}

/// Built by a harness adapter while it reads one transcript in order.
#[derive(Default)]
pub struct Collector {
    usage: ApiUsage,
    blocks: Vec<Block>,
    pub audit: SessionAudit,
}

impl Collector {
    pub fn call(&mut self, context: usize, cached: usize, output: usize) {
        self.usage.add_call(context, cached, output);
    }

    /// One more block of `text` from `source` entered the context now.
    pub fn add(&mut self, source: impl Into<String>, origin: Origin, text: &str) {
        self.push(source.into(), origin, text, 1, false, false);
    }

    /// A listing of `count` entries (skills, tools, agents) entered the context.
    pub fn add_listing(&mut self, source: impl Into<String>, origin: Origin, text: &str, count: usize) {
        self.push(source.into(), origin, text, count, true, false);
    }

    /// Text sent with every call that compaction never removes.
    pub fn add_pinned(&mut self, source: impl Into<String>, origin: Origin, text: &str) {
        self.push(source.into(), origin, text, 1, false, true);
    }

    fn push(&mut self, source: String, origin: Origin, text: &str, items: usize, listing: bool, pinned: bool) {
        let tokens = crate::helpers::est_tokens(text);
        if tokens > 0 {
            self.blocks.push(Block {
                source,
                origin,
                tokens,
                items,
                listing,
                pinned,
                from: self.usage.calls,
                until: None,
            });
        }
    }

    /// The harness summarised the context: everything so far stops being resent.
    pub fn compacted(&mut self) {
        let now = self.usage.calls;
        for b in self.blocks.iter_mut().filter(|b| b.until.is_none() && !b.pinned) {
            b.until = Some(now);
        }
    }

    pub fn hook_error(&mut self, command: &str, message: &str) {
        match self.audit.hook_errors.iter_mut().find(|e| e.command == command) {
            Some(e) => e.count += 1,
            None => self.audit.hook_errors.push(HookError {
                command: command.to_string(),
                message: message.to_string(),
                count: 1,
            }),
        }
    }

    pub fn finish(mut self) -> Option<SessionAudit> {
        let calls = self.usage.calls;
        if calls == 0 {
            return None;
        }
        let mut by: HashMap<String, Cost> = HashMap::new();
        for b in self.blocks {
            let c = by.entry(b.source.clone()).or_insert_with(|| Cost {
                source: b.source,
                origin: b.origin,
                ..Cost::default()
            });
            c.tokens += b.tokens;
            c.items = if b.listing { c.items.max(b.items) } else { c.items + b.items };
            c.resent += b.tokens * b.until.unwrap_or(calls).saturating_sub(b.from);
        }
        self.audit.usage = self.usage;
        self.audit.costs = by.into_values().collect();
        Some(self.audit)
    }
}

/// Sources below this share of all context sent are not worth a finding.
const MIN_SHARE: f64 = 0.005;

pub fn report(sessions: Vec<SessionAudit>) -> Report {
    let mut r = Report::default();
    let mut by: HashMap<String, Cost> = HashMap::new();
    let mut listed = BTreeSet::new();
    let mut used = BTreeSet::new();
    for s in sessions {
        if s.subagent {
            r.subagents += 1;
            r.subagent_context_sent += s.usage.context_sent;
        } else {
            r.sessions += 1;
        }
        r.calls += s.usage.calls;
        r.context_sent += s.usage.context_sent;
        r.cached += s.usage.cached;
        for c in s.costs {
            let e = by.entry(c.source.clone()).or_insert_with(|| Cost {
                source: c.source.clone(),
                origin: c.origin,
                ..Cost::default()
            });
            e.items = if c.source.starts_with("Tool results") || c.source.starts_with("Conversation") {
                e.items + c.items
            } else {
                e.items.max(c.items)
            };
            e.tokens += c.tokens;
            e.resent += c.resent;
        }
        for h in s.hook_errors {
            match r.hook_errors.iter_mut().find(|e| e.command == h.command) {
                Some(e) => e.count += h.count,
                None => r.hook_errors.push(h),
            }
        }
        r.skills_tracked |= s.skills_tracked;
        listed.extend(s.skills_listed);
        used.extend(s.skills_invoked);
    }
    r.costs = by.into_values().collect();
    r.costs.sort_by_key(|c| std::cmp::Reverse(c.resent));
    r.skills_listed = listed.len();
    r.skills_used = used.into_iter().collect();
    r.findings = findings(&r);
    r
}

fn findings(r: &Report) -> Vec<Finding> {
    let mut out = Vec::new();
    let mut by_cause: Vec<(&str, Vec<&HookError>)> = Vec::new();
    for h in &r.hook_errors {
        match by_cause.iter_mut().find(|(m, _)| *m == h.message) {
            Some((_, v)) => v.push(h),
            None => by_cause.push((&h.message, vec![h])),
        }
    }
    for (message, hooks) in by_cause {
        let count: usize = hooks.iter().map(|h| h.count).sum();
        let who = if hooks.len() == 1 {
            format!("Hook `{}`", ends(&hooks[0].command))
        } else {
            format!("{} hooks (e.g. `{}`)", hooks.len(), ends(&hooks[0].command))
        };
        out.push(Finding {
            severity: Severity::Broken,
            text: format!(
                "{who} failed {count} times: {}",
                message.trim_start_matches("Failed with non-blocking status code: ")
            ),
            fix: "Fix or remove it in the harness settings or plugin: it runs, and fails, on every matching event"
                .into(),
            resent: 0,
        });
    }
    let share = |n: usize| if r.context_sent == 0 { 0.0 } else { n as f64 / r.context_sent as f64 };
    for c in r.costs.iter().filter(|c| c.origin == Origin::Config && share(c.resent) >= MIN_SHARE) {
        let fix = if c.source == "Skills listing" {
            let used = if r.skills_tracked {
                format!("{} invoked in these sessions", r.skills_used.len())
            } else {
                "this harness does not record which ran".into()
            };
            format!(
                "{} skills listed on every call, {used}. Uninstall or disable the rest; \
                 skills in your home config and plugins load in every project",
                r.skills_listed.max(c.items)
            )
        } else if c.source.starts_with("Hook output") {
            "This hook prints into the context on every prompt. Make it silent or remove it".into()
        } else if c.source.starts_with("MCP server instructions") || c.source.starts_with("Deferred tool names") {
            "Disable MCP servers this project does not use (per-project config instead of global)".into()
        } else if c.source.starts_with("Auto-review") {
            "Every approval sends the conversation to a reviewer model. Use a permission profile that needs fewer approvals, or review manually".into()
        } else if c.source.starts_with("Instruction file ~/") && !c.source.contains("/Projects/") {
            "Loaded from your home directory, so it applies to every project below it. Keep only what applies everywhere".into()
        } else if c.source.starts_with("Agent types") {
            "Remove custom agent definitions you do not use".into()
        } else {
            "Trim it to what the agent needs on every call".into()
        };
        out.push(Finding { severity: Severity::Waste, text: c.source.clone(), fix, resent: c.resent });
    }
    out.sort_by(|a, b| a.severity.cmp(&b.severity).then(b.resent.cmp(&a.resent)));
    out
}

/// Long commands keep their start and their end, where the script name is.
fn ends(cmd: &str) -> String {
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
    use super::*;

    fn cost(source: &str, origin: Origin, resent: usize) -> Cost {
        Cost { source: source.into(), origin, items: 1, tokens: 10, resent }
    }

    #[test]
    fn compaction_stops_charging_what_came_before() {
        let mut col = Collector::default();
        col.add("old", Origin::Work, "some words here");
        col.call(100, 0, 1);
        col.call(100, 0, 1);
        col.compacted();
        col.add("new", Origin::Work, "some words here");
        col.call(100, 0, 1);
        let a = col.finish().unwrap();
        let resent = |k: &str| a.costs.iter().find(|c| c.source == k).unwrap().resent;
        assert_eq!(resent("old"), resent("new") * 2);
    }

    #[test]
    fn findings_rank_broken_first_then_by_cost() {
        let s = SessionAudit {
            usage: ApiUsage { calls: 10, context_sent: 1_000_000, ..ApiUsage::default() },
            costs: vec![
                cost("Skills listing", Origin::Config, 50_000),
                cost("Hook output on UserPromptSubmit: x", Origin::Config, 90_000),
                cost("Tool results: Bash", Origin::Work, 500_000),
                cost("Instruction file ~/CLAUDE.md", Origin::Config, 100),
            ],
            hook_errors: vec![HookError { command: "rtk hook claude".into(), message: "not found".into(), count: 3 }],
            ..SessionAudit::default()
        };
        let r = report(vec![s]);
        let texts: Vec<&str> = r.findings.iter().map(|f| f.text.as_str()).collect();
        assert!(texts[0].starts_with("Hook `rtk hook claude` failed 3 times"), "{texts:?}");
        assert_eq!(&texts[1..], ["Hook output on UserPromptSubmit: x", "Skills listing"]);
    }
}
