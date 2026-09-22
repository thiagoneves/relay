//! Which shell commands the hook routes through `relay x`, and what the
//! rewritten command line reads.

use crate::compress::command::{Joint, head_tokens, program, segments, split_shell_state};
use crate::harness::RewriteSupport;
use crate::harness::protocol::permission::{self, Rewrite};
use crate::helpers::shell;

/// A command line the hook hands back to the harness in place of the
/// agent's own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rewritten {
    pub command: String,
    /// Whether the hook also skips the harness's permission prompt.
    pub approve: bool,
}

/// The rewrite for `cmd`, if any. `relay` yields how to invoke relay;
/// it is only called when a rewrite happens, since resolving it touches
/// the filesystem.
pub fn rewrite(
    cmd: &str,
    session: &str,
    background: bool,
    support: RewriteSupport,
    relay: impl FnOnce() -> String,
) -> Option<Rewritten> {
    // A background command's output is read while it runs; `relay x`
    // would hold all of it until exit.
    if background {
        return None;
    }
    if support == RewriteSupport::Any
        && let Some(command) = remember_with_session(cmd, session)
    {
        return Some(Rewritten { command, approve: false });
    }
    let (prefix, body) = wrap_target(cmd)?;
    let approve = match permission::rewrite_for(prefix, body, support) {
        Rewrite::Skip => return None,
        Rewrite::Approve => true,
        Rewrite::Defer => false,
    };
    let command = format!("{prefix}{} x --session {} -- {}", relay(), shell::quote(session), shell::quote(body));
    Some(Rewritten { command, approve })
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
    if already_wrapped || INTERACTIVE.contains(&t0) || SHELL_STATE.contains(&t0) || runs_until_killed(t0, &toks, &words)
    {
        Verdict::Refuse
    } else if WRAP.contains(&t0) {
        Verdict::Worth
    } else {
        Verdict::Neutral
    }
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
        assert!(should_wrap("git checkout dev"));
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
        let r = rewrite("cd /repo && git status", "s1", false, RewriteSupport::Any, relay).unwrap();
        assert_eq!(r.command, "cd /repo && relay x --session 's1' -- 'git status'");
        assert!(r.approve);
        assert_eq!(rewrite("git status", "s1", true, RewriteSupport::Any, relay), None);
        assert_eq!(rewrite("cargo test", "s1", false, RewriteSupport::ApprovedOnly, relay), None);
    }

    #[test]
    fn remember_is_filed_under_the_session() {
        let r = rewrite("relay remember rule \"x\"", "s1", false, RewriteSupport::Any, relay).unwrap();
        assert_eq!(r.command, "relay remember --session 's1' rule \"x\"");
        assert!(!r.approve);
    }
}
