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

/// Write a one-line Claude transcript whose tool result for `t1` is `seen`.
fn transcript(repo: &Repo, seen: &str) -> String {
    let path = repo.root.join("transcript.jsonl");
    let line = serde_json::json!({
        "type": "user",
        "message": { "content": [{ "type": "tool_result", "tool_use_id": "t1", "content": seen }] }
    });
    std::fs::write(&path, format!("{line}\n")).unwrap();
    path.display().to_string()
}

fn end_session(repo: &Repo, transcript: &str) {
    repo.hook(
        "claude",
        json!({ "hook_event_name": "SessionEnd", "session_id": "s1", "reason": "exit", "transcript_path": transcript }),
    );
}

#[test]
fn a_replacement_the_harness_ignored_is_reported() {
    let repo = Repo::new("shrink-ignored");
    let long = (1..=400).map(|i| format!("line {i}\n")).collect::<Vec<_>>().concat();
    post(&repo, "claude", "python3 report.py", &long).unwrap();
    // The transcript holds the full original: the model never saw the view.
    end_session(&repo, &transcript(&repo, &long));
    let status = String::from_utf8(repo.run(&["status"]).stdout).unwrap();
    assert!(status.contains("Failures      1 in the last 7 days"), "{status}");
    assert!(status.contains("compression after a command is not taking effect"), "{status}");
    let failures = String::from_utf8(repo.run(&["log"]).stdout).unwrap();
    assert!(failures.contains("claude showed the model the full output for 1 of 1 commands"), "{failures}");
}

#[test]
fn a_replacement_the_model_saw_is_not_a_failure() {
    let repo = Repo::new("shrink-seen");
    let long = (1..=400).map(|i| format!("line {i}\n")).collect::<Vec<_>>().concat();
    let view = post(&repo, "claude", "python3 report.py", &long).unwrap()["updatedToolOutput"]["stdout"]
        .as_str()
        .unwrap()
        .to_string();
    end_session(&repo, &transcript(&repo, &view));
    let status = String::from_utf8(repo.run(&["status"]).stdout).unwrap();
    assert!(status.contains("Failures      none"), "{status}");
}
