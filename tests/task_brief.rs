//! `relay brief <query>`: the plan section for a task, the memory that
//! names it, and what the sessions that worked on it edited.

mod common;

use common::Repo;
use serde_json::json;

/// A plan doc, two commits (one about T-253, one about T-2530), a
/// remembered gotcha and a session that edited a file for T-253.
fn repo_with_history() -> Repo {
    let repo = Repo::new("task-brief");
    std::fs::create_dir_all(repo.root.join("docs")).unwrap();
    std::fs::write(
        repo.root.join("docs/plan.md"),
        "# Plan\n## Phase 1\n### T-25 · Login\nuses T-253\n### T-253 · Filters\nDeliver: the filter bar.\n### T-2530 · Later\nno\n",
    )
    .unwrap();
    std::fs::create_dir_all(repo.root.join("src")).unwrap();
    std::fs::write(repo.root.join("src/filters.ts"), "export {}\n").unwrap();
    std::fs::write(repo.root.join("src/bar.ts"), "export {}\n").unwrap();
    let git = |args: &[&str]| {
        let ok = std::process::Command::new("git").args(args).current_dir(&repo.root).status().unwrap().success();
        assert!(ok, "git {args:?}");
    };
    git(&["add", "docs/plan.md", "src/bar.ts"]);
    git(&["-c", "user.email=a@b.c", "-c", "user.name=t", "commit", "-q", "-m", "Filter bar for T-253"]);
    std::fs::write(repo.root.join("src/other.ts"), "x\n").unwrap();
    git(&["add", "src/other.ts"]);
    git(&["-c", "user.email=a@b.c", "-c", "user.name=t", "commit", "-q", "-m", "Unrelated T-2530 work"]);
    assert!(repo.run(&["remember", "gotcha", "T-253 filters reset on reload"]).status.success());
    repo.hook(
        "claude",
        json!({ "hook_event_name": "UserPromptSubmit", "session_id": "s1", "prompt": "implement T-253 now" }),
    );
    repo.hook(
        "claude",
        json!({
            "hook_event_name": "PostToolUse", "session_id": "s1", "tool_name": "Edit", "tool_use_id": "e1",
            "tool_input": { "file_path": repo.path("src/filters.ts") }, "tool_response": {}
        }),
    );

    repo
}

#[test]
fn a_task_id_brings_its_section_memory_and_files() {
    let repo = repo_with_history();
    let out = repo.run(&["brief", "T-253"]);
    let page = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(
        page.starts_with(
            "# relay brief: T-253\n\n## docs/plan.md L5-6\n### T-253 · Filters\nDeliver: the filter bar.\n"
        ),
        "{page}"
    );
    assert!(page.contains("## docs/plan.md L3-4\n### T-25 · Login\nuses T-253\n"), "{page}");
    assert!(!page.contains("T-2530"), "{page}");
    assert!(page.contains("- gotcha: T-253 filters reset on reload"), "{page}");
    assert!(page.contains("## Likely files\n- src/bar.ts · 1 commit\n- src/filters.ts · 1 session\n"), "{page}");
    assert!(!page.contains("src/other.ts"), "a commit about T-2530 is not about T-253: {page}");

    let none = repo.run(&["brief", "T-999"]);
    assert!(String::from_utf8_lossy(&none.stdout).contains("Nothing in the docs"), "no match is said");
}
