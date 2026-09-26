//! File reads through the harness's own read tool: a big whole-file read
//! gets an outline before it runs, a re-read gets a note or a diff after.

use serde_json::Value;

use super::record::Recorder;
use super::reply::Reply;
use crate::core::outputs::{self, OutputMeta};
use crate::core::read_guard;
use crate::core::reads::{self, Verdict};
use crate::helpers::{est_tokens, human_tokens, new_id, now_iso, slash};

/// A whole-file read of a big text file gets the file's outline instead.
pub fn guard_read(rec: &Recorder, input: &Value) -> Reply {
    let tool_input = &input["tool_input"];
    // Gemini CLI named it `absolute_path` before `file_path`.
    let Some(file) = tool_input["file_path"].as_str().or(tool_input["absolute_path"].as_str()) else {
        return Reply::Nothing;
    };
    let path = std::path::Path::new(input["cwd"].as_str().unwrap_or("")).join(file);
    let ranged = !tool_input["offset"].is_null() || !tool_input["limit"].is_null();
    let Some(g) = read_guard::check(rec.paths, &path, &rec.paths.rel_file(&slash(&path)), ranged) else {
        return Reply::Nothing;
    };
    let _ = rec.guarded_read(input, &g);
    Reply::Deny(g.message)
}

/// A read this agent already made: the same text becomes a note, a
/// changed one the diff since. The file on disk is untouched and the full
/// result stays one `relay get` away.
pub fn reread(rec: &Recorder, input: &Value) -> Reply {
    let response = &input["tool_response"];
    let (Some(content), Some(file)) = (response["file"]["content"].as_str(), input["tool_input"]["file_path"].as_str())
    else {
        return Reply::Nothing;
    };
    // Images, notebooks and PDFs come back in other shapes.
    if response["type"].as_str().is_some_and(|t| t != "text") {
        return Reply::Nothing;
    }
    let range = range_key(&input["tool_input"]);
    let rel = rec.paths.rel_file(file);
    let read = reads::Read {
        session: rec.session,
        agent: input["agent_id"].as_str().unwrap_or(""),
        file: &rel,
        start: response["file"]["startLine"].as_u64().and_then(|n| usize::try_from(n).ok()).unwrap_or(1),
        range: &range,
        content,
    };
    let note = match reads::observe(rec.paths, &read) {
        Ok(Verdict::Unchanged { ago }) => {
            format!("relay: {rel} is unchanged since this agent read it {ago}; that result is still current.")
        }
        Ok(Verdict::Changed { ago, diff }) => format!(
            "relay: {rel} changed since this agent read it {ago}. That result plus this diff is the file now:\n{diff}"
        ),
        Ok(Verdict::Fresh) | Err(_) => return Reply::Nothing,
    };
    let Some(id) = store(rec, &rel, content, &note) else { return Reply::Nothing };
    let view = format!(
        "{note}\n[relay {}→{} tokens · original: relay get {id}]",
        human_tokens(est_tokens(content)),
        human_tokens(est_tokens(&note))
    );
    if let Some(tool_use_id) = input["tool_use_id"].as_str() {
        let _ = rec.replaced(tool_use_id, &id);
    }
    let mut updated = response.clone();
    updated["file"]["numLines"] = view.lines().count().into();
    updated["file"]["content"] = view.into();
    Reply::ReplaceOutput(updated)
}

/// `offset:limit` as asked, empty for a whole-file read.
fn range_key(tool_input: &Value) -> String {
    if tool_input["offset"].is_null() && tool_input["limit"].is_null() {
        String::new()
    } else {
        format!("{}:{}", tool_input["offset"], tool_input["limit"])
    }
}

/// Keep what the read returned, as for a compressed command.
fn store(rec: &Recorder, rel: &str, raw: &str, view: &str) -> Option<String> {
    let meta = OutputMeta {
        id: new_id("o"),
        ts: now_iso(),
        session: Some(rec.session.to_string()),
        cwd: slash(&rec.paths.root),
        cmd: format!("Read {rel}"),
        exit: 0,
        filter: "reread".into(),
        bytes_in: raw.len(),
        bytes_out: view.len(),
        tokens_in: est_tokens(raw),
        tokens_out: est_tokens(view),
    };
    outputs::store(rec.paths, &meta, raw).ok().map(|()| meta.id)
}
