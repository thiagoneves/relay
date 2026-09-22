use crate::core::paths::Paths;
use crate::core::{outputs, spool};
use crate::helpers::{dir_size, human_bytes, human_tokens};

pub fn run() -> anyhow::Result<i32> {
    let paths = Paths::from_cwd()?;
    let outs = outputs::list(&paths);
    let tokens_in: usize = outs.iter().map(|m| m.tokens_in).sum();
    let tokens_out: usize = outs.iter().map(|m| m.tokens_out).sum();
    let saved = tokens_in.saturating_sub(tokens_out);
    let pct = if tokens_in > 0 { saved * 100 / tokens_in } else { 0 };
    let sessions = spool::sessions(&paths).len();
    let handoffs = std::fs::read_dir(paths.handoffs()).map(|r| r.count()).unwrap_or(0);

    println!("relay status · {}", paths.root.display());
    println!();
    println!("Compression   {} → {} tokens, saved {} ({pct}%) over {} outputs  [estimate: bytes/4]",
        human_tokens(tokens_in), human_tokens(tokens_out), human_tokens(saved), outs.len());
    println!("Sessions      {sessions} recorded, {handoffs} handoffs");
    println!("Layer cost    0 tokens (no LLM calls in the default path)");
    println!();
    println!("Shared (committed)  {}  {}", paths.rel(&paths.shared), human_bytes(dir_size(&paths.shared)));
    println!("Local (this worktree) {}  {}", paths.local.display(), human_bytes(dir_size(&paths.local)));
    println!("Leaves this machine: nothing.");
    Ok(0)
}
