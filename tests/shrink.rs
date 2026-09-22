//! After Claude Code runs a command itself, the `PostToolUse` hook swaps
//! the output the model sees for relay's compressed view.

mod common;

use common::Repo;
use serde_json::{Value, json};

fn post(repo: &Repo, harness: &str, command: &str, stdout: &str) -> Option<Value> {
    let out = repo.hook(
        harness,
        json!({
            "hook_event_name": "PostToolUse", "session_id": "s1", "tool_name": "Bash", "tool_use_id": "t1",
            "tool_input": { "command": command },
            "tool_response": { "stdout": stdout, "stderr": "", "interrupted": false, "isImage": false }
        }),
    );
    let out = out.trim();
    (!out.is_empty()).then(|| serde_json::from_str::<Value>(out).unwrap()["hookSpecificOutput"].clone())
}

#[test]
fn long_output_is_shrunk_and_kept() {
    let repo = Repo::new("shrink-long");
    let long = (1..=400).map(|i| format!("line {i} of a long python report\n")).collect::<Vec<_>>().concat();
    let out = post(&repo, "claude", "python3 report.py", &long).unwrap();
    assert_eq!(out["hookEventName"], "PostToolUse");
    let view = out["updatedToolOutput"]["stdout"].as_str().unwrap();
    assert!(view.len() < long.len() && view.contains("relay get o_"), "{view}");
    assert_eq!(out["updatedToolOutput"]["isImage"], false);

    let id = view.rsplit("relay get ").next().unwrap().trim_end_matches(']');
    let original = repo.run(&["get", id]);
    assert_eq!(String::from_utf8_lossy(&original.stdout), long);
}

#[test]
fn short_output_and_codex_are_left_alone() {
    let repo = Repo::new("shrink-short");
    assert!(post(&repo, "claude", "python3 -V", "Python 3.12.1\n").is_none());
    let long = "x\n".repeat(400);
    assert!(post(&repo, "codex", "python3 report.py", &long).is_none());
}
