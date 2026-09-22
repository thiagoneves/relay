//! A remembered item carries the commit and the paths it is about; the
//! brief says when those paths moved on since.

mod common;

use std::process::Command;

use common::Repo;

fn git(repo: &Repo, args: &[&str]) {
    let ok = Command::new("git")
        .args(["-c", "user.email=a@b.c", "-c", "user.name=t"])
        .args(args)
        .current_dir(&repo.root)
        .status()
        .unwrap()
        .success();
    assert!(ok, "git {args:?}");
}

fn brief(repo: &Repo) -> String {
    String::from_utf8(repo.run(&["brief"]).stdout).unwrap()
}

#[test]
fn an_item_about_a_file_that_changed_is_marked_stale() {
    let repo = Repo::new("memory-stale");
    std::fs::create_dir_all(repo.root.join("src")).unwrap();
    std::fs::write(repo.root.join("src/pay.rs"), "fn pay() {}\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "pay"]);
    let saved = repo.run(&["remember", "gotcha", "Amounts are in cents", "--path", "src/pay.rs"]);
    assert!(saved.status.success(), "{}", String::from_utf8_lossy(&saved.stderr));
    assert!(brief(&repo).contains("- gotcha: Amounts are in cents\n"), "{}", brief(&repo));

    std::fs::write(repo.root.join("src/pay.rs"), "fn pay(amount: f64) {}\n").unwrap();
    let out = brief(&repo);
    assert!(out.contains("Amounts are in cents _(may be stale: src/pay.rs changed since)_"), "{out}");
    let status = String::from_utf8(repo.run(&["status"]).stdout).unwrap();
    assert!(status.contains("1 may be stale"), "{status}");
}

#[test]
fn an_expired_item_leaves_the_brief_and_is_counted() {
    let repo = Repo::new("memory-expiry");
    assert!(repo.run(&["init"]).status.success());
    let saved = repo.run(&["remember", "gotcha", "Postgres 15 until the upgrade", "--until", "2000-01-01"]);
    assert!(saved.status.success(), "{}", String::from_utf8_lossy(&saved.stderr));
    assert!(!brief(&repo).contains("Postgres 15"), "{}", brief(&repo));
    let status = String::from_utf8(repo.run(&["status"]).stdout).unwrap();
    assert!(status.contains("1 expired, delete or renew"), "{status}");

    let bad = repo.run(&["remember", "gotcha", "x", "--until", "soon"]);
    assert!(!bad.status.success());
    assert!(String::from_utf8_lossy(&bad.stderr).contains("Use 30d, 12h or a date"));
    let ok = repo.run(&["remember", "gotcha", "Still valid", "--until", "30d"]);
    assert!(ok.status.success());
    assert!(brief(&repo).contains("Still valid"));
}

#[test]
fn local_memory_stays_out_of_the_repo() {
    let repo = Repo::new("memory-local");
    let init = repo.run(&["init", "--local"]);
    assert!(init.status.success(), "{}", String::from_utf8_lossy(&init.stderr));
    assert!(!repo.root.join(".relay").exists());
    assert!(repo.root.join(".git/relay/shared/project.md").exists());
    assert!(repo.run(&["remember", "rule", "Never force-push here"]).status.success());
    assert!(repo.root.join(".git/relay/shared/rules").read_dir().unwrap().next().is_some());
    assert!(brief(&repo).contains("Never force-push here"));
    let status = String::from_utf8(repo.run(&["status"]).stdout).unwrap();
    assert!(status.contains("local memory, never committed"), "{status}");
}
