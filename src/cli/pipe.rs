use std::io::Read;

use crate::compress;

/// Footer as `relay x` prints it, with a placeholder id of the same size,
/// so side-by-side comparisons charge relay for it.
const FOOTER_SAMPLE: &str = "[relay 1.2k→300 tokens · original: relay get o_1a0c82f5e8e_982]";

pub fn run(cmd: &str) -> anyhow::Result<i32> {
    let mut raw = String::new();
    std::io::stdin().read_to_string(&mut raw)?;
    let c = compress::compress(cmd, &raw);
    print!("{}", c.text);
    if c.shortened {
        print!("\n{FOOTER_SAMPLE}");
    }
    Ok(0)
}
