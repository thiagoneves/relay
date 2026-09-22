//! Rule-based session handoff: what one session leaves for the next,
//! built from the spool, the output store, git and the harness's own
//! transcript tail. No LLM. One file per session in the local tier,
//! rebuilt in place while that session is still running.

mod render;
mod summary;

use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use anyhow::{Context, Result};

use crate::core::outputs;
use crate::core::paths::Paths;
use crate::core::spool;
use crate::helpers::git as gitstate;
use crate::helpers::redact::redact;
use crate::helpers::{frontmatter, now_iso, write_atomic};
use render::Header;
use summary::Summary;

pub struct Handoff {
    pub path: PathBuf,
    pub body: String,
}

/// What only the harness transcript knows: how each turn ended, what the
/// user answered when the agent asked, the plan they approved.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Tail {
    /// The agent's closing message of each turn, oldest first.
    pub replies: Vec<String>,
    /// `topic: answer`, one per question the user answered.
    pub decisions: Vec<String>,
    pub plan: Option<String>,
    /// Run with no one at the keyboard (`claude -p`, an SDK app): a script
    /// or a test, not work the next session should pick up.
    pub headless: bool,
}

pub fn path_for(paths: &Paths, session: &str) -> PathBuf {
    paths.handoffs().join(format!("{session}.md"))
}

pub fn build(paths: &Paths, session: &str, reason: &str, tail: Option<&Tail>) -> Result<Handoff> {
    outputs::absorb_spill(paths);
    let events = spool::read(paths, session);
    let now = now_iso();
    let summary = Summary::collect(&events, &outputs::for_session(paths, session), |f| paths.rel_file(f), &now);
    let git = gitstate::state(&paths.root);
    let header = Header { session, reason, ended: &now };
    let body = render::render(&header, &summary, tail.unwrap_or(&Tail::default()), &git);

    let path = path_for(paths, session);
    write_atomic(&path, body.as_bytes())?;
    if !tail.is_some_and(|t| t.headless) {
        spool::set_last_session(paths, session);
    }
    Ok(Handoff { path, body })
}

/// Copy a session's handoff into the shared tier, credentials masked, so
/// whoever clones the repo gets it. Sharing is a decision, never a default.
pub fn share(paths: &Paths, session: &str) -> Result<PathBuf> {
    let body = std::fs::read_to_string(path_for(paths, session))
        .with_context(|| format!("no handoff for session {session}; `relay handoff --session {session}` builds one"))?;
    let shared = paths.shared.join("handoffs").join(format!("{session}.md"));
    write_atomic(&shared, redact(&body).as_bytes())?;
    Ok(shared)
}

/// Most recent handoff, preferring an interactive session over a headless
/// one, then the current branch over others.
pub fn latest(paths: &Paths, branch: &str) -> Option<(PathBuf, String)> {
    let mut all = stored(paths);
    all.sort_by(|a, b| b.0.cmp(&a.0));
    let bodies: Vec<&str> = all.iter().map(|(_, _, b)| b.as_str()).collect();
    pick(&bodies, branch).map(|i| (all[i].1.clone(), all[i].2.clone()))
}

/// The newest `max` handoffs, interactive sessions only.
pub fn recent(paths: &Paths, max: usize) -> Vec<(PathBuf, String)> {
    let mut all = stored(paths);
    all.sort_by(|a, b| b.0.cmp(&a.0));
    all.into_iter()
        .filter(|(_, _, body)| frontmatter::get(body, "headless").is_none())
        .take(max)
        .map(|(_, p, body)| (p, body))
        .collect()
}

/// One line about a session: when it ended, where it ran, and where it
/// stopped.
pub struct Glance {
    pub ended: String,
    pub harness: String,
    pub branch: String,
    pub stopped: String,
}

/// Sessions other than `shown` that ended in the last `days`, newest
/// first, at most `max`. Headless runs are skipped: they are scripts.
pub fn others(paths: &Paths, shown: &Path, days: u64, max: usize) -> Vec<Glance> {
    let since = crate::helpers::iso(std::time::SystemTime::now() - std::time::Duration::from_secs(days * 86_400));
    let mut all = stored(paths);
    all.sort_by(|a, b| b.0.cmp(&a.0));
    all.iter()
        .filter(|(ended, p, body)| p != shown && *ended >= since && frontmatter::get(body, "headless").is_none())
        .take(max)
        .map(|(ended, _, body)| Glance {
            ended: ended[..10.min(ended.len())].to_string(),
            harness: frontmatter::get(body, "harness").unwrap_or_default(),
            branch: frontmatter::get(body, "branch").unwrap_or_default(),
            stopped: stopped_line(body),
        })
        .collect()
}

/// The first line under "Where it stopped" (or "Last reply").
fn stopped_line(body: &str) -> String {
    let mut in_section = false;
    for l in body.lines() {
        if l.starts_with("## ") {
            in_section = matches!(l, "## Where it stopped" | "## Last reply");
            continue;
        }
        if in_section && !l.trim().is_empty() {
            return l.trim_start_matches(['*', '#', ' ']).trim().to_string();
        }
    }
    String::new()
}

/// Index of the handoff to show among `bodies`, newest first.
fn pick(bodies: &[&str], branch: &str) -> Option<usize> {
    let interactive = |b: &&str| frontmatter::get(b, "headless").as_deref() != Some("true");
    let on_branch = |b: &&str| frontmatter::get(b, "branch").as_deref() == Some(branch);
    let first = |f: &dyn Fn(&&str) -> bool| bodies.iter().position(f);
    first(&|b| interactive(b) && on_branch(b))
        .or_else(|| first(&interactive))
        .or_else(|| first(&on_branch))
        .or_else(|| (!bodies.is_empty()).then_some(0))
}

pub fn count(paths: &Paths) -> usize {
    std::fs::read_dir(paths.handoffs()).map_or(0, Iterator::count)
}

/// Local handoffs plus the shared ones, keyed by when each session ended
/// (the frontmatter, not the file: a clone rewrites mtimes) with the
/// file's mtime breaking ties within a second. A session present in both
/// tiers counts once, from the local copy.
fn stored(paths: &Paths) -> Vec<(String, PathBuf, String)> {
    let mut seen = std::collections::HashSet::new();
    let mut all = Vec::new();
    for dir in [paths.handoffs(), paths.shared.join("handoffs")] {
        let Ok(rd) = std::fs::read_dir(dir) else { continue };
        for p in rd.flatten().map(|e| e.path()) {
            if p.extension().and_then(|s| s.to_str()) != Some("md") {
                continue;
            }
            let Some(name) = p.file_name().map(std::ffi::OsStr::to_os_string) else { continue };
            if !seen.insert(name) {
                continue;
            }
            if let Ok(body) = std::fs::read_to_string(&p) {
                let mut ended = frontmatter::get(&body, "ended").unwrap_or_default();
                let nanos =
                    p.metadata().and_then(|m| m.modified()).ok().and_then(|m| m.duration_since(UNIX_EPOCH).ok());
                ended.push_str(&format!(" {:020}", nanos.map_or(0, |d| d.as_nanos())));
                all.push((ended, p, body));
            }
        }
    }
    all
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_stopped_line_is_the_first_of_its_section() {
        assert_eq!(
            stopped_line("# H\n## Asked\n- x\n## Where it stopped\n\n**Done.** Tests pass.\nmore\n"),
            "Done.** Tests pass."
        );
        assert_eq!(stopped_line("## Last reply\nAlmost there\n"), "Almost there");
        assert_eq!(stopped_line("## Asked\n- x\n"), "");
    }

    #[test]
    fn a_headless_run_does_not_hide_the_last_real_session() {
        let script = "---\nbranch: main\nheadless: true\n---\n";
        let work = "---\nbranch: main\n---\n";
        let other_branch = "---\nbranch: feat\n---\n";
        assert_eq!(pick(&[script, work], "main"), Some(1));
        assert_eq!(pick(&[script, other_branch], "main"), Some(1));
        assert_eq!(pick(&[script], "main"), Some(0));
        assert_eq!(pick(&[], "main"), None);
    }
}
