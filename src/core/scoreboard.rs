//! Who spent the context, from what the hooks saw: per session and per
//! subagent, the tool output each read (estimated), where its transcript
//! is for the exact numbers, the biggest reads, and what relay kept out.
//! Twenty subagents once spent about 9M tokens between them, 300k to
//! 650k each; this is where that shows up by name.

use crate::core::spool::Event;

/// One consumer of context: a session's main thread or one of its
/// subagents.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Agent {
    /// Empty for the main thread.
    pub id: String,
    /// The subagent's type (`Explore`, `general-purpose`, a custom name).
    pub kind: String,
    /// Tool output it received, estimated.
    pub tool_tokens: usize,
    pub calls: usize,
    /// The harness's own transcript, for exact API usage.
    pub transcript: Option<String>,
}

/// One read, for the list of the biggest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Read {
    pub file: String,
    pub tokens: usize,
    /// The subagent that read it; empty for the main thread.
    pub agent: String,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Tally {
    pub session: String,
    pub harness: String,
    /// The first thing the session was asked.
    pub label: String,
    /// The main thread first, then subagents in the order they started.
    pub agents: Vec<Agent>,
    pub reads: Vec<Read>,
    /// Whole-file reads the read guard turned down: tokens the file would
    /// have cost, and tokens of the outline shown instead.
    pub guarded: Vec<(usize, usize)>,
}

impl Tally {
    fn agent(&mut self, id: &str) -> &mut Agent {
        let i = self.agents.iter().position(|a| a.id == id).unwrap_or_else(|| {
            self.agents.push(Agent { id: id.to_string(), ..Agent::default() });
            self.agents.len() - 1
        });
        &mut self.agents[i]
    }
}

fn text(e: &Event, key: &str) -> String {
    e.data[key].as_str().unwrap_or("").to_string()
}

fn number(e: &Event, key: &str) -> usize {
    e.data[key].as_u64().and_then(|n| usize::try_from(n).ok()).unwrap_or(0)
}

pub fn tally(session: &str, events: &[Event]) -> Tally {
    let mut t = Tally { session: session.to_string(), ..Tally::default() };
    t.agent("");
    for e in events {
        match e.event.as_str() {
            "session_start" => {
                if t.harness.is_empty() {
                    t.harness = text(e, "harness");
                }
                if let Some(p) = e.data["transcript_path"].as_str() {
                    t.agents[0].transcript = Some(p.to_string());
                }
            }
            "prompt" if t.label.is_empty() => {
                t.label = text(e, "text").split_whitespace().collect::<Vec<_>>().join(" ");
            }
            "subagent_start" | "subagent_stop" => {
                let a = t.agent(&text(e, "agent"));
                if a.kind.is_empty() {
                    a.kind = text(e, "type");
                }
                if let Some(p) = e.data["transcript_path"].as_str() {
                    a.transcript = Some(p.to_string());
                }
            }
            "tool" => {
                let (agent, tokens) = (text(e, "agent"), number(e, "tokens"));
                let a = t.agent(&agent);
                a.tool_tokens += tokens;
                a.calls += 1;
                if text(e, "tool") == "Read" {
                    t.reads.push(Read { file: text(e, "file"), tokens, agent });
                }
            }
            "read_guarded" => t.guarded.push((number(e, "tokens"), number(e, "shown"))),
            _ => {}
        }
    }
    t
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    fn ev(event: &str, data: Value) -> Event {
        Event::new("s", event, None, data)
    }

    #[test]
    fn each_subagent_gets_its_own_line() {
        let events = vec![
            ev("session_start", json!({ "harness": "claude-code", "transcript_path": "/t/s.jsonl" })),
            ev("prompt", json!({ "text": "implement\nT-253" })),
            ev("tool", json!({ "tool": "Read", "file": "/r/plan.md", "tokens": 9000 })),
            ev("subagent_start", json!({ "agent": "a1", "type": "Explore" })),
            ev("tool", json!({ "tool": "Read", "file": "/r/a.rs", "tokens": 400, "agent": "a1" })),
            ev("tool", json!({ "tool": "Bash", "tokens": 100, "agent": "a1" })),
            ev("read_guarded", json!({ "file": "docs/plan.md", "tokens": 90_000, "shown": 2000, "agent": "a1" })),
            ev("subagent_stop", json!({ "agent": "a1", "type": "Explore", "transcript_path": "/t/sub/a1.jsonl" })),
        ];
        let t = tally("s", &events);
        assert_eq!((t.harness.as_str(), t.label.as_str()), ("claude-code", "implement T-253"));
        assert_eq!(t.agents.len(), 2);
        assert_eq!((t.agents[0].tool_tokens, t.agents[0].transcript.as_deref()), (9000, Some("/t/s.jsonl")));
        let sub = &t.agents[1];
        assert_eq!((sub.kind.as_str(), sub.tool_tokens, sub.calls), ("Explore", 500, 2));
        assert_eq!(sub.transcript.as_deref(), Some("/t/sub/a1.jsonl"));
        assert_eq!(t.reads[1], Read { file: "/r/a.rs".into(), tokens: 400, agent: "a1".into() });
        assert_eq!(t.guarded, [(90_000, 2000)]);
    }
}
