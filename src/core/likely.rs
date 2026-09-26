//! The files a task will likely touch, before anyone opens them: the files
//! changed by commits whose message names the task, and the files edited
//! in recent sessions' turns whose prompt named it. From git history and the
//! spool; no model.

use std::collections::BTreeMap;

use crate::core::paths::Paths;
use crate::core::task_brief::mentions;
use crate::core::{spool, usage};
use crate::helpers::git as gitstate;
use crate::helpers::text::count;
use crate::limits::task_brief::{COMMITS_READ, FILES, SESSIONS_READ};

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Likely {
    pub file: String,
    /// Commits naming the task that changed it.
    pub commits: usize,
    /// Sessions that edited it in a turn asked about the task.
    pub sessions: usize,
}

impl std::fmt::Display for Likely {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut why = Vec::new();
        if self.commits > 0 {
            why.push(count(self.commits, "commit"));
        }
        if self.sessions > 0 {
            why.push(count(self.sessions, "session"));
        }
        write!(f, "{} · {}", self.file, why.join(", "))
    }
}

/// Most evidence first; only files that still exist.
pub fn files(paths: &Paths, query: &str) -> Vec<Likely> {
    let mut by: BTreeMap<String, Likely> = BTreeMap::new();
    let first = query.split_whitespace().next().unwrap_or(query);
    for (message, changed) in gitstate::commits_mentioning(&paths.root, first, COMMITS_READ) {
        if !mentions(&message, query) {
            continue;
        }
        for f in changed {
            by.entry(f.clone()).or_insert_with(|| Likely { file: f, ..Likely::default() }).commits += 1;
        }
    }
    for (f, n) in session_edits(paths, query) {
        by.entry(f.clone()).or_insert_with(|| Likely { file: f, ..Likely::default() }).sessions += n;
    }
    let mut v: Vec<Likely> = by.into_values().filter(|l| paths.root.join(&l.file).exists()).collect();
    v.sort_by(|a, b| (b.commits * 2 + b.sessions).cmp(&(a.commits * 2 + a.sessions)).then(a.file.cmp(&b.file)));
    v.truncate(FILES);
    v
}

/// Files edited in the turns whose prompt named the query, in recent
/// sessions, and in how many sessions each. A long session that names the
/// task once does not lend it everything else it edited.
fn session_edits(paths: &Paths, query: &str) -> BTreeMap<String, usize> {
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for (session, _) in spool::sessions(paths).into_iter().take(SESSIONS_READ) {
        let mut asked = false;
        let mut edited = std::collections::BTreeSet::new();
        for e in spool::read(paths, &session) {
            match e.event.as_str() {
                "prompt" => asked = e.data["text"].as_str().is_some_and(|t| mentions(t, query)),
                "tool" if asked && e.data["tool"].as_str().is_some_and(usage::is_edit) => {
                    let rel = e.data["file"].as_str().map(|f| paths.rel_file(f));
                    // Files outside this repo (another checkout, a temp dir) are not its files.
                    if let Some(rel) = rel.filter(|r| !std::path::Path::new(r).has_root() && !r.starts_with("..")) {
                        edited.insert(rel);
                    }
                }
                _ => {}
            }
        }
        for f in edited {
            *counts.entry(f).or_default() += 1;
        }
    }
    counts
}
