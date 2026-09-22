use crate::core::memory::{self, Kind, NewItem};
use crate::core::paths::Paths;
use crate::core::spool::{self, Event};

pub fn run(kind: Kind, text: &str, files: &[String]) -> anyhow::Result<i32> {
    let paths = Paths::from_cwd()?;
    let session = spool::current_session(&paths);
    let path = memory::remember(&paths, &NewItem { kind, text, files, session: session.as_deref() })?;
    let rel = paths.rel(&path);
    // Best effort: the local tier can be read-only inside a sandbox.
    if let Some(s) = &session {
        let _ = spool::append(
            &paths,
            &Event::new(
                s,
                "remember",
                Some(&format!("remember:{rel}")),
                serde_json::json!({ "kind": kind.as_str(), "path": rel }),
            ),
        );
    }
    println!("relay: saved {kind} → {rel} (commit it to share)");
    Ok(0)
}
