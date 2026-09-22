//! Compression entry point. Structured filter chosen by command shape,
//! then the generic pipeline. Any panic inside a filter is caught and
//! the raw output is returned: a filter bug must never eat a real error.

pub mod command;
pub mod fidelity;
pub mod generic;
pub mod git;
pub mod listing;
pub mod tests_runner;

use crate::helpers::est_tokens;

pub use command::head_tokens;
use command::{Joint, program, segments};

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

/// The structured filter a command's output goes through before the
/// generic pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Filter {
    GitStatus,
    GitDiff,
    GitLog,
    CargoTest,
    GoTest,
    Pytest,
    JsTest,
    /// Search results: grouped by file, never cut.
    Grep,
    LsLong,
    /// File contents the agent asked for: cleaned, never cut.
    Read,
    Generic,
}

impl Filter {
    pub fn name(self) -> &'static str {
        match self {
            Self::GitStatus => "git-status",
            Self::GitDiff => "git-diff",
            Self::GitLog => "git-log",
            Self::CargoTest => "cargo-test",
            Self::GoTest => "go-test",
            Self::Pytest => "pytest",
            Self::JsTest => "js-test",
            Self::Grep => "grep",
            Self::LsLong => "ls-long",
            Self::Read => "read",
            Self::Generic => "generic",
        }
    }

    fn apply(self, raw: &str) -> String {
        if self == Self::Read {
            return generic::apply_read(raw);
        }
        let clean = generic::strip_ansi(raw);
        let structured = match self {
            Self::GitStatus => git::status(&clean),
            Self::GitDiff => git::diff(&clean),
            Self::GitLog => git::log(&clean),
            Self::CargoTest => tests_runner::cargo_test(&clean),
            Self::GoTest => tests_runner::go_test(&clean),
            Self::Pytest => tests_runner::pytest(&clean),
            Self::JsTest => tests_runner::js_test(&clean),
            Self::Grep => return generic::apply_read(&generic::group_by_file(&clean).unwrap_or(clean)),
            Self::LsLong => listing::ls_long(&clean),
            Self::Read | Self::Generic => clean,
        };
        generic::apply(&structured)
    }

    /// Whether every line of the output was asked for, so none may be cut.
    fn keeps_every_line(self) -> bool {
        matches!(self, Self::Read | Self::Grep)
    }
}

/// Segments that set up the shell and print nothing worth a filter.
const SETUP: &[&str] = &["cd", "pushd", "popd", "export", "set", "unset", "source", ".", "true", "mkdir"];

/// Pick the filter for a whole command line. A structured filter only
/// understands its own command's output, so it is used only when every
/// segment that prints agrees on it: `git diff && cargo test` goes to
/// `generic`. When they disagree and one of them is a read or a search,
/// nothing is cut.
pub fn classify(cmd: &str) -> Filter {
    let mut filters: Vec<Filter> = Vec::new();
    let mut fed_by_pipe = false;
    for seg in segments(cmd) {
        let text = ["do ", "then ", "else "].iter().find_map(|k| seg.text.strip_prefix(k)).unwrap_or(seg.text);
        let toks = head_tokens(text);
        let is_setup = toks.first().is_none_or(|t0| SETUP.contains(&program(t0)));
        let f = if is_read(text, &toks) {
            Filter::Read
        } else if fed_by_pipe || seg.then == Joint::Pipe {
            // Mid-pipeline output is not what the agent sees, and a last
            // stage prints a shape no filter knows (`git log | head`). A
            // search anywhere in it still means every line was asked for.
            if is_search(&toks) { Filter::Read } else { Filter::Generic }
        } else {
            classify_simple(text, &toks)
        };
        fed_by_pipe = seg.then == Joint::Pipe;
        if !(is_setup && f == Filter::Generic) {
            filters.push(f);
        }
    }
    let Some(&first) = filters.first() else { return Filter::Generic };
    if filters.iter().all(|f| *f == first) {
        first
    } else if filters.iter().any(|f| f.keeps_every_line()) {
        Filter::Read
    } else {
        Filter::Generic
    }
}

/// Filter for one simple command whose output reaches the agent.
fn classify_simple(seg: &str, toks: &[String]) -> Filter {
    const DIFF_OTHER_SHAPES: &[&str] = &[
        "--stat",
        "--shortstat",
        "--numstat",
        "--dirstat",
        "--name-only",
        "--name-status",
        "--raw",
        "--check",
        "--summary",
        "--oneline",
        "--format",
        "--pretty",
    ];
    const LOG_OTHER_SHAPES: &[&str] = &[
        "--oneline",
        "--format",
        "--pretty",
        "-p",
        "-u",
        "--patch",
        "--stat",
        "--shortstat",
        "--numstat",
        "--name-only",
        "--name-status",
        "--raw",
        "--graph",
    ];
    let words: Vec<&str> = seg.split_whitespace().collect();
    let has = |flags: &[&str]| words.iter().any(|w| flags.iter().any(|f| w == f || w.starts_with(&format!("{f}="))));
    let t0 = toks.first().map_or("", |t| program(t));
    let t1 = toks.get(1).map_or("", String::as_str);
    let t2 = toks.get(2).map_or("", String::as_str);
    match (t0, t1) {
        ("git", "status") if !has(&["--porcelain", "-s", "--short"]) => Filter::GitStatus,
        ("git", "diff" | "show") if !has(DIFF_OTHER_SHAPES) => Filter::GitDiff,
        ("git", "log")
            if !has(LOG_OTHER_SHAPES) && !words.iter().any(|w| w.starts_with("-S") || w.starts_with("-G")) =>
        {
            Filter::GitLog
        }
        ("cargo", "test" | "nextest") => Filter::CargoTest,
        ("go", "test") => Filter::GoTest,
        ("pytest", _) => Filter::Pytest,
        ("python" | "python3", "-m") | ("uv", "run") if t2 == "pytest" => Filter::Pytest,
        ("npx" | "bunx", "jest" | "vitest") | ("jest" | "vitest", _) | ("npm" | "pnpm" | "yarn" | "bun", "test") => {
            Filter::JsTest
        }
        ("npm" | "pnpm" | "yarn", "run") if t2.contains("test") => Filter::JsTest,
        _ if is_search(toks) => Filter::Grep,
        ("ls", _) if toks.iter().any(|t| t.starts_with('-') && !t.starts_with("--") && t.contains('l')) => {
            Filter::LsLong
        }
        _ => Filter::Generic,
    }
}

/// Searches: the agent acts on each match, so none may be cut.
fn is_search(toks: &[String]) -> bool {
    toks.first().is_some_and(|t0| matches!(program(t0), "grep" | "egrep" | "fgrep" | "rg" | "ag"))
}

/// A command that prints a file the agent asked to see. The agent quotes
/// read lines back in edits, so these are never cut, only cleaned.
fn is_read(seg: &str, toks: &[String]) -> bool {
    const READS: &[&str] = &[
        "cat", "sed", "head", "tail", "nl", "bat", "less", "more", "awk", "gawk", "jq", "yq", "perl", "xxd", "hexdump",
    ];
    let Some(t0) = toks.first().map(|t| program(t)) else { return false };
    if READS.contains(&t0) {
        return true;
    }
    // `git cat-file -p x`, `git show HEAD:src/lib.rs`: a file at a revision.
    t0 == "git"
        && match toks.get(1).map(String::as_str) {
            Some("cat-file") => true,
            Some("show") => seg.split_whitespace().skip(2).any(|w| !w.starts_with('-') && w.contains(':')),
            _ => false,
        }
}

/// Coarse name for reports: `git status`, `cargo test`, `sed`. Setup
/// segments (`cd`, `export`) and wrappers (`timeout N`) are skipped so the
/// family is the command that produced the output.
pub fn family(cmd: &str) -> String {
    const VERBED: &[&str] = &[
        "git",
        "cargo",
        "npm",
        "pnpm",
        "yarn",
        "bun",
        "go",
        "docker",
        "kubectl",
        "gh",
        "uv",
        "pip",
        "brew",
        "make",
        "dotnet",
        "terraform",
    ];
    for seg in segments(cmd) {
        let toks = head_tokens(seg.text);
        let Some(t0) = toks.first().map(|t| program(t).to_string()) else { continue };
        if SETUP.contains(&t0.as_str()) {
            continue;
        }
        return match toks.get(1) {
            Some(t1) if VERBED.contains(&t0.as_str()) && !t1.starts_with('-') => format!("{t0} {t1}"),
            _ => t0,
        };
    }
    "?".into()
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
    use Filter::*;

    #[test]
    fn classifies_commands() {
        assert_eq!(classify("git status"), GitStatus);
        assert_eq!(classify("git status --porcelain"), Generic);
        assert_eq!(classify("git diff HEAD~1 -- src"), GitDiff);
        assert_eq!(classify("git log -n 20"), GitLog);
        assert_eq!(classify("git log --oneline"), Generic);
        assert_eq!(classify("cargo test --release"), CargoTest);
        assert_eq!(classify("RUST_LOG=debug cargo test"), CargoTest);
        assert_eq!(classify("npm test"), JsTest);
        assert_eq!(classify("pnpm run test:unit"), JsTest);
        assert_eq!(classify("pnpm exec vitest run"), JsTest);
        assert_eq!(classify("pnpm run build"), Generic);
        assert_eq!(classify("rg foo src"), Grep);
        assert_eq!(classify("python -m pytest tests/"), Pytest);
        assert_eq!(classify("python3 -m pytest -q"), Pytest);
        assert_eq!(classify("ls -la"), LsLong);
        assert_eq!(classify("ls src"), Generic);
        assert_eq!(classify("python3 build.py"), Generic);
        assert_eq!(classify("timeout 60 cargo test"), CargoTest);
    }

    #[test]
    fn reads_are_never_cut() {
        assert_eq!(classify("sed -n '1,80p' src/main.rs"), Read);
        assert_eq!(classify("echo ---; cat a.rs; echo ---; head -40 b.rs"), Read);
        assert_eq!(classify("for f in a b; do cat $f; done"), Read);
        assert_eq!(classify("cat log | grep error"), Read);
        assert_eq!(classify("sed -n 1,9p a.py; python3 -c 'print(1)'"), Read);
        assert_eq!(classify("awk 'NR<=300' conf.py"), Read);
        assert_eq!(classify("jq . f.json"), Read);
        assert_eq!(classify("git show HEAD:src/lib.rs"), Read);
        assert_eq!(classify("git cat-file -p HEAD:src/lib.rs"), Read);
        assert_eq!(classify("rg -n foo src; ls"), Read);
        assert_eq!(classify("grep -in foo notes.md | cut -c1-250"), Read);
        assert_eq!(classify("find . -iname '*x*' | grep -v .obsidian; echo ---; ls t/"), Read);
        assert_eq!(classify("cd vault && grep -rn foo ."), Grep);
    }

    #[test]
    fn chains_use_a_structured_filter_only_when_every_segment_agrees() {
        assert_eq!(classify("git diff && cargo test"), Generic);
        assert_eq!(classify("git log -n 3 && cargo build"), Generic);
        assert_eq!(classify("cargo build && cargo test"), Generic);
        assert_eq!(classify("git diff\ncargo test"), Generic);
        assert_eq!(classify("cd crate && cargo test"), CargoTest);
        assert_eq!(classify("export RUST_BACKTRACE=1; cargo test 2>&1"), CargoTest);
        assert_eq!(classify("git status | wc -l"), Generic);
        assert_eq!(classify("git log | head -5"), Read);
    }

    #[test]
    fn git_shapes_other_than_a_patch_are_not_diffs() {
        for cmd in [
            "git diff --name-only main",
            "git diff --name-status",
            "git diff --stat",
            "git diff --numstat HEAD~1",
            "git diff --check",
            "git show --stat HEAD",
        ] {
            assert_eq!(classify(cmd), Generic, "{cmd}");
        }
        for cmd in ["git log -p -n 2", "git log --stat", "git log --name-only", "git log -Sfoo", "git log --graph"] {
            assert_eq!(classify(cmd), Generic, "{cmd}");
        }
    }

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
    fn families_name_the_command_that_produced_output() {
        assert_eq!(family("cd web && pnpm exec vitest run"), "vitest");
        assert_eq!(family("timeout 30 cargo test -q"), "cargo test");
        assert_eq!(family("git -C x status"), "git");
        assert_eq!(family("sed -n '1,80p' src/main.rs"), "sed");
        assert_eq!(family("/usr/bin/python3 x.py"), "python3");
        assert_eq!(family("export A=1; ls -la"), "ls");
    }

    #[test]
    fn never_grows_output() {
        let raw = "x\n".repeat(50);
        let c = compress("echo", &raw);
        assert!(c.text.len() <= raw.len());
    }
}
