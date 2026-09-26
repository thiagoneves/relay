use crate::core::hygiene::Advice;
use crate::core::memory::{self, Kind, NewItem};
use crate::core::paths::Paths;
use crate::core::{compile, spool};
use crate::helpers::text::count;
use crate::limits;

use super::ui::{Ui, problem};

/// `relay compile` lists what recent sessions decided and memory does not
/// hold yet; `--save` keeps the chosen ones as decisions.
pub fn run(save: Option<&str>, hygiene: bool) -> anyhow::Result<i32> {
    if hygiene {
        return review();
    }
    let paths = Paths::from_cwd()?;
    let ui = Ui::stdout();
    let found = compile::candidates(&paths, limits::compile::SESSIONS);
    if found.is_empty() {
        ui.ok("Nothing new to promote: every decision of the recent sessions is already remembered.");
        return Ok(0);
    }
    let Some(picked) = save else {
        ui.heading(
            "relay compile",
            &format!("{} candidates from the last {} sessions", found.len(), limits::compile::SESSIONS),
        );
        for (i, c) in found.iter().enumerate() {
            ui.field(&format!("{:>2}. {}", i + 1, c.ended), &c.text);
        }
        ui.blank();
        ui.next("Keep some with `relay compile --save 1,3`, or all with `relay compile --save all`.");
        return Ok(0);
    };
    let chosen = choose(picked, found.len())?;
    let session = spool::current_session(&paths);
    for i in chosen {
        let c = &found[i];
        let item = NewItem {
            kind: Kind::Decision,
            text: &c.text,
            files: &[],
            session: c.session.as_deref().or(session.as_deref()),
            expires: None,
        };
        let path = memory::remember(&paths, &item)?;
        ui.ok(&format!("Saved decision in {}", paths.rel(&path)));
    }
    ui.next("Commit them so every session and teammate gets them.");
    Ok(0)
}

/// What to merge, check, shorten or drop in memory. Advice only: items
/// are files under `.relay/`, removed or edited by hand and reviewed in
/// the diff like any other change.
fn review() -> anyhow::Result<i32> {
    let paths = Paths::from_cwd()?;
    let ui = Ui::stdout();
    let items = memory::list(&paths);
    let stale = memory::staleness(&paths.root, &items, limits::brief::STALE_COMMITS);
    let advice = crate::core::hygiene::review(&items, &stale, &crate::helpers::now_iso());
    ui.heading("relay compile --hygiene", &format!("{} remembered", items.len()));
    if advice.is_empty() {
        ui.ok("Memory is lean: no duplicates, nothing stale, expired or too long, and all of it fits the brief.");
        return Ok(0);
    }
    let name = |i: usize| paths.rel(&items[i].path);
    for a in &advice {
        let (label, text) = match a {
            Advice::Merge(i, j) => ("Merge", format!("{} and {} say nearly the same; keep one", name(*i), name(*j))),
            Advice::Stale(i, changed) => {
                ("Check", format!("{}: {} changed since it was saved", name(*i), changed.join(", ")))
            }
            Advice::Expired(i) => ("Expired", format!("{}: delete it or renew `stale_after`", name(*i))),
            Advice::Overlong(i) => ("Shorten", format!("{}: one line the brief can show", name(*i))),
            Advice::Unseen(left) => (
                "Unseen",
                format!(
                    "{} past the brief's budget reach no session; merge or drop older ones",
                    count(left.len(), "item")
                ),
            ),
        };
        ui.field(label, &text);
    }
    ui.blank();
    ui.next("Edit or delete the files, then commit: memory changes are reviewed like code.");
    Ok(0)
}

/// `all`, or 1-based numbers separated by commas, as indexes.
fn choose(picked: &str, n: usize) -> anyhow::Result<Vec<usize>> {
    if picked.trim() == "all" {
        return Ok((0..n).collect());
    }
    picked
        .split(',')
        .map(|s| {
            s.trim().parse::<usize>().ok().filter(|i| (1..=n).contains(i)).map(|i| i - 1).ok_or_else(|| {
                problem(
                    format!("`{s}` is not one of the candidates."),
                    format!("Use numbers from 1 to {n}, separated by commas, or `all`."),
                )
            })
        })
        .collect()
}
