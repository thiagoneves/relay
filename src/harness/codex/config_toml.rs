//! Minimal, line-based edit of Codex's `config.toml` to turn on the
//! hooks feature flag. No TOML parser: we only ever add one line under
//! `[features]`, and never touch anything else.

use std::path::Path;

use anyhow::{Context, Result, bail};

use crate::helpers::write_atomic;

/// Returns true when the file was changed.
pub fn enable_hooks(path: &Path) -> Result<bool> {
    // Only a missing file starts empty: an unreadable one would otherwise
    // be replaced by the two lines relay adds.
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e).with_context(|| format!("cannot read {}", path.display())),
    };
    if let Some(new) = with_hooks_enabled(&text)? {
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

/// The table a `[header]` line opens, ignoring a trailing comment and
/// spaces inside the brackets. `None` for array tables and non-headers.
fn table_name(line: &str) -> Option<String> {
    let t = line.split('#').next()?.trim();
    let inner = t.strip_prefix('[')?.strip_suffix(']')?;
    if inner.starts_with('[') {
        return None;
    }
    Some(inner.chars().filter(|c| !c.is_whitespace()).collect())
}

/// `key = value`, with the key's spaces removed (`features . hooks`).
fn key_value(line: &str) -> Option<(String, &str)> {
    let (k, v) = line.split_once('=')?;
    Some((k.chars().filter(|c| !c.is_whitespace()).collect(), v.trim()))
}

fn is_true(v: &str) -> bool {
    v.split('#').next().unwrap_or("").trim() == "true"
}

/// `None` when nothing needs to change. The flag can live in three
/// places: a `[features]` table, dotted `features.x` keys before the first
/// table, or an inline `features = { .. }`. A second `[features]` next to
/// either of the last two is invalid TOML and Codex refuses to start, so
/// dotted keys are extended in place and inline tables are refused.
fn with_hooks_enabled(text: &str) -> Result<Option<String>> {
    let lines: Vec<&str> = text.lines().collect();
    let mut table: Option<String> = None;
    let mut features_header: Option<usize> = None;
    let mut last_dotted: Option<usize> = None;
    for (i, l) in lines.iter().enumerate() {
        let t = l.trim();
        if let Some(name) = table_name(t) {
            if name == "features" {
                features_header = Some(i);
            }
            table = Some(name);
            continue;
        }
        let Some((key, value)) = key_value(t) else { continue };
        let hooks_line = match table.as_deref() {
            Some("features") => key == "hooks",
            None if key == "features" => {
                bail!("config.toml sets `features` as an inline table; add `hooks = true` to it by hand");
            }
            None if key.starts_with("features.") => {
                last_dotted = Some(i);
                key == "features.hooks"
            }
            _ => false,
        };
        if hooks_line {
            if is_true(value) {
                return Ok(None);
            }
            let mut out: Vec<String> = lines.iter().map(std::string::ToString::to_string).collect();
            out[i] = if table.is_some() { "hooks = true" } else { "features.hooks = true" }.into();
            return Ok(Some(finish(&out)));
        }
    }
    let mut out: Vec<String> = lines.iter().map(std::string::ToString::to_string).collect();
    if let Some(i) = features_header {
        out.insert(i + 1, "hooks = true".into());
    } else if let Some(i) = last_dotted {
        out.insert(i + 1, "features.hooks = true".into());
    } else {
        if out.last().is_some_and(|l| !l.trim().is_empty()) {
            out.push(String::new());
        }
        out.push("[features]".into());
        out.push("hooks = true".into());
    }
    Ok(Some(finish(&out)))
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
        assert!(with_hooks_enabled("model = \"x\"\n\n[features]\nhooks = true\n").unwrap().is_none());
    }

    #[test]
    fn adds_under_existing_section() {
        let out = with_hooks_enabled("[features]\njs_repl = false\n\n[other]\na = 1\n").unwrap().unwrap();
        assert_eq!(out, "[features]\nhooks = true\njs_repl = false\n\n[other]\na = 1\n");
    }

    #[test]
    fn flips_false_to_true() {
        let out = with_hooks_enabled("[features]\nhooks = false\n").unwrap().unwrap();
        assert_eq!(out, "[features]\nhooks = true\n");
    }

    #[test]
    fn appends_section_when_missing() {
        let out = with_hooks_enabled("model = \"x\"\n").unwrap().unwrap();
        assert_eq!(out, "model = \"x\"\n\n[features]\nhooks = true\n");
    }

    #[test]
    fn reads_top_level_keys_only() {
        let text = "model = \"x\"\napprovals_reviewer = \"user\"\n[features]\nhooks = true\n";
        assert_eq!(top_level(text, "approvals_reviewer").as_deref(), Some("user"));
        assert_eq!(top_level(text, "hooks"), None);
    }

    fn enabled(text: &str) -> String {
        with_hooks_enabled(text).unwrap().unwrap()
    }

    #[test]
    fn header_with_comment_or_spaces_is_the_same_table() {
        assert_eq!(
            enabled("[features] # flags\njs_repl = false\n"),
            "[features] # flags\nhooks = true\njs_repl = false\n"
        );
        assert_eq!(enabled("[ features ]\n"), "[ features ]\nhooks = true\n");
        assert!(with_hooks_enabled("[features]\nhooks = true # on\n").unwrap().is_none());
    }

    #[test]
    fn dotted_features_keys_are_extended_in_place() {
        let out = enabled("model = \"x\"\nfeatures.web_search = true\n\n[mcp_servers.a]\ncommand = \"a\"\n");
        assert_eq!(
            out,
            "model = \"x\"\nfeatures.web_search = true\nfeatures.hooks = true\n\n[mcp_servers.a]\ncommand = \"a\"\n"
        );
        assert!(with_hooks_enabled("features.hooks = true\n").unwrap().is_none());
        assert_eq!(enabled("features.hooks = false\n"), "features.hooks = true\n");
    }

    #[test]
    fn inline_features_table_is_refused() {
        assert!(with_hooks_enabled("features = { web_search = true }\n").is_err());
    }

    #[test]
    fn features_key_inside_another_table_is_not_the_flag() {
        let out = enabled("[profiles.x]\nfeatures.hooks = true\n");
        assert!(out.ends_with("[features]\nhooks = true\n"), "{out}");
    }

    #[test]
    fn unreadable_file_is_an_error_not_a_blank_config() {
        let dir = std::env::temp_dir().join(format!("relay-cfg-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        let original: &[u8] = b"model = \"\xe9t\xe9\"\n[mcp_servers.x]\ncommand = \"x\"\n";
        std::fs::write(&path, original).unwrap();
        assert!(enable_hooks(&path).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), original);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
