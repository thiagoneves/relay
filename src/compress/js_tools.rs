//! Playwright and the TypeScript compiler. Rule as for every runner:
//! passing noise goes, failures with their `file:line` and the counts stay.

use std::sync::LazyLock;

use regex::Regex;

/// Playwright: passing test lines, the worker banner and progress go;
/// every failure block, its code frame and the closing counts stay.
pub fn playwright(text: &str) -> String {
    static PROGRESS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\s*\[\d+/\d+\]").expect("valid regex"));
    let mut passed = 0usize;
    let mut out = Vec::new();
    for l in text.lines() {
        let t = l.trim_start();
        if t.starts_with(['✓', '✔']) || t.starts_with("ok ") {
            passed += 1;
            continue;
        }
        let failing = ["✘", "failed", "flaky", "timed out"].iter().any(|w| t.contains(w));
        if t.starts_with("Running ") && t.contains(" using ") || PROGRESS.is_match(l) && !failing {
            continue;
        }
        out.push(l.to_string());
    }
    if passed > 0 {
        out.insert(0, format!("{passed} tests passed (lines omitted)"));
    }
    out.join("\n")
}

/// `tsc`: each error line and the lines that continue its message, and
/// everything from the closing count on; code frames and the banner a
/// wrapper printed before go.
/// Output of another shape passes as is.
pub fn tsc(text: &str) -> String {
    static ERROR: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"error TS\d+:").expect("valid regex"));
    if !text.lines().any(|l| ERROR.is_match(l)) {
        return text.to_string();
    }
    let mut out = Vec::new();
    let mut in_error = false;
    let mut closing = false;
    for l in text.lines() {
        if ERROR.is_match(l) {
            in_error = true;
            out.push(l);
        } else if closing || l.starts_with("Found ") && l.contains(" error") {
            // The count, the per-file table and how the wrapper exited.
            closing = true;
            if !l.trim().is_empty() {
                out.push(l);
            }
        } else if in_error && continues_message(l) {
            out.push(l);
        } else if !l.trim().is_empty() {
            // A code frame or a new block: the message is over.
            in_error = l.trim_start().starts_with(['~', '^']) && in_error;
        }
    }
    out.join("\n")
}

/// Indented prose under an error, not a numbered code frame line.
fn continues_message(l: &str) -> bool {
    let t = l.trim_start();
    l.starts_with("  ") && !t.is_empty() && !t.starts_with(|c: char| c.is_ascii_digit() || c == '~' || c == '^')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn playwright_keeps_the_failure_block() {
        let raw = "Running 3 tests using 2 workers\n  ✓  1 [chromium] › e2e/a.spec.ts:3:5 › a (1.1s)\n  ✘  2 [chromium] › e2e/a.spec.ts:9:5 › b (5.0s)\n\n  1) [chromium] › e2e/a.spec.ts:9:5 › b ───\n\n    Error: expect(locator).toHaveText(expected)\n    > 9 |   await expect(page.getByRole('alert')).toHaveText('Wrong');\n        at /app/e2e/a.spec.ts:9:43\n\n  1 failed\n  1 passed (6.1s)\n";
        let out = playwright(raw);
        assert!(out.starts_with("1 tests passed (lines omitted)\n"), "{out}");
        assert!(!out.contains("Running 3 tests") && !out.contains("› a (1.1s)"), "{out}");
        for keep in
            ["✘  2 [chromium]", "Error: expect(locator)", "at /app/e2e/a.spec.ts:9:43", "1 failed", "1 passed (6.1s)"]
        {
            assert!(out.contains(keep), "{keep} lost: {out}");
        }
    }

    #[test]
    fn tsc_keeps_errors_and_their_messages() {
        let plain = "> app@1.0.0 typecheck\n> tsc --noEmit\n\nsrc/a.ts(3,7): error TS2322: Type 'string' is not assignable to type 'number'.\nsrc/b.ts(9,1): error TS2345: Argument of type 'X' is not assignable.\n  Property 'id' is missing in type 'X'.\n\nFound 2 errors in 2 files.\n";
        assert_eq!(
            tsc(plain),
            "src/a.ts(3,7): error TS2322: Type 'string' is not assignable to type 'number'.\nsrc/b.ts(9,1): error TS2345: Argument of type 'X' is not assignable.\n  Property 'id' is missing in type 'X'.\nFound 2 errors in 2 files."
        );
        let pretty = "src/a.ts:3:7 - error TS2322: Type 'string' is not assignable to type 'number'.\n\n3 const x: number = 's';\n        ~\n\nFound 1 error in src/a.ts:3\n";
        assert_eq!(
            tsc(pretty),
            "src/a.ts:3:7 - error TS2322: Type 'string' is not assignable to type 'number'.\nFound 1 error in src/a.ts:3"
        );
        assert_eq!(tsc("no errors here\n"), "no errors here\n");
    }
}
