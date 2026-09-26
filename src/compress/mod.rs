//! Compression entry point: pick the filter for a command, apply it, and
//! keep the result only when it pays for its footer. Any panic inside a
//! filter is caught and the raw output is returned: a filter bug must
//! never eat a real error.

mod classify;
pub mod command;
pub mod fidelity;
mod filter;
pub mod generic;
mod git;
mod green;
mod js_tools;
mod listing;
mod tests_runner;

use crate::helpers::est_tokens;

pub use classify::{classify, family};
pub use filter::Filter;

/// What the `[relay N→M tokens · original: relay get <id>]` footer costs.
pub const FOOTER_TOKENS: usize = 20;

pub struct Compressed {
    pub text: String,
    /// False when the view is the original (colour codes aside) because
    /// compressing would not pay for its own footer.
    pub shortened: bool,
    /// Name of the structured filter used, `generic` when none matched.
    pub filter: &'static str,
}

pub fn compress(cmd: &str, raw: &str) -> Compressed {
    let filter = classify(cmd);
    let Ok(text) = std::panic::catch_unwind(|| filter.apply(raw)) else {
        return Compressed { text: generic::strip_ansi(raw), shortened: false, filter: "raw" };
    };
    let clean = generic::strip_ansi(raw);
    if est_tokens(&text) + FOOTER_TOKENS >= est_tokens(&clean) {
        return Compressed { text: clean, shortened: false, filter: filter.name() };
    }
    Compressed { text, shortened: true, filter: filter.name() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_chain_keeps_test_failures_after_a_diff() {
        let mut raw = String::from("diff --git a/x b/x\n--- a/x\n+++ b/x\n@@ -1 +1 @@\n-a\n+b\n");
        for i in 0..200 {
            raw.push_str(&format!("test t{i} ... ok\n"));
        }
        raw.push_str("test tests::x ... FAILED\nthread 'tests::x' panicked at src/lib.rs:9:5\ntest result: FAILED. 200 passed; 1 failed\n");
        let c = compress("git diff && cargo test", &raw);
        for line in ["test tests::x ... FAILED", "panicked at src/lib.rs:9:5", "test result: FAILED"] {
            assert!(c.text.contains(line), "{line} lost:\n{}", c.text);
        }
    }

    #[test]
    fn never_grows_output() {
        let raw = "x\n".repeat(50);
        let c = compress("echo", &raw);
        assert!(c.text.len() <= raw.len());
    }
}
