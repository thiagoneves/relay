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

/// Every rewrite is approved; whatever relay would not approve stays
/// untouched, so the harness judges the agent's own command.
#[test]
fn rewrites_are_approved_and_the_rest_left_alone() {
    let repo = Repo::new("rewrite-perm");
    for harness in ["claude", "codex"] {
        for cmd in ["git status", "cargo test"] {
            let out = pre(&repo, harness, &json!({ "command": cmd })).unwrap();
            assert!(out["updatedInput"]["command"].as_str().unwrap().contains(" x --session "), "{cmd}");
            assert_eq!(out["permissionDecision"], "allow", "{harness}: {cmd}");
        }
        for cmd in ["git push --force origin main", "rm -rf build && ls", "cargo run"] {
            assert!(pre(&repo, harness, &json!({ "command": cmd })).is_none(), "{harness}: {cmd}");
        }
    }
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

#[test]
fn leading_cd_stays_in_the_harness_shell() {
    let repo = Repo::new("rewrite-cd");
    let out = pre(&repo, "claude", &json!({ "command": "cd /repo && git status" })).unwrap();
    let cmd = out["updatedInput"]["command"].as_str().unwrap();
    assert!(cmd.starts_with("cd /repo && ") && cmd.ends_with("-- 'git status'"), "{cmd}");
    assert_eq!(out["permissionDecision"], "allow");

    let out = pre(&repo, "claude", &json!({ "command": "export CI=1 && cargo test" })).unwrap();
    assert!(out["updatedInput"]["command"].as_str().unwrap().starts_with("export CI=1 && "), "{out}");

    assert!(pre(&repo, "claude", &json!({ "command": "cd /repo" })).is_none());
}
