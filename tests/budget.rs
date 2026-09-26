//! An agent hears when its tool output passes another 150k tokens, and
//! what it cost is filed under the task its session was asked about.

mod common;

use common::Repo;
use serde_json::{Value, json};

#[test]
fn a_costly_subagent_is_told_and_its_cost_is_filed_under_the_task() {
    let repo = Repo::new("budget");
    repo.hook("claude", json!({ "hook_event_name": "UserPromptSubmit", "session_id": "s1", "prompt": "do T-253" }));
    let read = |id: &str, words: usize| {
        repo.hook(
            "claude",
            json!({
                "hook_event_name": "PostToolUse", "session_id": "s1", "agent_id": "a1", "agent_type": "Explore",
                "tool_name": "Grep", "tool_use_id": id, "tool_input": { "pattern": "x" },
                "tool_response": { "content": "lorem ipsum dolor ".repeat(words) }
            }),
        )
    };
    assert_eq!(read("g1", 10), "");
    let out: Value = serde_json::from_str(&read("g2", 60_000)).unwrap();
    let note = out["hookSpecificOutput"]["additionalContext"].as_str().unwrap();
    assert!(note.starts_with("relay: This subagent has taken in ~"), "{note}");
    assert_eq!(out["hookSpecificOutput"]["hookEventName"], "PostToolUse");

    repo.hook(
        "claude",
        json!({ "hook_event_name": "SubagentStop", "session_id": "s1", "agent_id": "a1", "agent_type": "Explore" }),
    );
    let history = String::from_utf8_lossy(&repo.run(&["usage", "--history"]).stdout).into_owned();
    assert!(history.contains("T-253") && history.contains("over 1 agent"), "{history}");
}
