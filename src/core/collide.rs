//! Collisions before they happen: the files a task will likely touch,
//! against what other live sessions in this checkout are editing, the
//! uncommitted files they left, and branches not merged yet. What
//! overlaps is named with an order to integrate in, so the task starts
//! after the work it would conflict with instead of rebasing onto it.

use std::time::SystemTime;

use crate::core::claims;
use crate::core::paths::Paths;
use crate::helpers::git as gitstate;
use crate::limits::task_brief::BRANCHES_READ;

/// One piece of work in the way: who, and which of the files.
#[derive(Debug, PartialEq, Eq)]
pub struct Overlap {
    pub who: String,
    pub files: Vec<String>,
}

pub fn section(paths: &Paths, likely: &[String]) -> String {
    if likely.is_empty() {
        return String::new();
    }
    let sessions = with_sessions(&claims::all(paths), likely, SystemTime::now());
    let branches: Vec<Overlap> = gitstate::unmerged_branches(&paths.root, BRANCHES_READ)
        .into_iter()
        .map(|b| Overlap {
            files: overlap(&gitstate::branch_files(&paths.root, &b), likely),
            who: format!("branch {b}"),
        })
        .filter(|o| !o.files.is_empty())
        .collect();
    render(&sessions, &branches)
}

fn overlap(files: &[String], likely: &[String]) -> Vec<String> {
    likely.iter().filter(|l| files.contains(l)).cloned().collect()
}

/// Live sessions editing any of `likely`, and ended ones whose
/// uncommitted edits to them are still claimed.
fn with_sessions(all: &[claims::Claim], likely: &[String], now: SystemTime) -> Vec<Overlap> {
    all.iter()
        .filter_map(|c| {
            let files: Vec<String> = c.files.keys().cloned().collect();
            let hit = overlap(&files, likely);
            let state = if c.is_live(now) { "live" } else { "ended, edits may be uncommitted" };
            (!hit.is_empty()).then(|| Overlap { who: format!("session {} ({state})", c.name()), files: hit })
        })
        .collect()
}

/// Branches with the most overlap first: merge them before starting.
fn render(sessions: &[Overlap], branches: &[Overlap]) -> String {
    if sessions.is_empty() && branches.is_empty() {
        return String::new();
    }
    let mut out = String::from("\n## In the way\n");
    for o in sessions {
        out.push_str(&format!("- {} is on {}\n", o.who, o.files.join(", ")));
    }
    let mut order: Vec<&Overlap> = branches.iter().collect();
    order.sort_by_key(|o| std::cmp::Reverse(o.files.len()));
    for o in &order {
        out.push_str(&format!("- {} changes {}\n", o.who, o.files.join(", ")));
    }
    let mut steps = Vec::new();
    if !order.is_empty() {
        let names: Vec<&str> = order.iter().map(|o| o.who.trim_start_matches("branch ")).collect();
        steps.push(format!("merge {} first", names.join(", then ")));
    }
    if !sessions.is_empty() {
        steps.push("let the sessions above finish those files or split the work by path".to_string());
    }
    out.push_str(&format!("_Order: {}, then start here; or work in a worktree._\n", steps.join("; ")));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlaps_come_with_an_order() {
        let sessions = vec![Overlap { who: "session ab12 (live)".into(), files: vec!["src/a.ts".into()] }];
        let branches = vec![
            Overlap { who: "branch small".into(), files: vec!["src/b.ts".into()] },
            Overlap { who: "branch big".into(), files: vec!["src/a.ts".into(), "src/b.ts".into()] },
        ];
        let out = render(&sessions, &branches);
        assert!(
            out.contains(
                "- session ab12 (live) is on src/a.ts\n- branch big changes src/a.ts, src/b.ts\n- branch small"
            ),
            "{out}"
        );
        assert!(out.contains("_Order: merge big, then small first; let the sessions above"), "{out}");
        assert_eq!(render(&[], &[]), "");
    }

    #[test]
    fn a_live_claim_on_a_likely_file_is_in_the_way() {
        let now = SystemTime::now();
        let claim = claims::Claim {
            session: "sess-9999".into(),
            updated: crate::helpers::iso(now),
            files: [("src/a.ts".to_string(), crate::helpers::iso(now))].into(),
            ..claims::Claim::default()
        };
        let got = with_sessions(&[claim], &["src/a.ts".into(), "src/z.ts".into()], now);
        assert_eq!(got, [Overlap { who: "session sess-999 (live)".into(), files: vec!["src/a.ts".into()] }]);
    }
}
