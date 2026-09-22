//! What the `PreToolUse` hook answers: which commands it rewrites, and
//! which rewrites skip the harness's permission prompt.

mod common;

use common::Repo;
use serde_json::{Value, json};

fn pre(repo: &Repo, harness: &str, tool_input: &Value) -> Option<Value> {
    let out = repo.hook(
        harness,
        json!({ "hook_event_name": "PreToolUse", "session_id": "s1", "tool_name": "Bash", "tool_input": tool_input }),
    );
    let out = out.trim();
    (!out.is_empty()).then(|| serde_json::from_str::<Value>(out).unwrap()["hookSpecificOutput"].clone())
}

#[test]
fn only_read_only_commands_are_approved() {
    let repo = Repo::new("rewrite-perm");
    let ro = pre(&repo, "claude", &json!({ "command": "git status" })).unwrap();
    assert_eq!(ro["permissionDecision"], "allow");

    for cmd in ["rm -rf build && ls", "git push --force origin main", "cargo test"] {
        let out = pre(&repo, "claude", &json!({ "command": cmd })).unwrap();
        assert!(out["updatedInput"]["command"].as_str().unwrap().contains(" x --session "), "{cmd}");
        assert!(out.get("permissionDecision").is_none(), "{cmd} must go through the normal permission flow: {out}");
    }
}

#[test]
fn codex_leaves_what_it_would_have_to_approve() {
    let repo = Repo::new("rewrite-codex");
    assert_eq!(pre(&repo, "codex", &json!({ "command": "git status" })).unwrap()["permissionDecision"], "allow");
    assert!(pre(&repo, "codex", &json!({ "command": "cargo test" })).is_none());
}

#[test]
fn background_commands_are_left_alone() {
    let repo = Repo::new("rewrite-bg");
    assert!(pre(&repo, "claude", &json!({ "command": "npm run dev", "run_in_background": true })).is_none());
}

#[test]
fn remember_carries_the_session() {
    let repo = Repo::new("rewrite-remember");
    let out = pre(&repo, "claude", &json!({ "command": "relay remember rule \"Hooks fail open\"" })).unwrap();
    assert_eq!(out["updatedInput"]["command"], "relay remember --session 's1' rule \"Hooks fail open\"");
    assert!(out.get("permissionDecision").is_none());

    let run = repo.run(&["remember", "--session", "s1", "rule", "Hooks fail open"]);
    assert!(run.status.success(), "{}", String::from_utf8_lossy(&run.stderr));
    let item = std::fs::read_dir(repo.root.join(".relay/rules")).unwrap().flatten().next().unwrap().path();
    assert!(std::fs::read_to_string(item).unwrap().contains("s1"));
}
