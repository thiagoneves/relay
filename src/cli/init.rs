use crate::core::bootstrap;
use crate::core::paths::Paths;
use crate::helpers::env::tilde;

use super::ui::Ui;

pub fn run() -> anyhow::Result<i32> {
    let ui = Ui::stdout();
    let paths = Paths::from_cwd()?;
    paths.ensure_local()?;
    let created = bootstrap::ensure_shared(&paths)?;
    let project = paths.rel(&paths.project_file());
    if created {
        ui.ok(&format!("Created {project}: the rules a new session reads first. Edit it freely."));
    } else {
        ui.ok(&format!("{project} already exists; left as is."));
    }
    ui.ok(&format!("Local store at {}", tilde(&paths.local)));
    if !paths.in_git {
        ui.note("  Not a git repo, so the local store lives in your user data directory.");
    }
    ui.next(&format!("Commit {} to share it with your team.", paths.rel(&paths.shared)));
    Ok(0)
}
