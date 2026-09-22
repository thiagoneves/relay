//! What Claude Code and Codex transcripts have in common: JSONL files in
//! a directory tree, tool results that are a string or a list of text
//! parts, and sessions tagged with the directory they started in.

use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use serde_json::Value;

/// Closing messages a handoff keeps, newest last.
pub const MAX_REPLIES: usize = 8;

/// How far from the end a tail read starts. `SessionEnd` has a 2 s
/// budget and transcripts reach gigabytes; the handoff only needs the
/// last turns.
pub const TAIL_BYTES: u64 = 32 * 1024 * 1024;

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

/// The lines of the last `max_bytes` of a file. When the read starts
/// mid-file, the first (partial) line is dropped.
pub fn tail_lines(path: &Path, max_bytes: u64) -> Option<impl Iterator<Item = String>> {
    let mut f = std::fs::File::open(path).ok()?;
    let len = f.metadata().ok()?.len();
    let start = len.saturating_sub(max_bytes);
    // One byte early, so the skipped piece is empty when the cut falls
    // exactly on a line start.
    f.seek(SeekFrom::Start(start.saturating_sub(1))).ok()?;
    let mut lines = BufReader::new(f).lines().map_while(Result::ok);
    if start > 0 {
        lines.next();
    }
    Some(lines)
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

    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("relay-jsonl-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn tail_read_skips_the_cut_line() {
        let d = scratch("tail");
        let p = d.join("t.jsonl");
        std::fs::write(&p, "first line\nsecond\nthird\n").unwrap();
        let all: Vec<_> = tail_lines(&p, 1 << 20).unwrap().collect();
        assert_eq!(all, ["first line", "second", "third"]);
        // 10 bytes from the end falls inside "second"; 13 is its start.
        let cut: Vec<_> = tail_lines(&p, 10).unwrap().collect();
        assert_eq!(cut, ["third"]);
        let exact: Vec<_> = tail_lines(&p, 13).unwrap().collect();
        assert_eq!(exact, ["second", "third"]);
        let _ = std::fs::remove_dir_all(&d);
    }

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
