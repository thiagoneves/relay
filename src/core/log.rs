//! relay's own failure log. Hooks fail open, so a failure must never reach
//! the harness; it lands here instead, where `relay status` counts it and
//! `relay log` shows it.

use std::io::Write;
use std::time::SystemTime;

use crate::core::paths::Paths;
use crate::helpers::{iso, now_iso};
use crate::limits::store::LOG_BYTES;

/// One logged failure: when, and what.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub at: String,
    pub what: String,
}

/// Append a failure. Never fails loudly: it runs where nothing can report.
pub fn write(paths: &Paths, what: &str) {
    if std::fs::create_dir_all(&paths.local).is_err() {
        return;
    }
    let file = paths.log_file();
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&file) {
        let _ = writeln!(f, "{} {}", now_iso(), what.replace('\n', " "));
    }
    if std::fs::metadata(&file).is_ok_and(|m| m.len() > LOG_BYTES) {
        keep_newest_half(&file);
    }
}

fn keep_newest_half(file: &std::path::Path) {
    let Ok(text) = std::fs::read_to_string(file) else { return };
    let lines: Vec<&str> = text.lines().collect();
    let kept = lines[lines.len() / 2..].join("\n") + "\n";
    let _ = crate::helpers::write_atomic(file, kept.as_bytes());
}

/// Every logged failure, oldest first.
pub fn entries(paths: &Paths) -> Vec<Entry> {
    std::fs::read_to_string(paths.log_file()).map(|t| parse(&t)).unwrap_or_default()
}

/// Failures logged at or after `since`.
pub fn since(paths: &Paths, since: SystemTime) -> Vec<Entry> {
    let from = iso(since);
    entries(paths).into_iter().filter(|e| e.at >= from).collect()
}

fn parse(text: &str) -> Vec<Entry> {
    text.lines()
        .filter_map(|l| l.split_once(' '))
        .map(|(at, what)| Entry { at: at.to_string(), what: what.to_string() })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entries_split_time_from_message() {
        let got = parse("2026-09-22T10:00:00Z hook claude error: boom\n\n2026-09-23T10:00:00Z store failed: x\n");
        assert_eq!(got.len(), 2);
        assert_eq!(got[0], Entry { at: "2026-09-22T10:00:00Z".into(), what: "hook claude error: boom".into() });
    }

    #[test]
    fn a_full_log_keeps_its_newest_half() {
        let file = std::env::temp_dir().join(format!("relay-log-trim-{}.log", std::process::id()));
        std::fs::write(&file, "1\n2\n3\n4\n").unwrap();
        keep_newest_half(&file);
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "3\n4\n");
        let _ = std::fs::remove_file(&file);
    }
}
