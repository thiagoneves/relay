//! The section index: each file's outline (Markdown headings or top-level
//! code blocks, with line ranges), its token estimate, and which sections
//! mention which ids (`T-253`, `ADR-12`). Built once per content and kept
//! under `<local>/index/`, one small file per indexed path: a file whose
//! size and mtime did not change is not read again, and one whose content
//! hash did not change is not parsed again. The read guard and
//! `relay brief <query>` answer from it.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use serde::{Deserialize, Serialize};

use crate::core::outline::{self, Entry};
use crate::core::paths::Paths;
use crate::helpers::est_tokens;
use crate::helpers::fs::write_state;
use crate::helpers::hash::content_hash;

/// Bump when what an index file holds changes shape or meaning.
const VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Indexed {
    version: u32,
    size: u64,
    mtime_ms: u64,
    pub hash: String,
    pub tokens: usize,
    pub entries: Vec<Entry>,
    /// An id, upper-cased, and the entries whose own lines mention it.
    pub ids: BTreeMap<String, Vec<usize>>,
}

fn file_for(paths: &Paths, rel: &str) -> PathBuf {
    paths.local.join("index").join(format!("{}.json", content_hash(rel.as_bytes())))
}

/// The index of `full` (named `rel` in the repo), from the cache when the
/// file is unchanged. `None` when it cannot be read as text.
pub fn get(paths: &Paths, rel: &str, full: &Path) -> Option<Indexed> {
    let meta = std::fs::metadata(full).ok().filter(std::fs::Metadata::is_file)?;
    let size = meta.len();
    let mtime_ms = meta.modified().ok()?.duration_since(UNIX_EPOCH).ok()?.as_millis().try_into().ok()?;
    let cache = file_for(paths, rel);
    let cached: Option<Indexed> = std::fs::read(&cache)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .filter(|c: &Indexed| c.version == VERSION);
    if let Some(c) = cached.as_ref().filter(|c| c.size == size && c.mtime_ms == mtime_ms) {
        return Some(c.clone());
    }
    let bytes = std::fs::read(full).ok()?;
    if bytes[..bytes.len().min(8192)].contains(&0) {
        return None;
    }
    let hash = content_hash(&bytes);
    let fresh = match cached {
        // Touched, not changed: only the stamp moves.
        Some(c) if c.hash == hash => Indexed { size, mtime_ms, ..c },
        _ => build(&String::from_utf8_lossy(&bytes), outline::is_markdown(rel), size, mtime_ms, hash),
    };
    let _ = write_state(&cache, &serde_json::to_vec(&fresh).ok()?);
    Some(fresh)
}

fn build(text: &str, markdown: bool, size: u64, mtime_ms: u64, hash: String) -> Indexed {
    let entries = outline::of(text, markdown);
    let ids = ids_by_entry(text, &entries);
    Indexed { version: VERSION, size, mtime_ms, hash, tokens: est_tokens(text), entries, ids }
}

/// Which entries mention each id: a line belongs to the deepest entry
/// around it.
fn ids_by_entry(text: &str, entries: &[Entry]) -> BTreeMap<String, Vec<usize>> {
    let re = regex::Regex::new(r"\b[A-Za-z]{1,8}-\d{1,6}[a-z]?\b").expect("valid regex");
    let mut ids: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (i, line) in text.lines().enumerate() {
        let n = i + 1;
        let Some(owner) = entries
            .iter()
            .enumerate()
            .filter(|(_, e)| e.start <= n && n <= e.end)
            .max_by_key(|(_, e)| e.depth)
            .map(|(k, _)| k)
        else {
            continue;
        };
        for m in re.find_iter(line) {
            let list = ids.entry(m.as_str().to_uppercase()).or_default();
            if list.last() != Some(&owner) {
                list.push(owner);
            }
        }
    }
    ids
}

/// Whether `query` is a single id the index keeps (`T-253`).
pub fn is_id(query: &str) -> bool {
    regex::Regex::new(r"^[A-Za-z]{1,8}-\d{1,6}[a-z]?$").expect("valid regex").is_match(query.trim())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(name: &str) -> Paths {
        let root = std::env::temp_dir().join(format!("relay-ut-index-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        Paths { shared: root.join(".relay"), local: root.join("local"), root, in_git: false, memory_local: false }
    }

    #[test]
    fn ids_belong_to_the_deepest_section_around_them() {
        let doc = "# Plan\nsee T-9\n## T-1 · Login\nneeds T-2 and t-2\n## T-2 · Pay\nstripe\n";
        let idx = build(doc, true, 0, 0, String::new());
        assert_eq!(idx.ids["T-9"], [0]);
        assert_eq!(idx.ids["T-2"], [1, 2]);
        assert!(is_id("T-253a") && is_id("adr-12") && !is_id("login flow"));
    }

    #[test]
    fn an_unchanged_file_is_not_parsed_again() {
        let p = paths("cache");
        let f = p.root.join("plan.md");
        std::fs::write(&f, "# A\n## T-1\nx\n").unwrap();
        let first = get(&p, "plan.md", &f).unwrap();
        assert_eq!(first.entries.len(), 2);
        // A stale cache with the same stamp is trusted: proof it was not re-read.
        let mut forged = first.clone();
        forged.entries.clear();
        write_state(&file_for(&p, "plan.md"), &serde_json::to_vec(&forged).unwrap()).unwrap();
        assert!(get(&p, "plan.md", &f).unwrap().entries.is_empty());
        std::fs::write(&f, "# A\n## T-1\nx\n## T-2\ny\n").unwrap();
        assert_eq!(get(&p, "plan.md", &f).unwrap().entries.len(), 3, "a changed file is indexed again");
        let _ = std::fs::remove_dir_all(&p.root);
    }
}
