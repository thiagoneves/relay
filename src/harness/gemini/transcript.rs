//! Gemini CLI keeps each session as one JSON document, rewritten as the
//! session goes: `{messages: [{type: user|gemini|info, content, …}]}`.
//! The handoff wants the closing text of each turn.

use std::path::Path;

use serde_json::Value;

use crate::core::handoff::Tail;
use crate::harness::jsonl;
use crate::limits;

pub fn tail(path: &Path) -> Option<Tail> {
    let doc: Value = serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()?;
    Some(Tail { replies: jsonl::last(turn_ends(&doc), limits::handoff::REPLIES), ..Tail::default() })
}

/// The last `gemini` message before each `user` one, and at the end.
fn turn_ends(doc: &Value) -> Vec<String> {
    let mut ends = Vec::new();
    let mut last: Option<String> = None;
    for m in doc["messages"].as_array().into_iter().flatten() {
        match m["type"].as_str() {
            Some("gemini") => {
                last = m["content"].as_str().map(str::to_string).filter(|t| !t.trim().is_empty()).or(last);
            }
            Some("user") => ends.extend(last.take()),
            _ => {}
        }
    }
    ends.extend(last);
    ends
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_turn_ends_with_its_last_reply() {
        let doc = serde_json::json!({ "messages": [
            { "type": "user", "content": [{ "text": "fix" }] },
            { "type": "gemini", "content": "Looking." },
            { "type": "gemini", "content": "Fixed the build." },
            { "type": "user", "content": [{ "text": "ship" }] },
            { "type": "gemini", "content": "" },
            { "type": "gemini", "content": "Shipped." },
            { "type": "info", "content": "saved" }
        ]});
        assert_eq!(turn_ends(&doc), ["Fixed the build.", "Shipped."]);
    }
}
