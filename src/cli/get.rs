use crate::core::outputs;
use crate::core::paths::Paths;

use super::ui::problem;

pub fn run(id: &str, meta: bool) -> anyhow::Result<i32> {
    let paths = Paths::from_cwd()?;
    let (m, raw) = outputs::get(&paths, id).map_err(|e| match e.downcast_ref::<outputs::Unknown>() {
        Some(_) => problem(
            format!("No stored output with id `{id}`."),
            "Copy the id from the `[relay … original: relay get <id>]` line; originals older than 30 days are deleted.",
        ),
        None => e,
    })?;
    // Best effort: the local tier can be read-only inside a sandbox.
    let _ = outputs::record_fetch(&paths, &m.id);
    if meta {
        println!("{}", serde_json::to_string_pretty(&m)?);
    } else {
        print!("{raw}");
        if !raw.ends_with('\n') {
            println!();
        }
    }
    Ok(0)
}
