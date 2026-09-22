//! Shells and executables across platforms. relay speaks POSIX sh: on
//! Windows that is Git Bash, which Claude Code there already requires.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

/// Single-quote for POSIX sh.
pub fn quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// An executable path written as the first word of a hook command. Forward
/// slashes work in sh, cmd and `PowerShell` alike; backslashes are escapes in
/// sh. Quoted only when it has spaces, since `PowerShell` cannot run a quoted
/// path without `&`.
pub fn command_word(exe: &Path) -> String {
    let s = crate::helpers::fs::slash(exe);
    if s.contains(' ') { format!("\"{s}\"") } else { s }
}

/// `cmd` resolved on PATH, honouring PATHEXT on Windows so that npm shims
/// like `claude.cmd` are found.
pub fn which(cmd: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    let exts: Vec<String> = if cfg!(windows) {
        std::env::var("PATHEXT")
            .unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".into())
            .split(';')
            .filter(|e| !e.is_empty())
            .map(str::to_lowercase)
            .collect()
    } else {
        Vec::new()
    };
    which_in(cmd, &path, &exts)
}

fn which_in(cmd: &str, path: &OsStr, exts: &[String]) -> Option<PathBuf> {
    std::env::split_paths(path).find_map(|dir| {
        let bare = dir.join(cmd);
        if exts.is_empty() {
            return bare.is_file().then_some(bare);
        }
        exts.iter().map(|e| dir.join(format!("{cmd}{e}"))).find(|p| p.is_file())
    })
}

/// The POSIX shell `relay x` runs commands with. `RELAY_SHELL` overrides.
/// `None` on Windows without Git Bash.
pub fn posix_shell() -> Option<PathBuf> {
    if let Some(s) = std::env::var_os("RELAY_SHELL") {
        return Some(PathBuf::from(s));
    }
    if !cfg!(windows) {
        let bash = Path::new("/bin/bash");
        return Some(if bash.exists() { bash.into() } else { "/bin/sh".into() });
    }
    let git_bash_env = std::env::var_os("CLAUDE_CODE_GIT_BASH_PATH").map(PathBuf::from);
    let program_files =
        std::env::var_os("ProgramFiles").map(|p| PathBuf::from(p).join("Git").join("bin").join("bash.exe"));
    // git.exe lives in `<Git>\cmd\`, bash.exe in `<Git>\bin\`.
    let beside_git = which("git").and_then(|g| Some(g.parent()?.parent()?.join("bin").join("bash.exe")));
    // `System32\bash.exe` is WSL: it would run the command inside Linux.
    let on_path = which("bash").filter(|p| !is_wsl_launcher(p));
    [git_bash_env, program_files, beside_git, on_path].into_iter().flatten().find(|p| p.is_file())
}

fn is_wsl_launcher(p: &Path) -> bool {
    p.components().any(|c| c.as_os_str().eq_ignore_ascii_case("system32"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quotes_single_quotes() {
        assert_eq!(quote("echo 'a'"), "'echo '\\''a'\\'''");
    }

    #[test]
    fn command_word_quotes_only_paths_with_spaces() {
        assert_eq!(command_word(Path::new("/opt/relay")), "/opt/relay");
        assert_eq!(command_word(Path::new("/Users/Ana Lima/relay")), "\"/Users/Ana Lima/relay\"");
    }

    #[test]
    fn which_honours_extensions() {
        let dir = std::env::temp_dir().join(format!("relay-which-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("claude.cmd"), "").unwrap();
        let path = dir.clone().into_os_string();
        assert_eq!(which_in("claude", &path, &[".exe".into(), ".cmd".into()]), Some(dir.join("claude.cmd")));
        assert_eq!(which_in("claude", &path, &[]), None);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn spots_the_wsl_launcher() {
        assert!(is_wsl_launcher(Path::new("C:/Windows/System32/bash.exe")));
        assert!(!is_wsl_launcher(Path::new("C:/Program Files/Git/bin/bash.exe")));
    }
}
