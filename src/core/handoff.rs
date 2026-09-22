//! Rule-based session handoff. No LLM. Built from the spool, the output
//! store and git. One immutable file per session under the local tier;
//! rebuilt in place while the same session is still running.

use std::path::PathBuf;

use anyhow::Result;

use crate::core::outputs;
use crate::core::paths::Paths;
use crate::core::spool::{self, Event};
use crate::helpers::git as gitstate;
use crate::helpers::{now_iso, truncate_chars, write_atomic};

pub struct Handoff {
    pub path: PathBuf,
    pub body: String,
}

pub fn path_for(paths: &Paths, session: &str) -> PathBuf {
    paths.handoffs().join(format!("{session}.md"))
}

pub fn build(paths: &Paths, session: &str, reason: &str) -> Result<Handoff> {
    outputs::absorb_spill(paths);
    let events = spool::read(paths, session);
    let summary = Summary::collect(paths, &events, &outputs::for_session(paths, session));
    let git = gitstate::state(&paths.root);
    let body = render(session, reason, &summary, &git);

    let path = path_for(paths, session);
    write_atomic(&path, body.as_bytes())?;
    let _ = write_atomic(&paths.local.join("last_session"), session.as_bytes());
    Ok(Handoff { path, body })
}

struct Summary {
    started: String,
    harness: String,
    prompts: Vec<String>,
    last_reply: Option<String>,
    files: Vec<(String, usize)>,
    remembered: Vec<String>,
    commands: Vec<String>,
    failing: Vec<String>,
}

const MAX_COMMANDS: usize = 12;
const MAX_FAILING: usize = 5;

impl Summary {
    fn collect(paths: &Paths, events: &[Event], outs: &[outputs::OutputMeta]) -> Self {
        let started = events.first().map_or_else(now_iso, |e| e.ts.clone());
        let harness = events
            .iter()
            .find(|e| e.event == "session_start")
            .and_then(|e| e.data["harness"].as_str())
            .unwrap_or("unknown")
            .to_string();
        let prompts = events
            .iter()
            .filter(|e| e.event == "prompt")
            .filter_map(|e| e.data["text"].as_str().map(|s| s.replace('\n', " ")))
            .filter(|s| !s.trim().is_empty() && !s.starts_with('/'))
            .collect();
        let last_reply = events
            .iter()
            .rev()
            .find(|e| e.event == "stop")
            .and_then(|e| e.data["last"].as_str())
            .map(|s| s.replace('\n', " "));
        let remembered = events
            .iter()
            .filter(|e| e.event == "remember")
            .filter_map(|e| Some(format!("{}: {}", e.data["kind"].as_str()?, e.data["path"].as_str()?)))
            .collect();
        let (commands, failing) =
            if outs.is_empty() { (commands_from_events(events), Vec::new()) } else { commands_from_outputs(outs) };
        Self {
            started,
            harness,
            prompts,
            last_reply,
            files: files_touched(paths, events),
            remembered,
            commands,
            failing,
        }
    }
}

fn files_touched(paths: &Paths, events: &[Event]) -> Vec<(String, usize)> {
    let prefix = format!("{}/", paths.root.display());
    let mut files: Vec<(String, usize)> = Vec::new();
    for f in events.iter().filter(|e| e.event == "tool").filter_map(|e| e.data["file"].as_str()) {
        let rel = f.strip_prefix(&prefix).unwrap_or(f);
        match files.iter_mut().find(|(p, _)| p == rel) {
            Some(x) => x.1 += 1,
            None => files.push((rel.to_string(), 1)),
        }
    }
    files.sort_by(|a, b| b.1.cmp(&a.1));
    files
}

/// Latest run of each distinct command (first three words), with exit
/// status and the id of its original.
fn commands_from_outputs(outs: &[outputs::OutputMeta]) -> (Vec<String>, Vec<String>) {
    let mut seen: Vec<String> = Vec::new();
    let mut commands = Vec::new();
    let mut failing = Vec::new();
    for m in outs.iter().rev() {
        let key = m.cmd.split_whitespace().take(3).collect::<Vec<_>>().join(" ");
        if seen.contains(&key) {
            continue;
        }
        seen.push(key);
        let cmd = truncate_chars(&m.cmd, 80);
        let status = if m.exit == 0 { "ok".to_string() } else { format!("exit {}", m.exit) };
        commands.push(format!("`{cmd}` → {status} · relay get {}", m.id));
        if m.exit != 0 && failing.len() < MAX_FAILING {
            failing.push(format!("`{cmd}` (exit {}) · relay get {}", m.exit, m.id));
        }
        if commands.len() >= MAX_COMMANDS {
            break;
        }
    }
    (commands, failing)
}

/// Without `relay x` there are no exit codes, only what hooks saw.
fn commands_from_events(events: &[Event]) -> Vec<String> {
    events
        .iter()
        .rev()
        .filter(|e| e.event == "tool" && e.data["tool"] == "Bash")
        .filter_map(|e| e.data["command"].as_str())
        .map(|c| format!("`{}`", truncate_chars(c, 80)))
        .take(MAX_COMMANDS)
        .collect()
}

fn render(session: &str, reason: &str, s: &Summary, git: &gitstate::GitState) -> String {
    let ended = now_iso();
    let mut b = String::new();
    b.push_str("---\n");
    b.push_str(&format!(
        "session: {session}\nharness: {}\nbranch: {}\nsha: {}\ndirty: {}\nstarted: {}\nended: {ended}\nreason: {reason}\n",
        s.harness,
        git.branch,
        git.sha,
        git.dirty.len(),
        s.started
    ));
    b.push_str("---\n\n");
    let title_branch = if git.branch.is_empty() { String::new() } else { format!("{} · ", git.branch) };
    b.push_str(&format!("# Handoff · {title_branch}{}\n\n", &ended[..10.min(ended.len())]));

    let skip = s.prompts.len().saturating_sub(6);
    section(&mut b, "Asked", s.prompts.iter().skip(skip).map(|p| truncate_chars(p, 220)));
    if let Some(last) = &s.last_reply {
        b.push_str(&format!("## Last reply\n{}\n\n", truncate_chars(last, 400)));
    }
    section(
        &mut b,
        "Files touched",
        s.files.iter().take(15).map(|(f, n)| if *n > 1 { format!("{f} (×{n})") } else { f.clone() }),
    );
    section(&mut b, "Remembered", s.remembered.iter().cloned());
    section(&mut b, "Failing at end", s.failing.iter().cloned());
    section(&mut b, "Commands", s.commands.iter().cloned());
    render_git(&mut b, git);
    b
}

fn section(b: &mut String, title: &str, items: impl Iterator<Item = String>) {
    let mut items = items.peekable();
    if items.peek().is_none() {
        return;
    }
    b.push_str(&format!("## {title}\n"));
    for it in items {
        b.push_str(&format!("- {it}\n"));
    }
    b.push('\n');
}

fn render_git(b: &mut String, git: &gitstate::GitState) {
    let branch = if git.branch.is_empty() { "(no branch)" } else { &git.branch };
    let sha = if git.sha.is_empty() { "(no commits)" } else { &git.sha };
    b.push_str(&format!("## Git\n{branch} @ {sha}"));
    if git.dirty.is_empty() {
        b.push_str(", clean\n");
        return;
    }
    b.push_str(&format!(", {} dirty:\n", git.dirty.len()));
    for d in git.dirty.iter().take(10) {
        b.push_str(&format!("- {d}\n"));
    }
    if git.dirty.len() > 10 {
        b.push_str(&format!("- … +{}\n", git.dirty.len() - 10));
    }
}

/// Most recent handoff, preferring the current branch.
pub fn latest(paths: &Paths, branch: &str) -> Option<(PathBuf, String)> {
    let mut all: Vec<(std::time::SystemTime, PathBuf, String)> = Vec::new();
    for e in std::fs::read_dir(paths.handoffs()).ok()?.flatten() {
        let p = e.path();
        if p.extension().and_then(|s| s.to_str()) != Some("md") {
            continue;
        }
        let Ok(body) = std::fs::read_to_string(&p) else { continue };
        let m = e.metadata().and_then(|m| m.modified()).unwrap_or(std::time::SystemTime::UNIX_EPOCH);
        all.push((m, p, body));
    }
    all.sort_by(|a, b| b.0.cmp(&a.0));
    let same_branch = all.iter().find(|(_, _, body)| frontmatter(body, "branch").as_deref() == Some(branch));
    same_branch.or(all.first()).map(|(_, p, b)| (p.clone(), b.clone()))
}

pub fn frontmatter(body: &str, key: &str) -> Option<String> {
    let mut lines = body.lines();
    if lines.next()? != "---" {
        return None;
    }
    for l in lines {
        if l == "---" {
            break;
        }
        if let Some(v) = l.strip_prefix(&format!("{key}: ")) {
            return Some(v.trim().to_string());
        }
    }
    None
}

pub fn strip_frontmatter(body: &str) -> &str {
    if !body.starts_with("---\n") {
        return body;
    }
    match body[4..].find("\n---\n") {
        Some(i) => body[4 + i + 5..].trim_start(),
        None => body,
    }
}
