use crate::core::paths::Paths;
use crate::helpers::{dir_size, human_bytes};

pub fn run(yes: bool) -> anyhow::Result<i32> {
    let paths = Paths::from_cwd()?;
    let size = dir_size(&paths.local);
    println!("relay purge would delete the LOCAL store only:");
    println!("  {}  ({})", paths.local.display(), human_bytes(size));
    println!("  spool, outputs, handoffs, claims, log");
    println!("It never touches the committed tier: {}", paths.rel(&paths.shared));
    if !yes {
        println!("Re-run with --yes to proceed.");
        return Ok(1);
    }
    if paths.local.exists() {
        std::fs::remove_dir_all(&paths.local)?;
    }
    println!("relay: local store removed");
    Ok(0)
}
