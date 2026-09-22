//! relay's own failure log. Hooks fail open, so a failure must never reach
//! the harness; it lands here instead.

use std::io::Write;

use crate::core::paths::Paths;
use crate::helpers::now_iso;
use crate::limits::store::LOG_BYTES;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_full_log_keeps_its_newest_half() {
        let file = std::env::temp_dir().join(format!("relay-log-trim-{}.log", std::process::id()));
        std::fs::write(&file, "1\n2\n3\n4\n").unwrap();
        keep_newest_half(&file);
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "3\n4\n");
        let _ = std::fs::remove_file(&file);
    }
}
