//! Minimal, line-based edit of Codex's `config.toml` to turn on the
//! hooks feature flag. No TOML parser: we only ever add one line under
//! `[features]`, and never touch anything else.

use std::path::Path;

use anyhow::Result;

use crate::helpers::write_atomic;

/// Returns true when the file was changed.
pub fn enable_hooks(path: &Path) -> Result<bool> {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    if let Some(new) = with_hooks_enabled(&text) {
        if path.exists() {
            let bak = path.with_extension(format!("toml.relay-bak-{}", crate::helpers::now_millis()));
            std::fs::copy(path, bak)?;
        }
        write_atomic(path, new.as_bytes())?;
        return Ok(true);
    }
    Ok(false)
}

/// A top-level `key = "value"`, before the first table.
pub fn top_level(text: &str, key: &str) -> Option<String> {
    text.lines()
        .map(str::trim)
        .take_while(|l| !l.starts_with('['))
        .filter_map(|l| l.split_once('='))
        .find(|(k, _)| k.trim() == key)
        .map(|(_, v)| v.trim().trim_matches('"').to_string())
}

/// `None` when nothing needs to change.
fn with_hooks_enabled(text: &str) -> Option<String> {
    let lines: Vec<&str> = text.lines().collect();
    let mut in_features = false;
    let mut features_header: Option<usize> = None;
    for (i, l) in lines.iter().enumerate() {
        let t = l.trim();
        if t.starts_with('[') {
            in_features = t == "[features]";
            if in_features {
                features_header = Some(i);
            }
            continue;
        }
        if in_features && let Some(rest) = t.strip_prefix("hooks") {
            let rest = rest.trim_start();
            if let Some(v) = rest.strip_prefix('=') {
                if v.trim().starts_with("true") {
                    return None;
                }
                let mut out: Vec<String> = lines.iter().map(std::string::ToString::to_string).collect();
                out[i] = "hooks = true".into();
                return Some(finish(&out));
            }
        }
    }
    let mut out: Vec<String> = lines.iter().map(std::string::ToString::to_string).collect();
    if let Some(i) = features_header {
        out.insert(i + 1, "hooks = true".into());
    } else {
        if !out.is_empty() && !out.last().unwrap().trim().is_empty() {
            out.push(String::new());
        }
        out.push("[features]".into());
        out.push("hooks = true".into());
    }
    Some(finish(&out))
}

fn finish(lines: &[String]) -> String {
    let mut s = lines.join("\n");
    s.push('\n');
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn already_enabled_is_noop() {
        assert!(with_hooks_enabled("model = \"x\"\n\n[features]\nhooks = true\n").is_none());
    }

    #[test]
    fn adds_under_existing_section() {
        let out = with_hooks_enabled("[features]\njs_repl = false\n\n[other]\na = 1\n").unwrap();
        assert_eq!(out, "[features]\nhooks = true\njs_repl = false\n\n[other]\na = 1\n");
    }

    #[test]
    fn flips_false_to_true() {
        let out = with_hooks_enabled("[features]\nhooks = false\n").unwrap();
        assert_eq!(out, "[features]\nhooks = true\n");
    }

    #[test]
    fn appends_section_when_missing() {
        let out = with_hooks_enabled("model = \"x\"\n").unwrap();
        assert_eq!(out, "model = \"x\"\n\n[features]\nhooks = true\n");
    }

    #[test]
    fn reads_top_level_keys_only() {
        let text = "model = \"x\"\napprovals_reviewer = \"user\"\n[features]\nhooks = true\n";
        assert_eq!(top_level(text, "approvals_reviewer").as_deref(), Some("user"));
        assert_eq!(top_level(text, "hooks"), None);
    }
}
