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

/// A fake `claude` that opens its own session, then lets a parallel
/// `relay claude` touch the spool last, as it would while still running.
#[cfg(unix)]
#[test]
fn wrapper_closes_its_own_session_not_the_latest() {
    use std::os::unix::fs::PermissionsExt;

    let repo = Repo::new("parallel-wrap");
    let bin = repo.root.join("fakebin");
    std::fs::create_dir_all(&bin).unwrap();
    let relay = env!("CARGO_BIN_EXE_relay");
    let script = format!(
        "#!/bin/sh\n\
         ev() {{ printf '{{\"hook_event_name\":\"SessionStart\",\"session_id\":\"%s\",\"source\":\"startup\",\"cwd\":\"%s\"}}' \"$1\" \"$PWD\"; }}\n\
         ev mine | '{relay}' hook claude >/dev/null\n\
         sleep 1\n\
         ev other | RELAY_WRAPPER=someone-else '{relay}' hook claude >/dev/null\n"
    );
    let fake = bin.join("claude");
    std::fs::write(&fake, script).unwrap();
    std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();

    let path = format!("{}:{}", bin.display(), std::env::var("PATH").unwrap_or_default());
    let out = repo.isolated(common::relay()).arg("claude").env("PATH", path).output().unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{stderr}");
    assert!(stderr.contains("session mine"), "{stderr}");

    let local = repo.root.join(".git/relay");
    assert!(local.join("handoffs/mine.md").exists(), "{stderr}");
    assert!(!local.join("handoffs/other.md").exists(), "handoff built for a session still running");
    assert_eq!(std::fs::read_to_string(local.join("last_session")).unwrap().trim(), "mine");
}
