//! Shell calls recorded in Claude Code transcripts
//! (`<config>/projects/<project>/<session>.jsonl`): an assistant
//! `tool_use` named `Bash`, answered later by a user `tool_result` with
//! the same id. Read only, for `relay bench --history`.

use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::core::bench::ShellCall;

pub fn shell_calls(dir: &Path) -> Vec<ShellCall> {
    let mut files = Vec::new();
    collect_jsonl(dir, &mut files);
    files.sort();
    let mut calls = Vec::new();
    for f in files {
        read_file(&f, &mut calls);
    }
    calls
}

fn collect_jsonl(dir: &Path, into: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            collect_jsonl(&p, into);
        } else if p.extension().is_some_and(|x| x == "jsonl") {
            into.push(p);
        }
    }
}

fn read_file(path: &Path, calls: &mut Vec<ShellCall>) {
    let Ok(f) = std::fs::File::open(path) else { return };
    let mut pending: HashMap<String, String> = HashMap::new();
    for line in BufReader::new(f).lines().map_while(Result::ok) {
        // Most lines are prose; skip them before paying for a JSON parse.
        if !line.contains("\"tool_use\"") && !line.contains("\"tool_result\"") {
            continue;
        }
        let Ok(v) = serde_json::from_str::<Value>(&line) else { continue };
        let Some(blocks) = v["message"]["content"].as_array() else { continue };
        for b in blocks {
            match b["type"].as_str() {
                Some("tool_use") if b["name"] == "Bash" => {
                    if let (Some(id), Some(cmd)) = (b["id"].as_str(), b["input"]["command"].as_str()) {
                        pending.insert(id.to_string(), cmd.to_string());
                    }
                }
                Some("tool_result") => {
                    if let Some(cmd) = b["tool_use_id"].as_str().and_then(|id| pending.remove(id)) {
                        calls.push(ShellCall { cmd, output: result_text(&b["content"]) });
                    }
                }
                _ => {}
            }
        }
    }
}

fn result_text(content: &Value) -> String {
    match content {
        Value::String(s) => s.clone(),
        Value::Array(parts) => parts.iter().filter_map(|p| p["text"].as_str()).collect::<Vec<_>>().join(""),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pairs_tool_use_with_its_result() {
        let dir = std::env::temp_dir().join(format!("relay-transcript-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("proj")).unwrap();
        let lines = [
            r#"{"type":"user","message":{"content":"hi"}}"#,
            r#"{"type":"assistant","message":{"content":[{"type":"tool_use","id":"t1","name":"Bash","input":{"command":"git status"}}]}}"#,
            r#"{"type":"assistant","message":{"content":[{"type":"tool_use","id":"t2","name":"Read","input":{"file_path":"x"}}]}}"#,
            r#"{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"t2","content":"file"}]}}"#,
            r#"{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"t1","content":[{"type":"text","text":"On branch main"}]}]}}"#,
        ];
        std::fs::write(dir.join("proj/s.jsonl"), lines.join("\n")).unwrap();
        let calls = shell_calls(&dir);
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!((calls[0].cmd.as_str(), calls[0].output.as_str()), ("git status", "On branch main"));
    }
}
