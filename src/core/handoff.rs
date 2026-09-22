//! Rule-based session handoff. No LLM. Built from the spool, the output
//! store and git. One immutable file per session under the local tier;
//! rebuilt in place while the same session is still running.

use std::path::PathBuf;

use anyhow::Result;

use crate::helpers::git as gitstate;
use crate::helpers::{now_iso, truncate_chars, write_atomic};
use crate::core::outputs;
use crate::core::paths::Paths;
use crate::core::spool;

pub struct Handoff {
    pub path: PathBuf,
    pub body: String,
}

pub fn path_for(paths: &Paths, session: &str) -> PathBuf {
    paths.handoffs().join(format!("{session}.md"))
}

pub fn build(paths: &Paths, session: &str, reason: &str) -> Result<Handoff> {
    let events = spool::read(paths, session);
    let outs = outputs::for_session(paths, session);
    let git = gitstate::state(&paths.root);

    let started = events.first().map(|e| e.ts.clone()).unwrap_or_else(now_iso);
    let ended = now_iso();
    let harness = events
        .iter()
        .find(|e| e.event == "session_start")
        .and_then(|e| e.data["harness"].as_str())
        .unwrap_or("unknown")
        .to_string();

    // Prompts: what the user asked, most recent last.
    let prompts: Vec<String> = events
        .iter()
        .filter(|e| e.event == "prompt")
        .filter_map(|e| e.data["text"].as_str().map(|s| s.replace('\n', " ")))
        .filter(|s| !s.trim().is_empty() && !s.starts_with('/'))
        .collect();

    // Files touched via edit tools, counted.
    let mut files: Vec<(String, usize)> = Vec::new();
    for e in events.iter().filter(|e| e.event == "tool") {
        if let Some(f) = e.data["file"].as_str() {
            let rel = f.strip_prefix(&format!("{}/", paths.root.display())).unwrap_or(f).to_string();
            match files.iter_mut().find(|(p, _)| *p == rel) {
                Some(x) => x.1 += 1,
                None => files.push((rel, 1)),
            }
        }
    }

    // Commands: from the output store when we have it (exit codes,
    // ids), else from PostToolUse records.
    let mut commands: Vec<String> = Vec::new();
    let mut failing: Vec<String> = Vec::new();
    if !outs.is_empty() {
        let mut seen: Vec<String> = Vec::new();
        for m in outs.iter().rev() {
            let key = m.cmd.split_whitespace().take(3).collect::<Vec<_>>().join(" ");
            if seen.contains(&key) {
                continue;
            }
            seen.push(key);
            let status = if m.exit == 0 { "ok".to_string() } else { format!("exit {}", m.exit) };
            commands.push(format!("`{}` → {} · relay get {}", truncate_chars(&m.cmd, 80), status, m.id));
            if m.exit != 0 && failing.len() < 5 {
                failing.push(format!("`{}` (exit {}) · relay get {}", truncate_chars(&m.cmd, 80), m.exit, m.id));
            }
            if commands.len() >= 12 {
                break;
            }
        }
    } else {
        for e in events.iter().rev().filter(|e| e.event == "tool" && e.data["tool"] == "Bash") {
            if let Some(c) = e.data["command"].as_str() {
                commands.push(format!("`{}`", truncate_chars(c, 80)));
            }
            if commands.len() >= 12 {
                break;
            }
        }
    }

    let last_assistant = events
        .iter()
        .rev()
        .find(|e| e.event == "stop")
        .and_then(|e| e.data["last"].as_str())
        .map(|s| s.replace('\n', " "));

    let mut b = String::new();
    b.push_str("---\n");
    b.push_str(&format!("session: {session}\nharness: {harness}\nbranch: {}\nsha: {}\ndirty: {}\nstarted: {started}\nended: {ended}\nreason: {reason}\n", git.branch, git.sha, git.dirty.len()));
    b.push_str("---\n\n");
    let title_branch = if git.branch.is_empty() { String::new() } else { format!("{} · ", git.branch) };
    b.push_str(&format!("# Handoff · {}{}\n\n", title_branch, &ended[..10.min(ended.len())]));

    if !prompts.is_empty() {
        b.push_str("## Asked\n");
        let skip = prompts.len().saturating_sub(6);
        for p in prompts.iter().skip(skip) {
            b.push_str(&format!("- {}\n", truncate_chars(p, 220)));
        }
        b.push('\n');
    }
    if let Some(last) = last_assistant {
        b.push_str("## Last reply\n");
        b.push_str(&format!("{}\n\n", truncate_chars(&last, 400)));
    }
    if !files.is_empty() {
        b.push_str("## Files touched\n");
        files.sort_by(|a, b| b.1.cmp(&a.1));
        for (f, n) in files.iter().take(15) {
            b.push_str(&format!("- {f}{}\n", if *n > 1 { format!(" (×{n})") } else { String::new() }));
        }
        b.push('\n');
    }
    if !failing.is_empty() {
        b.push_str("## Failing at end\n");
        for f in &failing {
            b.push_str(&format!("- {f}\n"));
        }
        b.push('\n');
    }
    if !commands.is_empty() {
        b.push_str("## Commands\n");
        for c in &commands {
            b.push_str(&format!("- {c}\n"));
        }
        b.push('\n');
    }
    b.push_str("## Git\n");
    b.push_str(&format!("{} @ {}", if git.branch.is_empty() { "(no branch)" } else { &git.branch }, if git.sha.is_empty() { "(no commits)" } else { &git.sha }));
    if git.dirty.is_empty() {
        b.push_str(", clean\n");
    } else {
        b.push_str(&format!(", {} dirty:\n", git.dirty.len()));
        for d in git.dirty.iter().take(10) {
            b.push_str(&format!("- {d}\n"));
        }
        if git.dirty.len() > 10 {
            b.push_str(&format!("- … +{}\n", git.dirty.len() - 10));
        }
    }

    let path = path_for(paths, session);
    write_atomic(&path, b.as_bytes())?;
    let _ = write_atomic(&paths.local.join("last_session"), session.as_bytes());
    Ok(Handoff { path, body: b })
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

/// Body without the frontmatter block.
pub fn strip_frontmatter(body: &str) -> &str {
    if !body.starts_with("---\n") {
        return body;
    }
    match body[4..].find("\n---\n") {
        Some(i) => body[4 + i + 5..].trim_start(),
        None => body,
    }
}

