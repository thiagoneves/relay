use crate::core::log;
use crate::core::paths::Paths;

use super::ui::Ui;

/// The newest failures, one per line; the log itself stays in the local
/// store for anyone who wants all of it.
pub fn run(lines: usize) -> anyhow::Result<i32> {
    let paths = Paths::from_cwd()?;
    let ui = Ui::stdout();
    let entries = log::entries(&paths);
    if entries.is_empty() {
        ui.ok("Nothing has failed in this worktree.");
        return Ok(0);
    }
    let shown = &entries[entries.len().saturating_sub(lines)..];
    for e in shown {
        println!("{}  {}", e.at, e.what);
    }
    if shown.len() < entries.len() {
        ui.note(&format!("{} older in {}", entries.len() - shown.len(), paths.rel(&paths.log_file())));
    }
    Ok(0)
}
