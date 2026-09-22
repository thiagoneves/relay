//! Where a Codex session's context went, from its rollout.
//!
//! Codex puts everything it adds to the context in `response_item`
//! messages: `developer` and `user` blocks opened by a tag or heading
//! (`<skills_instructions>`, `# AGENTS.md instructions`, `<app-context>`),
//! plus tool outputs. API calls come from `token_count` events: a call is
//! counted when `total_token_usage` grows, priced by its `last_token_usage`,
//! where cached input is part of `input_tokens`.

use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::path::Path;

use serde_json::Value;

use crate::core::audit::{Collector, Origin, SessionAudit};

/// Known context blocks by their opening, with a label and who controls them.
const BLOCKS: &[(&str, &str, Origin)] = &[
    ("<skills_instructions>", "Skills listing", Origin::Config),
    ("# AGENTS.md instructions", "Instruction file AGENTS.md", Origin::Config),
    ("<user_instructions>", "Instruction file AGENTS.md", Origin::Config),
    ("<app-context>", "Codex desktop app context", Origin::Harness),
    ("# Computer and Browser Use", "Computer and browser use instructions", Origin::Config),
    (
        "The following is the Codex agent history",
        "Auto-review of approvals (history sent to a reviewer)",
        Origin::Config,
    ),
    ("<recommended_plugins>", "Recommended plugins list", Origin::Harness),
    ("<model_switch>", "Model switch context", Origin::Harness),
    ("<environment_context>", "Environment context", Origin::Harness),
    ("<permissions", "Permissions instructions", Origin::Harness),
    ("<collaboration_mode>", "Collaboration mode instructions", Origin::Harness),
    ("<multi_agent", "Multi-agent instructions", Origin::Harness),
    ("You are `/root`", "Multi-agent instructions", Origin::Harness),
    ("<in-app-browser-context", "In-app browser context", Origin::Harness),
];

pub fn session(path: &Path) -> Option<SessionAudit> {
    let f = std::fs::File::open(path).ok()?;
    let mut col = Collector::default();
    let mut last_total = 0;
    let mut calls: HashMap<String, String> = HashMap::new();

    for line in BufReader::new(f).lines().map_while(Result::ok) {
        let Ok(v) = serde_json::from_str::<Value>(&line) else { continue };
        let p = &v["payload"];
        match (v["type"].as_str(), p["type"].as_str()) {
            (Some("session_meta"), _) => {
                col.audit.subagent = p["parent_thread_id"].as_str().is_some_and(|s| !s.is_empty());
                let text = p["base_instructions"]["text"].as_str().or_else(|| p["base_instructions"].as_str());
                col.add_pinned("Codex system prompt", Origin::Harness, text.unwrap_or(""));
            }
            (Some("event_msg"), Some("token_count")) => {
                let info = &p["info"];
                let n = |u: &Value, k: &str| u[k].as_u64().and_then(|x| usize::try_from(x).ok()).unwrap_or(0);
                let total = n(&info["total_token_usage"], "total_tokens");
                if total > last_total {
                    last_total = total;
                    let last = &info["last_token_usage"];
                    col.call(n(last, "input_tokens"), n(last, "cached_input_tokens"), n(last, "output_tokens"));
                }
            }
            (Some("compacted"), _) | (Some("response_item"), Some("compaction")) => col.compacted(),
            (Some("response_item"), Some("message")) => message(p, &mut col),
            (Some("response_item"), Some("function_call" | "custom_tool_call")) => {
                let args = p["arguments"].as_str().or_else(|| p["input"].as_str()).unwrap_or("");
                col.add("Conversation: agent tool calls", Origin::Work, args);
                if let (Some(id), Some(name)) = (p["call_id"].as_str(), p["name"].as_str()) {
                    calls.insert(id.to_string(), name.to_string());
                }
            }
            (Some("response_item"), Some("function_call_output" | "custom_tool_call_output")) => {
                let name = p["call_id"].as_str().and_then(|id| calls.get(id)).map_or("tool", String::as_str);
                col.add(format!("Tool results: {name}"), Origin::Work, &output_text(&p["output"]));
            }
            _ => {}
        }
    }
    col.finish()
}

fn message(p: &Value, col: &mut Collector) {
    let role = p["role"].as_str().unwrap_or("");
    let text: String = p["content"].as_array().into_iter().flatten().filter_map(|c| c["text"].as_str()).collect();
    let head = text.trim_start();
    if let Some((_, label, origin)) = BLOCKS.iter().find(|(open, _, _)| head.starts_with(open)) {
        if *label == "Skills listing" {
            col.add_listing(*label, *origin, &text, text.lines().filter(|l| l.starts_with("- ")).count());
        } else {
            col.add(*label, *origin, &text);
        }
    } else if role == "developer" {
        // Plugin and hook output (claude-mem, design linters) lands here.
        let first = crate::helpers::truncate_chars(head.lines().next().unwrap_or(""), 50);
        col.add(format!("Hook or plugin output: {first}"), Origin::Config, &text);
    } else if role == "user" {
        col.add("Conversation: your messages", Origin::Work, &text);
    } else if role == "assistant" {
        col.add("Conversation: agent replies", Origin::Work, &text);
    }
}

fn output_text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Array(parts) => parts.iter().filter_map(|p| p["text"].as_str()).collect::<Vec<_>>().join(""),
        other => other["content"].as_str().unwrap_or("").to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_blocks_and_prices_them_by_later_calls() {
        let dir = std::env::temp_dir().join(format!("relay-codex-audit-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let msg = |role: &str, text: &str| {
            format!(
                r#"{{"type":"response_item","payload":{{"type":"message","role":"{role}","content":[{{"type":"input_text","text":"{text}"}}]}}}}"#
            )
        };
        let tc = |total: u32| {
            format!(
                r#"{{"type":"event_msg","payload":{{"type":"token_count","info":{{"total_token_usage":{{"total_tokens":{total}}},"last_token_usage":{{"input_tokens":1000,"cached_input_tokens":0,"output_tokens":1}}}}}}}}"#
            )
        };
        let lines = [
            msg("developer", "<skills_instructions>\\n- a: one\\n- b: two"),
            msg("developer", "# claude-mem status lots of words"),
            tc(10),
            msg("user", "fix the build please"),
            tc(20),
        ];
        std::fs::write(dir.join("r.jsonl"), lines.join("\n")).unwrap();
        let a = session(&dir.join("r.jsonl")).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
        let cost = |k: &str| a.costs.iter().find(|c| c.source.starts_with(k)).unwrap_or_else(|| panic!("{k}"));
        assert_eq!(cost("Skills listing").items, 2);
        assert_eq!(cost("Skills listing").resent, cost("Skills listing").tokens * 2);
        assert_eq!(cost("Hook or plugin output: # claude-mem").origin, Origin::Config);
        assert_eq!(cost("Conversation").resent, cost("Conversation").tokens);
        assert_eq!(a.usage.calls, 2);
    }
}
