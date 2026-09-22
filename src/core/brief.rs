//! The `SessionStart` brief: project.md head + remembered items + latest
//! handoff, capped at roughly 600 tokens. File reads and one git call; no LLM.

use crate::core::paths::Paths;
use crate::core::{handoff, memory};
use crate::helpers::frontmatter;
use crate::helpers::git as gitstate;
use crate::helpers::text::cut_lines;
use crate::limits;

pub fn build(paths: &Paths) -> String {
    let project = std::fs::read_to_string(paths.project_file()).unwrap_or_default();
    let branch = gitstate::branch(&paths.root);
    let handoff = handoff::latest(paths, &branch);
    let items = memory::list(paths);

    let mut out = String::new();
    out.push_str("# relay brief\n");
    if !project.trim().is_empty() {
        out.push_str(&cut(frontmatter::strip(&project).trim(), limits::brief::PROJECT_CHARS));
        out.push_str("\n\n");
    }
    if !items.is_empty() {
        out.push_str(&memory_section(paths, &items));
    }
    match handoff {
        Some((p, body)) => {
            let hb = frontmatter::get(&body, "branch").unwrap_or_default();
            let when = frontmatter::get(&body, "ended").unwrap_or_default();
            let mut label = when[..10.min(when.len())].to_string();
            if let Some(h) = frontmatter::get(&body, "harness").filter(|h| h != "unknown") {
                label.push_str(&format!(", {h}"));
            }
            if hb != branch && !hb.is_empty() {
                label.push_str(&format!(", branch {hb}"));
            }
            out.push_str(&format!("## Last session ({label})\n"));
            let room = limits::brief::MAX_CHARS.saturating_sub(out.len() + 200);
            out.push_str(&cut(&handoff_for_brief(&body), room));
            out.push_str(&format!("\n\n_Full handoff: {}_\n", paths.rel(&p)));
        }
        None => {
            if project.trim().is_empty() && items.is_empty() {
                return String::new();
            }
        }
    }
    out.push_str("_Outputs shown by relay are compressed; `relay get <id>` prints the original._\n");
    out.push_str("_When you settle a decision, hit a gotcha or learn a project rule, save it: `relay remember decision|gotcha|rule \"<one line>\"`._\n");
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
fn memory_section(paths: &Paths, items: &[memory::Item]) -> String {
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
        s.push_str(&format!("- … +{} more in {}\n", items.len() - shown, paths.rel(&paths.shared)));
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

    #[test]
    fn long_replies_leave_room_for_later_sections() {
        let reply = "word ".repeat(40).trim().to_string();
        let body = format!("## Where it stopped\n{}\n\n## Asked\n- ship it\n", [reply.as_str(); 10].join("\n"));
        let out = handoff_for_brief(&body);
        assert!(out.len() < 1000, "{}", out.len());
        assert!(out.contains("…") && out.ends_with("- ship it"), "{out}");
    }
}
