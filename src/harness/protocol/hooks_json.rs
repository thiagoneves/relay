//! Register relay hooks in a harness's hooks JSON file: Claude Code's
//! `settings.json`, Codex's `hooks.json` and Gemini CLI's `settings.json`
//! group handlers under matchers; Cursor's `hooks.json` lists them flat.
//! Idempotent: existing relay entries are removed before ours are
//! written, so upgrades and path changes converge. A backup is taken once
//! per run.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde_json::{Map, Value, json};

use crate::harness::InstallReport;
use crate::helpers::{now_millis, write_atomic};

/// One event relay listens to: its name in the harness's dialect, the
/// tool matcher if any, and the timeout in the unit the harness expects.
pub struct Event {
    pub name: &'static str,
    pub matcher: Option<&'static str>,
    pub timeout: u32,
}

pub const fn ev(name: &'static str, matcher: Option<&'static str>, timeout: u32) -> Event {
    Event { name, matcher, timeout }
}

/// The dialect Claude Code introduced and Codex adopted; timeouts in
/// seconds.
pub const CLAUDE_EVENTS: &[Event] = &[
    ev("PreToolUse", Some("Bash"), 5),
    ev("PostToolUse", Some("Bash|Read|Grep|Glob|Write|Edit|MultiEdit|NotebookEdit"), 5),
    ev("UserPromptSubmit", None, 5),
    ev("SessionStart", None, 5),
    ev("SessionEnd", None, 2),
    ev("PreCompact", None, 5),
    ev("Stop", None, 5),
];

/// How a harness lays out handlers under an event.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    /// `{"matcher": …, "hooks": [{"type": "command", "command": …}]}`
    Grouped,
    /// `{"command": …, "matcher": …}` directly, in a file with `"version": 1`.
    Flat,
}

/// Where hooks live for one harness.
pub struct Target {
    pub path: PathBuf,
    /// Suffix of the hook command, e.g. ` hook claude`. Identifies relay
    /// entries regardless of the binary path.
    pub marker: &'static str,
    pub events: &'static [Event],
    pub layout: Layout,
}

fn load(path: &Path) -> Result<Map<String, Value>> {
    if !path.exists() {
        return Ok(Map::new());
    }
    let text = std::fs::read_to_string(path)?;
    if text.trim().is_empty() {
        return Ok(Map::new());
    }
    let v: Value = serde_json::from_str(&text).with_context(|| format!("{} is not valid JSON", path.display()))?;
    match v {
        Value::Object(m) => Ok(m),
        _ => anyhow::bail!("{} is not a JSON object", path.display()),
    }
}

fn is_relay_hook(h: &Value, marker: &str) -> bool {
    h["command"].as_str().is_some_and(|c| c.contains("relay") && c.ends_with(marker))
}

/// Drop every relay handler, and the matcher groups and events that only
/// held relay handlers. Anything relay does not recognise (non-array
/// events, groups without a `hooks` list) is left exactly as it was.
fn strip_relay(hooks: &mut Map<String, Value>, marker: &str) -> bool {
    let mut changed = false;
    let mut emptied = Vec::new();
    for (event, groups) in hooks.iter_mut() {
        let Some(arr) = groups.as_array_mut() else { continue };
        let mut removed = false;
        arr.retain_mut(|g| {
            if is_relay_hook(g, marker) {
                removed = true;
                return false;
            }
            let Some(hs) = g.get_mut("hooks").and_then(Value::as_array_mut) else { return true };
            let n = hs.len();
            hs.retain(|h| !is_relay_hook(h, marker));
            let stripped = hs.len() != n;
            removed |= stripped;
            !(stripped && hs.is_empty())
        });
        changed |= removed;
        if removed && arr.is_empty() {
            emptied.push(event.clone());
        }
    }
    for e in emptied {
        hooks.remove(&e);
    }
    changed
}

/// The `hooks` object of a settings file, created when missing. A
/// `hooks` of another type cannot take entries without losing it.
fn hooks_mut<'a>(root: &'a mut Map<String, Value>, path: &Path) -> Result<&'a mut Map<String, Value>> {
    root.entry("hooks").or_insert_with(|| json!({})).as_object_mut().with_context(|| {
        format!("`hooks` in {} is not an object; fix it by hand, relay will not overwrite it", path.display())
    })
}

/// Replace relay's handlers in `root` with one per event of `target`.
fn with_relay(root: &mut Map<String, Value>, command: &str, target: &Target) -> Result<Vec<String>> {
    if target.layout == Layout::Flat {
        root.entry("version").or_insert_with(|| json!(1));
    }
    let hooks = hooks_mut(root, &target.path)?;
    strip_relay(hooks, target.marker);
    let mut events = Vec::new();
    for e in target.events {
        let groups = hooks.entry(e.name.to_string()).or_insert_with(|| json!([]));
        let arr = groups.as_array_mut().with_context(|| {
            let (name, path) = (e.name, target.path.display());
            format!("hooks.{name} in {path} is not a list; fix it by hand, relay will not overwrite it")
        })?;
        arr.push(Value::Object(entry(e, command, target.layout)));
        events.push(e.name.to_string());
    }
    Ok(events)
}

fn entry(e: &Event, command: &str, layout: Layout) -> Map<String, Value> {
    let mut out = Map::new();
    if let Some(m) = e.matcher {
        out.insert("matcher".into(), json!(m));
    }
    let handler = json!({ "type": "command", "command": command, "timeout": e.timeout });
    match layout {
        Layout::Grouped => {
            out.insert("hooks".into(), json!([handler]));
        }
        Layout::Flat => out.extend(handler.as_object().cloned().unwrap_or_default()),
    }
    out
}

/// Remove relay's handlers from `root`; an emptied `hooks` goes too.
fn without_relay(root: &mut Map<String, Value>, marker: &str) -> bool {
    let Some(hooks) = root.get_mut("hooks").and_then(Value::as_object_mut) else { return false };
    let changed = strip_relay(hooks, marker);
    if changed && hooks.is_empty() {
        root.remove("hooks");
    }
    changed
}

fn backup(path: &Path) -> Result<Option<PathBuf>> {
    if !path.exists() {
        return Ok(None);
    }
    let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("hooks.json");
    let bak = path.with_file_name(format!("{name}.relay-bak-{}", now_millis()));
    std::fs::copy(path, &bak)?;
    Ok(Some(bak))
}

/// Back up and write `root` when it differs from what was loaded.
fn save_if_changed(
    path: &Path,
    before: &Map<String, Value>,
    root: Map<String, Value>,
) -> Result<(bool, Option<PathBuf>)> {
    if *before == root {
        return Ok((false, None));
    }
    let backup_path = backup(path)?;
    write_atomic(path, serde_json::to_string_pretty(&Value::Object(root))?.as_bytes())?;
    Ok((true, backup_path))
}

pub fn install(target: &Target, exe: &Path) -> Result<InstallReport> {
    let path = target.path.clone();
    let before = load(&path)?;
    let mut root = before.clone();
    let command = format!("{}{}", crate::helpers::shell::command_word(exe), target.marker);
    let events = with_relay(&mut root, &command, target)?;
    let (changed, backup_path) = save_if_changed(&path, &before, root)?;
    Ok(InstallReport { settings_path: path, backup_path, events, changed })
}

pub fn uninstall(target: &Target) -> Result<InstallReport> {
    let path = target.path.clone();
    let before = load(&path)?;
    let mut root = before.clone();
    without_relay(&mut root, target.marker);
    let (changed, backup_path) = save_if_changed(&path, &before, root)?;
    Ok(InstallReport { settings_path: path, backup_path, events: vec![], changed })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_removes_only_relay_entries() {
        let mut hooks: Map<String, Value> = serde_json::from_value(json!({
            "PreToolUse": [
                { "matcher": "Bash", "hooks": [ { "type": "command", "command": "rtk hook claude" } ] },
                { "matcher": "Bash", "hooks": [ { "type": "command", "command": "/x/relay hook claude" } ] }
            ],
            "Stop": [ { "hooks": [ { "type": "command", "command": "/x/relay hook claude" } ] } ]
        }))
        .unwrap();
        assert!(strip_relay(&mut hooks, " hook claude"));
        assert_eq!(hooks["PreToolUse"].as_array().unwrap().len(), 1);
        assert!(hooks.get("Stop").is_none());
        assert!(!strip_relay(&mut hooks, " hook claude"));
    }

    fn obj(v: Value) -> Map<String, Value> {
        serde_json::from_value(v).unwrap()
    }

    const CMD: &str = "/x/relay hook claude";
    const MARK: &str = " hook claude";

    fn claude_target() -> Target {
        Target { path: "s.json".into(), marker: MARK, events: CLAUDE_EVENTS, layout: Layout::Grouped }
    }

    #[test]
    fn flat_layout_writes_handlers_directly_and_strips_them_back() {
        const FLAT: &[Event] = &[ev("preToolUse", Some("Shell"), 5), ev("stop", None, 5)];
        let target = Target { path: "h.json".into(), marker: " hook cursor", events: FLAT, layout: Layout::Flat };
        let mut root = obj(json!({ "hooks": { "preToolUse": [{ "command": "impeccable check" }] } }));
        with_relay(&mut root, "/x/relay hook cursor", &target).unwrap();
        assert_eq!(root["version"], 1);
        assert_eq!(
            root["hooks"]["preToolUse"][1],
            json!({ "matcher": "Shell", "type": "command", "command": "/x/relay hook cursor", "timeout": 5 })
        );
        assert!(without_relay(&mut root, " hook cursor"));
        assert_eq!(root["hooks"], json!({ "preToolUse": [{ "command": "impeccable check" }] }));
    }

    #[test]
    fn install_keeps_key_order_and_entries_relay_does_not_own() {
        let mut root = obj(json!({
            "env": { "Z": "1", "A": "2" },
            "hooks": {
                "Notification": { "bad": true },
                "Stop": [ { "hooks": [ { "type": "command", "command": "say done" } ] } ]
            },
            "model": "opus"
        }));
        with_relay(&mut root, CMD, &claude_target()).unwrap();
        let keys: Vec<_> = root.keys().cloned().collect();
        assert_eq!(keys, ["env", "hooks", "model"]);
        let env: Vec<_> = root["env"].as_object().unwrap().keys().cloned().collect();
        assert_eq!(env, ["Z", "A"]);
        assert_eq!(root["hooks"]["Notification"], json!({ "bad": true }));
        assert_eq!(root["hooks"]["Stop"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn install_refuses_to_overwrite_what_it_cannot_extend() {
        let mut root = obj(json!({ "hooks": "off" }));
        assert!(with_relay(&mut root, CMD, &claude_target()).is_err());
        let mut root = obj(json!({ "hooks": { "Stop": { "x": 1 } } }));
        assert!(with_relay(&mut root, CMD, &claude_target()).is_err());
    }

    #[test]
    fn install_twice_converges() {
        let mut root = Map::new();
        with_relay(&mut root, CMD, &claude_target()).unwrap();
        let once = root.clone();
        with_relay(&mut root, CMD, &claude_target()).unwrap();
        assert_eq!(root, once);
    }

    #[test]
    fn uninstall_leaves_foreign_values_alone() {
        let mut root = obj(json!({
            "hooks": {
                "Notification": { "bad": true },
                "Stop": [ { "hooks": [ { "type": "command", "command": CMD } ] } ],
                "Odd": []
            }
        }));
        assert!(without_relay(&mut root, MARK));
        assert_eq!(root["hooks"], json!({ "Notification": { "bad": true }, "Odd": [] }));
        let mut root = obj(json!({ "hooks": "off" }));
        assert!(!without_relay(&mut root, MARK));
        assert_eq!(root["hooks"], json!("off"));
    }
}
