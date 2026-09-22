//! Promote what recent sessions settled into memory. A handoff is a
//! buffer with a half-life of days; a decision the user answered during a
//! session deserves to outlive it. No model: the candidates are the
//! decisions the handoffs already list, minus what memory already holds.

use crate::core::handoff;
use crate::core::memory;
use crate::core::paths::Paths;
use crate::helpers::frontmatter;

/// A decision found in a handoff and not yet remembered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub text: String,
    /// The session it was decided in.
    pub session: Option<String>,
    pub ended: String,
}

pub fn candidates(paths: &Paths, sessions: usize) -> Vec<Candidate> {
    let remembered: Vec<String> = memory::list(paths).into_iter().map(|i| i.title).collect();
    let bodies: Vec<String> = handoff::recent(paths, sessions).into_iter().map(|(_, body)| body).collect();
    from_handoffs(&bodies, &remembered)
}

/// Newest handoffs first; each decision once, in the order first seen.
pub fn from_handoffs(bodies: &[String], remembered: &[String]) -> Vec<Candidate> {
    let mut out: Vec<Candidate> = Vec::new();
    for body in bodies {
        let session = frontmatter::get(body, "session");
        let ended = frontmatter::get(body, "ended").map(|e| e[..10.min(e.len())].to_string()).unwrap_or_default();
        for text in decisions(body) {
            if remembered.iter().any(|r| r == &text) || out.iter().any(|c| c.text == text) {
                continue;
            }
            out.push(Candidate { text, session: session.clone(), ended: ended.clone() });
        }
    }
    out
}

fn decisions(body: &str) -> Vec<String> {
    let mut in_section = false;
    let mut out = Vec::new();
    for l in body.lines() {
        if l.starts_with("## ") {
            in_section = l == "## Decisions";
            continue;
        }
        if in_section && let Some(item) = l.strip_prefix("- ") {
            out.push(item.trim().to_string());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decisions_not_yet_remembered_are_candidates_once() {
        let newer = "---\nsession: s2\nended: 2026-09-22T10:00:00Z\n---\n## Decisions\n- Backoff: exponential\n- DB: postgres\n## Asked\n- x\n";
        let older = "---\nsession: s1\nended: 2026-09-20T10:00:00Z\n---\n## Decisions\n- DB: postgres\n- Auth: oauth\n";
        let got = from_handoffs(&[newer.into(), older.into()], &["Auth: oauth".to_string()]);
        let texts: Vec<&str> = got.iter().map(|c| c.text.as_str()).collect();
        assert_eq!(texts, ["Backoff: exponential", "DB: postgres"]);
        assert_eq!((got[1].session.as_deref(), got[1].ended.as_str()), (Some("s2"), "2026-09-22"));
    }
}
