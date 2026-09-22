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
    let mut walk = Walk { col: Collector::default(), seen_msgs: HashSet::new(), tool_names: HashMap::new() };
    walk.col.audit.skills_tracked = true;
    walk.col.audit.subagent = path.parent().is_some_and(|d| d.ends_with("subagents"));
    for line in BufReader::new(f).lines().map_while(Result::ok) {
        if let Ok(v) = serde_json::from_str::<Value>(&line) {
            walk.line(&v);
        }
    }
    walk.col.finish()
}

/// One pass over a transcript, in order.
struct Walk {
    col: Collector,
    /// A message is split over several lines that repeat its usage.
    seen_msgs: HashSet<String>,
    /// `tool_use` id → label, to name the result that answers it.
    tool_names: HashMap<String, String>,
}

impl Walk {
    fn line(&mut self, v: &Value) {
        if v["subtype"] == "compact_boundary" {
            self.col.compacted();
            return;
        }
        if let Some(a) = v.get("attachment") {
            attachment(a, &mut self.col);
            return;
        }
        let m = &v["message"];
        self.usage(m);
        if let Some(prompt) = m["content"].as_str().filter(|_| m["role"] == "user") {
            self.col.add("Conversation: your messages", Origin::Work, prompt);
        }
        for b in m["content"].as_array().into_iter().flatten() {
            self.block(b, m["role"].as_str().unwrap_or(""));
        }
    }

    fn usage(&mut self, m: &Value) {
        let Some(u) = m.get("usage").filter(|u| u.is_object()) else { return };
        if !m["id"].as_str().is_some_and(|id| self.seen_msgs.insert(id.to_string())) {
            return;
        }
        let n = |k: &str| jsonl::count(&u[k]).unwrap_or(0);
        let cached = n("cache_read_input_tokens");
        self.col.call(n("input_tokens") + cached + n("cache_creation_input_tokens"), cached, n("output_tokens"));
    }

    fn block(&mut self, b: &Value, role: &str) {
        match (b["type"].as_str(), role) {
            (Some("text"), "assistant") => {
                self.col.add("Conversation: agent replies", Origin::Work, b["text"].as_str().unwrap_or(""));
            }
            (Some("text"), "user") => {
                self.col.add("Conversation: your messages", Origin::Work, b["text"].as_str().unwrap_or(""));
            }
            (Some("tool_use"), _) => self.tool_use(b),
            (Some("tool_result"), _) => {
                let name = b["tool_use_id"].as_str().and_then(|id| self.tool_names.get(id)).cloned();
                let name = name.unwrap_or_else(|| "tool".into());
                self.col.add(format!("Tool results: {name}"), Origin::Work, &jsonl::text_of(&b["content"]));
            }
            _ => {}
        }
    }

    fn tool_use(&mut self, b: &Value) {
        self.col.add("Conversation: agent tool calls", Origin::Work, &b["input"].to_string());
        if b["name"] == "Skill"
            && let Some(skill) = b["input"]["skill"].as_str()
        {
            self.col.audit.skills_invoked.insert(skill.to_string());
        }
        if let (Some(id), Some(name)) = (b["id"].as_str(), b["name"].as_str()) {
            self.tool_names.insert(id.to_string(), tool_label(name));
        }
    }
}

fn attachment(a: &Value, col: &mut Collector) {
    let kind = a["type"].as_str().unwrap_or("");
    match kind {
        "instructions" | "nested_memory" => instructions(a, col),
        "skill_listing" | "invoked_skills" => skills(kind, a, col),
        "agent_listing_delta" | "mcp_instructions_delta" | "deferred_tools_delta" => listing(kind, a, col),
        "hook_success" | "hook_additional_context" | "hook_system_message" | "hook_non_blocking_error" => {
            hook(kind, a, col);
        }
        "prompt_snapshot" => {
            col.add_pinned("Claude Code system prompt", Origin::Harness, a["systemPrompt"].as_str().unwrap_or(""));
        }
        _ => {}
    }
}

/// `instructions` lists every file loaded at start; `nested_memory` is one
/// file loaded later, when the agent entered its directory.
fn instructions(a: &Value, col: &mut Collector) {
    let files = a["files"].as_array().map_or_else(|| vec![a], |fs| fs.iter().collect());
    for f in files {
        let path = tilde(Path::new(f["path"].as_str().unwrap_or("?")));
        col.add(format!("Instruction file {path}"), Origin::Config, f["content"].as_str().unwrap_or(""));
    }
}

fn skills(kind: &str, a: &Value, col: &mut Collector) {
    if kind == "invoked_skills" {
        for s in a["skills"].as_array().into_iter().flatten() {
            if let Some(n) = s.as_str().or_else(|| s["name"].as_str()) {
                col.audit.skills_invoked.insert(n.to_string());
            }
        }
        return;
    }
    let names = jsonl::strings(&a["names"]);
    let count = jsonl::count(&a["skillCount"]).unwrap_or(names.len());
    col.audit.skills_listed.extend(names);
    col.add_listing("Skills listing", Origin::Config, a["content"].as_str().unwrap_or(""), count);
}

fn listing(kind: &str, a: &Value, col: &mut Collector) {
    if kind == "mcp_instructions_delta" {
        let text = jsonl::strings(&a["addedBlocks"]).join("\n");
        col.audit.mcp_servers.extend(jsonl::strings(&a["addedNames"]));
        let count = col.audit.mcp_servers.len();
        col.add_listing("MCP server instructions", Origin::Config, &text, count);
        return;
    }
    let source =
        if kind == "agent_listing_delta" { "Agent types listing" } else { "Deferred tool names (MCP and built-in)" };
    let lines = jsonl::strings(&a["addedLines"]);
    col.add_listing(source, Origin::Config, &lines.join("\n"), lines.len());
}

fn hook(kind: &str, a: &Value, col: &mut Collector) {
    if kind == "hook_non_blocking_error" {
        let why = a["stderr"].as_str().unwrap_or("").lines().next().unwrap_or("");
        col.hook_error(a["command"].as_str().unwrap_or("?"), why);
        return;
    }
    let event = a["hookEvent"].as_str().unwrap_or("");
    if kind == "hook_success" && !INJECTING_HOOKS.contains(&event) {
        return;
    }
    let who = a["command"].as_str().map_or_else(|| a["hookName"].as_str().unwrap_or("?").to_string(), short_cmd);
    col.add(format!("Hook output on {event}: {who}"), Origin::Config, a["content"].as_str().unwrap_or(""));
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
