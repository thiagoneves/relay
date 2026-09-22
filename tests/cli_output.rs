//! What each command prints, pinned in `tests/snapshots/`. A change in
//! wording shows up in review instead of slipping by. Run with
//! `RELAY_UPDATE_SNAPSHOTS=1` to rewrite them after an intended change.

mod common;

use std::path::PathBuf;

use common::Repo;

fn normalize(text: &str) -> String {
    let size = regex::Regex::new(r"\d+(\.\d+)? (B|KB|MB|GB)\b").unwrap();
    let id = regex::Regex::new(r"o_[0-9a-f]+_[0-9a-f]+").unwrap();
    let backup = regex::Regex::new(r"relay-bak-\d+").unwrap();
    let time = regex::Regex::new(r"\d{4}-\d\d-\d\dT\d\d:\d\d:\d\dZ").unwrap();
    let text = size.replace_all(text, "<size>");
    let text = id.replace_all(&text, "<id>");
    let text = backup.replace_all(&text, "relay-bak-<time>");
    let text = text.replace("relay.exe", "relay");
    time.replace_all(&text, "<time>").into_owned()
}

fn check(name: &str, output: &std::process::Output) {
    let text = format!(
        "exit: {}\n--- stdout\n{}--- stderr\n{}",
        output.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let text = normalize(&text);
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots").join(format!("{name}.txt"));
    if std::env::var_os("RELAY_UPDATE_SNAPSHOTS").is_some() {
        std::fs::write(&path, &text).unwrap();
        return;
    }
    let expected = std::fs::read_to_string(&path)
        .unwrap_or_else(|_| panic!("no snapshot {name}; run with RELAY_UPDATE_SNAPSHOTS=1 and review it"));
    assert_eq!(expected.replace("\r\n", "\n"), text, "output of `{name}` changed");
}

#[test]
fn a_fresh_repo() {
    let repo = Repo::new("snap-fresh");
    check("status-fresh", &repo.run(&["status"]));
    check("handoff-none", &repo.run(&["handoff", "--show"]));
    check("audit-none", &repo.run(&["audit"]));
    check("init", &repo.run(&["init"]));
    check("init-again", &repo.run(&["init"]));
    check("purge-preview", &repo.run(&["purge"]));
}

#[test]
fn remembering() {
    let repo = Repo::new("snap-remember");
    check("remember", &repo.run(&["remember", "rule", "Run cargo test before committing"]));
    check("remember-duplicate", &repo.run(&["remember", "rule", "Run cargo test before committing"]));
    check("remember-empty", &repo.run(&["remember", "rule", " "]));
    check("status-remembered", &repo.run(&["status"]));
}

#[test]
fn getting_an_unknown_output() {
    let repo = Repo::new("snap-get");
    check("get-unknown", &repo.run(&["get", "o_123_abc"]));
}

#[test]
fn installing_hooks() {
    let repo = Repo::new("snap-install");
    // The next step depends on whether `claude` is installed; a stub on
    // PATH makes it the same everywhere (`.cmd` for Windows' PATHEXT).
    let bin = repo.root.join("fakebin");
    std::fs::create_dir_all(&bin).unwrap();
    for name in ["claude", "claude.cmd"] {
        std::fs::write(bin.join(name), "").unwrap();
    }
    let path = std::env::join_paths(
        std::iter::once(bin).chain(std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())),
    )
    .unwrap();
    let install = || repo.isolated(common::relay()).args(["install", "claude"]).env("PATH", &path).output().unwrap();
    check("install-claude", &install());
    check("install-claude-again", &install());
    check("uninstall-claude", &repo.run(&["uninstall", "claude"]));
    check("uninstall-claude-again", &repo.run(&["uninstall", "claude"]));
}

#[test]
fn a_failing_hook_shows_up() {
    use std::io::Write;
    let repo = Repo::new("snap-failure");
    let mut hook =
        repo.isolated(common::relay()).args(["hook", "claude"]).stdin(std::process::Stdio::piped()).spawn().unwrap();
    hook.stdin.take().unwrap().write_all(b"not json").unwrap();
    assert!(hook.wait().unwrap().success(), "a hook must exit 0 even when it fails");
    check("status-after-failure", &repo.run(&["status"]));
    check("log-after-failure", &repo.run(&["log"]));
}
