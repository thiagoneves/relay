//! What Claude Code and Codex transcripts have in common: JSONL files in
//! a directory tree, tool results that are a string or a list of text
//! parts, and sessions tagged with the directory they started in.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use serde_json::Value;

/// Closing messages a handoff keeps, newest last.
pub const MAX_REPLIES: usize = 8;

/// Every `.jsonl` file under `dir`, recursively.
pub fn files_under(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    collect(dir, &mut out);
    out
}

fn collect(dir: &Path, into: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            collect(&p, into);
        } else if p.extension().is_some_and(|x| x == "jsonl") {
            into.push(p);
        }
    }
}

/// The first `n` lines of a file.
pub fn head_lines(path: &Path, n: usize) -> Option<impl Iterator<Item = String>> {
    Some(BufReader::new(std::fs::File::open(path).ok()?).lines().map_while(Result::ok).take(n))
}

/// The first line of a file, where Codex puts session metadata.
pub fn first_line(path: &Path) -> Option<String> {
    head_lines(path, 1)?.next()
}

/// A tool result's text: a plain string, a list of `{text}` parts, or an
/// object carrying `content`.
pub fn text_of(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Array(parts) => parts.iter().filter_map(|p| p["text"].as_str()).collect::<Vec<_>>().join(""),
        other => other["content"].as_str().unwrap_or("").to_string(),
    }
}

/// The last `n` items, in order.
pub fn last<T>(mut v: Vec<T>, n: usize) -> Vec<T> {
    v.split_off(v.len().saturating_sub(n))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_of_reads_every_result_shape() {
        assert_eq!(text_of(&serde_json::json!("a")), "a");
        assert_eq!(text_of(&serde_json::json!([{ "text": "a" }, { "text": "b" }])), "ab");
        assert_eq!(text_of(&serde_json::json!({ "content": "c" })), "c");
        assert_eq!(text_of(&serde_json::json!(3)), "");
    }

    #[test]
    fn last_keeps_the_newest() {
        assert_eq!(last(vec![1, 2, 3], 2), [2, 3]);
        assert_eq!(last(vec![1], 5), [1]);
    }
}
