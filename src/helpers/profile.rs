//! Putting a directory on the user's PATH through their shell's startup
//! file. Append-only and idempotent: a line is added once and never
//! rewritten, so undoing it is deleting that one line.

use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

pub enum PathChange {
    /// The directory is already in this process's PATH.
    Present,
    /// A startup file already adds it; a new terminal will see it.
    InProfile(PathBuf),
    Added(PathBuf),
    /// No startup file relay knows how to edit; the user has to do it.
    Manual,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Shell {
    Zsh,
    Bash,
    Fish,
}

pub fn ensure_on_path(dir: &Path, home: &Path) -> Result<PathChange> {
    if std::env::var_os("PATH").is_some_and(|p| std::env::split_paths(&p).any(|d| d == dir)) {
        return Ok(PathChange::Present);
    }
    let Some(shell) = login_shell() else { return Ok(PathChange::Manual) };
    let file = startup_file(shell, home);
    let shown = display_dir(dir, home);
    let existing = std::fs::read_to_string(&file).unwrap_or_default();
    if existing.contains(&shown) || existing.contains(&*dir.to_string_lossy()) {
        return Ok(PathChange::InProfile(file));
    }
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&file)
        .with_context(|| format!("cannot open {}", file.display()))?;
    let sep = if existing.is_empty() || existing.ends_with('\n') { "" } else { "\n" };
    write!(f, "{sep}\n# relay\n{}\n", path_line(shell, &shown))?;
    Ok(PathChange::Added(file))
}

/// The shell a new terminal opens, from `$SHELL`. `None` on Windows, where
/// PATH lives in the registry, not in a startup file.
fn login_shell() -> Option<Shell> {
    if cfg!(windows) {
        return None;
    }
    let shell = std::env::var("SHELL").ok()?;
    match Path::new(&shell).file_name()?.to_str()? {
        "zsh" => Some(Shell::Zsh),
        "bash" => Some(Shell::Bash),
        "fish" => Some(Shell::Fish),
        _ => None,
    }
}

/// macOS terminals start bash as a login shell, which reads
/// `.bash_profile` and skips `.bashrc`.
fn startup_file(shell: Shell, home: &Path) -> PathBuf {
    match shell {
        Shell::Zsh => std::env::var_os("ZDOTDIR").map_or_else(|| home.to_path_buf(), PathBuf::from).join(".zshrc"),
        Shell::Bash if cfg!(target_os = "macos") => home.join(".bash_profile"),
        Shell::Bash => home.join(".bashrc"),
        Shell::Fish => home.join(".config").join("fish").join("config.fish"),
    }
}

/// `$HOME/...` when under home, so the line survives a renamed user.
fn display_dir(dir: &Path, home: &Path) -> String {
    match dir.strip_prefix(home) {
        Ok(rest) => format!("$HOME/{}", rest.display()),
        Err(_) => dir.display().to_string(),
    }
}

fn path_line(shell: Shell, dir: &str) -> String {
    match shell {
        Shell::Fish => format!("fish_add_path \"{dir}\""),
        Shell::Zsh | Shell::Bash => format!("export PATH=\"{dir}:$PATH\""),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_home_relative_lines() {
        let home = Path::new("/Users/me");
        let dir = display_dir(Path::new("/Users/me/.cargo/bin"), home);
        assert_eq!(path_line(Shell::Zsh, &dir), "export PATH=\"$HOME/.cargo/bin:$PATH\"");
        assert_eq!(path_line(Shell::Fish, &dir), "fish_add_path \"$HOME/.cargo/bin\"");
        assert_eq!(display_dir(Path::new("/opt/bin"), home), "/opt/bin");
    }
}
