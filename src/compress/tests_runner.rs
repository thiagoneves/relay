//! Filters for test runners and build tools. Rule: passing noise goes,
//! failures and the summary stay verbatim.

use super::Filter;
use super::command::Simple;

pub fn filter_for(s: &Simple) -> Option<Filter> {
    let t2 = s.arg(2);
    Some(match (s.program(), s.arg(1)) {
        ("cargo", "test" | "nextest") => Filter::CargoTest,
        ("go", "test") => Filter::GoTest,
        ("pytest", _) => Filter::Pytest,
        ("python" | "python3", "-m") | ("uv", "run") if t2 == "pytest" => Filter::Pytest,
        ("npx" | "bunx", "jest" | "vitest") | ("jest" | "vitest", _) | ("npm" | "pnpm" | "yarn" | "bun", "test") => {
            Filter::JsTest
        }
        ("npm" | "pnpm" | "yarn", "run") if t2.contains("test") => Filter::JsTest,
        ("rspec" | "phpunit", _) | ("mix" | "composer", "test") | ("rake", "test" | "spec") => Filter::DotTest,
        ("bundle", "exec") if matches!(t2, "rspec" | "rake") => Filter::DotTest,
        ("dart" | "flutter", "test") => Filter::DartTest,
        _ => return None,
    })
}

/// `cargo test`: drop `test x ... ok` lines, keep failures and summaries.
pub fn cargo_test(text: &str) -> String {
    const PROGRESS: &[&str] = &["   Compiling ", "    Finished ", "     Running ", "   Doc-tests "];
    let mut out = Vec::new();
    let mut ok = 0usize;
    for l in text.lines() {
        let t = l.trim_end();
        if t.starts_with("test ") && t.ends_with(" ... ok") {
            ok += 1;
            continue;
        }
        if PROGRESS.iter().any(|p| t.starts_with(p)) {
            continue;
        }
        if t.starts_with("running ") && t.ends_with(" tests") || t == "running 1 test" {
            continue;
        }
        out.push(t.to_string());
    }
    if ok > 0 {
        out.insert(0, format!("{ok} tests passed (lines omitted)"));
    }
    out.join("\n")
}

/// jest / vitest: drop PASS lines and per-test ticks, keep FAIL blocks and summary.
pub fn js_test(text: &str) -> String {
    let mut out = Vec::new();
    let mut passed_files = 0usize;
    let mut passed_tests = 0usize;
    for l in text.lines() {
        let t = l.trim_end();
        let tt = t.trim_start();
        if tt.starts_with("PASS ") || tt.starts_with("✓ ") && tt.contains(".test.") {
            passed_files += 1;
            continue;
        }
        if tt.starts_with("✓ ") || tt.starts_with("√ ") || tt.starts_with("✔ ") {
            passed_tests += 1;
            continue;
        }
        if tt.starts_with("RUN ") || tt.starts_with("stdout |") && tt.len() < 40 {
            continue;
        }
        out.push(t.to_string());
    }
    if passed_files + passed_tests > 0 {
        out.insert(0, format!("{passed_files} files / {passed_tests} tests passed (lines omitted)"));
    }
    out.join("\n")
}

/// pytest: drop progress dots and the header, keep FAILED/ERROR and summary.
pub fn pytest(text: &str) -> String {
    const HEADER: &[&str] = &["platform ", "rootdir:", "cachedir:", "plugins:", "collected ", "configfile:"];
    let mut out = Vec::new();
    for l in text.lines() {
        let t = l.trim_end();
        let tt = t.trim();
        if HEADER.iter().any(|h| tt.starts_with(h)) {
            continue;
        }
        // Progress lines: "tests/test_x.py ......F..   [ 40%]"
        if tt.contains('[')
            && tt.ends_with("%]")
            && tt.chars().filter(|c| *c == '.').count() > 2
            && !tt.contains("FAILED")
        {
            if tt.contains('F') || tt.contains('E') {
                out.push(t.to_string());
            }
            continue;
        }
        out.push(t.to_string());
    }
    out.join("\n")
}

/// `go test`: keep one `ok` per package, drop `=== RUN` and `--- PASS`.
pub fn go_test(text: &str) -> String {
    const NOISE: &[&str] = &["=== RUN", "--- PASS", "=== PAUSE", "=== CONT"];
    text.lines()
        .filter(|l| {
            let t = l.trim_start();
            !(NOISE.iter().any(|n| t.starts_with(n)) || t == "PASS")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// `RSpec`, `ExUnit`, `PHPUnit` and Minitest share a shape: a dots line or a
/// list of passing names, then numbered failure blocks, then a summary.
/// Passing lines and run headers go; from the first failure on, every
/// line stays, `left:`/`right:` included.
pub fn dot_test(text: &str) -> String {
    const HEADER: &[&str] = &[
        "Run options:",
        "Randomized with seed",
        "Running ExUnit with seed",
        "Excluding tags:",
        "Including tags:",
        "All examples were filtered",
        "PHPUnit ",
        "Runtime:",
        "Configuration:",
        "Compiling ",
        "Generated ",
    ];
    let mut out = Vec::new();
    let mut passed = 0usize;
    let mut failures = false;
    for l in text.lines() {
        let t = l.trim_end();
        let tt = t.trim();
        failures |= starts_failures(tt);
        let noise = is_dots(tt) || HEADER.iter().any(|h| tt.starts_with(h)) || (out.is_empty() && tt.is_empty());
        if failures || !(noise || is_passing(t) && !super::fidelity::is_signal(t)) {
            out.push(t.to_string());
        } else if !noise {
            passed += 1;
        }
    }
    if passed > 0 {
        out.insert(0, format!("{passed} tests passed (lines omitted)"));
    }
    out.join("\n")
}

fn starts_failures(tt: &str) -> bool {
    tt == "Failures:"
        || tt.starts_with("There was ")
        || tt.starts_with("There were ")
        || tt.split_once(") ").is_some_and(|(n, rest)| n.parse::<u32>().is_ok() && !rest.is_empty())
}

/// `....F..`, with `PHPUnit`'s trailing `39 / 39 (100%)` allowed.
fn is_dots(tt: &str) -> bool {
    let core = tt.split_whitespace().next().unwrap_or("");
    core.len() >= 3 && core.chars().all(|c| matches!(c, '.' | 'F' | 'E' | '*' | 'S' | 'P' | 'W' | 'I' | 'R'))
}

/// A passing test in `RSpec`'s documentation format (indented name),
/// `ExUnit`'s `--trace` (`* test …`) or `PHPUnit`'s testdox (`✔ …`). The
/// caller keeps any such line that reads as signal, since a failing test
/// in trace or testdox output looks the same.
fn is_passing(line: &str) -> bool {
    let tt = line.trim();
    if tt.is_empty() {
        return false;
    }
    let marked = tt.contains("(FAILED") || tt.contains("(PENDING") || tt.contains("(ERROR") || tt.contains("[FAIL");
    (tt.starts_with("* test ") || tt.starts_with("✔ ") || tt.starts_with("✓ ") || line.starts_with("  ")) && !marked
}

/// `dart test` / `flutter test`: one `MM:SS +passed -failed: name` line
/// per test. Passing ones go; a failing one (`[E]`) and what follows it,
/// and the closing summary, stay.
pub fn dart_test(text: &str) -> String {
    let mut out = Vec::new();
    let mut passed = 0usize;
    for l in text.lines() {
        let t = l.trim_end();
        let progress = t.len() > 6 && t.as_bytes()[2] == b':' && t[6..].starts_with('+');
        if progress && !(t.ends_with("[E]") || t.ends_with("failed.") || t.ends_with("passed!")) {
            passed += 1;
            continue;
        }
        out.push(t.to_string());
    }
    if passed > 0 {
        out.insert(0, format!("{passed} tests passed (lines omitted)"));
    }
    out.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cargo_keeps_failures() {
        let s = "   Compiling relay v0.1.0\nrunning 3 tests\ntest a ... ok\ntest b ... ok\ntest c ... FAILED\n\nfailures:\n\n---- c stdout ----\nboom\n\ntest result: FAILED. 2 passed; 1 failed";
        let out = cargo_test(s);
        assert!(out.starts_with("2 tests passed"));
        assert!(out.contains("test c ... FAILED"));
        assert!(out.contains("boom"));
        assert!(out.contains("test result: FAILED"));
        assert!(!out.contains("Compiling"));
    }

    #[test]
    fn jest_drops_pass() {
        let s = "PASS src/a.test.ts\n  ✓ works (2 ms)\nFAIL src/b.test.ts\n  ● b › breaks\n\nTests: 1 failed, 1 passed";
        let out = js_test(s);
        assert!(!out.contains("PASS src/a"));
        assert!(out.contains("FAIL src/b.test.ts"));
        assert!(out.contains("Tests: 1 failed"));
    }

    #[test]
    fn dot_runners_keep_the_failure_block_and_the_summary() {
        let rspec = "Randomized with seed 1\n\nPayment\n  charges the card\n  retries on timeout (FAILED - 1)\n\nFailures:\n\n  1) Payment retries on timeout\n     Failure/Error: expect(a).to eq(3)\n       expected: 3\n            got: 1\n     # ./spec/payment_spec.rb:42\n\nFinished in 0.8 seconds\n2 examples, 1 failure\n";
        let out = dot_test(rspec);
        assert!(out.starts_with("1 tests passed (lines omitted)\n"), "{out}");
        assert!(!out.contains("charges the card") && !out.contains("Randomized"), "{out}");
        assert!(
            out.contains("retries on timeout (FAILED - 1)")
                && out.contains("got: 1")
                && out.contains("2 examples, 1 failure")
        );

        let exunit = "Compiling 2 files (.ex)\n....F..\n\n  1) test charge (Shop.PaymentTest)\n     test/payment_test.exs:42\n     left:  1\n     right: 3\n\nFinished in 0.4 seconds\n7 tests, 1 failure\n";
        let out = dot_test(exunit);
        assert!(out.starts_with("  1) test charge"), "{out}");
        assert!(out.contains("left:  1") && out.contains("7 tests, 1 failure"));

        let phpunit = "PHPUnit 10.5 by Sebastian Bergmann and contributors.\n\n..F.                         4 / 4 (100%)\n\nThere was 1 failure:\n\n1) Shop\\PaymentTest::testCharge\nFailed asserting that 1 matches expected 3.\n\n/app/tests/PaymentTest.php:42\n\nFAILURES!\nTests: 4, Assertions: 8, Failures: 1.\n";
        let out = dot_test(phpunit);
        assert!(out.starts_with("There was 1 failure:"), "{out}");
        assert!(out.contains("PaymentTest.php:42") && out.contains("FAILURES!"));
    }

    #[test]
    fn dart_keeps_failures_and_the_close() {
        let raw = "00:00 +0: loading test/payment_test.dart\n00:01 +1: Payment charges\n00:01 +2: Payment refunds\n00:02 +2 -1: Payment retries [E]\n  Expected: <3>\n    Actual: <1>\n  test/payment_test.dart 42:7  main.<fn>\n00:02 +2 -1: Some tests failed.\n";
        let out = dart_test(raw);
        assert_eq!(out.lines().next(), Some("3 tests passed (lines omitted)"));
        assert!(!out.contains("Payment charges") && out.contains("Payment retries [E]"), "{out}");
        assert!(out.contains("Actual: <1>") && out.ends_with("Some tests failed."), "{out}");
    }
}
