use crate::core::memory::{self, Kind, NewItem, NotSaved};
use crate::core::paths::Paths;
use crate::core::spool::{self, Event};

use super::ui::{Ui, problem};

pub fn run(kind: Kind, text: &str, files: &[String], session: Option<&str>) -> anyhow::Result<i32> {
    let paths = Paths::from_cwd()?;
    let session = session.map(str::to_string).or_else(|| spool::current_session(&paths));
    let path = memory::remember(&paths, &NewItem { kind, text, files, session: session.as_deref() }).map_err(|e| {
        match e.downcast_ref::<NotSaved>() {
            Some(NotSaved::Empty) => problem(
                "Nothing to remember: the text is empty.",
                format!("Pass one line, e.g. `relay remember {kind} \"Run cargo test before committing\"`."),
            ),
            Some(NotSaved::AlreadyRemembered(at)) => {
                problem(format!("Already remembered in {at}."), "Edit that file to change it.")
            }
            None => e,
        }
    })?;
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
    let ui = Ui::stdout();
    ui.ok(&format!("Saved {kind} in {rel}"));
    ui.next("Commit it so every session and teammate gets it.");
    Ok(0)
}
