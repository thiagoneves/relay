use crate::harness::HarnessId;
use crate::helpers::env::tilde;

use super::ui::Ui;

pub fn install(id: HarnessId) -> anyhow::Result<i32> {
    let ui = Ui::stdout();
    let h = id.adapter();
    let inst = crate::machine::install_self()?;
    super::setup::report(&inst);
    let r = h.install(&inst.exe)?;
    if r.changed {
        ui.ok(&format!("{} hooks written to {}", h.command(), tilde(&r.settings_path)));
        ui.note(&format!("  events: {}", r.events.join(", ")));
        if let Some(b) = r.backup_path {
            ui.note(&format!("  previous file saved as {}", tilde(&b)));
        }
    } else {
        ui.ok(&format!("{} hooks already current in {}", h.command(), tilde(&r.settings_path)));
    }
    if h.detect() {
        ui.next(&format!("Start a session with `relay {}`.", h.command()));
    } else {
        ui.warn(&format!("`{}` is not on your PATH; the hooks take effect once it is installed.", h.command()));
    }
    Ok(0)
}

pub fn uninstall(id: HarnessId) -> anyhow::Result<i32> {
    let ui = Ui::stdout();
    let h = id.adapter();
    let r = h.uninstall()?;
    if r.changed {
        ui.ok(&format!("relay hooks removed from {}", tilde(&r.settings_path)));
        if let Some(b) = r.backup_path {
            ui.note(&format!("  previous file saved as {}", tilde(&b)));
        }
    } else {
        ui.ok(&format!("No relay hooks in {}; nothing to remove.", tilde(&r.settings_path)));
    }
    Ok(0)
}
