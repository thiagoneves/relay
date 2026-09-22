//! Gemini CLI's hook dialect, translated to relay's internal (Claude
//! Code) shape on the way in and back on the way out. Gemini names the
//! shell tool `run_shell_command`, calls the prompt event `BeforeAgent`
//! and the end of a turn `AfterAgent`, and takes a rewritten command as
//! `hookSpecificOutput.tool_input`.

use serde_json::{Value, json};

use crate::harness::protocol::hooks_json::{Event, ev};
use crate::harness::protocol::reply::Reply;

const SHELL: &str = "run_shell_command";

/// Timeouts in milliseconds, as Gemini CLI expects.
pub const EVENTS: &[Event] = &[
    ev("SessionStart", None, 10_000),
    ev("BeforeAgent", None, 5_000),
    ev("BeforeTool", Some(SHELL), 5_000),
    ev("AfterTool", None, 5_000),
    ev("AfterAgent", None, 5_000),
    ev("PreCompress", None, 5_000),
    ev("SessionEnd", None, 10_000),
];

/// The event in the internal shape, or `None` for one relay does not use.
pub fn normalize(raw: &Value) -> Option<Value> {
    let event = match raw["hook_event_name"].as_str()? {
        "SessionStart" => "SessionStart",
        "BeforeAgent" => "UserPromptSubmit",
        "BeforeTool" => "PreToolUse",
        "AfterTool" => "PostToolUse",
        "AfterAgent" => "Stop",
        "PreCompress" => "PreCompact",
        "SessionEnd" => "SessionEnd",
        _ => return None,
    };
    let mut out = json!({
        "hook_event_name": event,
        "session_id": raw["session_id"],
        "cwd": raw["cwd"],
        "transcript_path": raw["transcript_path"].as_str().filter(|p| !p.is_empty()),
    });
    match event {
        "SessionStart" => out["source"] = raw["source"].clone(),
        "UserPromptSubmit" => out["prompt"] = raw["prompt"].clone(),
        "PreToolUse" | "PostToolUse" => {
            out["tool_name"] = tool_name(raw["tool_name"].as_str().unwrap_or("")).into();
            out["tool_input"] = raw["tool_input"].clone();
            out["tool_input"]["run_in_background"] = raw["tool_input"]["is_background"].clone();
            out["tool_response"] = json!({ "stdout": raw["tool_response"]["llmContent"] });
        }
        "Stop" => out["last_assistant_message"] = raw["prompt_response"].clone(),
        "PreCompact" => out["trigger"] = raw["trigger"].clone(),
        _ => out["reason"] = raw["reason"].clone(),
    }
    Some(out)
}

fn tool_name(gemini: &str) -> &str {
    match gemini {
        SHELL => "Bash",
        "read_file" | "read_many_files" => "Read",
        "write_file" => "Write",
        "replace" => "Edit",
        "glob" => "Glob",
        "search_file_content" => "Grep",
        other => other,
    }
}

pub fn render(event: &str, reply: &Reply) -> Option<String> {
    match (event, reply) {
        ("PreToolUse", Reply::Rewrite(r)) => {
            let mut out = json!({ "hookSpecificOutput": { "hookEventName": "BeforeTool", "tool_input": { "command": r.command } } });
            if r.approve {
                out["decision"] = "allow".into();
            }
            Some(out.to_string())
        }
        ("SessionStart", Reply::Context(text)) => Some(
            json!({ "hookSpecificOutput": { "hookEventName": "SessionStart", "additionalContext": text } }).to_string(),
        ),
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
            "hook_event_name": "BeforeTool", "session_id": "g1", "cwd": "/repo", "transcript_path": "",
            "tool_name": "run_shell_command", "tool_input": { "command": "cargo test", "is_background": false }
        });
        let got = normalize(&raw).unwrap();
        assert_eq!((got["hook_event_name"].as_str(), got["tool_name"].as_str()), (Some("PreToolUse"), Some("Bash")));
        assert_eq!(got["tool_input"]["command"], "cargo test");
        assert_eq!(got["tool_input"]["run_in_background"], false);
        assert!(got["transcript_path"].is_null(), "an empty path means none");
    }

    #[test]
    fn the_turn_and_prompt_events_map_to_theirs() {
        let stop = json!({ "hook_event_name": "AfterAgent", "session_id": "g1", "prompt_response": "Done." });
        assert_eq!(normalize(&stop).unwrap()["last_assistant_message"], "Done.");
        let ask = json!({ "hook_event_name": "BeforeAgent", "session_id": "g1", "prompt": "fix it" });
        assert_eq!(normalize(&ask).unwrap()["prompt"], "fix it");
        assert!(normalize(&json!({ "hook_event_name": "BeforeModel", "session_id": "g1" })).is_none());
    }

    #[test]
    fn replies_in_gemini_shape() {
        let rewrite = Reply::Rewrite(Rewritten { command: "relay x -- 'cargo test'".into(), approve: true });
        let out: Value = serde_json::from_str(&render("PreToolUse", &rewrite).unwrap()).unwrap();
        assert_eq!(out["decision"], "allow");
        assert_eq!(out["hookSpecificOutput"]["tool_input"]["command"], "relay x -- 'cargo test'");
        let ctx: Value = serde_json::from_str(&render("SessionStart", &Reply::Context("b".into())).unwrap()).unwrap();
        assert_eq!(ctx["hookSpecificOutput"]["additionalContext"], "b");
        assert_eq!(render("PreToolUse", &Reply::Nothing), None);
    }
}
