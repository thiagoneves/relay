//! Re-reads. An agent reads the same file again and again: after an edit,
//! to check a line, or because a subagent does not know its parent read
//! it. Each time the whole file lands in the context again. relay keeps,
//! per session and per subagent, the content hash of every file range it
//! read; a re-read of an unchanged range becomes a one-line note, and one
//! after a change becomes the diff since that read.
//!
//! Under `<local>/reads/`: one small JSON per session, and the text of
//! each read keyed by its hash, so identical reads are stored once. The
//! cost is one hash of what the harness already returned, plus a
//! `git diff` when the range changed. A compaction forgets it all: the
//! earlier read may no longer be in the context.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::SystemTime;

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::core::paths::Paths;
use crate::helpers::git as gitstate;
use crate::helpers::hash::content_hash;
use crate::helpers::{ago, now_iso, parse_iso, write_atomic};
use crate::limits::reads::{DEDUP_MAX_BYTES, DIFF_CONTEXT, KEEP_BLOBS};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Seen {
    hash: String,
    at: String,
}

/// What to tell the agent about a read it just made.
#[derive(Debug, PartialEq, Eq)]
pub enum Verdict {
    /// First read of this range here, or nothing useful to say.
    Fresh,
    /// The same text it already has, read `ago`.
    Unchanged { ago: String },
    /// What changed since it read the range `ago`, as a unified diff.
    Changed { ago: String, diff: String },
}

/// One read: who made it, of what, and what came back.
pub struct Read<'a> {
    pub session: &'a str,
    /// The subagent; empty for the main thread.
    pub agent: &'a str,
    /// Repo-relative file.
    pub file: &'a str,
    /// First line of the range, 1-based, so diff hunks name file lines.
    pub start: usize,
    /// `offset:limit` as asked, empty for the whole file.
    pub range: &'a str,
    pub content: &'a str,
}

fn dir(paths: &Paths) -> PathBuf {
    paths.local.join("reads")
}

fn blob(paths: &Paths, hash: &str) -> PathBuf {
    dir(paths).join("blobs").join(format!("{hash}.txt"))
}

fn file_for(paths: &Paths, session: &str) -> PathBuf {
    let safe: String =
        session.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect();
    dir(paths).join(format!("{safe}.json"))
}

fn load(paths: &Paths, session: &str) -> BTreeMap<String, Seen> {
    std::fs::read_to_string(file_for(paths, session))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

/// Remember `r` and say how it relates to the last read of the same range
/// by the same agent.
pub fn observe(paths: &Paths, r: &Read) -> Result<Verdict> {
    if r.content.len() > DEDUP_MAX_BYTES || r.content.trim().is_empty() {
        return Ok(Verdict::Fresh);
    }
    let key = format!("{}|{}|{}", r.agent, r.file, r.range);
    let hash = content_hash(r.content.as_bytes());
    let mut seen = load(paths, r.session);
    let before = seen.insert(key, Seen { hash: hash.clone(), at: now_iso() });
    let new_blob = blob(paths, &hash);
    if !new_blob.exists() {
        write_atomic(&new_blob, r.content.as_bytes())?;
    }
    write_atomic(&file_for(paths, r.session), &serde_json::to_vec(&seen)?)?;
    let Some(before) = before else { return Ok(Verdict::Fresh) };
    let when = parse_iso(&before.at).map_or_else(String::new, |t| ago(t, SystemTime::now()));
    if before.hash == hash {
        return Ok(Verdict::Unchanged { ago: when });
    }
    let old = blob(paths, &before.hash);
    let diff = gitstate::diff_files(&old, &new_blob, DIFF_CONTEXT).map(|d| shift_hunks(&d, r.start.saturating_sub(1)));
    Ok(match diff {
        // A diff about as long as the range tells the agent nothing new.
        Some(d) if !d.is_empty() && d.len() * 2 < r.content.len() => Verdict::Changed { ago: when, diff: d },
        _ => Verdict::Fresh,
    })
}

/// Hunk headers count from the range; move them to file lines.
fn shift_hunks(diff: &str, by: usize) -> String {
    if by == 0 {
        return diff.to_string();
    }
    let re = regex::Regex::new(r"^@@ -(\d+)(,\d+)? \+(\d+)(,\d+)? @@").expect("valid regex");
    diff.lines()
        .map(|l| {
            re.captures(l).map_or_else(
                || l.to_string(),
                |c| {
                    let n = |i: usize| c[i].parse::<usize>().unwrap_or(0) + by;
                    let opt = |i: usize| c.get(i).map_or("", |m| m.as_str());
                    format!("@@ -{}{} +{}{} @@{}", n(1), opt(2), n(3), opt(4), &l[c[0].len()..])
                },
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The session's context was compacted or restarted: an earlier read may
/// be gone from it, so none counts any more.
pub fn forget(paths: &Paths, session: &str) {
    let _ = std::fs::remove_file(file_for(paths, session));
}

/// Stored read texts nobody has touched in `KEEP_BLOBS`.
pub fn prune(paths: &Paths) {
    let Ok(rd) = std::fs::read_dir(dir(paths).join("blobs")) else { return };
    let cutoff = SystemTime::now().checked_sub(KEEP_BLOBS).unwrap_or(SystemTime::UNIX_EPOCH);
    for e in rd.flatten() {
        if e.metadata().and_then(|m| m.modified()).is_ok_and(|t| t < cutoff) {
            let _ = std::fs::remove_file(e.path());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(name: &str) -> Paths {
        let root = std::env::temp_dir().join(format!("relay-ut-reads-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        Paths { shared: root.join(".relay"), local: root.join("local"), root, in_git: false, memory_local: false }
    }

    fn read<'a>(agent: &'a str, content: &'a str) -> Read<'a> {
        Read { session: "s", agent, file: "src/a.rs", start: 1, range: "", content }
    }

    #[test]
    fn a_reread_is_unchanged_then_a_diff() {
        let p = paths("reread");
        let v1 = (1..=60).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n") + "\n";
        assert_eq!(observe(&p, &read("", &v1)).unwrap(), Verdict::Fresh);
        assert_eq!(observe(&p, &read("", &v1)).unwrap(), Verdict::Unchanged { ago: "just now".into() });
        assert_eq!(observe(&p, &read("sub-1", &v1)).unwrap(), Verdict::Fresh, "a subagent has its own context");
        let v2 = v1.replace("line 30\n", "line thirty\n");
        let Verdict::Changed { diff, .. } = observe(&p, &read("", &v2)).unwrap() else { panic!("expected a diff") };
        assert!(diff.starts_with("@@ -27,7 +27,7 @@") && diff.contains("-line 30\n+line thirty"), "{diff}");
        forget(&p, "s");
        assert_eq!(observe(&p, &read("", &v2)).unwrap(), Verdict::Fresh, "after a compaction it counts as new");
        let _ = std::fs::remove_dir_all(&p.root);
    }

    #[test]
    fn hunks_of_a_range_name_file_lines() {
        assert_eq!(shift_hunks("@@ -1,3 +1,4 @@ fn a\n-x\n+y", 99), "@@ -100,3 +100,4 @@ fn a\n-x\n+y");
        assert_eq!(shift_hunks("@@ -5 +5 @@\n-x", 10), "@@ -15 +15 @@\n-x");
    }
}
