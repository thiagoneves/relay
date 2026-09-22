//! Cheap git facts for briefs and handoffs.

use std::path::Path;
use std::process::Command;

#[derive(Debug, Default, Clone)]
pub struct GitState {
    pub branch: String,
    pub sha: String,
    pub dirty: Vec<String>,
    pub user: Option<String>,
}

fn git(root: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git").args(args).current_dir(root).output().ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// The committer's name, for provenance; `None` when git has none.
pub fn user(root: &Path) -> Option<String> {
    git(root, &["config", "user.name"]).filter(|s| !s.is_empty())
}

pub fn branch(root: &Path) -> String {
    git(root, &["rev-parse", "--abbrev-ref", "HEAD"]).unwrap_or_default()
}

pub fn state(root: &Path) -> GitState {
    let branch = branch(root);
    let sha = git(root, &["rev-parse", "--short", "HEAD"]).unwrap_or_default();
    let dirty = git(root, &["status", "--porcelain"])
        .map(|s| s.lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect())
        .unwrap_or_default();
    GitState { branch, sha, dirty, user: user(root) }
}

/// Files changed by commits since `sha`. Uncommitted edits do not count:
/// they may have been there when the item was saved. `None` when git does
/// not know the commit (rebased away, or a shallow clone).
pub fn changed_since(root: &Path, sha: &str) -> Option<Vec<String>> {
    git(root, &["diff", "--name-only", sha, "HEAD", "--"]).map(|s| s.lines().map(str::to_string).collect())
}

/// Files with the most commits in recent history, for the project brief.
pub fn hot_files(root: &Path, commits: usize, top: usize) -> Vec<(String, usize)> {
    let n = commits.to_string();
    let Some(out) = git(root, &["log", "--name-only", "--format=", "-n", &n]) else {
        return Vec::new();
    };
    let mut counts: std::collections::HashMap<String, usize> = std::collections::HashMap::default();
    for l in out.lines().map(str::trim).filter(|l| !l.is_empty()) {
        *counts.entry(l.to_string()).or_default() += 1;
    }
    let mut v: Vec<(String, usize)> = counts.into_iter().collect();
    v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    v.truncate(top);
    v
}

pub fn recent_commits(root: &Path, n: usize) -> Vec<String> {
    git(root, &["log", "--oneline", "-n", &n.to_string()])
        .map(|s| s.lines().map(str::to_string).collect())
        .unwrap_or_default()
}

pub fn tracked_files(root: &Path) -> Vec<String> {
    git(root, &["ls-files"]).map(|s| s.lines().map(str::to_string).collect()).unwrap_or_default()
}
