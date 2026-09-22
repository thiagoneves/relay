//! What a Claude Code transcript adds to the handoff: the closing text of
//! each turn, the answers the user gave to `AskUserQuestion`, and the last
//! plan approved through `ExitPlanMode`. Runs in `SessionEnd`, so lines are
//! filtered by substring before any JSON is parsed.

use std::path::Path;

use serde_json::Value;

use crate::core::handoff::Tail;
use crate::harness::jsonl::{self, last};
use crate::limits;

pub fn read(path: &Path) -> Option<Tail> {
    Some(read_lines(jsonl::tail_lines(path, limits::store::TRANSCRIPT_TAIL_BYTES)?))
}

fn read_lines(lines: impl Iterator<Item = String>) -> Tail {
    let mut tail = Tail::default();
    let mut turn_end: Option<String> = None;
    for line in lines {
        let prompt = line.contains("\"type\":\"user\"") && !line.contains("\"tool_result\"");
        let reply = line.contains("\"type\":\"assistant\"")
            && (line.contains("\"type\":\"text\"") || line.contains("\"ExitPlanMode\""));
        let answers = line.contains("\"answers\"");
        if !(prompt || reply || answers) {
            continue;
        }
        let Ok(v) = serde_json::from_str::<Value>(&line) else { continue };
        if v["isSidechain"] == true {
            continue;
        }
        if answers {
            decisions(&v["toolUseResult"], &mut tail.decisions);
        }
        if prompt && is_prompt(&v) {
            tail.replies.extend(turn_end.take());
        }
        if reply {
            for b in v["message"]["content"].as_array().into_iter().flatten() {
                match b["type"].as_str() {
                    Some("text") => turn_end = b["text"].as_str().map(str::to_string).filter(|t| !t.trim().is_empty()),
                    Some("tool_use") if b["name"] == "ExitPlanMode" => {
                        tail.plan = b["input"]["plan"].as_str().map(str::to_string);
                    }
                    _ => {}
                }
            }
        }
    }
    tail.replies.extend(turn_end);
    tail.replies = last(tail.replies, limits::handoff::REPLIES);
    tail.decisions = last(tail.decisions, limits::handoff::DECISIONS);
    tail
}

/// A message the user typed, not a slash command, its output, a reminder
/// the harness injected, or the summary that opens a compacted session.
fn is_prompt(v: &Value) -> bool {
    if v["isMeta"] == true || v["isCompactSummary"] == true {
        return false;
    }
    let text = jsonl::text_of(&v["message"]["content"]);
    let t = text.trim_start();
    !t.is_empty() && !t.starts_with('<')
}

/// `header: answer` for each question, the recommendation marker dropped.
fn decisions(result: &Value, into: &mut Vec<String>) {
    let Some(answers) = result["answers"].as_object() else { return };
    for q in result["questions"].as_array().into_iter().flatten() {
        let Some(question) = q["question"].as_str() else { continue };
        let Some(answer) = answers.get(question).and_then(Value::as_str) else { continue };
        let topic = q["header"].as_str().unwrap_or(question);
        into.push(format!("{topic}: {}", answer.trim_end_matches(" (Recommended)")));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_how_each_turn_ended_and_what_the_user_decided() {
        let dir = std::env::temp_dir().join(format!("relay-tail-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let lines = [
            r#"{"type":"user","message":{"role":"user","content":"fix the build"}}"#,
            r#"{"type":"assistant","message":{"content":[{"type":"text","text":"Looking."},{"type":"tool_use","id":"t1","name":"Bash","input":{}}]}}"#,
            r#"{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"t1","content":"ok"}]}}"#,
            r#"{"type":"assistant","message":{"content":[{"type":"text","text":"Build fixed."}]}}"#,
            r#"{"type":"user","message":{"content":"<command-name>/clear</command-name>"}}"#,
            r#"{"type":"assistant","isSidechain":true,"message":{"content":[{"type":"text","text":"subagent"}]}}"#,
            r#"{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"q","content":"answered"}]},"toolUseResult":{"questions":[{"question":"Which db?","header":"Database"}],"answers":{"Which db?":"Postgres (Recommended)"}}}"#,
            r#"{"type":"user","message":{"role":"user","content":[{"type":"text","text":"now ship it"}]}}"#,
            r#"{"type":"assistant","message":{"content":[{"type":"tool_use","id":"p","name":"ExitPlanMode","input":{"plan":"1. tag"}}]}}"#,
            r#"{"type":"assistant","message":{"content":[{"type":"text","text":"Shipped v1."}]}}"#,
        ];
        std::fs::write(dir.join("s.jsonl"), lines.join("\n")).unwrap();
        let t = read(&dir.join("s.jsonl")).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(t.replies, ["Build fixed.", "Shipped v1."]);
        assert_eq!(t.decisions, ["Database: Postgres"]);
        assert_eq!(t.plan.as_deref(), Some("1. tag"));
    }
}
