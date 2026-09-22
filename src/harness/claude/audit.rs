//! Where a Claude Code session's context went, from its transcript.
//!
//! Claude Code records every block it adds to the model's context as an
//! `attachment` line (instruction files, skill and agent listings, MCP
//! instructions, deferred tool names, hook output) and every tool result
//! as a `user` line. A block stays in the context and is resent on each
//! later API call, so its cost on the quota is its size times the calls
//! that followed it. Sizes are estimates; call counts and totals are the
//! exact `message.usage` numbers.

use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader};
use std::path::Path;

use serde_json::Value;

use crate::core::audit::{Collector, Origin, SessionAudit};
use crate::harness::jsonl;
use crate::helpers::env::tilde;

/// Hook events whose stdout Claude Code adds to the model's context.
const INJECTING_HOOKS: &[&str] = &["SessionStart", "UserPromptSubmit"];

pub fn session(path: &Path) -> Option<SessionAudit> {
    let f = std::fs::File::open(path).ok()?;
    let mut col = Collector::default();
    col.audit.skills_tracked = true;
    col.audit.subagent = path.parent().is_some_and(|d| d.ends_with("subagents"));
    let mut seen_msgs = HashSet::new();
    let mut tool_names: HashMap<String, String> = HashMap::new();

    for line in BufReader::new(f).lines().map_while(Result::ok) {
        let Ok(v) = serde_json::from_str::<Value>(&line) else { continue };
        if v["subtype"] == "compact_boundary" {
            col.compacted();
            continue;
        }
        if let Some(a) = v.get("attachment") {
            attachment(a, &mut col);
            continue;
        }
        let m = &v["message"];
        if let Some(u) = m.get("usage").filter(|u| u.is_object())
            && m["id"].as_str().is_some_and(|id| seen_msgs.insert(id.to_string()))
        {
            let n = |k: &str| u[k].as_u64().and_then(|x| usize::try_from(x).ok()).unwrap_or(0);
            let cached = n("cache_read_input_tokens");
            col.call(n("input_tokens") + cached + n("cache_creation_input_tokens"), cached, n("output_tokens"));
        }
        if let Some(prompt) = m["content"].as_str().filter(|_| m["role"] == "user") {
            col.add("Conversation: your messages", Origin::Work, prompt);
        }
        let Some(content) = m["content"].as_array() else { continue };
        for b in content {
            match b["type"].as_str() {
                Some("text") if m["role"] == "assistant" => {
                    col.add("Conversation: agent replies", Origin::Work, b["text"].as_str().unwrap_or(""));
                }
                Some("text") if m["role"] == "user" => {
                    col.add("Conversation: your messages", Origin::Work, b["text"].as_str().unwrap_or(""));
                }
                Some("tool_use") => {
                    col.add("Conversation: agent tool calls", Origin::Work, &b["input"].to_string());
                    if b["name"] == "Skill"
                        && let Some(skill) = b["input"]["skill"].as_str()
                    {
                        col.audit.skills_invoked.insert(skill.to_string());
                    }
                    if let (Some(id), Some(name)) = (b["id"].as_str(), b["name"].as_str()) {
                        tool_names.insert(id.to_string(), tool_label(name));
                    }
                }
                Some("tool_result") => {
                    let name = b["tool_use_id"].as_str().and_then(|id| tool_names.get(id)).cloned();
                    let name = name.unwrap_or_else(|| "tool".into());
                    col.add(format!("Tool results: {name}"), Origin::Work, &jsonl::text_of(&b["content"]));
                }
                _ => {}
            }
        }
    }
    col.finish()
}

fn attachment(a: &Value, col: &mut Collector) {
    let kind = a["type"].as_str().unwrap_or("");
    match kind {
        "instructions" => {
            for f in a["files"].as_array().into_iter().flatten() {
                let path = tilde(Path::new(f["path"].as_str().unwrap_or("?")));
                col.add(format!("Instruction file {path}"), Origin::Config, f["content"].as_str().unwrap_or(""));
            }
        }
        "nested_memory" => {
            let path = tilde(Path::new(a["path"].as_str().unwrap_or("?")));
            col.add(format!("Instruction file {path}"), Origin::Config, a["content"].as_str().unwrap_or(""));
        }
        "skill_listing" => {
            let names = strings(&a["names"]);
            let count = a["skillCount"].as_u64().and_then(|n| usize::try_from(n).ok()).unwrap_or(names.len());
            col.audit.skills_listed.extend(names);
            col.add_listing("Skills listing", Origin::Config, a["content"].as_str().unwrap_or(""), count);
        }
        "invoked_skills" => {
            for s in a["skills"].as_array().into_iter().flatten() {
                if let Some(n) = s.as_str().or_else(|| s["name"].as_str()) {
                    col.audit.skills_invoked.insert(n.to_string());
                }
            }
        }
        "agent_listing_delta" => {
            let lines = strings(&a["addedLines"]);
            col.add_listing("Agent types listing", Origin::Config, &lines.join("\n"), lines.len());
        }
        "mcp_instructions_delta" => {
            let names = strings(&a["addedNames"]);
            let text = strings(&a["addedBlocks"]).join("\n");
            col.audit.mcp_servers.extend(names.iter().cloned());
            let count = col.audit.mcp_servers.len();
            col.add_listing("MCP server instructions", Origin::Config, &text, count);
        }
        "deferred_tools_delta" => {
            let lines = strings(&a["addedLines"]);
            col.add_listing("Deferred tool names (MCP and built-in)", Origin::Config, &lines.join("\n"), lines.len());
        }
        "hook_success" | "hook_additional_context" | "hook_system_message" => {
            let event = a["hookEvent"].as_str().unwrap_or("");
            if kind == "hook_success" && !INJECTING_HOOKS.contains(&event) {
                return;
            }
            let who =
                a["command"].as_str().map_or_else(|| a["hookName"].as_str().unwrap_or("?").to_string(), short_cmd);
            col.add(format!("Hook output on {event}: {who}"), Origin::Config, a["content"].as_str().unwrap_or(""));
        }
        "hook_non_blocking_error" => {
            let why = a["stderr"].as_str().unwrap_or("").lines().next().unwrap_or("");
            col.hook_error(a["command"].as_str().unwrap_or("?"), why);
        }
        "prompt_snapshot" => {
            col.add_pinned("Claude Code system prompt", Origin::Harness, a["systemPrompt"].as_str().unwrap_or(""));
        }
        _ => {}
    }
}

/// Every `command` of every hook in a settings or plugin hooks file.
pub fn hook_commands(file: &Path) -> Vec<String> {
    let Ok(text) = std::fs::read_to_string(file) else { return Vec::new() };
    let Ok(v) = serde_json::from_str::<Value>(&text) else { return Vec::new() };
    let mut out = Vec::new();
    for groups in v["hooks"].as_object().into_iter().flat_map(|o| o.values()) {
        for group in groups.as_array().into_iter().flatten() {
            for h in group["hooks"].as_array().into_iter().flatten() {
                if let Some(c) = h["command"].as_str() {
                    out.push(c.to_string());
                }
            }
        }
    }
    out
}

/// `hooks/hooks.json` of every installed plugin.
pub fn plugin_hook_files(installed: &Path) -> Vec<std::path::PathBuf> {
    let Ok(text) = std::fs::read_to_string(installed) else { return Vec::new() };
    let Ok(v) = serde_json::from_str::<Value>(&text) else { return Vec::new() };
    v["plugins"]
        .as_object()
        .into_iter()
        .flat_map(|o| o.values())
        .flat_map(|installs| installs.as_array().cloned().unwrap_or_default())
        .filter_map(|i| i["installPath"].as_str().map(|p| Path::new(p).join("hooks/hooks.json")))
        .collect()
}

/// `Bash`, `Read`, or `MCP <server>` for MCP tools, which differ only in suffix.
fn tool_label(name: &str) -> String {
    match name.strip_prefix("mcp__") {
        Some(rest) => format!("MCP {}", rest.split("__").next().unwrap_or(rest)),
        None => name.to_string(),
    }
}

fn strings(v: &Value) -> Vec<String> {
    v.as_array().into_iter().flatten().filter_map(|x| x.as_str().map(str::to_string)).collect()
}

fn short_cmd(cmd: &str) -> String {
    crate::helpers::truncate_chars(cmd.split_whitespace().collect::<Vec<_>>().join(" ").as_str(), 70)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_hook_commands_from_settings() {
        let dir = std::env::temp_dir().join(format!("relay-hookcmds-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let settings = r#"{"hooks":{"PreToolUse":[{"matcher":"Bash","hooks":[{"type":"command","command":"a"}]}],"Stop":[{"hooks":[{"type":"command","command":"b"}]}]}}"#;
        std::fs::write(dir.join("s.json"), settings).unwrap();
        let mut cmds = hook_commands(&dir.join("s.json"));
        cmds.sort();
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(cmds, ["a", "b"]);
    }

    #[test]
    fn blocks_cost_their_size_times_the_calls_after_them() {
        let dir = std::env::temp_dir().join(format!("relay-audit-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let call =
            |id: &str| format!(r#"{{"message":{{"id":"{id}","usage":{{"input_tokens":1000,"output_tokens":1}}}}}}"#);
        let lines = [
            r#"{"attachment":{"type":"skill_listing","content":"alpha beta gamma delta","skillCount":2,"names":["a","b"]}}"#.to_string(),
            call("m1"),
            r#"{"attachment":{"type":"hook_success","hookEvent":"UserPromptSubmit","command":"noisy-hook","content":"lots of words here"}}"#.to_string(),
            r#"{"attachment":{"type":"hook_success","hookEvent":"PreToolUse","command":"quiet","content":"not injected"}}"#.to_string(),
            r#"{"attachment":{"type":"hook_non_blocking_error","command":"rtk hook claude","stderr":"rtk: command not found"}}"#.to_string(),
            r#"{"attachment":{"type":"invoked_skills","skills":[{"name":"a"}]}}"#.to_string(),
            call("m2"),
            call("m3"),
        ];
        std::fs::write(dir.join("s.jsonl"), lines.join("\n")).unwrap();
        let a = session(&dir.join("s.jsonl")).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();

        let cost = |k: &str| a.costs.iter().find(|c| c.source.starts_with(k)).unwrap_or_else(|| panic!("{k}"));
        let skills = cost("Skills listing");
        assert_eq!((skills.items, skills.resent), (2, skills.tokens * 3));
        let hook = cost("Hook output on UserPromptSubmit");
        assert_eq!(hook.resent, hook.tokens * 2);
        assert!(!a.costs.iter().any(|c| c.source.contains("quiet")));
        assert_eq!(a.hook_errors[0].count, 1);
        assert_eq!(a.skills_invoked.iter().collect::<Vec<_>>(), vec!["a"]);
        assert_eq!(a.usage.calls, 3);
    }
}
