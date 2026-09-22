//! Which filter a command line's output goes through, and the coarse
//! family name reports group it under.

use super::command::{Joint, Simple, head_tokens, program, segments};
use super::{Filter, git, listing, tests_runner};

/// Segments that set up the shell and print nothing worth a filter.
const SETUP: &[&str] = &["cd", "pushd", "popd", "export", "set", "unset", "source", ".", "true", "mkdir"];

/// The rule of each structured filter for a simple command whose output
/// reaches the agent, tried in order. Each rule lives next to the filter
/// it picks; adding a filter means adding its rule here.
const RULES: &[fn(&Simple) -> Option<Filter>] =
    &[git::filter_for, tests_runner::filter_for, search_filter_for, listing::filter_for];

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
        let simple = Simple::new(text);
        let f = if is_read(&simple) {
            Filter::Read
        } else if fed_by_pipe || seg.then == Joint::Pipe {
            // Mid-pipeline output is not what the agent sees, and a last
            // stage prints a shape no filter knows (`git log | head`). A
            // search anywhere in it still means every line was asked for.
            if is_search(&simple) { Filter::Read } else { Filter::Generic }
        } else {
            RULES.iter().find_map(|rule| rule(&simple)).unwrap_or(Filter::Generic)
        };
        fed_by_pipe = seg.then == Joint::Pipe;
        let is_setup = simple.toks.is_empty() || SETUP.contains(&simple.program());
        if !(is_setup && f == Filter::Generic) {
            filters.push(f);
        }
    }
    agreed(&filters)
}

/// One filter for the whole line from the filters of its segments.
fn agreed(filters: &[Filter]) -> Filter {
    let Some(&first) = filters.first() else { return Filter::Generic };
    if filters.iter().all(|f| *f == first) {
        first
    } else if filters.iter().any(|f| f.keeps_every_line()) {
        Filter::Read
    } else {
        Filter::Generic
    }
}

/// Searches: the agent acts on each match, so none may be cut.
fn is_search(s: &Simple) -> bool {
    matches!(s.program(), "grep" | "egrep" | "fgrep" | "rg" | "ag")
}

fn search_filter_for(s: &Simple) -> Option<Filter> {
    is_search(s).then_some(Filter::Grep)
}

/// A command that prints a file the agent asked to see. The agent quotes
/// read lines back in edits, so these are never cut, only cleaned.
fn is_read(s: &Simple) -> bool {
    const READS: &[&str] = &[
        "cat", "sed", "head", "tail", "nl", "bat", "less", "more", "awk", "gawk", "jq", "yq", "perl", "xxd", "hexdump",
    ];
    if READS.contains(&s.program()) {
        return true;
    }
    // `git cat-file -p x`, `git show HEAD:src/lib.rs`: a file at a revision.
    s.program() == "git"
        && match s.arg(1) {
            "cat-file" => true,
            "show" => s.words.iter().skip(2).any(|w| !w.starts_with('-') && w.contains(':')),
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
    fn families_name_the_command_that_produced_output() {
        assert_eq!(family("cd web && pnpm exec vitest run"), "vitest");
        assert_eq!(family("timeout 30 cargo test -q"), "cargo test");
        assert_eq!(family("git -C x status"), "git");
        assert_eq!(family("sed -n '1,80p' src/main.rs"), "sed");
        assert_eq!(family("/usr/bin/python3 x.py"), "python3");
        assert_eq!(family("export A=1; ls -la"), "ls");
    }
}
