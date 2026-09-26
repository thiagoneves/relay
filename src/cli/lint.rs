use std::path::PathBuf;

use crate::core::lint::{self, Finding};
use crate::core::paths::Paths;
use crate::helpers::git as gitstate;
use crate::helpers::text::count;

use super::ui::Ui;

pub struct Options {
    pub all: bool,
    pub staged: bool,
    pub commit_msg: Option<PathBuf>,
}

/// Exit 1 when anything is over its budget, so it works as a git hook:
/// `relay lint --staged` in pre-commit, `relay lint --commit-msg "$1"` in
/// commit-msg.
pub fn run(o: &Options) -> anyhow::Result<i32> {
    let paths = Paths::from_cwd()?;
    let ui = Ui::stdout();
    let mut found: Vec<Finding> = Vec::new();
    let mut checked = 0;
    if let Some(msg) = &o.commit_msg {
        found.extend(lint::subject(&std::fs::read_to_string(msg)?));
    } else {
        let files = if o.all {
            gitstate::tracked_files(&paths.root)
        } else if o.staged {
            gitstate::staged_files(&paths.root)
        } else {
            gitstate::dirty_files(&paths.root, usize::MAX)
        };
        for rel in files.iter().filter(|f| crate::core::outline::is_markdown(f)) {
            let Ok(text) = std::fs::read_to_string(paths.root.join(rel)) else { continue };
            checked += 1;
            found.extend(lint::file(rel, &text));
        }
        if !o.staged {
            found.extend(lint::subject(&gitstate::head_message(&paths.root)));
        }
    }
    for f in &found {
        ui.fail(&format!("{} {}", f.at, f.what));
    }
    if found.is_empty() {
        let scope = if o.commit_msg.is_some() { "commit message".to_string() } else { count(checked, "doc") };
        ui.ok(&format!("Within budget: {scope}."));
        return Ok(0);
    }
    ui.next("Budgets are in relay's limits; see `relay lint --help` for what each check reads.");
    Ok(1)
}
