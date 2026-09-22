//! Codex rollouts (`~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl`), read
//! only: which ones belong to a project, which are subagents, and how each
//! turn ended.

use std::path::Path;

use serde_json::Value;

use crate::core::audit::Transcript;
use crate::core::handoff::Tail;
use crate::harness::jsonl::{self, MAX_REPLIES, TAIL_BYTES};

/// Rollouts under `sessions/`, filtered by the `cwd` in their first line
/// (`session_meta`). Subagent rollouts name their parent thread.
pub fn rollouts(dir: &Path, root: Option<&Path>) -> Vec<Transcript> {
    jsonl::files_under(dir)
        .into_iter()
        .filter_map(|path| {
            let meta: Value = serde_json::from_str(&jsonl::first_line(&path)?).ok()?;
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

/// Codex closes every turn with a `task_complete` event that carries the
/// agent's last message.
pub fn tail(path: &Path) -> Option<Tail> {
    let replies = jsonl::tail_lines(path, TAIL_BYTES)?
        .filter(|l| l.contains("\"task_complete\""))
        .filter_map(|l| serde_json::from_str::<Value>(&l).ok())
        .filter_map(|v| v["payload"]["last_agent_message"].as_str().map(str::to_string))
        .filter(|m| !m.trim().is_empty())
        .collect();
    Some(Tail { replies: jsonl::last(replies, MAX_REPLIES), ..Tail::default() })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_last_message_of_each_turn() {
        let dir = std::env::temp_dir().join(format!("relay-codex-tail-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let lines = [
            r#"{"type":"event_msg","payload":{"type":"task_started"}}"#,
            r#"{"type":"event_msg","payload":{"type":"task_complete","last_agent_message":"Done: tests pass."}}"#,
            r#"{"type":"event_msg","payload":{"type":"task_complete","last_agent_message":null}}"#,
        ];
        std::fs::write(dir.join("r.jsonl"), lines.join("\n")).unwrap();
        let t = tail(&dir.join("r.jsonl")).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(t.replies, ["Done: tests pass."]);
    }
}
