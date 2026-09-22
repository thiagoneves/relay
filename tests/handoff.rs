//! A session in one harness must reach the next session in another
//! through the brief, within budget, with no LLM.

mod common;

use common::Repo;
use serde_json::json;

const BRIEF_BUDGET_CHARS: usize = 2600;

/// Drive a full session through `from`, then open a session in `to`
/// and return the brief it receives.
fn session_then_brief(repo: &Repo, from: &str, to: &str, tag: &str) -> String {
    let s = format!("{from}-{tag}");
    repo.hook(from, json!({ "hook_event_name": "SessionStart", "session_id": s, "source": "startup" }));
    repo.hook(from, json!({ "hook_event_name": "UserPromptSubmit", "session_id": s, "prompt": format!("Add retry to the payment client ({tag})") }));
    repo.hook(
        from,
        json!({
            "hook_event_name": "PostToolUse", "session_id": s, "tool_name": "Edit", "tool_use_id": format!("t-{tag}"),
            "tool_input": { "file_path": repo.path("src/pay.rs") }, "tool_response": {}
        }),
    );
    let failing = format!("echo 'error: payment timeout at src/pay.rs:12 ({tag})'; seq 1 100; exit 1");
    assert_eq!(repo.run(&["x", "--", &failing]).status.code(), Some(1), "relay x keeps the exit code");
    repo.run(&["remember", "gotcha", &format!("Payment sandbox rejects amounts under 50 cents ({tag})")]);
    repo.hook(from, json!({ "hook_event_name": "Stop", "session_id": s, "last_assistant_message": format!("Retry added; timeout test still fails ({tag}).") }));
    repo.hook(from, json!({ "hook_event_name": "SessionEnd", "session_id": s, "reason": "exit" }));

    repo.hook(
        to,
        json!({ "hook_event_name": "SessionStart", "session_id": format!("{to}-next-{tag}"), "source": "startup" }),
    )
}

fn assert_carries_session(brief: &str, from_id: &str, tag: &str) {
    let expected = [
        format!("Add retry to the payment client ({tag})"),
        format!("Retry added; timeout test still fails ({tag})."),
        "- src/pay.rs\n".to_string(),
        "exit 1".to_string(),
        format!("Payment sandbox rejects amounts under 50 cents ({tag})"),
        from_id.to_string(),
    ];
    let missing: Vec<&String> = expected.iter().filter(|e| !brief.contains(e.as_str())).collect();
    assert!(missing.is_empty(), "brief is missing {missing:?}\n--- brief ---\n{brief}");
    assert_eq!(brief.matches("50 cents").count(), 1, "remembered item repeated:\n{brief}");
    assert!(brief.len() <= BRIEF_BUDGET_CHARS, "brief is {} chars, budget {BRIEF_BUDGET_CHARS}", brief.len());
}

#[test]
fn claude_session_reaches_codex() {
    let repo = Repo::new("c2x");
    let brief = session_then_brief(&repo, "claude", "codex", "c2x");
    assert_carries_session(&brief, "claude-code", "c2x");
}

#[test]
fn codex_session_reaches_claude() {
    let repo = Repo::new("x2c");
    let brief = session_then_brief(&repo, "codex", "claude", "x2c");
    assert_carries_session(&brief, "codex", "x2c");
}

#[test]
fn latest_session_wins_across_a_round_trip() {
    let repo = Repo::new("trip");
    session_then_brief(&repo, "claude", "codex", "first");
    let brief = session_then_brief(&repo, "codex", "claude", "second");
    assert!(brief.contains("(second)") && brief.contains("Last session"), "{brief}");
    assert!(!brief.contains("Add retry to the payment client (first)"), "stale handoff leaked:\n{brief}");
    // Remembered items are durable: both survive.
    assert!(brief.contains("50 cents (first)") && brief.contains("50 cents (second)"), "{brief}");
}

#[test]
fn status_reports_orientation_and_reads_are_not_edits() {
    let repo = Repo::new("orient");
    let s = "orient-1";
    repo.hook("claude", json!({ "hook_event_name": "SessionStart", "session_id": s, "source": "startup" }));
    repo.hook(
        "claude",
        json!({
            "hook_event_name": "PostToolUse", "session_id": s, "tool_name": "Read", "tool_use_id": "r1",
            "tool_input": { "file_path": repo.path("src/big.rs") },
            "tool_response": { "type": "text", "file": { "content": "fn main() { println!(\"hi\"); }\n".repeat(200) } }
        }),
    );
    repo.hook("claude", json!({
        "hook_event_name": "PostToolUse", "session_id": s, "tool_name": "Edit", "tool_use_id": "e1",
        "tool_input": { "file_path": repo.path("src/pay.rs") }, "tool_response": { "originalFile": "x".repeat(5000) }
    }));
    repo.hook("claude", json!({ "hook_event_name": "SessionEnd", "session_id": s, "reason": "exit" }));

    let status = String::from_utf8(repo.run(&["status"]).stdout).unwrap();
    let line = status.lines().find(|l| l.starts_with("Orientation")).unwrap_or_else(|| panic!("{status}"));
    assert!(line.contains("without (n=1)"), "{line}");
    let handoff = String::from_utf8(repo.run(&["handoff", "--show"]).stdout).unwrap();
    let section = |name: &str| {
        let rest = handoff.split(&format!("## {name}\n")).nth(1).unwrap_or("");
        rest.split("\n\n").next().unwrap_or("").to_string()
    };
    assert_eq!(section("Files touched"), "- src/pay.rs", "reads leaked into Files touched:\n{handoff}");
    assert_eq!(section("Read first"), "- src/big.rs", "orientation reads missing:\n{handoff}");
}
