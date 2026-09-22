//! Hook protocol shared by Claude Code and Codex: one JSON event on
//! stdin, `hook_event_name` selects the handler, JSON on stdout only
//! when rewriting a tool input. Both harnesses speak this same dialect.

use anyhow::Result;
use serde_json::{Value, json};

use crate::core::paths::Paths;
use crate::core::spool::{self, Event};
use crate::core::usage;
use crate::core::{brief, handoff, outputs};
use crate::harness::protocol::permission::{self, Rewrite};
use crate::harness::{Harness, HarnessId, RewriteSupport};
use crate::helpers::env::{self, Var};
use crate::helpers::{est_tokens, shell, slash, truncate_chars};

/// Events relay wants, with the matcher used in settings.json.
pub const EVENTS: &[(&str, Option<&str>, u32)] = &[
    ("PreToolUse", Some("Bash"), 5),
    ("PostToolUse", Some("Bash|Read|Grep|Glob|Write|Edit|MultiEdit|NotebookEdit"), 5),
    ("UserPromptSubmit", None, 5),
    ("SessionStart", None, 5),
    ("SessionEnd", None, 2),
    ("PreCompact", None, 5),
    ("Stop", None, 5),
];

pub fn run(harness: &dyn Harness) -> Result<()> {
    let Some(input) = crate::harness::read_stdin_json()? else { return Ok(()) };
    let event = input["hook_event_name"].as_str().unwrap_or("").to_string();
    let session = input["session_id"].as_str().unwrap_or("unknown").to_string();
    let cwd = input["cwd"].as_str().map(str::to_string);
    let paths = match &cwd {
        Some(c) => Paths::discover(std::path::Path::new(c))?,
        None => Paths::from_cwd()?,
    };
    paths.ensure_local()?;

    match event.as_str() {
        "PreToolUse" => {
            pre_tool_use(&paths, &session, &input, harness.rewrites());
            Ok(())
        }
        "PostToolUse" => post_tool_use(&paths, &session, &input),
        "UserPromptSubmit" => {
            let prompt = input["prompt"].as_str().unwrap_or("");
            record(&paths, &session, "prompt", None, json!({ "text": truncate_chars(prompt, 600) }))
        }
        "SessionStart" => session_start(&paths, &session, &input, harness.id()),
        "SessionEnd" => {
            record(&paths, &session, "session_end", None, json!({ "reason": input["reason"] }))?;
            let tail = transcript_tail(harness, &input);
            let _ = handoff::build(&paths, &session, input["reason"].as_str().unwrap_or("end"), tail.as_ref());
            // After the handoff, which lists this session's outputs.
            outputs::prune(&paths, outputs::KEEP);
            Ok(())
        }
        "PreCompact" => {
            record(&paths, &session, "compact", None, json!({ "trigger": input["trigger"] }))?;
            let tail = transcript_tail(harness, &input);
            let _ = handoff::build(&paths, &session, "compact", tail.as_ref());
            Ok(())
        }
        "Stop" => record(
            &paths,
            &session,
            "stop",
            None,
            json!({ "last": input["last_assistant_message"].as_str().map(|s| truncate_chars(s, 400)) }),
        ),
        _ => Ok(()),
    }
}

fn transcript_tail(harness: &dyn Harness, input: &Value) -> Option<handoff::Tail> {
    let path = input["transcript_path"].as_str()?;
    harness.session_tail(std::path::Path::new(path))
}

fn record(paths: &Paths, session: &str, name: &str, key: Option<&str>, data: Value) -> Result<()> {
    spool::append(paths, &Event::new(session, name, key, data))
}

fn session_start(paths: &Paths, session: &str, input: &Value, harness: HarnessId) -> Result<()> {
    spool::set_current_session(paths, session);
    let text = brief::build(paths);
    record(
        paths,
        session,
        "session_start",
        None,
        json!({
            "source": input["source"],
            "transcript_path": input["transcript_path"],
            "cwd": input["cwd"],
            "harness": harness.stored(),
            "brief_tokens": est_tokens(&text),
            "wrapper": env::text(Var::RelayWrapper),
        }),
    )?;
    if !text.trim().is_empty() {
        // Plain stdout on SessionStart becomes context for the model.
        println!("{text}");
    }
    Ok(())
}

fn post_tool_use(paths: &Paths, session: &str, input: &Value) -> Result<()> {
    let tool = input["tool_name"].as_str().unwrap_or("");
    if tool == "Bash" {
        // Hooks run outside the tool sandbox: pull in anything relay x
        // could not write to the local tier.
        outputs::absorb_spill(paths);
    }
    let key = input["tool_use_id"].as_str();
    // What the agent read, for orientation cost. Edit responses echo the
    // file back to the harness, not to the model, so they are not counted.
    let tokens = if usage::is_edit(tool) { 0 } else { usage::response_tokens(&input["tool_response"]) };
    let data = match tool {
        "Bash" => {
            let cmd = input["tool_input"]["command"].as_str().unwrap_or("");
            let resp = &input["tool_response"];
            let out = resp["stdout"].as_str().or_else(|| resp.as_str()).unwrap_or("");
            json!({
                "tool": tool,
                "command": truncate_chars(cmd, 300),
                "interrupted": resp["interrupted"],
                "tail": truncate_chars(out.trim_end().rsplit('\n').next().unwrap_or(""), 200),
                "tokens": tokens,
            })
        }
        _ => json!({
            "tool": tool,
            "file": input["tool_input"]["file_path"].as_str().or(input["tool_input"]["notebook_path"].as_str()),
            "tokens": tokens,
        }),
    };
    record(paths, session, "tool", key, data)
}

fn pre_tool_use(paths: &Paths, session: &str, input: &Value, support: RewriteSupport) {
    if input["tool_name"].as_str() != Some("Bash") {
        return;
    }
    let cmd = input["tool_input"]["command"].as_str().unwrap_or("").trim();
    if cmd.is_empty() {
        return;
    }
    // Hand-typed `relay x` has no session of its own; it falls back to
    // this pointer.
    if spool::current_session(paths).as_deref() != Some(session) {
        spool::set_current_session(paths, session);
    }
    // A background command's output is read later, while it runs;
    // `relay x` would hold all of it until exit.
    if input["tool_input"]["run_in_background"].as_bool() == Some(true) {
        return;
    }
    if support == RewriteSupport::Any
        && let Some(rewritten) = remember_with_session(cmd, session)
    {
        println!("{}", rewrite_output(&rewritten, false));
        return;
    }
    let Some((prefix, body)) = wrap_target(cmd) else { return };
    let approve = match permission::rewrite_for(prefix, body, support) {
        Rewrite::Skip => return,
        Rewrite::Approve => true,
        Rewrite::Defer => false,
    };
    let rewritten =
        format!("{prefix}{} x --session {} -- {}", relay_invocation(), shell::quote(session), shell::quote(body));
    println!("{}", rewrite_output(&rewritten, approve));
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

fn rewrite_output(command: &str, approve: bool) -> Value {
    let mut out = json!({ "hookEventName": "PreToolUse", "updatedInput": { "command": command } });
    if approve {
        out["permissionDecision"] = "allow".into();
        out["permissionDecisionReason"] = "relay: read-only command".into();
    }
    json!({ "hookSpecificOutput": out })
}

/// The part of `cmd` to route through `relay x`, after any leading
/// `cd`/`export` steps that must stay in the harness's shell.
pub fn wrap_target(cmd: &str) -> Option<(&str, &str)> {
    let (prefix, body) = crate::compress::command::split_shell_state(cmd);
    should_wrap(body).then_some((prefix, body))
}

/// Commands worth routing through `relay x`. Conservative on purpose:
/// `relay x` runs the command in a child shell and prints only when it
/// exits, so anything interactive, long-running, backgrounded, or that
/// changes the calling shell (`cd`, `export`) is left alone, as is
/// anything already wrapped.
pub fn should_wrap(cmd: &str) -> bool {
    use crate::compress::command::{Joint, head_tokens, program, segments};

    const INTERACTIVE: &[&str] =
        &["vim", "vi", "nano", "less", "more", "top", "htop", "ssh", "tmux", "screen", "watch", "man"];
    // Their effect must outlive the command: the harness's shell keeps it.
    const SHELL_STATE: &[&str] = &["cd", "pushd", "popd", "export", "unset", "source", "."];
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
    if cmd.contains("<<") || cmd.contains("$(") || cmd.contains('`') {
        return false;
    }
    // Every segment of a chain counts: `git diff && cargo test` is worth
    // wrapping, and one `vim x` or `cd src` anywhere rules it out.
    let mut any_wrap = false;
    for seg in segments(cmd) {
        if seg.then == Joint::Background {
            return false;
        }
        let toks = head_tokens(seg.text);
        let Some(t0) = toks.first().map(|t| program(t)) else { continue };
        let words: Vec<&str> = seg.text.split_whitespace().collect();
        let already_wrapped = words.first().is_some_and(|w| matches!(program(w.trim_matches('\'')), "relay" | "rtk"));
        if already_wrapped
            || INTERACTIVE.contains(&t0)
            || SHELL_STATE.contains(&t0)
            || runs_until_killed(t0, &toks, &words)
        {
            return false;
        }
        any_wrap |= WRAP.contains(&t0);
    }
    any_wrap
}

/// Servers, watchers and followers: `npm run dev`, `pnpm dev`,
/// `python -m http.server`, `tail -f`, `docker logs -f`, `tsc --watch`.
fn runs_until_killed(t0: &str, toks: &[String], words: &[&str]) -> bool {
    const SCRIPTS: &[&str] = &["dev", "serve", "server", "start", "watch", "preview", "runserver", "http.server"];
    // Tools whose `start`/`server` subcommands return at once.
    const MANAGERS: &[&str] = &["git", "docker", "podman", "kubectl", "systemctl", "brew", "launchctl", "pkill"];
    let t1 = toks.get(1).map_or("", String::as_str);
    // `npm run dev`, `python -m http.server`, `python manage.py runserver`.
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

/// Use the bare name when `relay` on PATH is this very binary; otherwise
/// the quoted absolute path. The rewrite runs in a POSIX shell (Git Bash
/// on Windows), where backslashes and spaces need quoting.
fn relay_invocation() -> String {
    let exe = std::env::current_exe().ok();
    if let (Some(exe), Some(path)) = (&exe, env::get(Var::Path)) {
        for dir in std::env::split_paths(&path) {
            let cand = dir.join(format!("relay{}", std::env::consts::EXE_SUFFIX));
            if cand.exists() && std::fs::canonicalize(&cand).ok() == std::fs::canonicalize(exe).ok() {
                return "relay".into();
            }
        }
    }
    exe.map_or_else(|| "relay".into(), |p| shell::quote(&slash(&p)))
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
