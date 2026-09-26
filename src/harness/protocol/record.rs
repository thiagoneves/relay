//! Turns hook events into spool records.

use anyhow::Result;
use serde_json::{Value, json};

use crate::core::paths::Paths;
use crate::core::spool::{self, Event};
use crate::core::usage;
use crate::harness::HarnessId;
use crate::helpers::env::{self, Var};
use crate::helpers::redact::redact;
use crate::helpers::truncate_chars;
use crate::limits::store::{EVENT_COMMAND_CHARS, EVENT_PROMPT_CHARS, EVENT_REPLY_CHARS};

/// Where one hook event's records go.
pub struct Recorder<'a> {
    pub paths: &'a Paths,
    pub session: &'a str,
}

impl Recorder<'_> {
    fn append(&self, name: &str, key: Option<&str>, data: Value) -> Result<()> {
        spool::append(self.paths, &Event::new(self.session, name, key, data))
    }

    pub fn prompt(&self, input: &Value) -> Result<()> {
        let prompt = input["prompt"].as_str().unwrap_or("");
        self.append("prompt", None, json!({ "text": truncate_chars(&redact(prompt), EVENT_PROMPT_CHARS) }))
    }

    pub fn session_start(&self, input: &Value, harness: HarnessId, brief_tokens: usize) -> Result<()> {
        let data = json!({
            "source": input["source"],
            "transcript_path": input["transcript_path"],
            "cwd": input["cwd"],
            "harness": harness.stored(),
            "brief_tokens": brief_tokens,
            "wrapper": env::text(Var::RelayWrapper),
        });
        self.append("session_start", None, data)
    }

    /// An output relay replaced after the harness ran the command, for the
    /// end-of-session check that the model saw it.
    pub fn replaced(&self, tool_use_id: &str, output_id: &str) -> Result<()> {
        self.append(
            "replaced",
            Some(&format!("replaced:{tool_use_id}")),
            json!({ "tool_use_id": tool_use_id, "output": output_id }),
        )
    }

    pub fn tool_use(&self, input: &Value) -> Result<()> {
        self.append("tool", input["tool_use_id"].as_str(), tool_data(input))
    }

    /// A whole-file read relay turned down: what it would have cost and
    /// what the agent read instead, for `relay usage`.
    pub fn guarded_read(&self, input: &Value, g: &crate::core::read_guard::Guarded) -> Result<()> {
        let file = input["tool_input"]["file_path"].as_str().or(input["tool_input"]["absolute_path"].as_str());
        let data = json!({
            "file": file.map(|f| self.paths.rel_file(f)),
            "tokens": g.file_tokens,
            "shown": crate::helpers::est_tokens(&g.message),
            "agent": input["agent_id"],
        });
        let key = input["tool_use_id"].as_str().map(|id| format!("guard:{id}"));
        self.append("read_guarded", key.as_deref(), data)
    }

    pub fn subagent_start(&self, input: &Value, brief_tokens: usize) -> Result<()> {
        let data = json!({ "agent": input["agent_id"], "type": input["agent_type"], "brief_tokens": brief_tokens });
        self.append("subagent_start", None, data)
    }

    /// The subagent's own transcript, where `relay usage` reads what it
    /// really cost.
    pub fn subagent_stop(&self, input: &Value) -> Result<()> {
        let data = json!({
            "agent": input["agent_id"],
            "type": input["agent_type"],
            "transcript_path": input["agent_transcript_path"],
        });
        let key = input["agent_id"].as_str().map(|a| format!("subagent:{a}"));
        self.append("subagent_stop", key.as_deref(), data)
    }

    pub fn session_end(&self, input: &Value) -> Result<()> {
        self.append("session_end", None, json!({ "reason": input["reason"] }))
    }

    pub fn compact(&self, input: &Value) -> Result<()> {
        self.append("compact", None, json!({ "trigger": input["trigger"] }))
    }

    pub fn stop(&self, input: &Value) -> Result<()> {
        let last = input["last_assistant_message"].as_str().map(|s| truncate_chars(&redact(s), EVENT_REPLY_CHARS));
        self.append("stop", None, json!({ "last": last }))
    }
}

/// What a finished tool call leaves in the spool. Edit responses echo the
/// file back to the harness, not to the model, so they cost no tokens.
fn tool_data(input: &Value) -> Value {
    let mut data = tool_fields(input);
    // Calls made inside a subagent say which one, so its cost is its own.
    if let Some(agent) = input["agent_id"].as_str() {
        data["agent"] = agent.into();
    }
    data
}

fn tool_fields(input: &Value) -> Value {
    let tool = input["tool_name"].as_str().unwrap_or("");
    let tokens = if usage::is_edit(tool) { 0 } else { usage::response_tokens(&input["tool_response"]) };
    if tool != "Bash" {
        let file = input["tool_input"]["file_path"].as_str().or(input["tool_input"]["notebook_path"].as_str());
        return json!({ "tool": tool, "file": file, "tokens": tokens });
    }
    let cmd = input["tool_input"]["command"].as_str().unwrap_or("");
    let resp = &input["tool_response"];
    let out = resp["stdout"].as_str().or_else(|| resp.as_str()).unwrap_or("");
    json!({
        "tool": tool,
        "command": truncate_chars(&redact(cmd), EVENT_COMMAND_CHARS),
        "interrupted": resp["interrupted"],
        "tail": truncate_chars(out.trim_end().rsplit('\n').next().unwrap_or(""), 200),
        "tokens": tokens,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bash_records_command_and_last_line() {
        let input = json!({
            "tool_name": "Bash",
            "tool_input": { "command": "cargo test" },
            "tool_response": { "stdout": "a\nb\ntest result: ok\n", "interrupted": false },
        });
        let d = tool_data(&input);
        assert_eq!(d["command"], "cargo test");
        assert_eq!(d["tail"], "test result: ok");
    }

    #[test]
    fn edits_cost_no_tokens() {
        let input = json!({
            "tool_name": "Edit",
            "tool_input": { "file_path": "src/a.rs" },
            "tool_response": { "content": "x".repeat(4000) },
        });
        let d = tool_data(&input);
        assert_eq!(d["file"], "src/a.rs");
        assert_eq!(d["tokens"], 0);
    }
}
