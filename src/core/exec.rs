//! `relay x -- <cmd>`: run a shell command, print a compressed view, keep
//! the original. The core primitive of relay.

use std::process::{Command, Stdio};

use anyhow::Result;

use crate::compress;
use crate::core::outputs::{self, OutputMeta};
use crate::core::paths::Paths;
use crate::core::spool;
use crate::helpers::{est_tokens, human_tokens, new_id, now_iso, shell};

/// Outputs shorter than this are printed as is, no footer, not stored.
const MIN_STORE_BYTES: usize = 200;

pub struct Outcome {
    pub exit: i32,
    pub printed: String,
}

pub fn run(cmd: &str, raw_only: bool) -> Result<Outcome> {
    // Merge stderr into stdout in order, inside the user's shell, so
    // compilers and test runners keep their natural interleaving.
    let wrapped = format!("{{\n{cmd}\n}} 2>&1");
    let mut command = if let Some(sh) = shell::posix_shell() {
        let mut c = Command::new(sh);
        c.arg("-c").arg(&wrapped);
        c
    } else {
        // Windows without Git Bash: the harness ran it in cmd, so do we.
        let mut c = Command::new("cmd");
        c.arg("/C").arg(cmd);
        c
    };
    let out = command
        .env("RELAY_ACTIVE", "1")
        .stdin(Stdio::inherit())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()?;
    let exit = out.status.code().unwrap_or(1);
    let mut raw = String::from_utf8_lossy(&out.stdout).into_owned();
    raw.push_str(&String::from_utf8_lossy(&out.stderr));

    if raw_only || raw.len() < MIN_STORE_BYTES {
        let text = compress::generic::strip_ansi(&raw);
        return Ok(Outcome { exit, printed: text });
    }

    let c = compress::compress(cmd, &raw);
    let tokens_in = est_tokens(&raw);
    let tokens_out = est_tokens(&c.text);

    // Store the original whenever we changed anything or it was big
    // enough to be worth revisiting in a handoff.
    let paths = Paths::from_cwd();
    let mut footer = String::new();
    if let Ok(paths) = paths {
        let id = new_id("o");
        let meta = OutputMeta {
            id: id.clone(),
            ts: now_iso(),
            session: spool::current_session(&paths),
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
            Ok(_) => {
                if tokens_out < tokens_in {
                    footer = format!(
                        "\n[relay {}→{} tokens · original: relay get {}]",
                        human_tokens(tokens_in),
                        human_tokens(tokens_out),
                        id
                    );
                }
            }
            Err(e) => crate::core::paths::log(&paths, &format!("store failed: {e}")),
        }
    }
    let printed = if tokens_out < tokens_in { format!("{}{}", c.text, footer) } else { c.text };
    Ok(Outcome { exit, printed })
}
