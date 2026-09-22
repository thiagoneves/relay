use crate::core::bootstrap;
use crate::core::paths::Paths;

pub fn run() -> anyhow::Result<i32> {
    let paths = Paths::from_cwd()?;
    paths.ensure_local()?;
    let created = bootstrap::ensure_shared(&paths)?;
    println!(
        "relay: {} {}",
        paths.rel(&paths.project_file()),
        if created { "created (rules only, edit freely)" } else { "kept" }
    );
    println!("relay: local store {}", paths.local.display());
    if !paths.in_git {
        println!("relay: not a git repo; local store lives under the user data dir");
    }
    Ok(0)
}
