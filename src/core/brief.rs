//! The `SessionStart` brief: project.md head + remembered items + latest
//! handoff, capped at roughly 600 tokens. File reads and one git call; no LLM.

use crate::core::paths::Paths;
use crate::core::{handoff, memory};
use crate::helpers::frontmatter;
use crate::helpers::git as gitstate;
use crate::helpers::text::cut_lines;
use crate::limits;

pub fn build(paths: &Paths) -> String {
    compose(&gather(paths))
}

/// Everything the brief shows, read from disk and git.
struct Inputs {
    project: String,
    branch: String,
    /// Repo-relative path and body of the latest handoff.
    handoff: Option<(String, String)>,
    items: Vec<memory::Item>,
    shared_dir: String,
}

fn gather(paths: &Paths) -> Inputs {
    let branch = gitstate::branch(&paths.root);
    Inputs {
        project: std::fs::read_to_string(paths.project_file()).unwrap_or_default(),
        handoff: handoff::latest(paths, &branch).map(|(p, body)| (paths.rel(&p), body)),
        branch,
        items: memory::list(paths),
        shared_dir: paths.rel(&paths.shared),
    }
}

/// Empty when there is nothing to tell: no project file, no items, no
/// handoff.
fn compose(i: &Inputs) -> String {
    let mut out = String::from("# relay brief\n");
    if !i.project.trim().is_empty() {
        out.push_str(&cut(frontmatter::strip(&i.project).trim(), limits::brief::PROJECT_CHARS));
        out.push_str("\n\n");
    }
    if !i.items.is_empty() {
        out.push_str(&memory_section(&i.items, &i.shared_dir));
    }
    match &i.handoff {
        Some((path, body)) => out.push_str(&last_session(body, path, &i.branch, out.len())),
        None if i.project.trim().is_empty() && i.items.is_empty() => return String::new(),
        None => {}
    }
    out.push_str("_Outputs shown by relay are compressed; `relay get <id>` prints the original._\n");
    out.push_str("_When you settle a decision, hit a gotcha or learn a project rule, save it: `relay remember decision|gotcha|rule \"<one line>\"`._\n");
    out
}

/// The latest handoff under a heading that says when, where and on which
/// branch; `used` is how much of the brief budget is already spent.
fn last_session(body: &str, path: &str, branch: &str, used: usize) -> String {
    let hb = frontmatter::get(body, "branch").unwrap_or_default();
    let when = frontmatter::get(body, "ended").unwrap_or_default();
    let mut label = when[..10.min(when.len())].to_string();
    if let Some(h) = frontmatter::get(body, "harness").filter(|h| h != "unknown") {
        label.push_str(&format!(", {h}"));
    }
    if hb != branch && !hb.is_empty() {
        label.push_str(&format!(", branch {hb}"));
    }
    let mut out = format!("## Last session ({label})\n");
    let room = limits::brief::MAX_CHARS.saturating_sub(used + out.len() + 200);
    out.push_str(&cut(&handoff_for_brief(body), room));
    out.push_str(&format!("\n\n_Full handoff: {path}_\n"));
    out
}

/// Sections of agent prose get their own cap so they leave room for the
/// sections after them.
const SECTION_CAPS: &[(&str, usize)] = &[("## Where it stopped", 700), ("## Plan", 300)];

/// The handoff minus what the brief already says: its title (the section
/// header has date and harness) and its Remembered list. Its sections
/// nest under "Last session".
fn handoff_for_brief(body: &str) -> String {
    let mut out = Vec::new();
    let mut skipping = false;
    let mut cap: Option<usize> = None;
    let mut used = 0;
    for l in frontmatter::strip(body).lines() {
        if l.starts_with("# ") {
            continue;
        }
        if l.starts_with("## ") {
            skipping = l == "## Remembered";
            cap = SECTION_CAPS.iter().find(|(h, _)| *h == l).map(|(_, n)| *n);
            used = 0;
            out.push(format!("#{l}"));
            continue;
        }
        if skipping {
            continue;
        }
        if let Some(max) = cap {
            if used >= max {
                continue;
            }
            used += l.len() + 1;
            if used >= max && !l.is_empty() {
                out.push(format!("{l}\n…"));
                continue;
            }
        }
        out.push(l.to_string());
    }
    out.join("\n").trim().to_string()
}

/// One line per item, grouped by kind, until the budget runs out.
fn memory_section(items: &[memory::Item], shared_dir: &str) -> String {
    let mut s = String::from("## Remembered\n");
    let mut shown = 0;
    for it in items {
        let line = format!("- {}: {}\n", it.kind, it.title);
        if s.len() + line.len() > limits::brief::MEMORY_CHARS {
            break;
        }
        s.push_str(&line);
        shown += 1;
    }
    if shown < items.len() {
        s.push_str(&format!("- … +{} more in {shared_dir}\n", items.len() - shown));
    }
    s.push('\n');
    s
}

fn cut(s: &str, max: usize) -> String {
    match cut_lines(s, max) {
        (head, true) => format!("{}\n…", head.trim_end()),
        (all, false) => all,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inputs() -> Inputs {
        Inputs {
            project: "---\ngenerated: x\n---\n# app\n".into(),
            branch: "main".into(),
            handoff: None,
            items: vec![],
            shared_dir: ".relay".into(),
        }
    }

    #[test]
    fn nothing_to_tell_is_an_empty_brief() {
        let empty = Inputs { project: String::new(), ..inputs() };
        assert_eq!(compose(&empty), "");
    }

    #[test]
    fn handoff_from_another_branch_says_so() {
        let body = "---\nbranch: feat\nended: 2026-09-22T10:00:00Z\nharness: codex\n---\n# Handoff\n## Asked\n- ship\n";
        let out = compose(&Inputs { handoff: Some(("h.md".into(), body.into())), ..inputs() });
        assert!(out.starts_with("# relay brief\n# app\n\n## Last session (2026-09-22, codex, branch feat)\n"), "{out}");
        assert!(out.contains("### Asked\n- ship\n\n_Full handoff: h.md_\n"), "{out}");
    }

    #[test]
    fn long_replies_leave_room_for_later_sections() {
        let reply = "word ".repeat(40).trim().to_string();
        let body = format!("## Where it stopped\n{}\n\n## Asked\n- ship it\n", [reply.as_str(); 10].join("\n"));
        let out = handoff_for_brief(&body);
        assert!(out.len() < 1000, "{}", out.len());
        assert!(out.contains("…") && out.ends_with("- ship it"), "{out}");
    }
}
