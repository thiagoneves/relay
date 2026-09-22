//! Context waste across sessions: what each source cost on the quota and
//! which of those costs the user can remove. Harness-agnostic: an adapter
//! walks its own transcript and feeds a `Collector` with API calls and the
//! blocks that entered the context; this module prices every block at its
//! size times the calls that followed (a block stays in the context and is
//! resent on each of them), adds sessions up, and ranks findings.

pub mod findings;
pub mod run;

use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;

use serde::Serialize;

pub use findings::{Finding, Kind, Severity};

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
    #[serde(skip)]
    pub last_seen: Option<std::time::SystemTime>,
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
    pub mcp_servers: BTreeSet<String>,
    pub subagent: bool,
    /// When the transcript was last written; set by the caller.
    pub seen: Option<std::time::SystemTime>,
}

/// What the harness loads today, to tell live findings from history. A
/// session loads its context when it starts, so findings from sessions
/// started before a config change keep showing what was removed since.
#[derive(Default)]
pub struct Now {
    /// Hook commands in today's config, when the adapter can read them.
    pub hooks: Option<Vec<String>>,
    /// The harness's newest session in any project: the latest context
    /// the global config produced.
    pub newest: Option<SessionAudit>,
    pub newest_started: Option<std::time::SystemTime>,
    /// Project root, to resolve relative instruction files.
    pub root: Option<PathBuf>,
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
    /// Skills listed in the newest session and never invoked in any audited one.
    pub skills_unused: Vec<String>,
    /// MCP servers in the newest session, else in the audited ones.
    pub mcp_servers: Vec<String>,
    pub findings: Vec<Finding>,
}

/// A session transcript a harness wrote. Subagent transcripts carry the
/// id of the session that spawned them.
#[derive(Debug, Clone)]
pub struct Transcript {
    pub path: std::path::PathBuf,
    pub id: String,
    pub parent: Option<String>,
    /// When the session began: the harness loads its context then, so a
    /// config change only shows in sessions started after it.
    pub started: std::time::SystemTime,
    pub modified: std::time::SystemTime,
}

impl Transcript {
    /// File birth time where the filesystem records one, else last write.
    pub fn from_file(path: std::path::PathBuf, id: String, parent: Option<String>) -> Option<Self> {
        let meta = std::fs::metadata(&path).ok()?;
        let modified = meta.modified().ok()?;
        let started = meta.created().unwrap_or(modified);
        Some(Self { path, id, parent, started, modified })
    }
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

/// The top-level session started last.
pub fn newest(all: &[Transcript]) -> Option<Transcript> {
    all.iter().filter(|t| t.parent.is_none()).max_by_key(|t| t.started).cloned()
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

/// What the first call sent beyond the blocks seen before it: the system
/// prompt and tool schemas, which transcripts do not record. Sent on every
/// call.
pub const FIXED_OVERHEAD: &str = "System prompt and tool schemas (from 1st call)";

/// Built by a harness adapter while it reads one transcript in order.
#[derive(Default)]
pub struct Collector {
    usage: ApiUsage,
    blocks: Vec<Block>,
    first_context: Option<usize>,
    pub audit: SessionAudit,
}

impl Collector {
    pub fn call(&mut self, context: usize, cached: usize, output: usize) {
        self.first_context.get_or_insert(context);
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
                last_seen: None,
            }),
        }
    }

    pub fn finish(mut self) -> Option<SessionAudit> {
        let calls = self.usage.calls;
        if calls == 0 {
            return None;
        }
        let before_first: usize = self.blocks.iter().filter(|b| b.from == 0).map(|b| b.tokens).sum();
        let fixed = self.first_context.unwrap_or(0).saturating_sub(before_first);
        let mut by: HashMap<String, Cost> = HashMap::new();
        if fixed > 0 {
            let source = FIXED_OVERHEAD.to_string();
            by.insert(
                source.clone(),
                Cost { source, origin: Origin::Harness, items: 1, tokens: fixed, resent: fixed * calls },
            );
        }
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

pub fn report(sessions: Vec<SessionAudit>, now: &Now) -> Report {
    let mut m = Merge::default();
    for s in sessions {
        m.session(s);
    }
    m.finish(now)
}

/// Sessions added up into one report.
#[derive(Default)]
struct Merge {
    r: Report,
    /// Newest session each source appeared in.
    seen: HashMap<String, std::time::SystemTime>,
    by: HashMap<String, Cost>,
    listed: BTreeSet<String>,
    used: BTreeSet<String>,
    mcp: BTreeSet<String>,
}

impl Merge {
    fn session(&mut self, s: SessionAudit) {
        let r = &mut self.r;
        if s.subagent {
            r.subagents += 1;
            r.subagent_context_sent += s.usage.context_sent;
        } else {
            r.sessions += 1;
        }
        r.calls += s.usage.calls;
        r.context_sent += s.usage.context_sent;
        r.cached += s.usage.cached;
        r.skills_tracked |= s.skills_tracked;
        for c in &s.costs {
            self.cost(c, s.seen);
        }
        for h in s.hook_errors {
            self.hook_error(h, s.seen);
        }
        self.listed.extend(s.skills_listed);
        self.used.extend(s.skills_invoked);
        self.mcp.extend(s.mcp_servers);
    }

    fn cost(&mut self, c: &Cost, seen: Option<std::time::SystemTime>) {
        if let Some(t) = seen {
            let e = self.seen.entry(c.source.clone()).or_insert(t);
            *e = (*e).max(t);
        }
        let e = self.by.entry(c.source.clone()).or_insert_with(|| Cost {
            source: c.source.clone(),
            origin: c.origin,
            ..Cost::default()
        });
        // Listings count entries, the same ones in every session; work
        // sources count occurrences, which add up.
        e.items = if c.source.starts_with("Tool results") || c.source.starts_with("Conversation") {
            e.items + c.items
        } else {
            e.items.max(c.items)
        };
        e.tokens += c.tokens;
        e.resent += c.resent;
    }

    fn hook_error(&mut self, mut h: HookError, seen: Option<std::time::SystemTime>) {
        h.last_seen = seen;
        match self.r.hook_errors.iter_mut().find(|e| e.command == h.command) {
            Some(e) => {
                e.count += h.count;
                e.last_seen = e.last_seen.max(h.last_seen);
            }
            None => self.r.hook_errors.push(h),
        }
    }

    /// Skills and MCP servers come from the newest session when there is
    /// one: it shows what today's config loads.
    fn finish(mut self, now: &Now) -> Report {
        let mut r = self.r;
        r.costs = self.by.into_values().collect();
        r.costs.sort_by_key(|c| std::cmp::Reverse(c.resent));
        r.skills_listed = self.listed.len();
        if let Some(n) = &now.newest {
            self.used.extend(n.skills_invoked.iter().cloned());
            self.listed.clone_from(&n.skills_listed);
            self.mcp.clone_from(&n.mcp_servers);
        }
        r.skills_unused = self.listed.into_iter().filter(|s| !self.used.contains(s)).collect();
        r.skills_used = self.used.into_iter().collect();
        r.mcp_servers = self.mcp.into_iter().collect();
        r.findings = findings::findings(&r, &self.seen, now);
        r
    }
}

impl Report {
    /// Context resent on account of sources from `origin`.
    pub fn spent_by(&self, origin: Origin) -> usize {
        self.costs.iter().filter(|c| c.origin == origin).map(|c| c.resent).sum()
    }

    /// Context no source accounts for: thinking, and estimate error.
    pub fn unexplained(&self) -> usize {
        self.context_sent.saturating_sub(self.costs.iter().map(|c| c.resent).sum())
    }

    /// `n` as a percentage of all context sent.
    pub fn percent(&self, n: usize) -> f64 {
        if self.context_sent == 0 { 0.0 } else { n as f64 * 100.0 / self.context_sent as f64 }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn report_splits_what_was_sent_by_origin() {
        let cost = |origin, resent| Cost { source: format!("{origin:?}"), origin, items: 1, tokens: 1, resent };
        let r = Report {
            context_sent: 1000,
            costs: vec![cost(Origin::Work, 500), cost(Origin::Config, 200), cost(Origin::Harness, 100)],
            ..Report::default()
        };
        assert_eq!((r.spent_by(Origin::Work), r.spent_by(Origin::Config)), (500, 200));
        assert_eq!(r.unexplained(), 200);
        assert!((r.percent(250) - 25.0).abs() < f64::EPSILON);
        assert!(Report::default().percent(5).abs() < f64::EPSILON);
    }

    #[test]
    fn first_call_beyond_known_blocks_is_fixed_overhead() {
        let mut col = Collector::default();
        col.add("prompt", Origin::Work, "some words here");
        let known = crate::helpers::est_tokens("some words here");
        col.call(1000, 0, 1);
        col.call(1100, 0, 1);
        let a = col.finish().unwrap();
        let fixed = a.costs.iter().find(|c| c.source == FIXED_OVERHEAD).unwrap();
        assert_eq!((fixed.tokens, fixed.resent), (1000 - known, (1000 - known) * 2));
    }
}
