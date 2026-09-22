//! Two sessions open in one worktree at once must not pick up each
//! other's outputs or handoffs.

mod common;

use common::Repo;
use serde_json::json;

fn start(repo: &Repo, session: &str) {
    repo.hook("claude", json!({ "hook_event_name": "SessionStart", "session_id": session, "source": "startup" }));
}

fn stored_sessions(repo: &Repo) -> Vec<String> {
    let dir = repo.root.join(".git/relay/outputs");
    std::fs::read_dir(dir)
        .unwrap()
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
        .map(|e| {
            let meta: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(e.path()).unwrap()).unwrap();
            meta["session"].as_str().unwrap_or("").to_string()
        })
        .collect()
}

#[test]
fn output_belongs_to_the_session_that_ran_it() {
    let repo = Repo::new("parallel-x");
    start(&repo, "sess-a");
    start(&repo, "sess-b");

    let out = repo.hook(
        "claude",
        json!({
            "hook_event_name": "PreToolUse", "session_id": "sess-a", "tool_name": "Bash",
            "tool_input": { "command": "cargo test" }
        }),
    );
    assert!(out.contains("--session 'sess-a'"), "rewrite must carry the session: {out}");

    // Session B moves the shared pointer while A's command is in flight.
    repo.hook(
        "claude",
        json!({
            "hook_event_name": "PreToolUse", "session_id": "sess-b", "tool_name": "Bash",
            "tool_input": { "command": "git status" }
        }),
    );
    let run = repo.run(&["x", "--session", "sess-a", "--", "seq 1 300"]);
    assert!(run.status.success(), "{}", String::from_utf8_lossy(&run.stderr));
    assert_eq!(stored_sessions(&repo), ["sess-a"]);
}
