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

    pub fn tool_use(&self, input: &Value) -> Result<()> {
        self.append("tool", input["tool_use_id"].as_str(), tool_data(input))
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
