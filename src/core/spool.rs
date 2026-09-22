//! Append-only event spool, one JSONL file per harness session. Hooks
//! only ever append here; everything else is derived later.

use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::SystemTime;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::core::paths::Paths;
use crate::helpers::{new_id, now_iso, write_atomic};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[expect(clippy::struct_field_names, reason = "field names are the on-disk JSONL format")]
pub struct Event {
    pub ts: String,
    pub session: String,
    pub event: String,
    /// Idempotency key: `tool_use_id` when available, so a replayed tool
    /// call converges; otherwise unique, since two prompts can share a
    /// second.
    pub key: String,
    #[serde(default)]
    pub data: Value,
}

impl Event {
    pub fn new(session: &str, event: &str, key: Option<&str>, data: Value) -> Self {
        // `new_id` is unique across processes; the counter covers two
        // events from one process within the same millisecond.
        static SEQ: AtomicUsize = AtomicUsize::new(0);
        let ts = now_iso();
        let key =
            key.map_or_else(|| format!("{}_{}", new_id(event), SEQ.fetch_add(1, Ordering::Relaxed)), str::to_string);
        Self { ts, session: session.to_string(), event: event.to_string(), key, data }
    }
}

/// Set by `relay claude|codex` on the harness it launches. Hooks inherit
/// it and stamp it on `session_start`, which is how the wrapper finds its
/// own session when others run in the same worktree.
pub const WRAPPER_ENV: &str = "RELAY_WRAPPER";

fn file_for(paths: &Paths, session: &str) -> PathBuf {
    paths.spool().join(format!("{}.jsonl", safe(session)))
}

fn safe(s: &str) -> String {
    s.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect()
}

pub fn append(paths: &Paths, ev: &Event) -> Result<()> {
    fs::create_dir_all(paths.spool())?;
    let mut f = fs::OpenOptions::new().create(true).append(true).open(file_for(paths, &ev.session))?;
    let mut line = serde_json::to_vec(ev)?;
    line.push(b'\n');
    f.write_all(&line)?;
    Ok(())
}

/// Read events for a session, dropping duplicate keys (replays converge).
pub fn read(paths: &Paths, session: &str) -> Vec<Event> {
    let Ok(text) = fs::read_to_string(file_for(paths, session)) else {
        return Vec::new();
    };
    let mut seen = std::collections::HashSet::new();
    text.lines().filter_map(|l| serde_json::from_str::<Event>(l).ok()).filter(|e| seen.insert(e.key.clone())).collect()
}

/// Sessions with a spool file, most recently modified first.
pub fn sessions(paths: &Paths) -> Vec<(String, SystemTime)> {
    let mut v = Vec::new();
    if let Ok(rd) = fs::read_dir(paths.spool()) {
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().and_then(|s| s.to_str()) != Some("jsonl") {
                continue;
            }
            let Some(stem) = p.file_stem().and_then(|s| s.to_str()) else { continue };
            let mtime = e.metadata().and_then(|m| m.modified()).unwrap_or(SystemTime::UNIX_EPOCH);
            v.push((stem.to_string(), mtime));
        }
    }
    v.sort_by(|a, b| b.1.cmp(&a.1));
    v
}

/// The session whose hooks fired most recently in this worktree. Written
/// by `SessionStart` and refreshed by tool events. Known limitation: two
/// live sessions in the same worktree share this pointer.
pub fn current_session(paths: &Paths) -> Option<String> {
    fs::read_to_string(paths.current_session_file()).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

pub fn set_current_session(paths: &Paths, session: &str) {
    let _ = write_atomic(&paths.current_session_file(), session.as_bytes());
}

/// The wrapper that launched this session, from its `session_start`
/// events (a resumed session has several; any of them counts).
pub fn wrappers(paths: &Paths, session: &str) -> Vec<String> {
    read(paths, session)
        .into_iter()
        .filter(|e| e.event == "session_start")
        .filter_map(|e| e.data["wrapper"].as_str().map(str::to_string))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn paths(name: &str) -> Paths {
        let root = std::env::temp_dir().join(format!("relay-ut-spool-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        Paths { shared: root.join(".relay"), local: root.join("local"), root, in_git: false }
    }

    #[test]
    fn keyless_events_in_the_same_second_are_all_kept() {
        let p = paths("keyless");
        for text in ["first ask", "second ask"] {
            append(&p, &Event::new("s", "prompt", None, json!({ "text": text }))).unwrap();
        }
        assert_eq!(read(&p, "s").len(), 2);
        let _ = fs::remove_dir_all(&p.root);
    }

    #[test]
    fn a_replayed_tool_call_converges() {
        let p = paths("replay");
        for _ in 0..2 {
            append(&p, &Event::new("s", "tool", Some("toolu_1"), json!({}))).unwrap();
        }
        assert_eq!(read(&p, "s").len(), 1);
        let _ = fs::remove_dir_all(&p.root);
    }
}
