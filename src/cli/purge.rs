use crate::core::outputs;
use crate::core::paths::Paths;
use crate::helpers::env::tilde;
use crate::helpers::{dir_size, human_bytes};

use super::ui::Ui;

pub fn run(yes: bool) -> anyhow::Result<i32> {
    let ui = Ui::stdout();
    let paths = Paths::from_cwd()?;
    if !yes {
        ui.heading("relay purge", "preview, nothing deleted");
        let size = human_bytes(dir_size(&paths.local));
        ui.field("Deletes", &format!("{} ({size}): sessions, outputs, handoffs, log", tilde(&paths.local)));
        let spill = outputs::spill_dir(&paths);
        if spill.exists() {
            ui.field("Also", &format!("originals spilled from sandboxed runs in {}", tilde(&spill)));
        }
        ui.field("Keeps", &format!("{}, the committed rules and memory", paths.rel(&paths.shared)));
        ui.blank();
        ui.next("Run `relay purge --yes` to delete.");
        return Ok(0);
    }
    if paths.local.exists() {
        std::fs::remove_dir_all(&paths.local)?;
    }
    outputs::purge_spill(&paths)?;
    ui.ok(&format!("Deleted the local store; {} is untouched.", paths.rel(&paths.shared)));
    Ok(0)
}
