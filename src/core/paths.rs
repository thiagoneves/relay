//! Where relay keeps things. Two tiers only:
//!  - `.relay/` at the repo root: committed, shared by choice.
//!  - `<gitdir>/relay/`: local, per worktree, never committed.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result};

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
            let slug = slugify(&root);
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
    pub fn claims(&self) -> PathBuf {
        self.local.join("claims")
    }
    pub fn current_session_file(&self) -> PathBuf {
        self.local.join("current_session")
    }
    pub fn log_file(&self) -> PathBuf {
        self.local.join("relay.log")
    }
    pub fn project_file(&self) -> PathBuf {
        self.shared.join("project.md")
    }

    pub fn ensure_local(&self) -> Result<()> {
        for d in [self.spool(), self.outputs(), self.handoffs(), self.claims()] {
            std::fs::create_dir_all(&d).with_context(|| format!("mkdir {}", d.display()))?;
        }
        Ok(())
    }

    pub fn rel(&self, p: &Path) -> String {
        p.strip_prefix(&self.root).map_or_else(|_| p.display().to_string(), |r| r.display().to_string())
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

pub fn home() -> PathBuf {
    std::env::var_os("HOME").map_or_else(|| PathBuf::from("/tmp"), PathBuf::from)
}

/// `$XDG_DATA_HOME/relay` or `~/.local/share/relay`.
pub fn data_home() -> PathBuf {
    std::env::var_os("XDG_DATA_HOME").map_or_else(|| home().join(".local/share"), PathBuf::from).join("relay")
}

fn slugify(p: &Path) -> String {
    p.display().to_string().chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect()
}

/// Append one line to the local log. Never fails loudly.
pub fn log(paths: &Paths, msg: &str) {
    use std::io::Write;
    if std::fs::create_dir_all(&paths.local).is_err() {
        return;
    }
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(paths.log_file()) {
        let _ = writeln!(f, "{} {}", crate::helpers::now_iso(), msg);
    }
}
