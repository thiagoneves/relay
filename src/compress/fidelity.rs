//! Did a compressed view keep what an agent needs? Savings without this
//! number are meaningless: a filter that drops everything saves 100%.
//!
//! Signal lines are found by rule in the original (errors, failures,
//! panics, `file:line` references). A signal line is kept when the
//! compressed text contains it, or its first `PREFIX_CHARS` characters
//! when a long line was truncated on purpose.

use std::sync::LazyLock;

use regex::Regex;

use super::generic::strip_ansi;

const PREFIX_CHARS: usize = 60;

static SIGNAL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?ix)
        \b(error|errors|failed|failure|fail|failing|panic(ked)?|exception|traceback|fatal|assert(ion)?|
           warning|denied|refused|not\ found|cannot|undefined|segmentation)\b
        | \S+\.[A-Za-z]{1,5}:\d+        # path/file.ext:line
        | \bexit\ (code|status)\b
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
    let mut f = Fidelity { total: expected.len(), ..Fidelity::default() };
    for line in expected {
        if contains_line(compressed, line) {
            f.kept += 1;
        } else {
            f.missing.push(line.clone());
        }
    }
    f
}

static LOCATED: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^([^:\s][^:]*?):(\d+)(?::\d+)?:(.*)$").expect("valid regex"));

fn contains_line(haystack: &str, line: &str) -> bool {
    if haystack.contains(line) {
        return true;
    }
    let prefix: String = line.chars().take(PREFIX_CHARS).collect();
    if line.chars().count() > PREFIX_CHARS && haystack.contains(&prefix) {
        return true;
    }
    // `generic::group_by_file` turns `a.rs:10:body` into `a.rs:` + `  10: body`.
    LOCATED.captures(line).is_some_and(|c| {
        haystack.lines().any(|l| l == format!("{}:", &c[1]))
            && haystack.contains(&format!("  {}: {}", &c[2], c[3].trim()))
    })
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
        assert_eq!(measure(raw, "src/a.rs:\n  10: panic!(\"boom\")").kept, 1);
    }

    #[test]
    fn no_signal_is_perfect_recall() {
        assert!((measure("a\nb\n", "").recall() - 1.0).abs() < f64::EPSILON);
    }
}
