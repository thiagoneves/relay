//! Two sessions in one checkout: the second hears what the first is
//! editing, in its brief and once before it edits the same file.

mod common;

use common::Repo;
use serde_json::{Value, json};

fn edit(repo: &Repo, event: &str, session: &str, file: &str) -> String {
    repo.hook(
        "claude",
        json!({
            "hook_event_name": event, "session_id": session, "tool_name": "Edit", "tool_use_id": format!("{session}-{event}"),
            "tool_input": { "file_path": repo.path(file), "old_string": "a", "new_string": "b" },
            "tool_response": { "filePath": repo.path(file) }
        }),
    )
}

fn start(repo: &Repo, session: &str) -> String {
    repo.hook("claude", json!({ "hook_event_name": "SessionStart", "session_id": session, "source": "startup" }))
}

#[test]
fn a_second_session_hears_about_the_first() {
    let repo = Repo::new("claims");
    std::fs::create_dir_all(repo.root.join("apps/backstage")).unwrap();
    std::fs::write(repo.root.join("apps/backstage/page.tsx"), "x").unwrap();
    start(&repo, "sess-aaaa1111");
    repo.hook("claude", json!({ "hook_event_name": "UserPromptSubmit", "session_id": "sess-aaaa1111", "prompt": "fix the content filters" }));
    assert_eq!(edit(&repo, "PreToolUse", "sess-aaaa1111", "apps/backstage/page.tsx"), "", "nobody else is here");
    edit(&repo, "PostToolUse", "sess-aaaa1111", "apps/backstage/page.tsx");

    let brief = start(&repo, "sess-bbbb2222");
    assert!(brief.contains("## Other sessions in this checkout\n"), "{brief}");
    assert!(
        brief.contains("sess-aaa (claude-code, \"fix the content filters\"), last edit just now: editing apps/backstage/page.tsx; uncommitted: apps/backstage/page.tsx"),
        "{brief}"
    );

    let warned = edit(&repo, "PreToolUse", "sess-bbbb2222", "apps/backstage/page.tsx");
    let reply: Value = serde_json::from_str(&warned).unwrap();
    let note = reply["hookSpecificOutput"]["additionalContext"].as_str().unwrap();
    assert!(note.contains("session sess-aaa") && note.contains("apps/backstage/page.tsx"), "{note}");
    assert!(reply["hookSpecificOutput"]["permissionDecision"].is_null(), "a warning never decides");
    assert_eq!(edit(&repo, "PreToolUse", "sess-bbbb2222", "apps/backstage/page.tsx"), "", "warned once");

    // Once A ends, its path is free, though the file it left is still its.
    repo.hook("claude", json!({ "hook_event_name": "SessionEnd", "session_id": "sess-aaaa1111", "reason": "exit" }));
    assert_eq!(edit(&repo, "PreToolUse", "sess-cccc3333", "apps/backstage/page.tsx"), "");
    let brief = start(&repo, "sess-cccc3333");
    assert!(brief.contains("ended just now; uncommitted: apps/backstage/page.tsx"), "{brief}");
}
