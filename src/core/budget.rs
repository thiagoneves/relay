//! A running total per agent, and a reminder when it gets expensive.
//! Subagents of 300k to 650k tokens each were only found afterwards; a
//! note when an agent passes each `limits::budget::WARN_EVERY` of tool
//! output lets it change course while it still can. When an agent ends,
//! its total joins a history keyed by the task ids its session was asked
//! about, so `relay usage --history` shows what each task cost over time.
//!
//! One small JSON per live session under `<local>/budget/`, updated in
//! place on each tool call, and one append-only `usage.jsonl`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::LazyLock;

use anyhow::Result;
use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::core::paths::Paths;
use crate::helpers::{human_tokens, now_iso, write_atomic};
use crate::limits::budget::WARN_EVERY;

#[derive(Debug, Default, Serialize, Deserialize)]
struct Board {
    /// Task ids the session's prompts named.
    #[serde(default)]
    tasks: BTreeSet<String>,
    #[serde(default)]
    agents: BTreeMap<String, Counter>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Counter {
    tokens: usize,
    /// How many `WARN_EVERY` steps the agent was told about.
    warned: usize,
    #[serde(default)]
    kind: String,
}

/// What an agent cost, as the history keeps it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Spent {
    pub ts: String,
    pub session: String,
    /// Empty for the main thread.
    pub agent: String,
    #[serde(default)]
    pub kind: String,
    /// Tool output it received, estimated.
    pub tokens: usize,
    pub tasks: Vec<String>,
}

static TASK_ID: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b[A-Z]{1,8}-\d{1,6}[a-z]?\b").expect("valid regex"));

fn file_for(paths: &Paths, session: &str) -> PathBuf {
    let safe: String =
        session.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect();
    paths.local.join("budget").join(format!("{safe}.json"))
}

fn history_file(paths: &Paths) -> PathBuf {
    paths.local.join("usage.jsonl")
}

fn load(paths: &Paths, session: &str) -> Board {
    std::fs::read(file_for(paths, session)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

fn save(paths: &Paths, session: &str, board: &Board) -> Result<()> {
    write_atomic(&file_for(paths, session), &serde_json::to_vec(board)?)
}

/// The session was asked `prompt`: the task ids in it are what its
/// agents' cost is filed under.
pub fn asked(paths: &Paths, session: &str, prompt: &str) -> Result<()> {
    let ids: Vec<String> = TASK_ID.find_iter(prompt).map(|m| m.as_str().to_string()).collect();
    if ids.is_empty() {
        return Ok(());
    }
    let mut b = load(paths, session);
    b.tasks.extend(ids);
    save(paths, session, &b)
}

/// Add `tokens` to `agent`'s total (empty for the main thread); the
/// reminder to show it when the total just passed another step.
pub fn add(paths: &Paths, session: &str, agent: &str, kind: Option<&str>, tokens: usize) -> Result<Option<String>> {
    if tokens == 0 {
        return Ok(None);
    }
    let mut b = load(paths, session);
    let c = b.agents.entry(agent.to_string()).or_default();
    c.tokens += tokens;
    if let Some(k) = kind.filter(|k| !k.is_empty()) {
        c.kind = k.to_string();
    }
    let step = c.tokens / WARN_EVERY;
    let note = (step > c.warned).then(|| {
        c.warned = step;
        let who = if agent.is_empty() { "This session" } else { "This subagent" };
        format!(
            "relay: {who} has taken in ~{} tokens of tool output (estimated). Before the next big read: \
             `relay brief <task id>` for one plan section, Read with offset and limit, Grep for a word; \
             hand back what you have if the task is done.",
            human_tokens(c.tokens)
        )
    });
    save(paths, session, &b)?;
    Ok(note)
}

/// `agent` ended (or, with `None`, the whole session): its total joins
/// the history.
pub fn close(paths: &Paths, session: &str, agent: Option<&str>) -> Result<()> {
    let mut b = load(paths, session);
    let ended: Vec<(String, Counter)> = match agent {
        Some(a) => b.agents.remove(a).map(|c| (a.to_string(), c)).into_iter().collect(),
        None => std::mem::take(&mut b.agents).into_iter().collect(),
    };
    let tasks: Vec<String> = b.tasks.iter().cloned().collect();
    let mut lines = String::new();
    for (agent, c) in ended.into_iter().filter(|(_, c)| c.tokens > 0) {
        let spent = Spent {
            ts: now_iso(),
            session: session.to_string(),
            agent,
            kind: c.kind,
            tokens: c.tokens,
            tasks: tasks.clone(),
        };
        lines.push_str(&serde_json::to_string(&spent)?);
        lines.push('\n');
    }
    if !lines.is_empty() {
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new().create(true).append(true).open(history_file(paths))?;
        f.write_all(lines.as_bytes())?;
    }
    if agent.is_none() {
        let _ = std::fs::remove_file(file_for(paths, session));
        Ok(())
    } else {
        save(paths, session, &b)
    }
}

/// Every agent that ended, oldest first.
pub fn history(paths: &Paths) -> Vec<Spent> {
    std::fs::read_to_string(history_file(paths))
        .unwrap_or_default()
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(name: &str) -> Paths {
        let root = std::env::temp_dir().join(format!("relay-ut-budget-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("local")).unwrap();
        Paths { shared: root.join(".relay"), local: root.join("local"), root, in_git: false, memory_local: false }
    }

    #[test]
    fn warns_once_per_step_and_files_the_cost_under_the_task() {
        let p = paths("steps");
        asked(&p, "s", "implement T-253 and check T-241").unwrap();
        assert_eq!(add(&p, "s", "a1", Some("Explore"), WARN_EVERY - 10).unwrap(), None);
        let note = add(&p, "s", "a1", None, 20).unwrap().unwrap();
        assert!(note.starts_with("relay: This subagent has taken in ~150.0k tokens"), "{note}");
        assert_eq!(add(&p, "s", "a1", None, 100).unwrap(), None, "once per step");
        assert!(add(&p, "s", "", None, 2 * WARN_EVERY).unwrap().unwrap().contains("This session has taken in ~300.0k"));

        close(&p, "s", Some("a1")).unwrap();
        close(&p, "s", None).unwrap();
        let h = history(&p);
        assert_eq!(h.len(), 2);
        assert_eq!((h[0].agent.as_str(), h[0].kind.as_str(), h[0].tokens), ("a1", "Explore", WARN_EVERY + 110));
        assert_eq!(h[1].tasks, ["T-241", "T-253"]);
        assert!(!file_for(&p, "s").exists(), "a closed session leaves no counter");
        let _ = std::fs::remove_dir_all(&p.root);
    }
}
