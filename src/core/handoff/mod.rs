//! Rule-based session handoff: what one session leaves for the next,
//! built from the spool, the output store, git and the harness's own
//! transcript tail. No LLM. One file per session in the local tier,
//! rebuilt in place while that session is still running.

mod render;
mod summary;

use std::path::PathBuf;
use std::time::SystemTime;

use anyhow::Result;

use crate::core::outputs;
use crate::core::paths::Paths;
use crate::core::spool;
use crate::helpers::git as gitstate;
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
    spool::set_last_session(paths, session);
    Ok(Handoff { path, body })
}

/// Most recent handoff, preferring the current branch.
pub fn latest(paths: &Paths, branch: &str) -> Option<(PathBuf, String)> {
    let mut all = stored(paths);
    all.sort_by(|a, b| b.0.cmp(&a.0));
    let same_branch = all.iter().find(|(_, _, body)| frontmatter::get(body, "branch").as_deref() == Some(branch));
    same_branch.or(all.first()).map(|(_, p, b)| (p.clone(), b.clone()))
}

pub fn count(paths: &Paths) -> usize {
    std::fs::read_dir(paths.handoffs()).map(Iterator::count).unwrap_or(0)
}

fn stored(paths: &Paths) -> Vec<(SystemTime, PathBuf, String)> {
    let Ok(rd) = std::fs::read_dir(paths.handoffs()) else { return Vec::new() };
    rd.flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("md"))
        .filter_map(|p| {
            let body = std::fs::read_to_string(&p).ok()?;
            let modified = p.metadata().and_then(|m| m.modified()).unwrap_or(SystemTime::UNIX_EPOCH);
            Some((modified, p, body))
        })
        .collect()
}
