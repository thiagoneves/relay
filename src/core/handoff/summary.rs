//! What a session did, distilled from its spool events and stored outputs.

use crate::core::outputs::OutputMeta;
use crate::core::spool::Event;
use crate::core::usage;
use crate::helpers::truncate_chars;
use crate::limits;

pub struct Summary {
    pub started: String,
    pub harness: String,
    pub prompts: Vec<String>,
    pub last_reply: Option<String>,
    pub files: Vec<(String, usize)>,
    pub read_first: Vec<String>,
    pub remembered: Vec<String>,
    pub commands: Vec<String>,
    pub failing: Vec<String>,
}

impl Summary {
    /// `rel` turns a harness-reported path into a repo-relative one; `now`
    /// stands in for the start of a session with no events.
    pub fn collect(events: &[Event], outs: &[OutputMeta], rel: impl Fn(&str) -> String, now: &str) -> Self {
        let (commands, failing) =
            if outs.is_empty() { (commands_from_events(events), Vec::new()) } else { commands_from_outputs(outs) };
        let files = files_touched(events, &rel);
        Self {
            started: events.first().map_or_else(|| now.to_string(), |e| e.ts.clone()),
            harness: harness(events),
            prompts: prompts(events),
            last_reply: last_reply(events),
            read_first: read_first(events, &files, &rel),
            files,
            remembered: remembered(events),
            commands,
            failing,
        }
    }
}

fn harness(events: &[Event]) -> String {
    events
        .iter()
        .find(|e| e.event == "session_start")
        .and_then(|e| e.data["harness"].as_str())
        .unwrap_or("unknown")
        .to_string()
}

/// Typed prompts. Slash commands and what the harness delivers as a user
/// turn (subagent reports, task notices, command echoes) are not asks.
fn prompts(events: &[Event]) -> Vec<String> {
    events
        .iter()
        .filter(|e| e.event == "prompt")
        .filter_map(|e| e.data["text"].as_str())
        .filter(|s| !s.trim().is_empty() && !s.starts_with('/') && !is_harness_turn(s) && !is_continuer(s))
        .map(|s| without_pasted(s).replace('\n', " "))
        .collect()
}

/// One or two words ("continue", "ok", "sim", "go ahead") only say to go
/// on; the ask they answer is in a reply, not here. Any language.
fn is_continuer(text: &str) -> bool {
    text.split_whitespace().count() < 3
}

/// A pasted block is the user's material, not their ask; the words
/// around it are.
fn without_pasted(text: &str) -> String {
    let Some(start) = text.find("<pasted_content") else { return text.to_string() };
    let end = text[start..]
        .find("</pasted_content")
        .and_then(|close| text[start + close..].find('>').map(|gt| start + close + gt + 1))
        .unwrap_or(text.len());
    format!("{} [pasted content] {}", text[..start].trim(), text[end..].trim()).trim().to_string()
}

fn is_harness_turn(text: &str) -> bool {
    const TAGS: &[&str] = &[
        "<task-notification>",
        "<agent-message",
        "<system-reminder>",
        "<local-command-",
        "<command-name>",
        "<command-message>",
        "<bash-input>",
        "<bash-stdout>",
        "<bash-stderr>",
    ];
    let t = text.trim_start();
    TAGS.iter().any(|tag| t.starts_with(tag))
}

fn last_reply(events: &[Event]) -> Option<String> {
    events.iter().rev().find(|e| e.event == "stop").and_then(|e| e.data["last"].as_str()).map(|s| s.replace('\n', " "))
}

fn remembered(events: &[Event]) -> Vec<String> {
    events
        .iter()
        .filter(|e| e.event == "remember")
        .filter_map(|e| Some(format!("{}: {}", e.data["kind"].as_str()?, e.data["path"].as_str()?)))
        .collect()
}

/// Edited files, most edited first.
fn files_touched(events: &[Event], rel: &impl Fn(&str) -> String) -> Vec<(String, usize)> {
    let edits = events.iter().filter(|e| e.event == "tool" && e.data["tool"].as_str().is_some_and(usage::is_edit));
    tally(edits.filter_map(|e| e.data["file"].as_str()).map(rel).filter(|f| in_repo(f)))
}

/// `rel` leaves a path outside the repo absolute. Those are the agent's
/// scratch and notes, not the project: the next session cannot use them.
fn in_repo(rel: &str) -> bool {
    !std::path::Path::new(rel).is_absolute()
}

/// Files the agent read to orient itself before its first edit, most
/// read first, minus the ones it then edited (those are already listed).
/// The next session can open them directly instead of searching again.
fn read_first(events: &[Event], edited: &[(String, usize)], rel: &impl Fn(&str) -> String) -> Vec<String> {
    let before_first_edit = events
        .iter()
        .filter(|e| e.event == "tool")
        .take_while(|e| !e.data["tool"].as_str().is_some_and(usage::is_edit));
    let reads = before_first_edit.filter(|e| e.data["tool"] == "Read").filter_map(|e| e.data["file"].as_str()).map(rel);
    tally(reads.filter(|f| in_repo(f)))
        .into_iter()
        .filter(|(f, _)| !edited.iter().any(|(e, _)| e == f))
        .take(limits::handoff::READ_FIRST)
        .map(|(f, _)| f)
        .collect()
}

/// Distinct values with their counts, most frequent first, ties in
/// first-seen order.
fn tally(items: impl Iterator<Item = String>) -> Vec<(String, usize)> {
    let mut counts: Vec<(String, usize)> = Vec::new();
    for item in items {
        match counts.iter_mut().find(|(p, _)| *p == item) {
            Some(x) => x.1 += 1,
            None => counts.push((item, 1)),
        }
    }
    counts.sort_by_key(|x| std::cmp::Reverse(x.1));
    counts
}

/// Latest run of each distinct command (first three words), with exit
/// status and the id of its original.
fn commands_from_outputs(outs: &[OutputMeta]) -> (Vec<String>, Vec<String>) {
    let mut seen: Vec<String> = Vec::new();
    let mut commands = Vec::new();
    let mut failing = Vec::new();
    for m in outs.iter().rev() {
        let key = m.cmd.split_whitespace().take(3).collect::<Vec<_>>().join(" ");
        if seen.contains(&key) {
            continue;
        }
        seen.push(key);
        let cmd = truncate_chars(&m.cmd, limits::handoff::COMMAND_CHARS);
        if m.exit != 0 && failing.len() < limits::handoff::FAILING {
            failing.push(format!("`{cmd}` (exit {}) · relay get {}", m.exit, m.id));
        } else {
            let status = if m.exit == 0 { "ok".to_string() } else { format!("exit {}", m.exit) };
            commands.push(format!("`{cmd}` → {status} · relay get {}", m.id));
        }
        if commands.len() + failing.len() >= limits::handoff::COMMANDS {
            break;
        }
    }
    (commands, failing)
}

/// Without `relay x` there are no exit codes, only what hooks saw.
fn commands_from_events(events: &[Event]) -> Vec<String> {
    events
        .iter()
        .rev()
        .filter(|e| e.event == "tool" && e.data["tool"] == "Bash")
        .filter_map(|e| e.data["command"].as_str())
        .map(|c| format!("`{}`", truncate_chars(c, limits::handoff::COMMAND_CHARS)))
        .take(limits::handoff::COMMANDS)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asks_leave_out_continuers_and_pasted_blocks() {
        let events = [
            ev("prompt", json!({ "text": "continue" })),
            ev("prompt", json!({ "text": "Sim." })),
            ev("prompt", json!({ "text": "go ahead" })),
            ev(
                "prompt",
                json!({ "text": "review this: <pasted_content id=\"7\">\n# big\n</pasted_content id=\"7\"> please" }),
            ),
        ];
        assert_eq!(prompts(&events), ["review this: [pasted content] please"]);
    }

    #[test]
    fn files_outside_the_repo_are_not_the_projects() {
        let rel = |f: &str| f.strip_prefix("/repo/").map_or_else(|| f.to_string(), str::to_string);
        let events = [
            ev("tool", json!({ "tool": "Edit", "file": "/repo/src/a.rs" })),
            ev("tool", json!({ "tool": "Edit", "file": "/Users/me/.claude/projects/x/memory/note.md" })),
        ];
        assert_eq!(files_touched(&events, &rel), [("src/a.rs".to_string(), 1)]);
    }
    use serde_json::json;

    fn ev(event: &str, data: serde_json::Value) -> Event {
        Event { ts: "2026-09-22T10:00:00Z".into(), session: "s".into(), event: event.into(), key: String::new(), data }
    }

    #[test]
    fn reads_before_the_first_edit_orient_minus_edited_files() {
        let events = [
            ev("tool", json!({ "tool": "Read", "file": "a.rs" })),
            ev("tool", json!({ "tool": "Read", "file": "b.rs" })),
            ev("tool", json!({ "tool": "Read", "file": "b.rs" })),
            ev("tool", json!({ "tool": "Edit", "file": "a.rs" })),
            ev("tool", json!({ "tool": "Read", "file": "c.rs" })),
        ];
        let s = Summary::collect(&events, &[], str::to_string, "now");
        assert_eq!(s.files, [("a.rs".to_string(), 1)]);
        assert_eq!(s.read_first, ["b.rs"]);
        assert_eq!(s.started, "2026-09-22T10:00:00Z");
    }

    #[test]
    fn slash_commands_are_not_asks() {
        let events = [
            ev("prompt", json!({ "text": "/clear" })),
            ev("prompt", json!({ "text": "fix\nthe build" })),
            ev("prompt", json!({ "text": "<task-notification>\n<task-id>a1</task-id>" })),
            ev("prompt", json!({ "text": "<agent-message from=\"a1\">report</agent-message>" })),
        ];
        assert_eq!(Summary::collect(&events, &[], str::to_string, "now").prompts, ["fix the build"]);
    }
}
