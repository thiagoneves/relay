//! Claude Code runs relay's hooks inside subagents too: a subagent gets
//! the part of the brief that applies to it, its calls are its own in the
//! spool, and its transcript is noted when it stops.

mod common;

use common::Repo;
use serde_json::{Value, json};

#[test]
fn a_subagent_gets_a_brief_and_its_calls_are_its_own() {
    let repo = Repo::new("subagents");
    assert!(repo.run(&["remember", "rule", "Plan docs are read by section"]).status.success());
    let out = repo.hook(
        "claude",
        json!({ "hook_event_name": "SubagentStart", "session_id": "s1", "agent_id": "agent-1", "agent_type": "Explore" }),
    );
    let reply: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(reply["hookSpecificOutput"]["hookEventName"], "SubagentStart");
    let text = reply["hookSpecificOutput"]["additionalContext"].as_str().unwrap();
    assert!(
        text.contains("- rule: Plan docs are read by section\n") && text.contains("relay brief <task id"),
        "{text}"
    );
    assert!(!text.contains("Last session"), "{text}");

    repo.hook(
        "claude",
        json!({
            "hook_event_name": "PostToolUse", "session_id": "s1", "agent_id": "agent-1", "agent_type": "Explore",
            "tool_name": "Read", "tool_use_id": "t1", "tool_input": { "file_path": repo.path("a.md") },
            "tool_response": { "file": { "content": "hello" } }
        }),
    );
    repo.hook(
        "claude",
        json!({
            "hook_event_name": "SubagentStop", "session_id": "s1", "agent_id": "agent-1", "agent_type": "Explore",
            "agent_transcript_path": "/tmp/t/subagents/agent-1.jsonl", "stop_hook_active": false
        }),
    );
    let spool = std::fs::read_to_string(repo.root.join(".git/relay/spool/s1.jsonl")).unwrap();
    assert!(spool.contains("\"tool\":\"Read\"") && spool.contains("\"agent\":\"agent-1\""), "{spool}");
    assert!(spool.contains("\"event\":\"subagent_stop\"") && spool.contains("agent-1.jsonl"), "{spool}");
}

#[test]
fn only_claude_code_registers_the_subagent_events() {
    let repo = Repo::new("subagent-install");
    assert!(repo.run(&["install", "claude"]).status.success());
    let settings = std::fs::read_to_string(repo.root.join("settings.json")).unwrap();
    assert!(settings.contains("SubagentStart") && settings.contains("SubagentStop"), "{settings}");
    assert!(repo.run(&["install", "codex"]).status.success());
    let hooks = std::fs::read_to_string(repo.root.join("hooks.json")).unwrap();
    assert!(!hooks.contains("Subagent"), "{hooks}");
}
