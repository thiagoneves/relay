//! Where relay keeps things. Two tiers only:
//!  - `.relay/` at the repo root: committed, shared by choice.
//!  - `<gitdir>/relay/`: local, per worktree, never committed.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result};

use crate::helpers::env::{self, Var};
use crate::helpers::fs::{path_slug, simplify};
use crate::helpers::slash;

#[derive(Debug, Clone)]
pub struct Paths {
    /// Repo root (or cwd when not inside a git repo).
    pub root: PathBuf,
    /// Committed tier: `<root>/.relay`.
    pub shared: PathBuf,
    /// Local tier: `<gitdir>/relay` (falls back to a per-project dir under
    /// the user's data dir when there is no git repo).
    pub local: PathBuf,
    pub in_git: bool,
}

impl Paths {
    pub fn discover(start: &Path) -> Result<Self> {
        let start = if start.is_absolute() { start.to_path_buf() } else { std::env::current_dir()?.join(start) };
        if let Some((root, gitdir)) = git_paths(&start) {
            Ok(Self { shared: root.join(".relay"), local: gitdir.join("relay"), root, in_git: true })
        } else {
            let root = start.clone();
            let slug = path_slug(&root);
            Ok(Self {
                shared: root.join(".relay"),
                local: data_home().join("projects").join(slug),
                root,
                in_git: false,
            })
        }
    }

    pub fn from_cwd() -> Result<Self> {
        let cwd = std::env::current_dir().context("cannot read cwd")?;
        Self::discover(&cwd)
    }

    pub fn spool(&self) -> PathBuf {
        self.local.join("spool")
    }
    pub fn outputs(&self) -> PathBuf {
        self.local.join("outputs")
    }
    pub fn handoffs(&self) -> PathBuf {
        self.local.join("handoffs")
    }
    pub fn current_session_file(&self) -> PathBuf {
        self.local.join("current_session")
    }
    pub fn last_session_file(&self) -> PathBuf {
        self.local.join("last_session")
    }
    pub fn fetches_file(&self) -> PathBuf {
        self.local.join("fetches.jsonl")
    }
    pub fn log_file(&self) -> PathBuf {
        self.local.join("relay.log")
    }
    pub fn project_file(&self) -> PathBuf {
        self.shared.join("project.md")
    }

    pub fn ensure_local(&self) -> Result<()> {
        for d in [self.spool(), self.outputs(), self.handoffs()] {
            std::fs::create_dir_all(&d).with_context(|| format!("mkdir {}", d.display()))?;
        }
        Ok(())
    }

    /// Repo-relative form of a path reported by a harness, which may go
    /// through a symlink the git toplevel does not (`/var` vs
    /// `/private/var` on macOS) or differ in drive-letter case and
    /// separators (Windows). Unchanged when outside the repo.
    pub fn rel_file(&self, f: &str) -> String {
        let p = Path::new(f);
        if let Ok(r) = p.strip_prefix(&self.root) {
            return slash(r);
        }
        let Ok(root) = self.root.canonicalize().map(simplify) else { return f.to_string() };
        // The file may be gone; canonicalize the deepest ancestor that exists.
        for anc in p.ancestors().skip(1) {
            if let Ok(canon) = anc.canonicalize().map(simplify) {
                let rest = p.strip_prefix(anc).unwrap_or(p);
                return canon.join(rest).strip_prefix(&root).map_or_else(|_| f.to_string(), slash);
            }
        }
        f.to_string()
    }

    pub fn rel(&self, p: &Path) -> String {
        p.strip_prefix(&self.root).map_or_else(|_| slash(p), slash)
    }
}

/// Returns (toplevel, absolute gitdir). The gitdir is worktree specific
/// (`.git/worktrees/<name>` inside a linked worktree), which gives relay
/// per-worktree isolation for free.
fn git_paths(start: &Path) -> Option<(PathBuf, PathBuf)> {
    let out = Command::new("git")
        .args(["rev-parse", "--show-toplevel", "--absolute-git-dir"])
        .current_dir(start)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let mut lines = text.lines();
    let top = PathBuf::from(lines.next()?.trim());
    let gitdir = PathBuf::from(lines.next()?.trim());
    Some((top, gitdir))
}

/// `$XDG_DATA_HOME/relay`, else `%LOCALAPPDATA%\\relay` on Windows, else
/// `~/.local/share/relay`.
pub fn data_home() -> PathBuf {
    if let Some(x) = env::path(Var::XdgDataHome) {
        return x.join("relay");
    }
    if cfg!(windows)
        && let Some(local) = env::path(Var::LocalAppData)
    {
        return local.join("relay");
    }
    env::home().join(".local").join("share").join("relay")
}
