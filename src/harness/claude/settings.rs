//! Register relay hooks in `~/.claude/settings.json`. Idempotent: any
//! existing relay entries are removed before ours are written, so
//! upgrades and path changes converge. A backup is taken once per run.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde_json::{Map, Value, json};

use super::hook::EVENTS;
use crate::core::paths::home;
use crate::harness::InstallReport;
use crate::helpers::{now_millis, write_atomic};

pub const MARKER: &str = " hook claude";

pub fn settings_path() -> PathBuf {
    std::env::var_os("CLAUDE_CONFIG_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".claude"))
        .join("settings.json")
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

fn is_relay_hook(h: &Value) -> bool {
    h["command"].as_str().is_some_and(|c| c.contains("relay") && c.contains(MARKER))
}

/// Drop every relay handler; drop matcher groups left empty.
fn strip_relay(hooks: &mut Map<String, Value>) -> bool {
    let mut changed = false;
    for (_, groups) in hooks.iter_mut() {
        let Some(arr) = groups.as_array_mut() else { continue };
        for g in arr.iter_mut() {
            if let Some(hs) = g["hooks"].as_array_mut() {
                let before = hs.len();
                hs.retain(|h| !is_relay_hook(h));
                changed |= hs.len() != before;
            }
        }
        let before = arr.len();
        arr.retain(|g| g["hooks"].as_array().is_some_and(|hs| !hs.is_empty()));
        changed |= arr.len() != before;
    }
    hooks.retain(|_, v| v.as_array().is_some_and(|a| !a.is_empty()));
    changed
}

fn backup(path: &Path) -> Result<Option<PathBuf>> {
    if !path.exists() {
        return Ok(None);
    }
    let bak = path.with_file_name(format!("settings.json.relay-bak-{}", now_millis()));
    std::fs::copy(path, &bak)?;
    Ok(Some(bak))
}

pub fn install(exe: &Path) -> Result<InstallReport> {
    let path = settings_path();
    let mut root = load(&path)?;
    let before = serde_json::to_string(&root)?;
    let mut hooks = root.remove("hooks").and_then(|v| v.as_object().cloned()).unwrap_or_default();
    strip_relay(&mut hooks);

    let command = format!("{}{}", exe.display(), MARKER);
    let mut events = Vec::new();
    for (event, matcher, timeout) in EVENTS {
        let mut group = Map::new();
        if let Some(m) = matcher {
            group.insert("matcher".into(), json!(m));
        }
        group.insert("hooks".into(), json!([{ "type": "command", "command": command, "timeout": timeout }]));
        hooks.entry(event.to_string()).or_insert_with(|| json!([])).as_array_mut().unwrap().push(Value::Object(group));
        events.push(event.to_string());
    }
    root.insert("hooks".into(), Value::Object(hooks));
    let after = serde_json::to_string(&root)?;
    let changed = before != after;
    let backup_path = if changed { backup(&path)? } else { None };
    if changed {
        write_atomic(&path, serde_json::to_string_pretty(&Value::Object(root))?.as_bytes())?;
    }
    Ok(InstallReport { settings_path: path, backup_path, events, changed })
}

pub fn uninstall() -> Result<InstallReport> {
    let path = settings_path();
    let mut root = load(&path)?;
    let mut hooks = root.remove("hooks").and_then(|v| v.as_object().cloned()).unwrap_or_default();
    let changed = strip_relay(&mut hooks);
    if !hooks.is_empty() {
        root.insert("hooks".into(), Value::Object(hooks));
    }
    let backup_path = if changed { backup(&path)? } else { None };
    if changed {
        write_atomic(&path, serde_json::to_string_pretty(&Value::Object(root))?.as_bytes())?;
    }
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
        assert!(strip_relay(&mut hooks));
        assert_eq!(hooks["PreToolUse"].as_array().unwrap().len(), 1);
        assert!(hooks.get("Stop").is_none());
        assert!(!strip_relay(&mut hooks));
    }
}
