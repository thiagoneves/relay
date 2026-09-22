use crate::core::memory::{self, Kind, NewItem, NotSaved};
use crate::core::paths::Paths;
use crate::core::spool::{self, Event};
use crate::helpers::{iso, parse_until};

use super::ui::{Ui, problem};

pub fn run(
    kind: Kind,
    text: &str,
    files: &[String],
    session: Option<&str>,
    until: Option<&str>,
) -> anyhow::Result<i32> {
    let paths = Paths::from_cwd()?;
    let session = session.map(str::to_string).or_else(|| spool::current_session(&paths));
    let expires = until
        .map(|u| {
            parse_until(u, std::time::SystemTime::now()).map(iso).ok_or_else(|| {
                problem(
                    format!("`--until {u}` is not a time relay understands."),
                    "Use 30d, 12h or a date like 2026-12-01.",
                )
            })
        })
        .transpose()?;
    let path = memory::remember(&paths, &NewItem { kind, text, files, session: session.as_deref(), expires }).map_err(
        |e| match e.downcast_ref::<NotSaved>() {
            Some(NotSaved::Empty) => problem(
                "Nothing to remember: the text is empty.",
                format!("Pass one line, e.g. `relay remember {kind} \"Run cargo test before committing\"`."),
            ),
            Some(NotSaved::AlreadyRemembered(at)) => {
                problem(format!("Already remembered in {at}."), "Edit that file to change it.")
            }
            None => e,
        },
    )?;
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
