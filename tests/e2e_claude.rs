//! Against the real Claude Code: for a long command the model gets relay's
//! compressed view, not the full output. Run it after a Claude Code update.
//! It needs `claude` on PATH, signed in, and makes one small model call,
//! so it only runs on request:
//!
//! ```sh
//! cargo test --test e2e_claude -- --ignored
//! ```

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn claude_home() -> PathBuf {
    std::env::var_os("CLAUDE_CONFIG_DIR")
        .filter(|v| !v.is_empty())
        .map_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".claude"), PathBuf::from)
}

/// Claude Code's transcript dir name for a project path.
fn slug(p: &Path) -> String {
    p.display().to_string().chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect()
}

fn newest_transcript(dir: &Path) -> PathBuf {
    std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("no transcripts in {}: {e}", dir.display()))
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "jsonl"))
        .max_by_key(|p| p.metadata().and_then(|m| m.modified()).ok())
        .expect("a transcript")
}

/// Every tool result in a transcript, as the model received it.
fn tool_results(transcript: &Path) -> Vec<String> {
    let text = std::fs::read_to_string(transcript).unwrap();
    let mut out = Vec::new();
    for line in text.lines().filter(|l| l.contains("\"tool_result\"")) {
        let v: serde_json::Value = serde_json::from_str(line).unwrap();
        for b in v["message"]["content"].as_array().into_iter().flatten() {
            match &b["content"] {
                serde_json::Value::String(s) => out.push(s.clone()),
                parts => out.push(parts.as_array().into_iter().flatten().filter_map(|p| p["text"].as_str()).collect()),
            }
        }
    }
    out
}

#[test]
#[ignore = "needs a signed-in `claude` and makes a model call"]
fn claude_code_shows_the_model_relays_view() {
    let dir = std::env::temp_dir().join(format!("relay-e2e-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let dir = dir.canonicalize().unwrap();
    assert!(Command::new("git").args(["init", "-q"]).current_dir(&dir).status().unwrap().success());
    let hook = format!("{} hook claude", env!("CARGO_BIN_EXE_relay"));
    let settings = serde_json::json!({
        "hooks": { "PostToolUse": [{ "matcher": "Bash", "hooks": [{ "type": "command", "command": hook }] }] }
    });
    std::fs::write(dir.join("settings.json"), settings.to_string()).unwrap();

    let run = Command::new("claude")
        .args(["-p", "Run this bash command exactly once: seq 1 400\nThen reply with only: done"])
        .args(["--settings", "settings.json", "--allowedTools", "Bash", "--model", "haiku"])
        .current_dir(&dir)
        .stdin(Stdio::null())
        .output()
        .expect("`claude` on PATH");
    assert!(run.status.success(), "claude -p failed: {}", String::from_utf8_lossy(&run.stderr));

    let results = tool_results(&newest_transcript(&claude_home().join("projects").join(slug(&dir))));
    let _ = std::fs::remove_dir_all(&dir);
    assert!(!results.is_empty(), "the model never ran the command");
    assert!(
        results.iter().any(|r| r.contains("relay get o_") && !r.contains("\n250\n")),
        "the model got the full output, not relay's view:\n{}",
        results.join("\n---\n")
    );
}
