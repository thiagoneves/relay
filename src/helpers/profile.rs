//! Putting a directory on the user's PATH through their shell's startup
//! file. Append-only and idempotent: a line is added once and never
//! rewritten, so undoing it is deleting that one line.
//!
//! Deciding (which file, what text) is pure; `ensure_on_path` and
//! `update_profile` are the only functions that touch the environment or
//! the disk.

use std::io::Write;
use std::path::{Path, PathBuf};

use super::env::{self, Var};

pub enum PathChange {
    /// The directory is already in this process's PATH.
    Present,
    /// A startup file already adds it; a new terminal will see it.
    InProfile(PathBuf),
    Added(PathBuf),
    /// No startup file relay can edit (unknown shell, Windows, or the file
    /// is not writable); the user has to do it.
    Manual,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Shell {
    Zsh,
    Bash,
    Fish,
}

/// Never fails: a profile relay cannot write is the user's to edit, and
/// must not stop setup, install or the wrapper.
pub fn ensure_on_path(dir: &Path, home: &Path) -> PathChange {
    if env::get(Var::Path).is_some_and(|p| std::env::split_paths(&p).any(|d| d == dir)) {
        return PathChange::Present;
    }
    let Some(shell) = env::text(Var::Shell).and_then(|s| shell_named(&s)) else {
        return PathChange::Manual;
    };
    let zdotdir = env::path(Var::ZDotDir);
    let file = startup_file(shell, home, zdotdir.as_deref(), cfg!(target_os = "macos"), Path::exists);
    update_profile(&file, shell, dir, home)
}

/// Read, decide, append. Any IO error means the user edits it by hand.
fn update_profile(file: &Path, shell: Shell, dir: &Path, home: &Path) -> PathChange {
    let existing = std::fs::read_to_string(file).unwrap_or_default();
    let shown = display_dir(dir, home);
    if mentions_dir(&existing, &[&shown, &dir.to_string_lossy()]) {
        return PathChange::InProfile(file.to_path_buf());
    }
    match append(file, &addition(&existing, shell, &shown)) {
        Ok(()) => PathChange::Added(file.to_path_buf()),
        Err(_) => PathChange::Manual,
    }
}

fn append(file: &Path, text: &str) -> std::io::Result<()> {
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::OpenOptions::new().create(true).append(true).open(file)?.write_all(text.as_bytes())
}

/// The shell a new terminal opens, from `$SHELL`. `None` on Windows, where
/// PATH lives in the registry, not in a startup file.
fn shell_named(shell: &str) -> Option<Shell> {
    if cfg!(windows) {
        return None;
    }
    match Path::new(shell).file_name()?.to_str()? {
        "zsh" => Some(Shell::Zsh),
        "bash" => Some(Shell::Bash),
        "fish" => Some(Shell::Fish),
        _ => None,
    }
}

/// macOS terminals start bash as a login shell, which reads only the
/// first of `.bash_profile`, `.bash_login`, `.profile`: creating
/// `.bash_profile` next to an existing `.profile` would hide the latter.
fn startup_file(
    shell: Shell,
    home: &Path,
    zdotdir: Option<&Path>,
    macos: bool,
    exists: impl Fn(&Path) -> bool,
) -> PathBuf {
    match shell {
        Shell::Zsh => zdotdir.unwrap_or(home).join(".zshrc"),
        Shell::Bash if macos => [".bash_profile", ".bash_login", ".profile"]
            .iter()
            .map(|f| home.join(f))
            .find(|p| exists(p))
            .unwrap_or_else(|| home.join(".bash_profile")),
        Shell::Bash => home.join(".bashrc"),
        Shell::Fish => home.join(".config").join("fish").join("config.fish"),
    }
}

/// Whether a non-comment line names one of `dirs` as a whole path, not as
/// the prefix of a longer one (`.local/bin2`).
fn mentions_dir(text: &str, dirs: &[&str]) -> bool {
    text.lines().map(str::trim).filter(|l| !l.starts_with('#')).any(|line| {
        dirs.iter().filter(|d| !d.is_empty()).any(|d| {
            line.match_indices(d).any(|(i, _)| {
                line[i + d.len()..].chars().next().is_none_or(|c| matches!(c, ':' | '"' | '\'' | '/' | ' ' | ';'))
            })
        })
    })
}

/// What to append: a blank line, relay's marker, the PATH line.
fn addition(existing: &str, shell: Shell, shown: &str) -> String {
    let sep = if existing.is_empty() || existing.ends_with('\n') { "" } else { "\n" };
    format!("{sep}\n# relay\n{}\n", path_line(shell, shown))
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

    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("relay-profile-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn writes_home_relative_lines() {
        let home = Path::new("/Users/me");
        let dir = display_dir(Path::new("/Users/me/.cargo/bin"), home);
        assert_eq!(path_line(Shell::Zsh, &dir), "export PATH=\"$HOME/.cargo/bin:$PATH\"");
        assert_eq!(path_line(Shell::Fish, &dir), "fish_add_path \"$HOME/.cargo/bin\"");
        assert_eq!(display_dir(Path::new("/opt/bin"), home), "/opt/bin");
    }

    #[test]
    fn macos_bash_appends_to_the_file_bash_actually_reads() {
        let home = Path::new("/h");
        let has = |names: &'static [&'static str]| move |p: &Path| names.iter().any(|n| p == home.join(n));
        let pick = |names| startup_file(Shell::Bash, home, None, true, has(names));
        assert_eq!(pick(&[".profile"]), home.join(".profile"));
        assert_eq!(pick(&[".profile", ".bash_login"]), home.join(".bash_login"));
        assert_eq!(pick(&[".profile", ".bash_profile"]), home.join(".bash_profile"));
        assert_eq!(pick(&[]), home.join(".bash_profile"));
        assert_eq!(startup_file(Shell::Bash, home, None, false, has(&[".profile"])), home.join(".bashrc"));
        assert_eq!(startup_file(Shell::Zsh, home, Some(Path::new("/z")), true, |_| false), Path::new("/z/.zshrc"));
    }

    #[test]
    fn only_live_whole_path_mentions_count() {
        let d = ["$HOME/.local/bin"];
        assert!(mentions_dir("export PATH=\"$HOME/.local/bin:$PATH\"", &d));
        assert!(mentions_dir("fish_add_path \"$HOME/.local/bin\"", &d));
        assert!(mentions_dir("path+=$HOME/.local/bin", &d));
        assert!(!mentions_dir("# export PATH=\"$HOME/.local/bin:$PATH\"", &d));
        assert!(!mentions_dir("export PATH=\"$HOME/.local/bin2:$PATH\"", &d));
        assert!(!mentions_dir("", &d));
    }

    #[test]
    fn appends_once_and_keeps_the_last_line_intact() {
        let home = scratch("append");
        let dir = home.join(".local").join("bin");
        let file = home.join(".zshrc");
        std::fs::write(&file, "alias ll='ls -l'").unwrap();

        assert!(matches!(update_profile(&file, Shell::Zsh, &dir, &home), PathChange::Added(_)));
        assert!(matches!(update_profile(&file, Shell::Zsh, &dir, &home), PathChange::InProfile(_)));
        let text = std::fs::read_to_string(&file).unwrap();
        assert_eq!(text, "alias ll='ls -l'\n\n# relay\nexport PATH=\"$HOME/.local/bin:$PATH\"\n");
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn an_unwritable_profile_is_left_to_the_user() {
        let home = scratch("unwritable");
        // A directory where the file should be: opening it for append fails
        // on every OS, unlike permission bits.
        let file = home.join(".zshrc");
        std::fs::create_dir_all(&file).unwrap();
        let dir = home.join(".local").join("bin");
        assert!(matches!(update_profile(&file, Shell::Zsh, &dir, &home), PathChange::Manual));
        let _ = std::fs::remove_dir_all(&home);
    }
}
