//! `relay x -- <cmd>`: run a shell command, print a compressed view, keep
//! the original. The core primitive of relay.

use std::process::{Command, Stdio};

use anyhow::Result;

use crate::compress;
use crate::core::outputs::{self, OutputMeta};
use crate::core::paths::Paths;
use crate::core::spool;
use crate::helpers::env::Var;
use crate::helpers::{est_tokens, human_tokens, new_id, now_iso, shell};
use crate::limits;

pub struct Outcome {
    pub exit: i32,
    pub printed: String,
}

pub fn run(cmd: &str, raw_only: bool, session: Option<&str>) -> Result<Outcome> {
    // Resolved before the command runs: the shared pointer follows
    // whichever session touched the worktree last, and a long command
    // gives a parallel session time to move it.
    let session =
        session.map(str::to_string).or_else(|| Paths::from_cwd().ok().and_then(|p| spool::current_session(&p)));
    // Merge stderr into stdout in order, inside the user's shell, so
    // compilers and test runners keep their natural interleaving.
    let wrapped = format!("{{\n{cmd}\n}} 2>&1");
    let mut command = if let Some(sh) = shell::posix_shell() {
        let mut c = Command::new(sh);
        c.arg("-c").arg(&wrapped);
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
    let exit = out.status.code().unwrap_or(1);
    let mut raw = String::from_utf8_lossy(&out.stdout).into_owned();
    raw.push_str(&String::from_utf8_lossy(&out.stderr));

    if raw_only || raw.len() < limits::store::MIN_OUTPUT_BYTES {
        let text = compress::generic::strip_ansi(&raw);
        return Ok(Outcome { exit, printed: text });
    }

    let c = compress::compress(cmd, &raw);
    let tokens_in = est_tokens(&raw);
    let tokens_out = est_tokens(&c.text);

    // The original is stored even when the view is unchanged: handoffs
    // cite it. A cut view is shown only if its original is retrievable.
    let stored = Paths::from_cwd().ok().and_then(|paths| {
        let meta = OutputMeta {
            id: new_id("o"),
            ts: now_iso(),
            session,
            cwd: std::env::current_dir().map(|p| p.display().to_string()).unwrap_or_default(),
            cmd: cmd.to_string(),
            exit,
            filter: c.filter.to_string(),
            bytes_in: raw.len(),
            bytes_out: c.text.len(),
            tokens_in,
            tokens_out,
        };
        match outputs::store(&paths, &meta, &raw) {
            Ok(()) => Some(meta.id),
            Err(e) => {
                crate::core::paths::log(&paths, &format!("store failed: {e}"));
                None
            }
        }
    });
    let printed = match stored {
        Some(id) if c.shortened => format!(
            "{}\n[relay {}→{} tokens · original: relay get {id}]",
            c.text,
            human_tokens(tokens_in),
            human_tokens(tokens_out)
        ),
        _ if c.shortened => compress::generic::strip_ansi(&raw),
        _ => c.text,
    };
    Ok(Outcome { exit, printed })
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
