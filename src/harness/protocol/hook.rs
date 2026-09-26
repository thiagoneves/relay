//! One hook event, end to end: read it from stdin, hand it to recording
//! or rewriting, and write the reply the harness expects on stdout.

use anyhow::Result;
use serde_json::Value;

use super::policy;
use super::record::Recorder;
use super::reply::Reply;
use super::verify;
use crate::core::paths::Paths;
use crate::core::spool;
use crate::core::{brief, claims, condense, handoff, log, outputs, timings, usage};
use crate::harness::Harness;
use crate::helpers::env::{self, Var};
use crate::helpers::{est_tokens, shell, slash};
use crate::limits::store::KEEP_OUTPUTS;

/// Errors are the caller's to swallow: `run_fail_open` logs them and the
/// harness never sees a failure.
pub fn run(harness: &dyn Harness) -> Result<()> {
    let started = std::time::Instant::now();
    let Some(raw) = crate::harness::read_stdin_json()? else { return Ok(()) };
    // Another dialect, or an event this adapter leaves to another one.
    let Some(input) = harness.normalize(raw) else { return Ok(()) };
    let session = input["session_id"].as_str().unwrap_or("unknown");
    let paths = match input["cwd"].as_str() {
        Some(c) => Paths::discover(std::path::Path::new(c))?,
        None => Paths::from_cwd()?,
    };
    paths.ensure_local()?;
    let rec = Recorder { paths: &paths, session };
    let event = input["hook_event_name"].as_str().unwrap_or("");
    let result = dispatch(event, &rec, &input, harness);
    if let Some(out) = harness.render(event, result.as_ref().unwrap_or(&Reply::Nothing)) {
        println!("{out}");
    }
    timings::record(&paths, event, started.elapsed());
    result.map(|_| ())
}

fn dispatch(event: &str, rec: &Recorder, input: &Value, harness: &dyn Harness) -> Result<Reply> {
    let (paths, session) = (rec.paths, rec.session);
    let nothing = |r: Result<()>| r.map(|()| Reply::Nothing);
    match event {
        "PreToolUse" => Ok(pre_tool_use(paths, session, input, harness)),
        "PostToolUse" => post_tool_use(rec, input, harness.replaces_output()),
        "UserPromptSubmit" => nothing(rec.prompt(input)),
        "SessionStart" => session_start(rec, input, harness),
        "SessionEnd" => nothing(session_end(rec, input, harness)),
        "PreCompact" => {
            rec.compact(input)?;
            nothing(build_handoff(rec, input, harness, "compact"))
        }
        "Stop" => nothing(rec.stop(input)),
        _ => Ok(Reply::Nothing),
    }
}

fn session_start(rec: &Recorder, input: &Value, harness: &dyn Harness) -> Result<Reply> {
    spool::set_current_session(rec.paths, rec.session);
    let text = brief::build_for(rec.paths, Some(rec.session));
    rec.session_start(input, harness.id(), est_tokens(&text))?;
    Ok(if text.trim().is_empty() { Reply::Nothing } else { Reply::Context(text) })
}

fn session_end(rec: &Recorder, input: &Value, harness: &dyn Harness) -> Result<()> {
    rec.session_end(input)?;
    claims::end(rec.paths, rec.session)?;
    let built = build_handoff(rec, input, harness, input["reason"].as_str().unwrap_or("end"));
    // After the handoff, which lists this session's outputs; and even when
    // it failed, so storage stays bounded.
    outputs::prune(rec.paths, KEEP_OUTPUTS);
    check_replacements(rec, input, harness);
    built
}

/// A harness that ignores relay's replacement gives no error, so the
/// session's transcript is checked and a miss lands in the failure log,
/// where `relay status` shows it.
fn check_replacements(rec: &Recorder, input: &Value, harness: &dyn Harness) {
    let replaced: Vec<verify::Replaced> = spool::read(rec.paths, rec.session)
        .into_iter()
        .filter(|e| e.event == "replaced")
        .filter_map(|e| {
            Some(verify::Replaced {
                tool_use_id: e.data["tool_use_id"].as_str()?.to_string(),
                output_id: e.data["output"].as_str()?.to_string(),
            })
        })
        .collect();
    let Some(transcript) = input["transcript_path"].as_str().filter(|_| !replaced.is_empty()) else { return };
    let ids: Vec<&str> = replaced.iter().map(|r| r.tool_use_id.as_str()).collect();
    let outcome = verify::check(&replaced, &harness.tool_results(std::path::Path::new(transcript), &ids));
    if outcome.ignored > 0 {
        log::write(
            rec.paths,
            &format!(
                "compression after a command is not taking effect: {} showed the model the full output for {} of {} commands relay compressed (a harness update?)",
                harness.command(),
                outcome.ignored,
                outcome.checked
            ),
        );
    }
}

fn build_handoff(rec: &Recorder, input: &Value, harness: &dyn Harness, reason: &str) -> Result<()> {
    let tail = input["transcript_path"].as_str().and_then(|p| harness.session_tail(std::path::Path::new(p)));
    handoff::build(rec.paths, rec.session, reason, tail.as_ref()).map(|_| ())
}

fn post_tool_use(rec: &Recorder, input: &Value, replaces_output: bool) -> Result<Reply> {
    if let Some(file) = edited_file(rec.paths, input) {
        claims::edited(rec.paths, rec.session, &file)?;
    }
    if input["tool_name"].as_str() != Some("Bash") {
        return rec.tool_use(input).map(|()| Reply::Nothing);
    }
    // Hooks run outside the tool sandbox: pull in anything relay x could
    // not write to the local tier.
    outputs::absorb_spill(rec.paths);
    rec.tool_use(input)?;
    Ok(if replaces_output { shrink_output(rec, input) } else { Reply::Nothing })
}

/// Replace what the model sees of a command the harness ran itself with
/// relay's compressed view. Only reached on success: the harness reports
/// a failed command through another event that cannot be rewritten.
fn shrink_output(rec: &Recorder, input: &Value) -> Reply {
    let cmd = input["tool_input"]["command"].as_str().unwrap_or("");
    let response = &input["tool_response"];
    let Some(raw) = policy::output_to_shrink(cmd, response) else { return Reply::Nothing };
    let cwd = input["cwd"].as_str().unwrap_or("");
    let run = condense::Run { cmd, cwd, exit: 0, session: Some(rec.session.to_string()) };
    let view = condense::view_of(Some(rec.paths), run, &raw);
    if view == raw {
        return Reply::Nothing;
    }
    if let (Some(tool_use_id), Some(output_id)) = (input["tool_use_id"].as_str(), condense::stored_id(&view)) {
        let _ = rec.replaced(tool_use_id, output_id);
    }
    let mut updated = response.clone();
    updated["stdout"] = view.into();
    updated["stderr"] = "".into();
    Reply::ReplaceOutput(updated)
}

/// The repo-relative file an edit tool call is about; `None` for other
/// tools and for files outside the repo.
fn edited_file(paths: &Paths, input: &Value) -> Option<String> {
    if !input["tool_name"].as_str().is_some_and(usage::is_edit) {
        return None;
    }
    let tool_input = &input["tool_input"];
    let file = tool_input["file_path"].as_str().or(tool_input["notebook_path"].as_str())?;
    let rel = paths.rel_file(file);
    (rel != file || !std::path::Path::new(file).is_absolute()).then_some(rel)
}

fn pre_tool_use(paths: &Paths, session: &str, input: &Value, harness: &dyn Harness) -> Reply {
    if let Some(file) = edited_file(paths, input) {
        return claims::warn_once(paths, session, &file).map_or(Reply::Nothing, Reply::Warn);
    }
    let cmd = input["tool_input"]["command"].as_str().unwrap_or("").trim();
    if input["tool_name"].as_str() != Some("Bash") || cmd.is_empty() {
        return Reply::Nothing;
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
        support: harness.rewrites(),
        timeout: harness.command_timeout(&input["tool_input"]),
    };
    policy::rewrite(&call, relay_invocation).map_or(Reply::Nothing, Reply::Rewrite)
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
