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

/// Replace a small, rebuildable state file by rename, without waiting for
/// the disk: a hook writes several per tool call, and an fsync each would
/// cost more than the rest of the hook. Readers still never see half a
/// file; a crash may lose the last update, which only costs a saving.
pub fn write_state(path: &Path, content: &[u8]) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension(format!("tmp{}", std::process::id()));
    fs::write(&tmp, content)?;
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
/// `path` with every non-alphanumeric character turned into `-`: a flat
/// directory name that stays stable for the same path.
pub fn path_slug(path: &Path) -> String {
    path.display().to_string().chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect()
}

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

/// The owner id of `path`, or `None` off Unix. Used as the current
/// user's id through the home directory: std does not expose `getuid`.
#[cfg(unix)]
pub fn owner(path: &Path) -> Option<u32> {
    use std::os::unix::fs::MetadataExt;
    fs::metadata(path).ok().map(|m| m.uid())
}

#[cfg(not(unix))]
pub fn owner(_: &Path) -> Option<u32> {
    None
}

/// Make `dir` a directory only its owner can open, or refuse one that
/// already exists as a symlink, with another owner or open to others. A
/// shared temp dir is where another user could pre-create it.
#[cfg(unix)]
pub fn ensure_private_dir(dir: &Path, uid: u32) -> std::io::Result<()> {
    use std::os::unix::fs::{DirBuilderExt, MetadataExt};
    match fs::DirBuilder::new().mode(0o700).create(dir) {
        Ok(()) => return Ok(()),
        Err(e) if e.kind() != std::io::ErrorKind::AlreadyExists => return Err(e),
        Err(_) => {}
    }
    let m = fs::symlink_metadata(dir)?;
    if !m.is_dir() || m.uid() != uid || m.mode() & 0o077 != 0 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            format!("{} is not a private directory of this user", dir.display()),
        ));
    }
    Ok(())
}

/// Windows keeps a temp dir per user already.
#[cfg(not(unix))]
pub fn ensure_private_dir(dir: &Path, _: u32) -> std::io::Result<()> {
    fs::create_dir_all(dir)
}

/// Append `line` to a log-style file, and past `max_bytes` cut it back
/// to its newest half. Never fails loudly: callers run where nothing can
/// report an error.
pub fn append_capped(file: &Path, line: &str, max_bytes: u64) {
    if let Some(dir) = file.parent()
        && fs::create_dir_all(dir).is_err()
    {
        return;
    }
    if let Ok(mut f) = fs::OpenOptions::new().create(true).append(true).open(file) {
        let _ = writeln!(f, "{line}");
    }
    if fs::metadata(file).is_ok_and(|m| m.len() > max_bytes) {
        keep_newest_half(file);
    }
}

fn keep_newest_half(file: &Path) {
    let Ok(text) = fs::read_to_string(file) else { return };
    let lines: Vec<&str> = text.lines().collect();
    let kept = lines[lines.len() / 2..].join("\n") + "\n";
    let _ = write_atomic(file, kept.as_bytes());
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

    #[cfg(unix)]
    #[test]
    fn private_dir_is_owner_only_and_refuses_an_open_one() {
        use std::os::unix::fs::PermissionsExt;
        let base = std::env::temp_dir().join(format!("relay-private-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        let uid = owner(&base).unwrap();
        let mine = base.join("mine");
        ensure_private_dir(&mine, uid).unwrap();
        assert_eq!(fs::metadata(&mine).unwrap().permissions().mode() & 0o777, 0o700);
        ensure_private_dir(&mine, uid).unwrap();

        let open = base.join("open");
        fs::create_dir(&open).unwrap();
        fs::set_permissions(&open, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(ensure_private_dir(&open, uid).is_err());
        assert!(ensure_private_dir(&mine, uid + 1).is_err());
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn a_capped_file_keeps_its_newest_half() {
        let file = std::env::temp_dir().join(format!("relay-capped-{}.log", std::process::id()));
        let _ = fs::remove_file(&file);
        for i in 1..=4 {
            append_capped(&file, &i.to_string(), 1024);
        }
        append_capped(&file, "5", 8);
        assert_eq!(fs::read_to_string(&file).unwrap(), "3\n4\n5\n");
        let _ = fs::remove_file(&file);
    }
}
