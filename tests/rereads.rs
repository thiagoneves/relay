//! A re-read of a file the agent already has becomes a note, or the diff
//! since, with the full result one `relay get` away.

mod common;

use common::Repo;
use serde_json::{Value, json};

fn read(repo: &Repo, agent: Option<&str>, id: &str, content: &str) -> String {
    let mut ev = json!({
        "hook_event_name": "PostToolUse", "session_id": "s1", "tool_name": "Read", "tool_use_id": id,
        "tool_input": { "file_path": repo.path("src/app.ts") },
        "tool_response": { "type": "text", "file": {
            "filePath": repo.path("src/app.ts"), "content": content, "numLines": 80, "startLine": 1, "totalLines": 80
        } }
    });
    if let Some(a) = agent {
        ev["agent_id"] = a.into();
    }
    repo.hook("claude", ev)
}

fn content_of(reply: &str) -> String {
    let v: Value = serde_json::from_str(reply).unwrap();
    v["hookSpecificOutput"]["updatedToolOutput"]["file"]["content"].as_str().unwrap().to_string()
}

#[test]
fn rereads_become_a_note_or_a_diff() {
    let repo = Repo::new("rereads");
    let v1 = (1..=80).map(|i| format!("export const line{i} = {i};")).collect::<Vec<_>>().join("\n") + "\n";
    assert_eq!(read(&repo, None, "r1", &v1), "", "the first read goes through");
    let note = content_of(&read(&repo, None, "r2", &v1));
    assert!(note.starts_with("relay: src/app.ts is unchanged since this agent read it just now"), "{note}");
    let id = note.rsplit("relay get ").next().unwrap().trim_end_matches(']').to_string();
    let original = repo.run(&["get", &id]);
    assert_eq!(String::from_utf8_lossy(&original.stdout).trim_end(), v1.trim_end());

    assert_eq!(read(&repo, Some("agent-9"), "r3", &v1), "", "a subagent's first read is its own");

    let v2 = v1.replace("line40 = 40", "line40 = 41");
    let diff = content_of(&read(&repo, None, "r4", &v2));
    assert!(diff.contains("changed since this agent read it") && diff.contains("@@ -37,7 +37,7 @@"), "{diff}");
    assert!(diff.contains("-export const line40 = 40;\n+export const line40 = 41;"), "{diff}");

    repo.hook("claude", json!({ "hook_event_name": "PreCompact", "session_id": "s1", "trigger": "auto" }));
    assert_eq!(read(&repo, None, "r5", &v2), "", "after a compaction the read counts again");
}
