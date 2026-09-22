//! Context spent per session, from the spool. The number memory has to
//! move is orientation: tokens of tool output the agent read before its
//! first edit. Measured on real history it has a median of ~19k per
//! session, five times what output compression saves; comparing sessions
//! that started with a brief against those that did not says whether the
//! brief pays for itself.

use serde_json::Value;

use crate::core::spool::Event;
use crate::helpers::est_tokens;

pub const EDIT_TOOLS: &[&str] = &["Write", "Edit", "MultiEdit", "NotebookEdit"];

pub fn is_edit(tool: &str) -> bool {
    EDIT_TOOLS.contains(&tool)
}

/// Tokens of every string inside a tool response: the text the agent got.
pub fn response_tokens(v: &Value) -> usize {
    match v {
        Value::String(s) => est_tokens(s),
        Value::Array(a) => a.iter().map(response_tokens).sum(),
        Value::Object(o) => o.values().map(response_tokens).sum(),
        _ => 0,
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct SessionUsage {
    /// `None` for sessions recorded before relay measured the brief.
    pub brief_tokens: Option<usize>,
    pub tool_tokens: usize,
    pub orientation_tokens: usize,
    pub edited: bool,
}

pub fn of(events: &[Event]) -> SessionUsage {
    let mut u = SessionUsage::default();
    for e in events {
        match e.event.as_str() {
            "session_start" if u.brief_tokens.is_none() => {
                u.brief_tokens = e.data["brief_tokens"].as_u64().and_then(|n| usize::try_from(n).ok());
            }
            "tool" => {
                if e.data["tool"].as_str().is_some_and(is_edit) {
                    u.edited = true;
                }
                let t = e.data["tokens"].as_u64().and_then(|n| usize::try_from(n).ok()).unwrap_or(0);
                u.tool_tokens += t;
                if !u.edited {
                    u.orientation_tokens += t;
                }
            }
            _ => {}
        }
    }
    u
}

/// Exact API usage of one session, read from the harness's transcript.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ApiUsage {
    pub calls: usize,
    /// Context of the first call: everything sent before any work.
    pub first_context: usize,
    pub peak_context: usize,
    /// Sum of the context of every call; each call resends the history.
    pub context_sent: usize,
    /// Part of `context_sent` served from the provider's prompt cache.
    pub cached: usize,
    pub output: usize,
}

impl ApiUsage {
    pub fn add_call(&mut self, context: usize, cached: usize, output: usize) {
        if self.calls == 0 {
            self.first_context = context;
        }
        self.calls += 1;
        self.peak_context = self.peak_context.max(context);
        self.context_sent += context;
        self.cached += cached;
        self.output += output;
    }
}

pub fn median(mut v: Vec<usize>) -> Option<usize> {
    v.sort_unstable();
    v.get(v.len() / 2).copied()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ev(event: &str, data: Value) -> Event {
        Event::new("s", event, None, data)
    }

    #[test]
    fn orientation_stops_at_the_first_edit() {
        let events = vec![
            ev("session_start", json!({ "brief_tokens": 300 })),
            ev("tool", json!({ "tool": "Read", "tokens": 1000 })),
            ev("tool", json!({ "tool": "Bash", "tokens": 200 })),
            ev("tool", json!({ "tool": "Edit", "file": "a.rs" })),
            ev("tool", json!({ "tool": "Read", "tokens": 50 })),
        ];
        let u = of(&events);
        assert_eq!(
            u,
            SessionUsage { brief_tokens: Some(300), tool_tokens: 1250, orientation_tokens: 1200, edited: true }
        );
    }

    #[test]
    fn counts_every_string_in_a_response() {
        let r = json!({ "file": { "content": "hello world", "numLines": 1 }, "extra": ["hello world"] });
        assert_eq!(response_tokens(&r), 2 * est_tokens("hello world"));
    }

    #[test]
    fn median_of_empty_is_none() {
        assert_eq!(median(vec![]), None);
        assert_eq!(median(vec![5, 1, 9]), Some(5));
    }
}
