//! Codex rollouts (`~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl`), read
//! only: which ones belong to a project, which are subagents, and how each
//! turn ended.

use std::path::Path;

use serde_json::Value;

use crate::core::audit::Transcript;
use crate::core::handoff::Tail;
use crate::harness::jsonl::{self};
use crate::limits;

/// Rollouts under `sessions/`, filtered by the `cwd` in their first line
/// (`session_meta`): the project root or a directory below it. Subagent
/// rollouts name their parent thread.
pub fn rollouts(dir: &Path, root: Option<&Path>) -> Vec<Transcript> {
    jsonl::files_under(dir)
        .into_iter()
        .filter_map(|path| {
            let meta: Value = serde_json::from_str(&jsonl::first_line(&path)?).ok()?;
            let p = &meta["payload"];
            if let Some(r) = root
                && !p["cwd"].as_str().is_some_and(|cwd| jsonl::in_project(Path::new(cwd), r))
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
    let replies = jsonl::tail_lines(path, limits::store::TRANSCRIPT_TAIL_BYTES)?
        .filter(|l| l.contains("\"task_complete\""))
        .filter_map(|l| serde_json::from_str::<Value>(&l).ok())
        .filter_map(|v| v["payload"]["last_agent_message"].as_str().map(str::to_string))
        .filter(|m| !m.trim().is_empty())
        .collect();
    Some(Tail { replies: jsonl::last(replies, limits::handoff::REPLIES), ..Tail::default() })
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

    #[test]
    fn rollouts_started_below_the_root_belong_to_the_project() {
        let dir = std::env::temp_dir().join(format!("relay-codex-roll-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let root = dir.join("repo");
        std::fs::create_dir_all(root.join("packages/web")).unwrap();
        std::fs::create_dir_all(dir.join("sessions/2026/09/22")).unwrap();
        let meta = |id: &str, cwd: &Path| {
            format!(r#"{{"type":"session_meta","payload":{{"id":"{id}","cwd":"{}"}}}}"#, cwd.display())
        };
        std::fs::write(dir.join("sessions/2026/09/22/a.jsonl"), meta("sub", &root.join("packages/web"))).unwrap();
        std::fs::write(dir.join("sessions/2026/09/22/b.jsonl"), meta("top", &root)).unwrap();
        std::fs::write(dir.join("sessions/2026/09/22/c.jsonl"), meta("other", &dir.join("elsewhere"))).unwrap();
        let mut ids: Vec<_> = rollouts(&dir.join("sessions"), Some(&root)).into_iter().map(|t| t.id).collect();
        ids.sort();
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(ids, ["sub", "top"]);
    }
}
