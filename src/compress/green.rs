//! A green run in one line. When a test run printed no signal line at all
//! (no failure, error, warning or location outside a passing test), the
//! agent needs its counts and nothing else; the original is still one
//! `relay get` away. Any signal line and the runner's own filter applies.

use std::sync::LazyLock;

use regex::Regex;

use super::fidelity::is_signal;

fn clean_run(text: &str) -> bool {
    !text.lines().any(is_signal)
}

static COUNT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(\d+) (passed|ignored)").expect("valid regex"));

/// `cargo test`: every `test result:` ok.
pub fn cargo(text: &str) -> Option<String> {
    let results: Vec<&str> = text.lines().filter(|l| l.starts_with("test result: ")).collect();
    if results.is_empty() || !results.iter().all(|l| l.starts_with("test result: ok.")) || !clean_run(text) {
        return None;
    }
    let (mut passed, mut ignored) = (0, 0);
    for l in &results {
        for c in COUNT.captures_iter(l) {
            let n: usize = c[1].parse().unwrap_or(0);
            if &c[2] == "passed" { passed += n } else { ignored += n }
        }
    }
    Some(format!("cargo test: ok · {passed} passed, {ignored} ignored in {} suites", results.len()))
}

/// jest and vitest: their summary block only.
pub fn js(text: &str) -> Option<String> {
    static SUMMARY: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"^\s*(Test Suites:|Tests:|Test Files\s|Tests\s+\d|Snapshots:|Time:|Duration\s)")
            .expect("valid regex")
    });
    let summary: Vec<&str> = text.lines().filter(|l| SUMMARY.is_match(l)).map(str::trim).collect();
    let counted = summary.iter().any(|l| l.starts_with("Tests") && l.contains("passed"));
    (counted && clean_run(text)).then(|| summary.join(" · "))
}

/// Playwright: its closing counts.
pub fn playwright(text: &str) -> Option<String> {
    static CLOSE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^\s*\d+ (passed|skipped|did not run)\b").expect("valid regex"));
    // A skipped test's line names its location but asks nothing.
    static SKIPPED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\s*-\s+\d+ \[").expect("valid regex"));
    let close: Vec<&str> = text.lines().filter(|l| CLOSE.is_match(l)).map(str::trim).collect();
    let counted = close.iter().any(|l| l.contains("passed"));
    let rest: String = text.lines().filter(|l| !SKIPPED.is_match(l)).collect::<Vec<_>>().join("\n");
    (counted && clean_run(&rest)).then(|| format!("playwright: {}", close.join(" · ")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_green_cargo_run_is_one_line() {
        let raw = "   Compiling relay v0.1.0\nrunning 2 tests\ntest a ... ok\ntest b ... ok\n\ntest result: ok. 2 passed; 0 failed; 1 ignored; 0 measured\n\nrunning 3 tests\ntest result: ok. 3 passed; 0 failed; 0 ignored\n";
        assert_eq!(cargo(raw).as_deref(), Some("cargo test: ok · 5 passed, 1 ignored in 2 suites"));
        assert_eq!(cargo(&raw.replace("test b ... ok", "warning: unused variable `x`")), None, "a warning is kept");
        assert_eq!(cargo("test result: FAILED. 1 passed; 1 failed\n"), None);
    }

    #[test]
    fn a_green_js_run_is_its_summary() {
        let vitest = " ✓ src/a.test.ts (3 tests) 5ms\n ✓ src/b.test.ts (2 tests) 3ms\n\n Test Files  2 passed (2)\n      Tests  5 passed (5)\n   Duration  1.2s\n";
        assert_eq!(js(vitest).as_deref(), Some("Test Files  2 passed (2) · Tests  5 passed (5) · Duration  1.2s"));
        let jest =
            "PASS src/a.test.ts\nTest Suites: 1 passed, 1 total\nTests:       4 passed, 4 total\nTime:        0.9 s\n";
        assert_eq!(
            js(jest).as_deref(),
            Some("Test Suites: 1 passed, 1 total · Tests:       4 passed, 4 total · Time:        0.9 s")
        );
        assert_eq!(js("FAIL src/b.test.ts\n  ● b › breaks\nTests: 1 failed, 1 total\n"), None);
    }

    #[test]
    fn a_green_playwright_run_is_its_counts() {
        let raw = "Running 3 tests using 2 workers\n\n  ✓  1 [chromium] › e2e/a.spec.ts:3:5 › a (1.1s)\n  ✓  2 [chromium] › e2e/a.spec.ts:9:5 › b (0.9s)\n  -  3 [chromium] › e2e/a.spec.ts:15:5 › c\n\n  1 skipped\n  2 passed (3.0s)\n";
        assert_eq!(playwright(raw).as_deref(), Some("playwright: 1 skipped · 2 passed (3.0s)"));
        assert_eq!(playwright(&raw.replace("✓  2", "✘  2")), None);
    }
}
