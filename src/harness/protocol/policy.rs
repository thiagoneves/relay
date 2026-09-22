//! Which shell commands the hook routes through `relay x`, and what the
//! rewritten command line reads.

use crate::compress::command::{Joint, head_tokens, program, segments, split_shell_state};
use crate::harness::RewriteSupport;
use crate::harness::protocol::permission;
use crate::helpers::shell;

/// A command line the hook hands back to the harness in place of the
/// agent's own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rewritten {
    pub command: String,
    /// Whether the hook also skips the harness's permission prompt.
    pub approve: bool,
}

/// One shell call the hook may rewrite.
#[derive(Debug, Clone, Copy)]
pub struct Call<'a> {
    pub cmd: &'a str,
    pub session: &'a str,
    /// Its output is read while it runs; `relay x` would hold all of it
    /// until exit.
    pub background: bool,
    /// Run by a Claude Code agent isolated in its own worktree. Its guard
    /// refuses any command with `git` text it cannot see is aimed at the
    /// worktree, which is what a rewrite turns `git status` into.
    pub isolated: bool,
    pub support: RewriteSupport,
}

/// The rewrite for `call`, if any. `relay` yields how to invoke relay;
/// it is only called when a rewrite happens, since resolving it touches
/// the filesystem.
pub fn rewrite(call: &Call, relay: impl FnOnce() -> String) -> Option<Rewritten> {
    let Call { cmd, session, background, isolated, support } = *call;
    if background {
        return None;
    }
    if support == RewriteSupport::Any
        && let Some(command) = remember_with_session(cmd, session)
    {
        return Some(Rewritten { command, approve: false });
    }
    if support == RewriteSupport::Never {
        return None;
    }
    let (prefix, body) = approved_target(cmd)?;
    if isolated && body.contains("git") {
        return None;
    }
    let command = format!("{prefix}{} x --session {} -- {}", relay(), shell::quote(session), shell::quote(body));
    Some(Rewritten { command, approve: true })
}

/// Whether `cwd` is inside a worktree Claude Code made for an isolated
/// agent: `<repo>/.claude/worktrees/<name>`.
pub fn in_isolated_worktree(cwd: &std::path::Path) -> bool {
    let parts: Vec<_> = cwd.components().map(std::path::Component::as_os_str).collect();
    parts.windows(2).any(|w| w[0] == ".claude" && w[1] == "worktrees")
}

/// `relay remember …` typed by the agent, with the session added so the
/// item is filed under it even when another session moved the pointer.
fn remember_with_session(cmd: &str, session: &str) -> Option<String> {
    let rest = cmd.strip_prefix("relay remember ")?;
    if rest.contains("--session") {
        return None;
    }
    Some(format!("relay remember --session {} {rest}", shell::quote(session)))
}

/// The `(prefix, body)` split of a command relay would rewrite and
/// approve. `bench` uses it to model the hook.
pub fn approved_target(cmd: &str) -> Option<(&str, &str)> {
    wrap_target(cmd).filter(|(prefix, body)| permission::approves(prefix, body))
}

/// The part of `cmd` to route through `relay x`, after any leading
/// `cd`/`export` steps that must stay in the harness's shell.
pub fn wrap_target(cmd: &str) -> Option<(&str, &str)> {
    let (prefix, body) = split_shell_state(cmd);
    should_wrap(body).then_some((prefix, body))
}

const INTERACTIVE: &[&str] =
    &["vim", "vi", "nano", "less", "more", "top", "htop", "ssh", "tmux", "screen", "watch", "man"];

/// Their effect must outlive the command: the harness's shell keeps it.
const SHELL_STATE: &[&str] = &["cd", "pushd", "popd", "export", "unset", "source", "."];

/// Programs whose output is worth compressing.
const WRAP: &[&str] = &[
    "git",
    "ls",
    "tree",
    "find",
    "grep",
    "rg",
    "ag",
    "cargo",
    "npm",
    "pnpm",
    "yarn",
    "npx",
    "bun",
    "bunx",
    "pytest",
    "python",
    "python3",
    "go",
    "make",
    "tsc",
    "eslint",
    "prettier",
    "docker",
    "kubectl",
    "cat",
    "head",
    "tail",
    "wc",
    "diff",
    "curl",
    "mvn",
    "gradle",
    "dotnet",
    "swift",
    "xcodebuild",
    "terraform",
    "gh",
    "brew",
    "pip",
    "uv",
    "ruff",
    "mypy",
    "jest",
    "vitest",
    "playwright",
    "cypress",
    "next",
    "vite",
    "webpack",
    "gcc",
    "clang",
    "cmake",
    "ninja",
    "node",
    "bundle",
    "rspec",
    "gradlew",
    "mvnw",
    "jq",
    "sed",
];

/// Commands worth routing through `relay x`. Conservative on purpose:
/// `relay x` runs the command in a child shell and prints only when it
/// exits, so anything interactive, long-running, backgrounded, or that
/// changes the calling shell (`cd`, `export`) is left alone, as is
/// anything already wrapped.
pub fn should_wrap(cmd: &str) -> bool {
    if cmd.contains("<<") || cmd.contains("$(") || cmd.contains('`') {
        return false;
    }
    // Every segment of a chain counts: `git diff && cargo test` is worth
    // wrapping, and one `vim x` or `cd src` anywhere rules it out.
    let mut any_wrap = false;
    for seg in segments(cmd) {
        match segment_verdict(seg.text, seg.then) {
            Verdict::Refuse => return false,
            Verdict::Worth => any_wrap = true,
            Verdict::Neutral => {}
        }
    }
    any_wrap
}

enum Verdict {
    Refuse,
    Worth,
    Neutral,
}

fn segment_verdict(text: &str, then: Joint) -> Verdict {
    if then == Joint::Background {
        return Verdict::Refuse;
    }
    let toks = head_tokens(text);
    let Some(t0) = toks.first().map(|t| program(t)) else { return Verdict::Neutral };
    let words: Vec<&str> = text.split_whitespace().collect();
    let already_wrapped = words.first().is_some_and(|w| matches!(program(w.trim_matches('\'')), "relay" | "rtk"));
    if already_wrapped
        || INTERACTIVE.contains(&t0)
        || SHELL_STATE.contains(&t0)
        || runs_until_killed(t0, &toks, &words)
        || (t0 == "git" && !git_reads(&words))
    {
        Verdict::Refuse
    } else if WRAP.contains(&t0) {
        Verdict::Worth
    } else {
        Verdict::Neutral
    }
}

/// Git subcommands whose output is worth compressing. Anything else
/// changes the repository and prints little: rewriting it gains nothing
/// and hides the real command from permission rules and guards.
fn git_reads(words: &[&str]) -> bool {
    const READS: &[&str] = &[
        "status",
        "diff",
        "log",
        "show",
        "blame",
        "grep",
        "ls-files",
        "ls-tree",
        "shortlog",
        "reflog",
        "describe",
        "rev-list",
        "cat-file",
        "whatchanged",
        "range-diff",
    ];
    const TAKES_VALUE: &[&str] = &["-C", "-c", "--git-dir", "--work-tree", "--namespace"];
    let mut rest = words.iter().copied().skip_while(|w| program(w) != "git").skip(1);
    while let Some(t) = rest.next() {
        if TAKES_VALUE.contains(&t) {
            rest.next();
        } else if !t.starts_with('-') {
            return READS.contains(&t);
        }
    }
    false
}

/// Servers, watchers and followers: `npm run dev`, `pnpm dev`,
/// `python -m http.server`, `tail -f`, `docker logs -f`, `tsc --watch`.
fn runs_until_killed(t0: &str, toks: &[String], words: &[&str]) -> bool {
    const SCRIPTS: &[&str] = &["dev", "serve", "server", "start", "watch", "preview", "runserver", "http.server"];
    // Tools whose `start`/`server` subcommands return at once.
    const MANAGERS: &[&str] = &["git", "docker", "podman", "kubectl", "systemctl", "brew", "launchctl", "pkill"];
    let t1 = toks.get(1).map_or("", String::as_str);
    let script = if matches!(t1, "run" | "run-script" | "-m" | "manage.py") {
        toks.get(2).map_or("", String::as_str)
    } else {
        t1
    };
    let follows = words.iter().any(|w| matches!(*w, "-f" | "-F" | "--follow"));
    let detached = words.iter().any(|w| matches!(*w, "-d" | "--detach"));
    (!MANAGERS.contains(&t0) && SCRIPTS.contains(&script))
        || words.contains(&"--watch")
        || (matches!(t0, "tail" | "journalctl") || words.contains(&"logs")) && follows
        || matches!(t0, "docker" | "podman" | "docker-compose") && words.contains(&"up") && !detached
        || t0 == "vite" && t1 != "build"
}

#[cfg(test)]
mod tests {
    use super::*;

    fn relay() -> String {
        "relay".into()
    }

    fn call(cmd: &str) -> Call<'_> {
        Call { cmd, session: "s1", background: false, isolated: false, support: RewriteSupport::Any }
    }

    #[test]
    fn isolated_agents_keep_git_visible() {
        let isolated = |cmd| Call { isolated: true, ..call(cmd) };
        assert_eq!(rewrite(&isolated("git status"), relay), None);
        assert_eq!(rewrite(&isolated("cmp src/compress/git.rs x"), relay), None);
        assert!(rewrite(&isolated("cargo test"), relay).is_some());
        assert!(in_isolated_worktree(std::path::Path::new("/r/.claude/worktrees/agent-1/src")));
        assert!(!in_isolated_worktree(std::path::Path::new("/r/worktrees/.claude")));
    }

    #[test]
    fn wraps_known_and_skips_risky() {
        assert!(should_wrap("git status"));
        assert!(should_wrap("RUST_LOG=debug cargo test"));
        assert!(should_wrap("git diff && cargo test 2>&1"));
        assert!(!should_wrap("echo hi"));
        assert!(!should_wrap("rtk git status"));
        assert!(!should_wrap("relay x -- git status"));
        assert!(!should_wrap("cat <<EOF > f\nx\nEOF"));
        assert!(!should_wrap("npm run dev &"));
        assert!(!should_wrap("vim file"));
        assert!(!should_wrap("git status\nvim f"));
    }

    #[test]
    fn only_git_reads_are_wrapped() {
        assert!(should_wrap("git -C /repo --no-pager log -n 5"));
        assert!(should_wrap("/usr/bin/git diff --stat"));
        assert!(!should_wrap("git commit -m 'x'"));
        assert!(!should_wrap("git add . && git commit -m x"));
        assert!(!should_wrap("git status && git push"));
        assert!(!should_wrap("git -c user.name=a merge main"));
        assert!(!should_wrap("git"));
    }

    #[test]
    fn a_background_job_anywhere_is_not_wrapped() {
        assert!(!should_wrap("sleep 4 & git --version"));
        assert!(!should_wrap("npm run dev & sleep 3; curl localhost:3000"));
        assert!(should_wrap("cargo build &> build.log && tail -5 build.log"));
    }

    #[test]
    fn commands_that_change_the_shell_are_not_wrapped() {
        for cmd in
            ["cd src && ls -la", "export A=1; cargo test", "source .env && npm test", ". venv/bin/activate; pytest"]
        {
            assert!(!should_wrap(cmd), "{cmd}");
        }
    }

    #[test]
    fn long_running_commands_are_not_wrapped() {
        for cmd in [
            "tail -f log/dev.log",
            "docker logs -f api",
            "kubectl logs --follow pod/x",
            "npm run dev",
            "pnpm dev",
            "npm start",
            "python3 -m http.server 8000",
            "cargo watch -x test",
            "tsc --watch",
            "vite",
            "next dev",
        ] {
            assert!(!should_wrap(cmd), "{cmd}");
        }
        assert!(!should_wrap("docker compose up api"));
        assert!(!should_wrap("python manage.py runserver"));
        assert!(should_wrap("vite build"));
        assert!(should_wrap("git log dev"));
        assert!(should_wrap("tail -n 50 log/dev.log"));
        assert!(should_wrap("grep -rn \"dev\" src"));
        assert!(should_wrap("docker start api"));
        assert!(should_wrap("docker compose up -d"));
    }

    #[test]
    fn programs_match_by_name_wherever_they_live() {
        for cmd in [
            "./gradlew test",
            "./mvnw verify",
            "/usr/bin/git status",
            "node --test",
            "bundle exec rspec",
            "jq . a.json",
        ] {
            assert!(should_wrap(cmd), "{cmd}");
        }
    }

    #[test]
    fn rewrite_keeps_the_shell_prefix_and_carries_the_session() {
        let r = rewrite(&call("cd /repo && git status"), relay).unwrap();
        assert_eq!(r.command, "cd /repo && relay x --session 's1' -- 'git status'");
        assert!(r.approve);
        assert_eq!(rewrite(&Call { background: true, ..call("git status") }, relay), None);
        assert_eq!(rewrite(&Call { support: RewriteSupport::Never, ..call("cargo test") }, relay), None);
        assert_eq!(rewrite(&call("rm -rf build && ls"), relay), None);
    }

    #[test]
    fn remember_is_filed_under_the_session() {
        let r = rewrite(&call("relay remember rule \"x\""), relay).unwrap();
        assert_eq!(r.command, "relay remember --session 's1' rule \"x\"");
        assert!(!r.approve);
    }
}
