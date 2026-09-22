use anyhow::bail;

use crate::core::paths::Paths;
use crate::core::{handoff, spool};
use crate::helpers::git;

pub fn run(session: Option<&str>, show: bool) -> anyhow::Result<i32> {
    let paths = Paths::from_cwd()?;
    if show {
        match handoff::latest(&paths, &git::branch(&paths.root)) {
            Some((p, body)) => {
                eprintln!("# {}", paths.rel(&p));
                print!("{body}");
            }
            None => println!("relay: no handoff yet"),
        }
        return Ok(0);
    }
    let session = match session {
        Some(s) => s.to_string(),
        None => match spool::sessions(&paths).first() {
            Some((s, _)) => s.clone(),
            None => bail!("no sessions recorded yet; run a harness with relay hooks installed"),
        },
    };
    let tail = crate::harness::tail_for(&paths, &session);
    let h = handoff::build(&paths, &session, "manual", tail.as_ref())?;
    eprintln!("# {}", paths.rel(&h.path));
    print!("{}", h.body);
    Ok(0)
}
