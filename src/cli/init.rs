use crate::core::paths::Paths;
use crate::core::{agents_md, bootstrap};
use crate::helpers::env::tilde;

use super::ui::Ui;

pub fn run(local: bool, terse: bool, agents_md: bool) -> anyhow::Result<i32> {
    let ui = Ui::stdout();
    let mut paths = Paths::from_cwd()?;
    if local {
        if paths.root.join(".relay").exists() {
            ui.warn("This repo already has a committed .relay/; leaving memory there.");
        } else {
            paths = paths.with_local_memory();
        }
    }
    paths.ensure_local()?;
    let created = bootstrap::ensure_shared(&paths)?;
    let project = paths.rel(&paths.project_file());
    if created {
        ui.ok(&format!("Created {project}: the rules a new session reads first. Edit it freely."));
    } else {
        ui.ok(&format!("{project} already exists; left as is."));
    }
    if terse && bootstrap::ensure_terse(&paths)? {
        ui.ok(&format!("Added an Answers section to {project}: shorter replies from now on."));
    }
    // A repo that is not the user's to commit to gets no edits.
    if agents_md && !paths.memory_local {
        for f in agents_md::ensure(&paths)? {
            ui.ok(&format!("Pointed {} at relay's memory, for agents without hooks.", paths.rel(&f)));
        }
    }
    ui.ok(&format!("Local store at {}", tilde(&paths.local)));
    if !paths.in_git {
        ui.note("  Not a git repo, so the local store lives in your user data directory.");
    }
    if paths.memory_local {
        ui.note(&format!(
            "  Memory stays in {}, never committed (this repo is not yours to commit to).",
            tilde(&paths.shared)
        ));
        ui.next("To share it later, move that directory to .relay/ in the repo and commit it.");
    } else {
        ui.next(&format!("Commit {} to share it with your team.", paths.rel(&paths.shared)));
    }
    Ok(0)
}
