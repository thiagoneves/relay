//! The command lines relay writes into other programs' configs, and the
//! ones its hook hands back to the harness. Installed hooks keep calling
//! these after an upgrade, so changing their shape breaks every install
//! without a word.

mod common;

use std::io::Write;
use std::process::Stdio;

use common::Repo;
use serde_json::Value;

/// Run a hook command line as the harness would: the argv after the
/// executable, an empty event on stdin.
fn run_hook(repo: &Repo, args: &[&str]) -> std::process::Output {
    let mut child = repo
        .isolated(common::relay())
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(b"{}").unwrap();
    child.wait_with_output().unwrap()
}

#[test]
fn every_hook_command_ever_installed_still_parses() {
    let repo = Repo::new("contract-hooks");
    // `claude-code` is what the first releases wrote.
    for harness in ["claude", "codex", "claude-code"] {
        let out = run_hook(&repo, &["hook", harness]);
        assert!(out.status.success(), "relay hook {harness}: {}", String::from_utf8_lossy(&out.stderr));
    }
}

#[test]
fn installed_settings_call_a_command_relay_accepts() {
    let repo = Repo::new("contract-install");
    assert!(repo.run(&["install", "claude"]).status.success());
    let settings: Value =
        serde_json::from_str(&std::fs::read_to_string(repo.root.join("settings.json")).unwrap()).unwrap();
    let commands: Vec<&str> = settings["hooks"]
        .as_object()
        .unwrap()
        .values()
        .flat_map(|groups| groups.as_array().unwrap())
        .flat_map(|g| g["hooks"].as_array().unwrap())
        .filter_map(|h| h["command"].as_str())
        .collect();
    assert!(!commands.is_empty());
    for command in commands {
        let args: Vec<&str> = command.split_whitespace().skip(1).collect();
        let out = run_hook(&repo, &args);
        assert!(out.status.success(), "`{command}`: {}", String::from_utf8_lossy(&out.stderr));
    }
}

#[test]
fn the_rewrite_the_hook_hands_back_runs() {
    let repo = Repo::new("contract-x");
    for args in [
        &["x", "--session", "s1", "--", "echo contract-ok"][..],
        &["x", "--session", "s1", "--stop-after-ms", "60000", "--", "echo contract-ok"],
    ] {
        let out = repo.run(args);
        assert!(out.status.success(), "{args:?}: {}", String::from_utf8_lossy(&out.stderr));
        assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "contract-ok");
    }
}
