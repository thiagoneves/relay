//! Whether a rewritten command may skip the harness's permission prompt.
//! Pure: no IO, so the policy is testable apart from the hook plumbing.
//!
//! A rewrite is `relay x … -- '<cmd>'`, and harness permission rules are
//! matched against that rewritten text, not the original. Approving it
//! outright would let any command through that merely contains `ls` or
//! `git`; leaving it to the harness means the user's own allow rules no
//! longer match. Only commands that cannot change anything are approved.

use crate::harness::RewriteSupport;

/// What the hook answers for a command it wants to route through relay.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rewrite {
    /// Rewrite and approve: every segment is read-only.
    Approve,
    /// Rewrite and let the harness's rules and mode decide.
    Defer,
    /// Leave the command alone.
    Skip,
}

/// `prefix` is the `cd`/`export` part kept in the harness's shell, `body`
/// the part routed through `relay x`; approval needs both read-only.
pub fn rewrite_for(prefix: &str, body: &str, support: RewriteSupport) -> Rewrite {
    let read_only = is_read_only(body) && (prefix.is_empty() || is_read_only(prefix));
    match (support, read_only) {
        (RewriteSupport::Never, _) | (RewriteSupport::ApprovedOnly, false) => Rewrite::Skip,
        (_, true) => Rewrite::Approve,
        (RewriteSupport::Any, false) => Rewrite::Defer,
    }
}

/// Redirections that only merge or discard streams.
const HARMLESS_REDIRECTS: &[&str] = &["2>&1", "1>&2", ">&2", "2>/dev/null", ">/dev/null", "&>/dev/null"];

/// True when every segment of the chain is a known read-only command and
/// nothing writes to a file or runs a substituted command. Unknown means
/// not read-only: a false negative costs a prompt, a false positive
/// skips one.
pub fn is_read_only(cmd: &str) -> bool {
    let mut cleaned = cmd.to_string();
    for r in HARMLESS_REDIRECTS {
        cleaned = cleaned.replace(r, " ");
    }
    let Some(segments) = split_unquoted(&cleaned) else { return false };
    !segments.is_empty() && segments.iter().all(|s| segment_read_only(s))
}

/// Split on `; & | newline` outside quotes. `None` when the command
/// redirects output or substitutes a command anywhere outside single
/// quotes, since those can write or run anything.
fn split_unquoted(cmd: &str) -> Option<Vec<String>> {
    let mut segments = Vec::new();
    let mut cur = String::new();
    let (mut single, mut double) = (false, false);
    let mut chars = cmd.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\'' if !double => single = !single,
            '"' if !single => double = !double,
            '`' if !single => return None,
            '$' if !single && chars.peek() == Some(&'(') => return None,
            '>' | '<' if !single && !double => return None,
            ';' | '&' | '|' | '\n' if !single && !double => {
                segments.push(std::mem::take(&mut cur));
                continue;
            }
            _ => {}
        }
        cur.push(c);
    }
    segments.push(cur);
    Some(segments.into_iter().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect())
}

fn segment_read_only(seg: &str) -> bool {
    let mut words = seg.split_whitespace().skip_while(|w| is_env_assignment(w));
    let Some(program) = words.next() else { return false };
    let args: Vec<&str> = words.collect();
    let has = |flags: &[&str]| args.iter().any(|a| flags.iter().any(|f| a == f || a.starts_with(&format!("{f}="))));
    match program {
        "ls" | "cat" | "head" | "wc" | "grep" | "pwd" | "du" | "df" | "stat" | "which" | "cd" => true,
        "rg" => !has(&["--pre"]),
        "tree" => !has(&["-o"]),
        "file" => !has(&["-C", "--compile"]),
        "tail" => !has(&["-f", "-F", "--follow"]),
        "find" => !has(&["-exec", "-execdir", "-ok", "-okdir", "-delete", "-fprint", "-fprint0", "-fprintf", "-fls"]),
        "git" => git_read_only(&args),
        _ => false,
    }
}

fn is_env_assignment(w: &str) -> bool {
    w.split_once('=').is_some_and(|(k, _)| {
        !k.is_empty()
            && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
            && !k.starts_with(|c: char| c.is_ascii_digit())
    })
}

fn git_read_only(args: &[&str]) -> bool {
    // Global options before the subcommand; `-C <dir>` and `-c <k=v>`
    // take a value. `-c` can set an alias or pager, so it disqualifies.
    let mut i = 0;
    while let Some(a) = args.get(i) {
        match *a {
            "-C" => i += 2,
            "--no-pager" | "-P" => i += 1,
            _ if a.starts_with("--git-dir=") || a.starts_with("--work-tree=") => i += 1,
            _ => break,
        }
    }
    let Some(sub) = args.get(i) else { return false };
    let rest = &args[i + 1..];
    if rest.iter().any(|a| a.starts_with("--output") || a.starts_with("--ext-diff")) {
        return false;
    }
    match *sub {
        "status" | "log" | "diff" | "show" | "blame" => true,
        "branch" => rest.iter().all(|a| BRANCH_LISTING.iter().any(|f| a == f || a.starts_with(&format!("{f}=")))),
        _ => false,
    }
}

/// `git branch` flags that only list; anything else (a name, -d, -m, …)
/// creates, deletes or moves a branch.
const BRANCH_LISTING: &[&str] = &[
    "-a",
    "--all",
    "-r",
    "--remotes",
    "-v",
    "-vv",
    "--verbose",
    "-l",
    "--list",
    "--show-current",
    "--merged",
    "--no-merged",
    "--contains",
    "--no-contains",
    "--format",
    "--sort",
    "--color",
    "--no-color",
    "--column",
    "--no-column",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn approves_read_only_chains() {
        for cmd in [
            "git status",
            "git -C sub log --oneline -5",
            "git diff HEAD~1 -- src/a.rs 2>&1",
            "git branch -a",
            "ls -la && git status",
            "cd src && rg -n 'a|b' .",
            "grep -rn \"x > y\" src | head -20",
            "find . -name '*.rs' | wc -l",
            "RUST_LOG=debug git log",
            "tail -n 50 log.txt",
            "cat f 2>/dev/null",
        ] {
            assert!(is_read_only(cmd), "{cmd}");
        }
    }

    #[test]
    fn refuses_anything_that_can_change_state() {
        for cmd in [
            "rm -rf build && ls",
            "git push --force origin main",
            "git status; git reset --hard",
            "git branch new-feature",
            "git branch -D old",
            "git diff --output=patch.txt",
            "git -c alias.st='!rm -rf x' st",
            "ls > files.txt",
            "git log >> notes",
            "cat f | tee copy",
            "cat f | sh",
            "find . -name '*.tmp' -delete",
            "find . -exec rm {} \\;",
            "rg --pre ./script pattern",
            "tail -f server.log",
            "ls $(rm -rf x)",
            "ls `rm -rf x`",
            "cargo test",
            "npm run build",
            "",
        ] {
            assert!(!is_read_only(cmd), "{cmd}");
        }
    }

    #[test]
    fn codex_skips_what_it_cannot_defer() {
        assert_eq!(rewrite_for("", "git status", RewriteSupport::ApprovedOnly), Rewrite::Approve);
        assert_eq!(rewrite_for("", "cargo test", RewriteSupport::Any), Rewrite::Defer);
        assert_eq!(rewrite_for("", "cargo test", RewriteSupport::ApprovedOnly), Rewrite::Skip);
        assert_eq!(rewrite_for("cd /x && ", "git status", RewriteSupport::Any), Rewrite::Approve);
        assert_eq!(rewrite_for("export A=1 && ", "git status", RewriteSupport::Any), Rewrite::Defer);
        assert_eq!(rewrite_for("", "git status", RewriteSupport::Never), Rewrite::Skip);
    }
}
