//! One hook event, end to end: read it from stdin, hand it to recording
//! or rewriting, and write the reply the harness expects on stdout.

use anyhow::Result;
use serde_json::{Value, json};

use super::policy::{self, Rewritten};
use super::record::Recorder;
use crate::core::paths::Paths;
use crate::core::spool;
use crate::core::{brief, handoff, outputs};
use crate::harness::{Harness, RewriteSupport};
use crate::helpers::env::{self, Var};
use crate::helpers::{est_tokens, shell, slash};
use crate::limits::store::KEEP_OUTPUTS;

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

/// Errors are the caller's to swallow: `run_fail_open` logs them and the
/// harness never sees a failure.
pub fn run(harness: &dyn Harness) -> Result<()> {
    let Some(input) = crate::harness::read_stdin_json()? else { return Ok(()) };
    let session = input["session_id"].as_str().unwrap_or("unknown");
    let paths = match input["cwd"].as_str() {
        Some(c) => Paths::discover(std::path::Path::new(c))?,
        None => Paths::from_cwd()?,
    };
    paths.ensure_local()?;
    let rec = Recorder { paths: &paths, session };

    match input["hook_event_name"].as_str().unwrap_or("") {
        "PreToolUse" => {
            pre_tool_use(&paths, session, &input, harness.rewrites());
            Ok(())
        }
        "PostToolUse" => post_tool_use(&rec, &input),
        "UserPromptSubmit" => rec.prompt(&input),
        "SessionStart" => session_start(&rec, &input, harness),
        "SessionEnd" => session_end(&rec, &input, harness),
        "PreCompact" => {
            rec.compact(&input)?;
            build_handoff(&rec, &input, harness, "compact")
        }
        "Stop" => rec.stop(&input),
        _ => Ok(()),
    }
}

fn session_start(rec: &Recorder, input: &Value, harness: &dyn Harness) -> Result<()> {
    spool::set_current_session(rec.paths, rec.session);
    let text = brief::build(rec.paths);
    rec.session_start(input, harness.id(), est_tokens(&text))?;
    if !text.trim().is_empty() {
        // Plain stdout on SessionStart becomes context for the model.
        println!("{text}");
    }
    Ok(())
}

fn session_end(rec: &Recorder, input: &Value, harness: &dyn Harness) -> Result<()> {
    rec.session_end(input)?;
    let built = build_handoff(rec, input, harness, input["reason"].as_str().unwrap_or("end"));
    // After the handoff, which lists this session's outputs; and even when
    // it failed, so storage stays bounded.
    outputs::prune(rec.paths, KEEP_OUTPUTS);
    built
}

fn build_handoff(rec: &Recorder, input: &Value, harness: &dyn Harness, reason: &str) -> Result<()> {
    let tail = input["transcript_path"].as_str().and_then(|p| harness.session_tail(std::path::Path::new(p)));
    handoff::build(rec.paths, rec.session, reason, tail.as_ref()).map(|_| ())
}

fn post_tool_use(rec: &Recorder, input: &Value) -> Result<()> {
    if input["tool_name"].as_str() == Some("Bash") {
        // Hooks run outside the tool sandbox: pull in anything relay x
        // could not write to the local tier.
        outputs::absorb_spill(rec.paths);
    }
    rec.tool_use(input)
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
    let call = policy::Call {
        cmd,
        session,
        background: input["tool_input"]["run_in_background"].as_bool() == Some(true),
        isolated: policy::in_isolated_worktree(&paths.root),
        support,
    };
    if let Some(r) = policy::rewrite(&call, relay_invocation) {
        println!("{}", reply(&r));
    }
}

fn reply(r: &Rewritten) -> Value {
    let mut out = json!({ "hookEventName": "PreToolUse", "updatedInput": { "command": r.command } });
    if r.approve {
        out["permissionDecision"] = "allow".into();
        out["permissionDecisionReason"] = "relay: read-only command".into();
    }
    json!({ "hookSpecificOutput": out })
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
