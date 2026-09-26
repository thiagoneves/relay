//! A whole-file read of a big text file gets its outline; a ranged read
//! goes through, in Claude Code and in Gemini CLI.

mod common;

use common::Repo;
use serde_json::{Value, json};

fn big_plan(repo: &Repo) -> String {
    let body = "The learner opens the lesson and hears the phrase twice before speaking.\n".repeat(30);
    let doc: String = (1..=60).map(|i| format!("## T-{i} Task {i}\n")).collect::<Vec<_>>().join(&body);
    std::fs::create_dir_all(repo.root.join("docs")).unwrap();
    std::fs::write(repo.root.join("docs/plan.md"), format!("# Plan\n{doc}")).unwrap();
    repo.path("docs/plan.md")
}

#[test]
fn claude_code_gets_the_outline_instead_of_the_whole_file() {
    let repo = Repo::new("guard-claude");
    let file = big_plan(&repo);
    let read = |input: Value| {
        repo.hook(
            "claude",
            json!({ "hook_event_name": "PreToolUse", "session_id": "s1", "tool_name": "Read", "tool_use_id": "t1", "tool_input": input }),
        )
    };
    let out: Value = serde_json::from_str(&read(json!({ "file_path": file }))).unwrap();
    assert_eq!(out["hookSpecificOutput"]["permissionDecision"], "deny");
    let reason = out["hookSpecificOutput"]["permissionDecisionReason"].as_str().unwrap();
    assert!(reason.starts_with("relay: docs/plan.md is "), "{reason}");
    assert!(reason.contains("T-42 Task 42"), "{reason}");
    assert_eq!(read(json!({ "file_path": file, "offset": 40, "limit": 30 })), "");

    let spool = std::fs::read_to_string(repo.root.join(".git/relay/spool/s1.jsonl")).unwrap();
    assert!(spool.contains("\"event\":\"read_guarded\"") && spool.contains("\"file\":\"docs/plan.md\""), "{spool}");
}

#[test]
fn gemini_cli_gets_it_as_the_tool_error() {
    let repo = Repo::new("guard-gemini");
    big_plan(&repo);
    let out = repo.hook(
        "gemini",
        json!({ "hook_event_name": "BeforeTool", "session_id": "g1", "tool_name": "read_file", "tool_input": { "file_path": "docs/plan.md" } }),
    );
    let out: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(out["decision"], "deny");
    assert!(out["reason"].as_str().unwrap().contains("L1-"), "{out}");
}
