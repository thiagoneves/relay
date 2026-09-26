//! `relay lint` as git hooks: exit 1 on a doc or subject over budget.

mod common;

use common::Repo;

#[test]
fn staged_docs_and_commit_messages_are_checked() {
    let repo = Repo::new("lint");
    std::fs::write(repo.root.join("AGENTS.md"), "rule\n".repeat(2000)).unwrap();
    std::fs::write(repo.root.join("notes.md"), "# Notes\n").unwrap();
    assert!(
        std::process::Command::new("git")
            .args(["add", "AGENTS.md"])
            .current_dir(&repo.root)
            .status()
            .unwrap()
            .success()
    );

    let staged = repo.run(&["lint", "--staged"]);
    let text = String::from_utf8_lossy(&staged.stdout);
    assert_eq!(staged.status.code(), Some(1), "{text}");
    assert!(text.contains("AGENTS.md is ") && !text.contains("notes.md"), "{text}");

    let msg = repo.root.join("MSG");
    std::fs::write(&msg, "Fix the thing\n\nWhy it broke.\n").unwrap();
    assert_eq!(repo.run(&["lint", "--commit-msg", msg.to_str().unwrap()]).status.code(), Some(0));
    std::fs::write(&msg, format!("{}\n", "word ".repeat(20))).unwrap();
    let long = repo.run(&["lint", "--commit-msg", msg.to_str().unwrap()]);
    assert_eq!(long.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&long.stdout).contains("subject is 100 characters"));
}
