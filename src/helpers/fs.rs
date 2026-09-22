use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

/// Write through a temp file and rename, so readers never see half a
/// file. A symlink is followed, so a config kept in a dotfiles repo is
/// updated there instead of being replaced by a plain file, and the
/// replacement keeps the original's permissions (a 0600 config holding
/// tokens must not become world readable).
pub fn write_atomic(path: &Path, content: &[u8]) -> anyhow::Result<()> {
    let path = &link_target(path);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension(format!("tmp{}", std::process::id()));
    {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(content)?;
        f.sync_all().ok();
    }
    if let Ok(meta) = fs::metadata(path) {
        fs::set_permissions(&tmp, meta.permissions())?;
    }
    fs::rename(&tmp, path)?;
    Ok(())
}

/// The file a symlink chain ends at, even when that file does not exist
/// yet; `path` itself when it is not a link.
fn link_target(path: &Path) -> PathBuf {
    let mut p = path.to_path_buf();
    // Bounded: a link cycle must not hang a hook.
    for _ in 0..16 {
        let Ok(next) = fs::read_link(&p) else { break };
        p = match p.parent() {
            Some(dir) if next.is_relative() => dir.join(next),
            _ => next,
        };
    }
    p
}

/// Directory size in bytes, best effort.
pub fn dir_size(path: &Path) -> u64 {
    let mut total = 0;
    if let Ok(rd) = fs::read_dir(path) {
        for entry in rd.flatten() {
            let p = entry.path();
            if p.is_dir() {
                total += dir_size(&p);
            } else if let Ok(m) = entry.metadata() {
                total += m.len();
            }
        }
    }
    total
}

/// Display with forward slashes on Windows, so paths in handoffs and
/// hook commands read the same on every machine.
pub fn slash(p: &Path) -> String {
    let s = p.display().to_string();
    if cfg!(windows) { s.replace('\\', "/") } else { s }
}

/// Drop the `\\?\` prefix `canonicalize` adds on Windows for local
/// drives, so the result compares equal to paths reported by git.
pub fn simplify(p: PathBuf) -> PathBuf {
    let s = p.to_string_lossy();
    match s.strip_prefix(r"\\?\") {
        Some(rest) if rest.as_bytes().get(1) == Some(&b':') => PathBuf::from(rest),
        _ => p,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simplify_strips_verbatim_drive_prefix_only() {
        assert_eq!(simplify(PathBuf::from(r"\\?\C:\repo")), PathBuf::from(r"C:\repo"));
        assert_eq!(simplify(PathBuf::from(r"\\?\UNC\srv\share")), PathBuf::from(r"\\?\UNC\srv\share"));
        assert_eq!(simplify(PathBuf::from("/repo")), PathBuf::from("/repo"));
    }

    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("relay-fs-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[cfg(unix)]
    #[test]
    fn write_atomic_updates_the_link_target_and_keeps_its_mode() {
        use std::os::unix::fs::PermissionsExt;
        let d = scratch("link");
        let real = d.join("dotfiles/settings.json");
        fs::create_dir_all(real.parent().unwrap()).unwrap();
        fs::write(&real, "{}").unwrap();
        fs::set_permissions(&real, fs::Permissions::from_mode(0o600)).unwrap();
        let link = d.join("settings.json");
        std::os::unix::fs::symlink("dotfiles/settings.json", &link).unwrap();

        write_atomic(&link, b"{\"a\":1}").unwrap();

        assert!(fs::symlink_metadata(&link).unwrap().file_type().is_symlink());
        assert_eq!(fs::read_to_string(&real).unwrap(), "{\"a\":1}");
        assert_eq!(fs::metadata(&real).unwrap().permissions().mode() & 0o777, 0o600);
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn write_atomic_creates_missing_files() {
        let d = scratch("new");
        let p = d.join("a/b.json");
        write_atomic(&p, b"x").unwrap();
        assert_eq!(fs::read_to_string(&p).unwrap(), "x");
        let _ = fs::remove_dir_all(&d);
    }
}
