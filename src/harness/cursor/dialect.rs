//! Cursor's hook dialect, translated to relay's internal (Claude Code)
//! shape on the way in and back on the way out. Cursor names events in
//! camelCase, calls the shell tool `Shell`, carries `conversation_id`
//! instead of `session_id` on most events, and passes a shell result as
//! a JSON string.

use serde_json::{Value, json};

use crate::harness::protocol::hooks_json::{Event, ev};
use crate::harness::protocol::reply::Reply;

/// Timeouts in seconds, as Cursor expects.
pub const EVENTS: &[Event] = &[
    ev("sessionStart", None, 10),
    ev("beforeSubmitPrompt", None, 5),
    ev("preToolUse", Some("Shell"), 5),
    ev("postToolUse", None, 5),
    ev("afterAgentResponse", None, 5),
    ev("preCompact", None, 5),
    ev("sessionEnd", None, 10),
];

/// The event in the internal shape, or `None` for one relay does not use.
pub fn normalize(raw: &Value) -> Option<Value> {
    let event = match raw["hook_event_name"].as_str()? {
        "sessionStart" => "SessionStart",
        "beforeSubmitPrompt" => "UserPromptSubmit",
        "preToolUse" => "PreToolUse",
        "postToolUse" => "PostToolUse",
        "afterAgentResponse" => "Stop",
        "preCompact" => "PreCompact",
        "sessionEnd" => "SessionEnd",
        _ => return None,
    };
    let session = raw["session_id"].as_str().or(raw["conversation_id"].as_str())?;
    let cwd = raw["cwd"].as_str().or(raw["workspace_roots"][0].as_str());
    let mut out = json!({
        "hook_event_name": event,
        "session_id": session,
        "cwd": cwd,
        "transcript_path": raw["transcript_path"],
    });
    match event {
        "SessionStart" => out["source"] = "startup".into(),
        "UserPromptSubmit" => out["prompt"] = raw["prompt"].clone(),
        "PreToolUse" | "PostToolUse" => {
            out["tool_name"] = tool_name(raw["tool_name"].as_str().unwrap_or("")).into();
            out["tool_input"] = raw["tool_input"].clone();
            out["tool_use_id"] = raw["tool_use_id"].clone();
            out["tool_response"] = response(&raw["tool_output"]);
        }
        "Stop" => out["last_assistant_message"] = raw["text"].clone(),
        "PreCompact" => out["trigger"] = raw["trigger"].clone(),
        _ => out["reason"] = raw["reason"].clone(),
    }
    Some(out)
}

fn tool_name(cursor: &str) -> &str {
    if cursor == "Shell" { "Bash" } else { cursor }
}

/// A shell result arrives as a JSON string (`{"exitCode":0,"stdout":…}`).
fn response(output: &Value) -> Value {
    match output.as_str().and_then(|s| serde_json::from_str::<Value>(s).ok()) {
        Some(parsed) => json!({ "stdout": parsed["stdout"], "stderr": parsed["stderr"] }),
        None => output.clone(),
    }
}

/// Cursor blocks a permission hook's action on invalid JSON, so
/// `preToolUse` always answers with an object, empty when relay has
/// nothing to say.
pub fn render(event: &str, reply: &Reply) -> Option<String> {
    match (event, reply) {
        ("PreToolUse", Reply::Rewrite(r)) => {
            let mut out = json!({ "updated_input": { "command": r.command } });
            if r.approve {
                out["permission"] = "allow".into();
            }
            Some(out.to_string())
        }
        ("PreToolUse", _) => Some("{}".into()),
        ("SessionStart", Reply::Context(text)) => Some(json!({ "additional_context": text }).to_string()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::protocol::policy::Rewritten;

    #[test]
    fn a_shell_call_becomes_the_internal_shape() {
        let raw = json!({
            "hook_event_name": "preToolUse", "conversation_id": "c1", "cwd": "/repo", "tool_name": "Shell",
            "tool_input": { "command": "cargo test", "working_directory": "/repo" }, "tool_use_id": "t1",
            "cursor_version": "2.5.17"
        });
        let got = normalize(&raw).unwrap();
        assert_eq!(got["hook_event_name"], "PreToolUse");
        assert_eq!((got["session_id"].as_str(), got["tool_name"].as_str()), (Some("c1"), Some("Bash")));
        assert_eq!(got["tool_input"]["command"], "cargo test");
    }

    #[test]
    fn events_without_cwd_use_the_workspace_and_results_are_parsed() {
        let raw = json!({ "hook_event_name": "sessionStart", "session_id": "c1", "workspace_roots": ["/repo"] });
        assert_eq!(normalize(&raw).unwrap()["cwd"], "/repo");
        let post = json!({
            "hook_event_name": "postToolUse", "conversation_id": "c1", "cwd": "/repo", "tool_name": "Shell",
            "tool_output": "{\"exitCode\":0,\"stdout\":\"ok\\n\"}"
        });
        assert_eq!(normalize(&post).unwrap()["tool_response"]["stdout"], "ok\n");
        assert!(normalize(&json!({ "hook_event_name": "beforeReadFile", "conversation_id": "c1" })).is_none());
    }

    #[test]
    fn replies_in_cursor_shape() {
        let rewrite = Reply::Rewrite(Rewritten { command: "relay x -- 'cargo test'".into(), approve: true });
        let out: Value = serde_json::from_str(&render("PreToolUse", &rewrite).unwrap()).unwrap();
        assert_eq!(out, json!({ "permission": "allow", "updated_input": { "command": "relay x -- 'cargo test'" } }));
        assert_eq!(render("PreToolUse", &Reply::Nothing).as_deref(), Some("{}"));
        assert_eq!(
            render("SessionStart", &Reply::Context("brief".into())).as_deref(),
            Some(r#"{"additional_context":"brief"}"#)
        );
        assert_eq!(render("Stop", &Reply::Nothing), None);
    }
}
