//! Filters for test runners and build tools. Rule: passing noise goes,
//! failures and the summary stay verbatim.

/// `cargo test`: drop `test x ... ok` lines, keep failures and summaries.
pub fn cargo_test(text: &str) -> String {
    let mut out = Vec::new();
    let mut ok = 0usize;
    for l in text.lines() {
        let t = l.trim_end();
        if t.starts_with("test ") && t.ends_with(" ... ok") {
            ok += 1;
            continue;
        }
        if t.starts_with("   Compiling ") || t.starts_with("    Finished ") || t.starts_with("     Running ") || t.starts_with("   Doc-tests ") {
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
    let mut out = Vec::new();
    for l in text.lines() {
        let t = l.trim_end();
        let tt = t.trim();
        if tt.starts_with("platform ")
            || tt.starts_with("rootdir:")
            || tt.starts_with("cachedir:")
            || tt.starts_with("plugins:")
            || tt.starts_with("collected ")
            || tt.starts_with("configfile:")
        {
            continue;
        }
        // Progress lines: "tests/test_x.py ......F..   [ 40%]"
        if tt.contains("[") && tt.ends_with("%]") && tt.chars().filter(|c| *c == '.').count() > 2 && !tt.contains("FAILED") {
            if tt.contains('F') || tt.contains('E') {
                out.push(t.to_string());
            }
            continue;
        }
        out.push(t.to_string());
    }
    out.join("\n")
}

/// `go test`: drop `ok`/`PASS` lines for packages with no failures? No:
/// keep `ok` (one per package, useful), drop `=== RUN` and `--- PASS`.
pub fn go_test(text: &str) -> String {
    text.lines()
        .filter(|l| {
            let t = l.trim_start();
            !(t.starts_with("=== RUN") || t.starts_with("--- PASS") || t.starts_with("=== PAUSE") || t.starts_with("=== CONT") || t == "PASS")
        })
        .collect::<Vec<_>>()
        .join("\n")
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
}
