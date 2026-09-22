//! The SessionStart brief: project.md head + latest handoff, capped at
//! roughly 600 tokens. File reads and one git call; no LLM.

use crate::helpers::git as gitstate;
use crate::core::handoff;
use crate::core::paths::Paths;

pub const BRIEF_MAX_CHARS: usize = 2400;
const PROJECT_MAX_CHARS: usize = 1000;

pub fn build(paths: &Paths) -> String {
    let project = std::fs::read_to_string(paths.project_file()).unwrap_or_default();
    let branch = gitstate::branch(&paths.root);
    let handoff = handoff::latest(paths, &branch);

    let mut out = String::new();
    out.push_str("# relay brief\n");
    if !project.trim().is_empty() {
        out.push_str(&cut(handoff::strip_frontmatter(&project).trim(), PROJECT_MAX_CHARS));
        out.push_str("\n\n");
    }
    match handoff {
        Some((p, body)) => {
            let hb = handoff::frontmatter(&body, "branch").unwrap_or_default();
            let when = handoff::frontmatter(&body, "ended").unwrap_or_default();
            out.push_str(&format!("## Last session ({}{})\n", &when[..10.min(when.len())], if hb != branch && !hb.is_empty() { format!(", branch {hb}") } else { String::new() }));
            let room = BRIEF_MAX_CHARS.saturating_sub(out.len() + 200);
            out.push_str(&cut(handoff::strip_frontmatter(&body), room));
            out.push_str(&format!("\n\n_Full handoff: {}_\n", paths.rel(&p)));
        }
        None => {
            if project.trim().is_empty() {
                return String::new();
            }
        }
    }
    out.push_str("_Outputs shown by relay are compressed; `relay get <id>` prints the original._\n");
    out
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
    let head = head.rfind('\n').map(|i| &head[..i]).unwrap_or(head);
    format!("{head}\n…")
}
