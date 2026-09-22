//! Did a compressed view keep what an agent needs? Savings without this
//! number are meaningless: a filter that drops everything saves 100%.
//!
//! Signal lines are found by rule in the original (errors, failures,
//! panics, `file:line` references). A signal line is kept when the
//! compressed text contains it, or its first `PREFIX_CHARS` characters
//! when a long line was truncated on purpose.

use std::collections::HashSet;
use std::sync::LazyLock;

use regex::Regex;

use super::generic::strip_ansi;

const PREFIX_CHARS: usize = 60;

static SIGNAL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?ix)
        \b(error|errors|failed|failure|fail|failing|panic(ked)?|exception|traceback|fatal|assert(ion)?|
           warning|denied|refused|not\ found|cannot|undefined|segmentation|
           time[ds]?\ ?out|killed|abort(ed)?)\b
        | \S+\.[A-Za-z]{1,5}:\d+        # path/file.ext:line
        | \bexit\ (code|status)\b
        | (^|\s)[✕✖×●](\s|$)           # jest, vitest, mocha failure marks
        | ^\s*\d+\)\s                   # mocha: `  1) suite name`
        ",
    )
    .expect("valid regex")
});

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Fidelity {
    pub kept: usize,
    pub total: usize,
    pub missing: Vec<String>,
}

impl Fidelity {
    /// 1.0 when the original had no signal to lose.
    pub fn recall(&self) -> f64 {
        if self.total == 0 { 1.0 } else { self.kept as f64 / self.total as f64 }
    }
}

pub fn is_signal(line: &str) -> bool {
    SIGNAL.is_match(line)
}

pub fn signal_lines(raw: &str) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    strip_ansi(raw)
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && is_signal(l))
        .filter(|l| seen.insert(l.to_string()))
        .map(str::to_string)
        .collect()
}

pub fn measure(raw: &str, compressed: &str) -> Fidelity {
    let lines = signal_lines(raw);
    check(&lines, compressed)
}

/// Explicit expectations (a corpus `.keep` file): every line must survive.
pub fn check(expected: &[String], compressed: &str) -> Fidelity {
    let view = View::new(compressed);
    let mut f = Fidelity { total: expected.len(), ..Fidelity::default() };
    for line in expected {
        if view.contains(line) {
            f.kept += 1;
        } else {
            f.missing.push(line.clone());
        }
    }
    f
}

static LOCATED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^([^:\s][^:]*?):\d+(?::\d+)?:").expect("valid regex"));

/// A compressed text indexed by line, so checking thousands of signal
/// lines does not rescan it for each one.
struct View<'a> {
    text: &'a str,
    lines: HashSet<&'a str>,
}

impl<'a> View<'a> {
    fn new(text: &'a str) -> Self {
        Self { text, lines: text.lines().map(str::trim).collect() }
    }

    fn contains(&self, line: &str) -> bool {
        if self.lines.contains(line) {
            return true;
        }
        // `generic::group_by_file` turns `a.rs:10:body` into `a.rs:` + `  10:body`.
        if let Some(file) = LOCATED.captures(line).and_then(|c| c.get(1)) {
            let rest = line[file.end() + 1..].trim();
            if self.lines.contains(&line[..=file.end()]) && self.lines.contains(rest) {
                return true;
            }
        }
        if self.text.contains(line) {
            return true;
        }
        line.chars().count() > PREFIX_CHARS && self.text.contains(&line.chars().take(PREFIX_CHARS).collect::<String>())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_errors_and_locations_not_noise() {
        let raw = "Compiling foo\nerror[E0425]: cannot find value `x`\n  --> src/main.rs:3:5\nok\n";
        assert_eq!(signal_lines(raw), vec!["error[E0425]: cannot find value `x`", "--> src/main.rs:3:5"]);
    }

    #[test]
    fn recall_counts_dropped_signal() {
        let raw = "test a ... FAILED\ntest b ... ok\npanicked at src/lib.rs:9:1\n";
        let f = measure(raw, "test a ... FAILED\n");
        assert_eq!((f.kept, f.total), (1, 2));
        assert_eq!(f.missing, vec!["panicked at src/lib.rs:9:1"]);
    }

    #[test]
    fn truncated_long_lines_still_count() {
        let long = format!("error: {}", "x".repeat(200));
        let short: String = long.chars().take(80).collect();
        assert_eq!(measure(&long, &format!("{short}…")).kept, 1);
    }

    #[test]
    fn grouped_grep_lines_count_as_kept() {
        let raw = "src/a.rs:10:panic!(\"boom\")\n";
        assert_eq!(measure(raw, "src/a.rs:\n  10:panic!(\"boom\")").kept, 1);
    }

    #[test]
    fn test_runner_failure_marks_are_signal() {
        for line in [
            "  ✕ breaks case 3 (4 ms)",
            " × rejects expired token",
            "  ● Suite › case",
            "  1) Cart adds items:",
            "Error: timed out after 5000ms",
            "worker killed",
            "Aborted",
        ] {
            assert!(is_signal(line), "{line}");
        }
        assert!(!is_signal("compiled module (×3)"));
    }

    #[test]
    fn long_grouped_outputs_are_checked_without_rescanning() {
        let raw = (0..20_000).map(|i| format!("src/m{}.rs:{i}:error here", i / 50)).collect::<Vec<_>>().join("\n");
        let grouped = super::super::generic::group_by_file(&raw).unwrap();
        let started = std::time::Instant::now();
        assert_eq!(measure(&raw, &grouped).kept, 20_000);
        assert!(started.elapsed().as_secs() < 2, "{:?}", started.elapsed());
    }

    #[test]
    fn no_signal_is_perfect_recall() {
        assert!((measure("a\nb\n", "").recall() - 1.0).abs() < f64::EPSILON);
    }
}
