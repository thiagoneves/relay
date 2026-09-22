//! Where relay lives on this machine. Whatever binary the user ran (a
//! build dir, an npx cache, a download), relay copies itself to one stable
//! path, the same `~/.local/bin` Claude Code installs into, and hooks
//! point there. Running a newer build refreshes that copy.

use std::path::PathBuf;

use anyhow::{Context, Result};

use crate::core::paths::home;
use crate::helpers::profile::{self, PathChange};
use crate::helpers::shell;

pub struct Installed {
    /// The binary hooks should call.
    pub exe: PathBuf,
    /// The stable copy was created or refreshed by this run.
    pub updated: bool,
    pub path: PathChange,
}

pub fn bin_dir() -> PathBuf {
    home().join(".local").join("bin")
}

pub fn install_self() -> Result<Installed> {
    let running = std::fs::canonicalize(std::env::current_exe()?)?;
    let dir = bin_dir();
    let target = dir.join(format!("relay{}", std::env::consts::EXE_SUFFIX));
    let updated = copy_if_changed(&running, &target)?;

    let on_path =
        shell::which("relay").and_then(|p| std::fs::canonicalize(p).ok()) == std::fs::canonicalize(&target).ok();
    let path = if on_path { PathChange::Present } else { profile::ensure_on_path(&dir, &home())? };
    Ok(Installed { exe: target, updated, path })
}

/// Copy through a temp file and rename, so a hook starting mid-copy never
/// runs half a binary. A running binary can be replaced this way on Unix;
/// on Windows the rename fails while relay runs, and the old copy stays.
fn copy_if_changed(from: &std::path::Path, to: &std::path::Path) -> Result<bool> {
    if std::fs::canonicalize(to).ok().as_deref() == Some(from) {
        return Ok(false);
    }
    let new = std::fs::read(from)?;
    if std::fs::read(to).is_ok_and(|old| old == new) {
        return Ok(false);
    }
    let dir = to.parent().context("binary path has no parent")?;
    std::fs::create_dir_all(dir)?;
    let tmp = dir.join(format!(".relay-{}.tmp", std::process::id()));
    std::fs::copy(from, &tmp)?;
    if let Err(e) = std::fs::rename(&tmp, to) {
        let _ = std::fs::remove_file(&tmp);
        if to.exists() {
            return Ok(false);
        }
        return Err(e).with_context(|| format!("cannot install {}", to.display()));
    }
    Ok(true)
}
