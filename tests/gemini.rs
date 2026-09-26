//! relay's hooks as Gemini CLI calls them: Gemini's event names and
//! payloads in, Gemini-shaped JSON out.

mod common;

use common::Repo;
use serde_json::{Value, json};

fn before_tool(repo: &Repo, command: &str) -> String {
    repo.hook(
        "gemini",
        json!({ "hook_event_name": "BeforeTool", "session_id": "g1", "transcript_path": "",
                "tool_name": "run_shell_command", "tool_input": { "command": command } }),
    )
    .trim()
    .to_string()
}

/// On Windows Gemini CLI runs commands in `PowerShell`, which `relay x`
/// does not speak, so nothing is rewritten there.
#[cfg(windows)]
#[test]
fn nothing_is_rewritten_on_windows() {
    let repo = Repo::new("gemini-pre-win");
    assert_eq!(before_tool(&repo, "cargo test"), "");
}

#[cfg(unix)]
#[test]
fn routine_commands_are_rewritten_and_the_rest_left_alone() {
    let repo = Repo::new("gemini-pre");
    let out: Value = serde_json::from_str(&before_tool(&repo, "cargo test")).unwrap();
    assert_eq!(out["decision"], "allow");
    let command = out["hookSpecificOutput"]["tool_input"]["command"].as_str().unwrap();
    assert!(command.contains(" x --session 'g1' -- 'cargo test'"), "{out}");
    assert_eq!(before_tool(&repo, "rm -rf build"), "");
}

#[test]
fn session_start_puts_the_brief_in_context() {
    let repo = Repo::new("gemini-start");
    assert!(repo.run(&["init"]).status.success());
    let out =
        repo.hook("gemini", json!({ "hook_event_name": "SessionStart", "session_id": "g1", "source": "startup" }));
    let v: Value = serde_json::from_str(out.trim()).unwrap();
    assert!(v["hookSpecificOutput"]["additionalContext"].as_str().unwrap().starts_with("# relay brief"), "{out}");
}

#[test]
fn install_adds_hooks_to_settings_and_keeps_the_rest() {
    let repo = Repo::new("gemini-install");
    let file = repo.root.join(".gemini/settings.json");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, r#"{"selectedAuthType":"oauth-personal","general":{"vimMode":false}}"#).unwrap();
    assert!(repo.run(&["install", "gemini"]).status.success());
    let v: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    assert_eq!(v["selectedAuthType"], "oauth-personal");
    let group = &v["hooks"]["BeforeTool"][0];
    assert_eq!(group["matcher"], "run_shell_command|read_file");
    assert!(group["hooks"][0]["command"].as_str().unwrap().ends_with(" hook gemini"));
    assert!(repo.run(&["uninstall", "gemini"]).status.success());
    let v: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    assert!(v.get("hooks").is_none(), "{v}");
}
