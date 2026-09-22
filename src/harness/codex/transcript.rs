//! Codex rollouts (`~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl`), read
//! only: which ones belong to a project, and which are subagents.

use std::io::{BufRead, BufReader};
use std::path::Path;

use serde_json::Value;

use crate::core::audit::Transcript;

/// Rollouts under `sessions/`, filtered by the `cwd` in their first line
/// (`session_meta`). Subagent rollouts name their parent thread.
pub fn rollouts(dir: &Path, root: Option<&Path>) -> Vec<Transcript> {
    let mut files = Vec::new();
    collect(dir, &mut files);
    files
        .into_iter()
        .filter_map(|path| {
            let first = BufReader::new(std::fs::File::open(&path).ok()?).lines().next()?.ok()?;
            let meta: Value = serde_json::from_str(&first).ok()?;
            let p = &meta["payload"];
            if let Some(r) = root
                && p["cwd"].as_str().map(Path::new) != Some(r)
            {
                return None;
            }
            let id = p["id"].as_str().unwrap_or_default().to_string();
            let parent = p["parent_thread_id"].as_str().filter(|s| !s.is_empty()).map(str::to_string);
            Transcript::from_file(path, id, parent)
        })
        .collect()
}

fn collect(dir: &Path, into: &mut Vec<std::path::PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            collect(&p, into);
        } else if p.extension().is_some_and(|x| x == "jsonl") {
            into.push(p);
        }
    }
}
