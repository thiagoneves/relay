//! Hook protocol shared by Claude Code and Codex: one JSON event on
//! stdin, `hook_event_name` selects the handler, JSON on stdout only
//! when rewriting a tool input. Both harnesses speak this same dialect.

use anyhow::Result;
use serde_json::{Value, json};

use crate::helpers::truncate_chars;
use crate::core::paths::Paths;
use crate::core::spool::{self, Event};
use crate::core::{brief, handoff};

/// Events relay wants, with the matcher used in settings.json.
pub const EVENTS: &[(&str, Option<&str>, u32)] = &[
    ("PreToolUse", Some("Bash"), 5),
    ("PostToolUse", Some("Bash|Write|Edit|MultiEdit|NotebookEdit"), 5),
    ("UserPromptSubmit", None, 5),
    ("SessionStart", None, 5),
    ("SessionEnd", None, 2),
    ("PreCompact", None, 5),
    ("Stop", None, 5),
];

pub fn run(harness_id: &str, hook_cmd: &str) -> Result<()> {
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
        "PreToolUse" => pre_tool_use(&paths, &session, &input, hook_cmd),
        "PostToolUse" => post_tool_use(&paths, &session, &input),
        "UserPromptSubmit" => {
            let prompt = input["prompt"].as_str().unwrap_or("");
            record(&paths, &session, "prompt", None, json!({ "text": truncate_chars(prompt, 600) }))
        }
        "SessionStart" => session_start(&paths, &session, &input, harness_id),
        "SessionEnd" => {
            record(&paths, &session, "session_end", None, json!({ "reason": input["reason"] }))?;
            let _ = handoff::build(&paths, &session, input["reason"].as_str().unwrap_or("end"));
            Ok(())
        }
        "PreCompact" => {
            record(&paths, &session, "compact", None, json!({ "trigger": input["trigger"] }))?;
            let _ = handoff::build(&paths, &session, "compact");
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

fn record(paths: &Paths, session: &str, name: &str, key: Option<&str>, data: Value) -> Result<()> {
    spool::append(paths, &Event::new(session, name, key, data))
}

fn session_start(paths: &Paths, session: &str, input: &Value, harness_id: &str) -> Result<()> {
    spool::set_current_session(paths, session);
    record(
        paths,
        session,
        "session_start",
        None,
        json!({
            "source": input["source"],
            "transcript_path": input["transcript_path"],
            "cwd": input["cwd"],
            "harness": harness_id,
        }),
    )?;
    let text = brief::build(paths);
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
        crate::core::outputs::absorb_spill(paths);
    }
    let key = input["tool_use_id"].as_str();
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
            })
        }
        _ => json!({
            "tool": tool,
            "file": input["tool_input"]["file_path"].as_str().or(input["tool_input"]["notebook_path"].as_str()),
        }),
    };
    record(paths, session, "tool", key, data)
}

fn pre_tool_use(paths: &Paths, session: &str, input: &Value, _hook_cmd: &str) -> Result<()> {
    if input["tool_name"].as_str() != Some("Bash") {
        return Ok(());
    }
    let cmd = input["tool_input"]["command"].as_str().unwrap_or("").trim();
    if cmd.is_empty() {
        return Ok(());
    }
    // Keep the "current session" pointer fresh for `relay x`.
    if spool::current_session(paths).as_deref() != Some(session) {
        spool::set_current_session(paths, session);
    }
    if !should_wrap(cmd) {
        return Ok(());
    }
    let rewritten = format!("{} x -- {}", relay_invocation(), shell_quote(cmd));
    let out = json!({
        "hookSpecificOutput": {
            "hookEventName": "PreToolUse",
            "permissionDecision": "allow",
            "permissionDecisionReason": "relay compress",
            "updatedInput": { "command": rewritten },
        }
    });
    println!("{out}");
    Ok(())
}

/// Commands worth routing through `relay x`. Conservative on purpose:
/// anything interactive, backgrounded, or already wrapped is left alone.
pub fn should_wrap(cmd: &str) -> bool {
    let t = cmd.trim();
    if t.starts_with("relay ") || t.contains("relay x ") || t.starts_with("rtk ") {
        return false;
    }
    if t.contains("<<") || t.ends_with('&') || t.contains("$(") || t.contains('`') {
        return false;
    }
    const INTERACTIVE: &[&str] = &["vim", "vi", "nano", "less", "more", "top", "htop", "ssh", "tmux", "screen", "watch", "man"];
    const WRAP: &[&str] = &[
        "git", "ls", "tree", "find", "grep", "rg", "ag", "cargo", "npm", "pnpm", "yarn", "npx", "bun", "bunx", "pytest",
        "python", "python3", "go", "make", "tsc", "eslint", "prettier", "docker", "kubectl", "cat", "head", "tail", "wc",
        "diff", "curl", "mvn", "gradle", "dotnet", "swift", "xcodebuild", "terraform", "gh", "brew", "pip", "uv", "ruff",
        "mypy", "jest", "vitest", "playwright", "cypress", "next", "vite", "webpack", "gcc", "clang", "cmake", "ninja",
    ];
    // Look at every segment of a chain: `cd src && cargo test` is worth
    // wrapping because of the second command, `vim x` never is.
    let mut any_wrap = false;
    for seg in t.split(|c| c == '&' || c == ';' || c == '|').map(str::trim).filter(|s| !s.is_empty()) {
        let Some(t0) = crate::compress::head_tokens(seg).into_iter().next() else { continue };
        if INTERACTIVE.contains(&t0.as_str()) {
            return false;
        }
        any_wrap |= WRAP.contains(&t0.as_str());
    }
    any_wrap
}

/// Use the bare name when `relay` on PATH is this very binary; otherwise
/// the absolute path, so the rewrite works in any shell.
fn relay_invocation() -> String {
    let exe = std::env::current_exe().ok();
    if let (Some(exe), Some(path)) = (&exe, std::env::var_os("PATH")) {
        for dir in std::env::split_paths(&path) {
            let cand = dir.join("relay");
            if cand.exists() && std::fs::canonicalize(&cand).ok() == std::fs::canonicalize(exe).ok() {
                return "relay".into();
            }
        }
    }
    exe.map(|p| p.display().to_string()).unwrap_or_else(|| "relay".into())
}

pub fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wraps_known_and_skips_risky() {
        assert!(should_wrap("git status"));
        assert!(should_wrap("RUST_LOG=debug cargo test"));
        assert!(should_wrap("cd src && ls -la"));
        assert!(!should_wrap("echo hi"));
        assert!(!should_wrap("rtk git status"));
        assert!(!should_wrap("relay x -- git status"));
        assert!(!should_wrap("cat <<EOF > f\nx\nEOF"));
        assert!(!should_wrap("npm run dev &"));
        assert!(!should_wrap("vim file"));
    }

    #[test]
    fn quotes_single_quotes() {
        assert_eq!(shell_quote("echo 'a'"), "'echo '\\''a'\\'''");
    }
}
