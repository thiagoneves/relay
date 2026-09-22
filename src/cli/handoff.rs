use crate::core::paths::Paths;
use crate::core::{handoff, spool};
use crate::helpers::git;

use super::ui::{Ui, problem};

/// The handoff text goes to stdout as is; where it lives goes to stderr.
pub fn run(session: Option<&str>, show: bool, share: bool) -> anyhow::Result<i32> {
    let paths = Paths::from_cwd()?;
    let notes = Ui::stderr();
    if share {
        let session = match session {
            Some(s) => s.to_string(),
            None => spool::last_session(&paths).ok_or_else(no_sessions)?,
        };
        let path = handoff::share(&paths, &session)?;
        let ui = Ui::stdout();
        ui.ok(&format!(
            "Shared the handoff of session {} in {}, credentials masked.",
            &session[..8.min(session.len())],
            paths.rel(&path)
        ));
        ui.next("Commit it, and whoever opens this repo next starts from it.");
        return Ok(0);
    }
    if show {
        let Some((p, body)) = handoff::latest(&paths, &git::branch(&paths.root)) else {
            return Err(no_sessions());
        };
        notes.note(&format!("# {}", paths.rel(&p)));
        print!("{body}");
        return Ok(0);
    }
    let session = match session {
        Some(s) => s.to_string(),
        None => spool::sessions(&paths).first().map(|(s, _)| s.clone()).ok_or_else(no_sessions)?,
    };
    let tail = crate::harness::tail_for(&paths, &session);
    let h = handoff::build(&paths, &session, "manual", tail.as_ref())?;
    notes.note(&format!("# {}", paths.rel(&h.path)));
    print!("{}", h.body);
    Ok(0)
}

fn no_sessions() -> anyhow::Error {
    problem("No session recorded in this repo yet.", "Start one with `relay claude` or `relay codex`.")
}
