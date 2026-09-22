//! A command's raw output turned into what the model sees, with the
//! original kept one `relay get` away. Shared by `relay x` and the hook
//! that shrinks output after the harness ran the command itself.

use crate::compress;
use crate::core::outputs::{self, OutputMeta};
use crate::core::paths::Paths;
use crate::helpers::{est_tokens, human_tokens, new_id, now_iso};
use crate::limits;

/// One finished command, as relay records it.
pub struct Run<'a> {
    pub cmd: &'a str,
    pub cwd: &'a str,
    pub exit: i32,
    pub session: Option<String>,
}

/// What the model should see of `raw`. Short outputs pass with only
/// colour codes removed; `paths` is `None` when there is nowhere to keep
/// an original, and then nothing is cut.
pub fn view_of(paths: Option<&Paths>, run: Run, raw: &str) -> String {
    if raw.len() < limits::store::MIN_OUTPUT_BYTES {
        return compress::generic::strip_ansi(raw);
    }
    let c = compress::compress(run.cmd, raw);
    let tokens = Tokens { before: est_tokens(raw), after: est_tokens(&c.text) };
    let stored = paths.and_then(|p| store_original(p, run, raw, &c, tokens));
    view(c, raw, stored.as_deref(), tokens)
}

#[derive(Clone, Copy)]
struct Tokens {
    before: usize,
    after: usize,
}

/// The original is stored even when the view is unchanged: handoffs cite
/// it. Returns its id, or `None` when it could not be kept.
fn store_original(paths: &Paths, run: Run, raw: &str, c: &compress::Compressed, tokens: Tokens) -> Option<String> {
    let meta = OutputMeta {
        id: new_id("o"),
        ts: now_iso(),
        session: run.session,
        cwd: run.cwd.to_string(),
        cmd: run.cmd.to_string(),
        exit: run.exit,
        filter: c.filter.to_string(),
        bytes_in: raw.len(),
        bytes_out: c.text.len(),
        tokens_in: tokens.before,
        tokens_out: tokens.after,
    };
    match outputs::store(paths, &meta, raw) {
        Ok(()) => Some(meta.id),
        Err(e) => {
            crate::core::log::write(paths, &format!("store failed: {e}"));
            None
        }
    }
}

/// A cut view is shown only when its original is retrievable.
fn view(c: compress::Compressed, raw: &str, stored: Option<&str>, tokens: Tokens) -> String {
    match stored {
        Some(id) if c.shortened => format!(
            "{}\n[relay {}→{} tokens · original: relay get {id}]",
            c.text,
            human_tokens(tokens.before),
            human_tokens(tokens.after)
        ),
        _ if c.shortened => compress::generic::strip_ansi(raw),
        _ => c.text,
    }
}
