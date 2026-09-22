//! relay's hooks as Cursor calls them: Cursor's own event names and
//! payloads in, Cursor-shaped JSON out.

mod common;

use common::Repo;
use serde_json::{Value, json};

fn hook(repo: &Repo, harness: &str, event: Value) -> String {
    repo.hook(harness, event).trim().to_string()
}

fn pre(repo: &Repo, command: &str) -> Value {
    let out = hook(
        repo,
        "cursor",
        json!({ "hook_event_name": "preToolUse", "conversation_id": "c1", "tool_name": "Shell",
                "tool_input": { "command": command }, "tool_use_id": "t1", "cursor_version": "2.5.17" }),
    );
    serde_json::from_str(&out).unwrap_or_else(|_| panic!("preToolUse must always print JSON: {out:?}"))
}

#[test]
fn routine_commands_are_rewritten_and_the_rest_left_alone() {
    let repo = Repo::new("cursor-pre");
    let out = pre(&repo, "cargo test");
    assert_eq!(out["permission"], "allow");
    assert!(out["updated_input"]["command"].as_str().unwrap().contains(" x --session 'c1' -- 'cargo test'"), "{out}");
    assert_eq!(pre(&repo, "rm -rf build"), json!({}));
}

#[test]
fn session_start_puts_the_brief_in_context_and_records_the_session() {
    let repo = Repo::new("cursor-start");
    assert!(repo.run(&["init"]).status.success());
    let out = hook(
        &repo,
        "cursor",
        json!({ "hook_event_name": "sessionStart", "session_id": "c1", "cursor_version": "2.5.17" }),
    );
    let v: Value = serde_json::from_str(&out).unwrap();
    assert!(v["additional_context"].as_str().unwrap().starts_with("# relay brief"), "{out}");
    assert!(repo.root.join(".git/relay/spool/c1.jsonl").exists());
}

#[test]
fn claude_hooks_run_by_cursor_do_nothing() {
    let repo = Repo::new("cursor-claude");
    let out = hook(
        &repo,
        "claude",
        json!({ "hook_event_name": "sessionStart", "conversation_id": "c1", "cursor_version": "2.5.17" }),
    );
    assert_eq!(out, "");
    assert!(!repo.root.join(".git/relay/spool/c1.jsonl").exists());
}

#[test]
fn install_writes_cursor_hooks_flat_and_keeps_others() {
    let repo = Repo::new("cursor-install");
    let file = repo.root.join(".cursor/hooks.json");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, r#"{"version":1,"hooks":{"preToolUse":[{"command":"impeccable check"}]}}"#).unwrap();
    assert!(repo.run(&["install", "cursor"]).status.success());
    let v: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    let pre = v["hooks"]["preToolUse"].as_array().unwrap();
    assert_eq!(pre[0]["command"], "impeccable check");
    assert!(pre[1]["command"].as_str().unwrap().ends_with(" hook cursor") && pre[1]["matcher"] == "Shell");
    assert!(repo.run(&["uninstall", "cursor"]).status.success());
    let v: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    assert_eq!(v["hooks"], json!({ "preToolUse": [{ "command": "impeccable check" }] }));
}
