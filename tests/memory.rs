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
