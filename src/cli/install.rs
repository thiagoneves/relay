use crate::harness;

pub fn install(name: &str) -> anyhow::Result<i32> {
    let h = harness::by_name(name)?;
    let inst = crate::core::machine::install_self()?;
    super::setup::report(&inst);
    let r = h.install(&inst.exe)?;
    if r.changed {
        println!("relay: hooks for {} written to {}", h.id(), r.settings_path.display());
        println!("relay: events {}", r.events.join(", "));
        if let Some(b) = r.backup_path {
            println!("relay: backup at {}", b.display());
        }
    } else {
        println!("relay: hooks for {} already current in {}", h.id(), r.settings_path.display());
    }
    if !h.detect() {
        println!("relay: note: `{}` not found on PATH", h.command());
    }
    Ok(0)
}

pub fn uninstall(name: &str) -> anyhow::Result<i32> {
    let h = harness::by_name(name)?;
    let r = h.uninstall()?;
    if r.changed {
        println!("relay: hooks removed from {}", r.settings_path.display());
        if let Some(b) = r.backup_path {
            println!("relay: backup at {}", b.display());
        }
    } else {
        println!("relay: no relay hooks in {}", r.settings_path.display());
    }
    Ok(0)
}
