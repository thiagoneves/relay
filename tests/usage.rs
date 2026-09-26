//! `relay usage`: the main thread and each subagent on their own line,
//! exact where the transcript is, estimated where it is not.

mod common;

use common::Repo;
use serde_json::json;

fn call(id: &str, input: u64, output: u64) -> String {
    format!(
        r#"{{"message":{{"id":"{id}","role":"assistant","usage":{{"input_tokens":{input},"output_tokens":{output}}}}}}}"#
    )
}

#[test]
fn subagents_are_scored_by_their_own_transcript() {
    let repo = Repo::new("usage");
    let sub = repo.root.join("t/subagents");
    std::fs::create_dir_all(&sub).unwrap();
    std::fs::write(sub.join("agent-a1.jsonl"), [call("m1", 300_000, 2000), call("m2", 350_000, 1000)].join("\n"))
        .unwrap();
    let hook = |event: serde_json::Value| repo.hook("claude", event);
    hook(json!({ "hook_event_name": "SessionStart", "session_id": "sess-1234abcd", "source": "startup" }));
    hook(json!({ "hook_event_name": "UserPromptSubmit", "session_id": "sess-1234abcd", "prompt": "ship T-253" }));
    hook(
        json!({ "hook_event_name": "SubagentStart", "session_id": "sess-1234abcd", "agent_id": "a1", "agent_type": "Explore" }),
    );
    hook(json!({
        "hook_event_name": "PostToolUse", "session_id": "sess-1234abcd", "agent_id": "a1", "agent_type": "Explore",
        "tool_name": "Read", "tool_use_id": "r1", "tool_input": { "file_path": repo.path("docs/plan.md") },
        "tool_response": { "file": { "content": "word ".repeat(3000) } }
    }));
    hook(json!({
        "hook_event_name": "SubagentStop", "session_id": "sess-1234abcd", "agent_id": "a1", "agent_type": "Explore",
        "agent_transcript_path": sub.join("agent-a1.jsonl").display().to_string()
    }));

    let out = repo.run(&["usage"]);
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(text.contains("650.0k tokens sent in 1 session and 1 subagent"), "{text}");
    assert!(text.contains("650.0k sent ·   3000 out  sess-123 › Explore a1"), "{text}");

    assert!(text.contains("docs/plan.md  (sess-123, subagent a1)"), "{text}");
    assert!(text.contains("read guard   0 → 0 tokens over 0 whole-file reads turned down"), "{text}");
}
