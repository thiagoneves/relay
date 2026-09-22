//! Where relay lives on this machine. Whatever binary the user ran (a
//! build dir, an npx cache, a download), relay copies itself to one stable
//! path, the same `~/.local/bin` Claude Code installs into, and hooks
//! point there. Running a newer build refreshes that copy.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::helpers::env::home;
use crate::helpers::profile::{self, PathChange};
use crate::helpers::shell;

pub struct Installed {
    /// The binary hooks should call.
    pub exe: PathBuf,
    pub refresh: Refresh,
    pub path: PathChange,
}

/// What happened to the stable copy on this run.
pub enum Refresh {
    Unchanged,
    Updated,
    /// The stable path could not be replaced; hooks keep running the old
    /// build until a later run succeeds.
    KeptOld(String),
}

pub fn bin_dir() -> PathBuf {
    home().join(".local").join("bin")
}

pub fn install_self() -> Result<Installed> {
    let running = std::fs::canonicalize(std::env::current_exe()?)?;
    let dir = bin_dir();
    let target = dir.join(format!("relay{}", std::env::consts::EXE_SUFFIX));
    let refresh = copy_if_changed(&running, &target)?;

    let on_path =
        shell::which("relay").and_then(|p| std::fs::canonicalize(p).ok()) == std::fs::canonicalize(&target).ok();
    let path = if on_path { PathChange::Present } else { profile::ensure_on_path(&dir, &home()) };
    Ok(Installed { exe: target, refresh, path })
}

/// Copy through a temp file and rename, so a hook starting mid-copy never
/// runs half a binary.
fn copy_if_changed(from: &Path, to: &Path) -> Result<Refresh> {
    if std::fs::canonicalize(to).ok().as_deref() == Some(from) {
        return Ok(Refresh::Unchanged);
    }
    let new = std::fs::read(from)?;
    if std::fs::read(to).is_ok_and(|old| old == new) {
        return Ok(Refresh::Unchanged);
    }
    let dir = to.parent().context("binary path has no parent")?;
    std::fs::create_dir_all(dir)?;
    let tmp = dir.join(format!(".relay-{}.tmp", std::process::id()));
    std::fs::copy(from, &tmp)?;
    let outcome = replace(&tmp, to);
    let _ = std::fs::remove_file(&tmp);
    match outcome {
        Ok(()) => Ok(Refresh::Updated),
        Err(e) if to.exists() => Ok(Refresh::KeptOld(e.to_string())),
        Err(e) => Err(e).with_context(|| format!("cannot install {}", to.display())),
    }
}

/// Windows refuses to replace a running executable but lets it be
/// renamed: an open `relay claude` would otherwise pin the old build. The
/// set-aside copy is removed by the next run, once nothing runs it.
fn replace(tmp: &Path, to: &Path) -> std::io::Result<()> {
    let aside = aside_path(to);
    let _ = std::fs::remove_file(&aside);
    let Err(first) = std::fs::rename(tmp, to) else { return Ok(()) };
    if !to.exists() || std::fs::rename(to, &aside).is_err() {
        return Err(first);
    }
    std::fs::rename(tmp, to).inspect_err(|_| {
        let _ = std::fs::rename(&aside, to);
    })
}

/// `relay.exe` → `relay.old.exe`, `relay` → `relay.old`.
fn aside_path(to: &Path) -> PathBuf {
    let stem = to.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let name = match to.extension() {
        Some(ext) => format!("{stem}.old.{}", ext.to_string_lossy()),
        None => format!("{stem}.old"),
    };
    to.with_file_name(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("relay-machine-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d.canonicalize().unwrap()
    }

    #[test]
    fn copies_only_when_the_binary_changed() {
        let d = scratch("copy");
        let (from, to) = (d.join("build"), d.join("bin").join("relay"));
        std::fs::write(&from, b"v1").unwrap();

        assert!(matches!(copy_if_changed(&from, &to).unwrap(), Refresh::Updated));
        assert!(matches!(copy_if_changed(&from, &to).unwrap(), Refresh::Unchanged));
        std::fs::write(&from, b"v2").unwrap();
        assert!(matches!(copy_if_changed(&from, &to).unwrap(), Refresh::Updated));
        assert_eq!(std::fs::read(&to).unwrap(), b"v2");
        assert!(matches!(copy_if_changed(&to, &to).unwrap(), Refresh::Unchanged), "running the stable copy itself");
        assert!(std::fs::read_dir(to.parent().unwrap()).unwrap().count() == 1, "no temp or aside file left");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn names_the_set_aside_copy() {
        assert_eq!(aside_path(Path::new("/b/relay.exe")), Path::new("/b/relay.old.exe"));
        assert_eq!(aside_path(Path::new("/b/relay")), Path::new("/b/relay.old"));
    }
}
