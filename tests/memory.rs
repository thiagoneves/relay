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
    assert!(!brief(&repo).contains("may be stale"), "an uncommitted edit is not a change since");
    git(&repo, &["commit", "-qam", "pay takes an amount"]);
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

#[test]
fn compile_promotes_decisions_from_handoffs() {
    let repo = Repo::new("memory-compile");
    assert!(repo.run(&["init"]).status.success());
    let dir = repo.root.join(".git/relay/handoffs");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("c-1.md"),
        "---\nsession: c-1\nharness: claude-code\nbranch: main\nended: 2026-09-22T10:00:00Z\n---\n# Handoff\n## Decisions\n- Backoff: exponential with jitter\n- DB: postgres\n",
    )
    .unwrap();
    let listed = String::from_utf8(repo.run(&["compile"]).stdout).unwrap();
    assert!(listed.contains("1. 2026-09-22") && listed.contains("Backoff: exponential with jitter"), "{listed}");
    assert!(listed.contains("2. 2026-09-22") && listed.contains("DB: postgres"), "{listed}");

    let bad = repo.run(&["compile", "--save", "9"]);
    assert!(!bad.status.success());
    let saved = repo.run(&["compile", "--save", "2"]);
    assert!(saved.status.success(), "{}", String::from_utf8_lossy(&saved.stderr));
    let items: Vec<_> = std::fs::read_dir(repo.root.join(".relay/decisions")).unwrap().flatten().collect();
    assert_eq!(items.len(), 1);
    assert!(std::fs::read_to_string(items[0].path()).unwrap().contains("DB: postgres"));

    let again = String::from_utf8(repo.run(&["compile"]).stdout).unwrap();
    assert!(again.contains("Backoff") && !again.contains("DB: postgres"), "{again}");
    assert!(repo.run(&["compile", "--save", "all"]).status.success());
    assert!(String::from_utf8(repo.run(&["compile"]).stdout).unwrap().contains("Nothing new to promote"));
}

#[test]
fn init_points_agent_instruction_files_at_the_memory() {
    let repo = Repo::new("memory-agents-md");
    std::fs::write(repo.root.join("CLAUDE.md"), "# House rules\n\nRun the tests.\n").unwrap();
    let init = repo.run(&["init"]);
    assert!(init.status.success(), "{}", String::from_utf8_lossy(&init.stderr));
    let agents = std::fs::read_to_string(repo.root.join("AGENTS.md")).unwrap();
    assert!(agents.starts_with("<!-- relay -->") && agents.contains("`.relay/project.md`"), "{agents}");
    let claude = std::fs::read_to_string(repo.root.join("CLAUDE.md")).unwrap();
    assert!(claude.starts_with("# House rules") && claude.contains("<!-- /relay -->"), "{claude}");
    assert!(repo.run(&["init"]).status.success());
    assert_eq!(std::fs::read_to_string(repo.root.join("AGENTS.md")).unwrap(), agents, "a second init changes nothing");

    let plain = Repo::new("memory-no-agents-md");
    assert!(plain.run(&["init", "--no-agents-md"]).status.success());
    assert!(!plain.root.join("AGENTS.md").exists());
}

#[test]
fn terse_adds_the_answers_section_once() {
    let repo = Repo::new("memory-terse");
    assert!(repo.run(&["init", "--terse"]).status.success());
    let project = std::fs::read_to_string(repo.root.join(".relay/project.md")).unwrap();
    assert_eq!(project.matches("## Answers").count(), 1, "{project}");
    assert!(repo.run(&["init", "--terse"]).status.success());
    assert_eq!(std::fs::read_to_string(repo.root.join(".relay/project.md")).unwrap(), project);
    assert!(String::from_utf8(repo.run(&["brief"]).stdout).unwrap().contains("Lead with the answer"));
}

#[test]
fn hygiene_names_near_duplicates_to_merge() {
    let repo = Repo::new("memory-hygiene");
    assert!(repo.run(&["remember", "rule", "Run cargo test before every commit"]).status.success());
    assert!(repo.run(&["remember", "rule", "Run cargo test before each commit"]).status.success());
    let out = repo.run(&["compile", "--hygiene"]);
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{text}");
    assert!(text.contains("Merge") && text.contains("say nearly the same; keep one"), "{text}");
}
