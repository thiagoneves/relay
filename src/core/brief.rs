//! The `SessionStart` brief: project.md head + remembered items + latest
//! handoff, capped at roughly 600 tokens. File reads and one git call; no LLM.

use crate::core::paths::Paths;
use crate::core::{handoff, memory};
use crate::helpers::git as gitstate;

pub const BRIEF_MAX_CHARS: usize = 2400;
const PROJECT_MAX_CHARS: usize = 1000;
const MEMORY_MAX_CHARS: usize = 700;

pub fn build(paths: &Paths) -> String {
    let project = std::fs::read_to_string(paths.project_file()).unwrap_or_default();
    let branch = gitstate::branch(&paths.root);
    let handoff = handoff::latest(paths, &branch);
    let items = memory::list(paths);

    let mut out = String::new();
    out.push_str("# relay brief\n");
    if !project.trim().is_empty() {
        out.push_str(&cut(handoff::strip_frontmatter(&project).trim(), PROJECT_MAX_CHARS));
        out.push_str("\n\n");
    }
    if !items.is_empty() {
        out.push_str(&memory_section(paths, &items));
    }
    match handoff {
        Some((p, body)) => {
            let hb = handoff::frontmatter(&body, "branch").unwrap_or_default();
            let when = handoff::frontmatter(&body, "ended").unwrap_or_default();
            out.push_str(&format!(
                "## Last session ({}{})\n",
                &when[..10.min(when.len())],
                if hb != branch && !hb.is_empty() { format!(", branch {hb}") } else { String::new() }
            ));
            let room = BRIEF_MAX_CHARS.saturating_sub(out.len() + 200);
            out.push_str(&cut(handoff::strip_frontmatter(&body), room));
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

/// One line per item, grouped by kind, until the budget runs out.
fn memory_section(paths: &Paths, items: &[memory::Item]) -> String {
    let mut s = String::from("## Remembered\n");
    let mut shown = 0;
    for it in items {
        let line = format!("- {}: {}\n", it.kind, it.title);
        if s.len() + line.len() > MEMORY_MAX_CHARS {
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
    if s.len() <= max {
        return s.to_string();
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    let head = &s[..end];
    let head = head.rfind('\n').map_or(head, |i| &head[..i]);
    format!("{head}\n…")
}
