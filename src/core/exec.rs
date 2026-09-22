//! `relay x -- <cmd>`: run a shell command, print a compressed view, keep
//! the original. The core primitive of relay.

use std::process::{Command, Stdio};

use anyhow::Result;

use crate::compress;
use crate::core::condense;
use crate::core::paths::Paths;
use crate::core::spool;
use crate::helpers::env::Var;
use crate::helpers::shell;

pub struct Outcome {
    pub exit: i32,
    pub printed: String,
}

pub fn run(cmd: &str, raw_only: bool, session: Option<&str>) -> Result<Outcome> {
    let paths = Paths::from_cwd().ok();
    // Resolved before the command runs: the shared pointer follows
    // whichever session touched the worktree last, and a long command
    // gives a parallel session time to move it.
    let session = session.map(str::to_string).or_else(|| paths.as_ref().and_then(spool::current_session));
    let (exit, raw) = run_shell(cmd)?;
    if raw_only {
        return Ok(Outcome { exit, printed: compress::generic::strip_ansi(&raw) });
    }
    let cwd = std::env::current_dir().map(|p| p.display().to_string()).unwrap_or_default();
    let printed = condense::view_of(paths.as_ref(), condense::Run { cmd, cwd: &cwd, exit, session }, &raw);
    Ok(Outcome { exit, printed })
}

/// Exit code and combined output. stderr is merged into stdout inside the
/// user's shell, so compilers and test runners keep their natural
/// interleaving.
fn run_shell(cmd: &str) -> Result<(i32, String)> {
    let mut command = if let Some(sh) = shell::posix_shell() {
        let mut c = Command::new(sh);
        c.arg("-c").arg(format!("{{\n{cmd}\n}} 2>&1"));
        c
    } else {
        cmd_shell(cmd)
    };
    let out = command
        .env(Var::RelayActive.name(), "1")
        .stdin(Stdio::inherit())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()?;
    let mut raw = String::from_utf8_lossy(&out.stdout).into_owned();
    raw.push_str(&String::from_utf8_lossy(&out.stderr));
    Ok((out.status.code().unwrap_or(1), raw))
}

/// Windows without Git Bash: the harness ran it in cmd, so do we. The
/// line goes through verbatim; `Command::arg` would apply C-runtime
/// quoting, which cmd does not undo (`--format="%h %s"` would break).
#[cfg(windows)]
fn cmd_shell(cmd: &str) -> Command {
    use std::os::windows::process::CommandExt;
    let mut c = Command::new("cmd");
    c.arg("/C").raw_arg(cmd);
    c
}

/// Unreachable off Windows: `posix_shell` always finds one there.
#[cfg(not(windows))]
fn cmd_shell(cmd: &str) -> Command {
    let mut c = Command::new("sh");
    c.arg("-c").arg(cmd);
    c
}

#[cfg(all(test, windows))]
mod tests {
    #[test]
    fn cmd_gets_quotes_verbatim() {
        let out = super::cmd_shell(r#"echo "a b""#).output().unwrap();
        assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), r#""a b""#);
    }
}
