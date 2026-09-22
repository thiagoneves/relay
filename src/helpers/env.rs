//! Every environment variable relay reads or sets. An empty value counts
//! as unset everywhere: `CODEX_HOME=` must not turn into the current
//! directory.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use super::slash;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Var {
    Home,
    UserProfile,
    XdgDataHome,
    LocalAppData,
    ProgramFiles,
    Path,
    PathExt,
    Shell,
    ZDotDir,
    NoColor,
    ClaudeConfigDir,
    ClaudeCodeGitBashPath,
    CodexHome,
    /// Any value turns every hook into a no-op.
    RelayDisable,
    /// POSIX shell `relay x` runs commands with.
    RelayShell,
    /// Set by `relay claude|codex` on the harness it launches; hooks stamp
    /// it on `session_start` so the wrapper can find its own session.
    RelayWrapper,
    /// Set on commands run by `relay x`.
    RelayActive,
}

impl Var {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Home => "HOME",
            Self::UserProfile => "USERPROFILE",
            Self::XdgDataHome => "XDG_DATA_HOME",
            Self::LocalAppData => "LOCALAPPDATA",
            Self::ProgramFiles => "ProgramFiles",
            Self::Path => "PATH",
            Self::PathExt => "PATHEXT",
            Self::Shell => "SHELL",
            Self::ZDotDir => "ZDOTDIR",
            Self::NoColor => "NO_COLOR",
            Self::ClaudeConfigDir => "CLAUDE_CONFIG_DIR",
            Self::ClaudeCodeGitBashPath => "CLAUDE_CODE_GIT_BASH_PATH",
            Self::CodexHome => "CODEX_HOME",
            Self::RelayDisable => "RELAY_DISABLE",
            Self::RelayShell => "RELAY_SHELL",
            Self::RelayWrapper => "RELAY_WRAPPER",
            Self::RelayActive => "RELAY_ACTIVE",
        }
    }
}

pub fn get(var: Var) -> Option<OsString> {
    non_empty(std::env::var_os(var.name()))
}

pub fn text(var: Var) -> Option<String> {
    get(var).and_then(|v| v.into_string().ok())
}

pub fn path(var: Var) -> Option<PathBuf> {
    get(var).map(PathBuf::from)
}

pub fn is_set(var: Var) -> bool {
    get(var).is_some()
}

fn non_empty(value: Option<OsString>) -> Option<OsString> {
    value.filter(|v| !v.is_empty())
}

/// The user's home: `HOME`, else `USERPROFILE`, else the temp dir.
pub fn home() -> PathBuf {
    path(Var::Home).or_else(|| path(Var::UserProfile)).unwrap_or_else(std::env::temp_dir)
}

/// `~/rest` for paths under home, for display.
pub fn tilde(p: &Path) -> String {
    match p.strip_prefix(home()) {
        Ok(rest) if rest.as_os_str().is_empty() => "~".to_string(),
        Ok(rest) => format!("~/{}", slash(rest)),
        Err(_) => p.display().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_counts_as_unset() {
        assert_eq!(non_empty(Some(OsString::new())), None);
        assert_eq!(non_empty(None), None);
        assert_eq!(non_empty(Some("/cfg".into())), Some("/cfg".into()));
    }
}
