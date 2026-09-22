use crate::core::brief;
use crate::core::paths::Paths;
use crate::helpers::est_tokens;

pub fn run() -> anyhow::Result<i32> {
    let paths = Paths::from_cwd()?;
    let b = brief::build(&paths);
    if b.is_empty() {
        println!("relay: nothing to brief yet (run `relay init`)");
    } else {
        print!("{b}");
        eprintln!("\n(~{} tokens, estimate)", est_tokens(&b));
    }
    Ok(0)
}
