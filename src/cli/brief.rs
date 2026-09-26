use crate::core::brief;
use crate::core::paths::Paths;
use crate::helpers::est_tokens;

use super::ui::Ui;

/// The brief goes to stdout exactly as a session receives it; with a
/// query, the page for that task.
pub fn run(query: &str) -> anyhow::Result<i32> {
    let paths = Paths::from_cwd()?;
    if !query.trim().is_empty() {
        let page = crate::core::task_brief::build(&paths, query.trim());
        print!("{page}");
        Ui::stderr().note(&format!("~{} tokens (estimate)", est_tokens(&page)));
        return Ok(0);
    }
    let b = brief::build(&paths);
    let notes = Ui::stderr();
    if b.is_empty() {
        notes.ok("Nothing to brief yet.");
        notes.next("Run `relay init` to create the project rules a session reads first.");
    } else {
        print!("{b}");
        notes.blank();
        notes.note(&format!("~{} tokens (estimate)", est_tokens(&b)));
    }
    Ok(0)
}
