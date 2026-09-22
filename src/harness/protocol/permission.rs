//! Which commands relay may rewrite, and so approve.
//!
//! A rewrite is `relay x … -- '<cmd>'`, and the harness judges that text,
//! not the original: permission rules stop matching and auto mode reads
//! the wrapper as an attempt to get around it. So relay only rewrites
//! what it can approve itself, reads and routine development tasks, and
//! leaves every other command untouched for the harness to judge.

use crate::compress::command::segments;

/// Whether `prefix` (the `cd`/`export` part kept in the harness's shell)
/// and `body` (the part routed through `relay x`) are safe to approve.
pub fn approves(prefix: &str, body: &str) -> bool {
    safe_chain(body) && (prefix.is_empty() || safe_chain(prefix))
}

/// Redirections that only merge or discard streams.
const HARMLESS_REDIRECTS: &[&str] = &["2>&1", "1>&2", ">&2", "2>/dev/null", ">/dev/null", "&>/dev/null"];

/// True when every segment of the chain is a read or a routine dev task
/// and nothing writes to a file or runs a substituted command. Unknown
/// means unsafe: a false negative costs compression, a false positive
/// skips a prompt.
fn safe_chain(cmd: &str) -> bool {
    let mut cleaned = cmd.to_string();
    for r in HARMLESS_REDIRECTS {
        cleaned = cleaned.replace(r, " ");
    }
    if writes_or_substitutes(&cleaned) {
        return false;
    }
    let segs = segments(&cleaned);
    !segs.is_empty() && segs.iter().all(|s| segment_safe(s.text))
}

/// A redirection outside quotes, or a command substitution outside single
/// quotes: either can write or run anything.
fn writes_or_substitutes(cmd: &str) -> bool {
    let (mut single, mut double) = (false, false);
    let mut chars = cmd.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' if !single => {
                chars.next();
            }
            '\'' if !double => single = !single,
            '"' if !single => double = !double,
            '`' if !single => return true,
            '$' if !single && chars.peek() == Some(&'(') => return true,
            '>' | '<' if !single && !double => return true,
            _ => {}
        }
    }
    false
}

fn segment_safe(seg: &str) -> bool {
    let mut words = seg.split_whitespace().skip_while(|w| is_env_assignment(w));
    let Some(first) = words.next() else { return false };
    let program = crate::compress::command::program(first);
    let args: Vec<&str> = words.collect();
    reads(program, &args) || super::dev_tasks::is_dev_task(program, &args)
}

fn has(args: &[&str], flags: &[&str]) -> bool {
    args.iter().any(|a| flags.iter().any(|f| a == f || a.starts_with(&format!("{f}="))))
}

fn reads(program: &str, args: &[&str]) -> bool {
    let has = |flags: &[&str]| has(args, flags);
    match program {
        "ls" | "cat" | "head" | "wc" | "grep" | "pwd" | "du" | "df" | "stat" | "which" | "diff" | "sort" | "uniq"
        | "cut" | "nl" | "jq" | "basename" | "dirname" | "realpath" | "cd" | "pushd" | "popd" | "export" => true,
        "rg" => !has(&["--pre"]),
        "tree" => !has(&["-o"]),
        "file" => !has(&["-C", "--compile"]),
        "tail" => !has(&["-f", "-F", "--follow"]),
        "find" => !has(&["-exec", "-execdir", "-ok", "-okdir", "-delete", "-fprint", "-fprint0", "-fprintf", "-fls"]),
        "git" => git_read_only(args),
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
    fn approves_reads() {
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
            assert!(safe_chain(cmd), "{cmd}");
        }
    }

    #[test]
    fn refuses_anything_else() {
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
            "cargo run",
            "npm install",
            "make deploy",
            "",
        ] {
            assert!(!safe_chain(cmd), "{cmd}");
        }
    }

    #[test]
    fn prefix_and_body_both_count() {
        assert!(approves("", "git status"));
        assert!(approves("cd /x && ", "cargo test"));
        assert!(approves("export CI=1 && ", "git status"));
        assert!(!approves("", "rm -rf build && ls"));
        assert!(!approves("rm -rf x; ", "git status"));
    }
}
