//! What the PreToolUse hook answers: which commands it rewrites, and
//! which it leaves alone.

mod common;

use common::Repo;
use serde_json::{Value, json};

fn pre(repo: &Repo, harness: &str, tool_input: Value) -> Option<Value> {
    let out = repo.hook(
        harness,
        json!({ "hook_event_name": "PreToolUse", "session_id": "s1", "tool_name": "Bash", "tool_input": tool_input }),
    );
    let out = out.trim();
    (!out.is_empty()).then(|| serde_json::from_str::<Value>(out).unwrap()["hookSpecificOutput"].clone())
}

#[test]
fn background_commands_are_left_alone() {
    let repo = Repo::new("rewrite-bg");
    assert!(pre(&repo, "claude", json!({ "command": "npm run dev", "run_in_background": true })).is_none());
}
