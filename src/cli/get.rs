use crate::core::outputs;
use crate::core::paths::Paths;

pub fn run(id: &str, meta: bool) -> anyhow::Result<i32> {
    let paths = Paths::from_cwd()?;
    let (m, raw) = outputs::get(&paths, id)?;
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
