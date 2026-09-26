//! Size budgets for what agents read over and over: plan docs, notes on
//! finished tasks, ADR implementation sections, instruction files, the
//! memory index and commit subjects. Each check is a rule of shape (bytes,
//! lines, a heading's first word, a marker a Markdown renderer knows), so
//! it holds in any language. The budgets live in `limits::lint`.

use std::sync::LazyLock;

use regex::Regex;

use crate::core::outline;
use crate::helpers::human_bytes;
use crate::limits::lint::{ADR_SECTION_LINES, DOC_BYTES, DONE_LINES, INDEX_BYTES, INSTRUCTIONS_BYTES, SUBJECT_CHARS};

/// One budget a file or message is over.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    /// `path` or `path:line`, or `commit message`.
    pub at: String,
    pub what: String,
}

/// Every check that applies to the Markdown file `rel` with `text`.
pub fn file(rel: &str, text: &str) -> Vec<Finding> {
    let mut out = Vec::new();
    let name = rel.rsplit('/').next().unwrap_or(rel);
    let size = text.len() as u64;
    if matches!(name, "CLAUDE.md" | "AGENTS.md" | "GEMINI.md") && size > INSTRUCTIONS_BYTES {
        out.push(Finding {
            at: rel.into(),
            what: format!(
                "is {}, over {}: every call of every session carries it; move detail into docs it points to",
                human_bytes(size),
                human_bytes(INSTRUCTIONS_BYTES)
            ),
        });
    } else if rel == ".relay/index.md" && size > INDEX_BYTES {
        out.push(Finding {
            at: rel.into(),
            what: format!(
                "is {}, over {}: see what to merge or drop with `relay compile --hygiene`",
                human_bytes(size),
                human_bytes(INDEX_BYTES)
            ),
        });
    } else if outline::is_markdown(rel) && size > DOC_BYTES {
        out.push(Finding {
            at: rel.into(),
            what: format!(
                "is {}, over {}: split it by phase or area; agents read it by section",
                human_bytes(size),
                human_bytes(DOC_BYTES)
            ),
        });
    }
    if outline::is_markdown(rel) {
        out.extend(done_notes(rel, text));
        if is_adr(rel) {
            out.extend(adr_sections(rel, text));
        }
    }
    out
}

/// A finished task (a checked box or a check mark in its heading or list
/// item) keeps a one-line note; its history is in git.
fn done_notes(rel: &str, text: &str) -> Vec<Finding> {
    static CHECKED: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(^|\s)(\[[xX]\]|✅|✓|✔)(\s|$)").expect("valid regex"));
    static ITEM: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(\s*)[-*+] \[[xX]\] ").expect("valid regex"));
    let lines: Vec<&str> = text.lines().collect();
    let mut out = Vec::new();
    for e in headings(&lines, text).iter().filter(|e| CHECKED.is_match(&e.title)) {
        let body = own_lines(&lines, e.start, e.end);
        if body > DONE_LINES {
            out.push(Finding { at: format!("{rel}:{}", e.start), what: done_what(&e.title, body) });
        }
    }
    for (i, l) in lines.iter().enumerate() {
        let Some(c) = ITEM.captures(l) else { continue };
        let indent = c[1].len();
        let more = lines[i + 1..]
            .iter()
            .take_while(|n| !n.trim().is_empty() && n.len() - n.trim_start().len() > indent && !ITEM.is_match(n))
            .count();
        if more + 1 > DONE_LINES {
            out.push(Finding { at: format!("{rel}:{}", i + 1), what: done_what(l.trim(), more + 1) });
        }
    }
    out
}

fn done_what(title: &str, lines: usize) -> String {
    format!(
        "\"{}\" is done but keeps {lines} lines; leave {DONE_LINES}, git has the rest",
        crate::helpers::truncate_chars(title, 60)
    )
}

/// Real headings: without any, the outline falls back to fixed chunks.
fn headings(lines: &[&str], text: &str) -> Vec<outline::Entry> {
    outline::of(text, true).into_iter().filter(|e| is_heading(lines, e)).collect()
}

fn is_heading(lines: &[&str], e: &outline::Entry) -> bool {
    lines.get(e.start - 1).is_some_and(|l| l.trim_start().starts_with('#'))
}

/// Non-blank lines of a section, headings of subsections included.
fn own_lines(lines: &[&str], start: usize, end: usize) -> usize {
    lines[start.min(lines.len())..end.min(lines.len())].iter().filter(|l| !l.trim().is_empty()).count()
}

/// An ADR lives under a directory named for them.
fn is_adr(rel: &str) -> bool {
    rel.split('/').rev().skip(1).any(|d| {
        let d = d.to_ascii_lowercase();
        d == "adr" || d == "adrs" || d == "decisions"
    })
}

/// An ADR's implementation section (its heading starts with "Implement",
/// which covers implementation, implementação and implementación) says
/// where the code is, in a few lines; the code and git say the rest.
fn adr_sections(rel: &str, text: &str) -> Vec<Finding> {
    let lines: Vec<&str> = text.lines().collect();
    outline::of(text, true)
        .iter()
        .filter(|e| is_heading(&lines, e) && e.title.to_lowercase().starts_with("implement"))
        .filter_map(|e| {
            let n = own_lines(&lines, e.start, e.end);
            (n > ADR_SECTION_LINES).then(|| Finding {
                at: format!("{rel}:{}", e.start),
                what: format!(
                    "\"{}\" has {n} lines, over {ADR_SECTION_LINES}: point at the code instead of retelling it",
                    e.title
                ),
            })
        })
        .collect()
}

/// A commit message's subject line.
pub fn subject(message: &str) -> Option<Finding> {
    let first = message.lines().find(|l| !l.trim().is_empty() && !l.starts_with('#'))?;
    let n = first.chars().count();
    (n > SUBJECT_CHARS).then(|| Finding {
        at: "commit message".into(),
        what: format!("subject is {n} characters, over {SUBJECT_CHARS}: `git log --oneline` cuts it"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_finished_task_keeps_one_line() {
        let doc = "# Plan\n## T-1 ✅ Login\nShipped in a1b2.\n## T-2 [x] Pay\nShipped.\nDetails.\nMore.\n## T-3 Next\nlong\nlong\n";
        let got = file("docs/plan.md", doc);
        assert_eq!(got.len(), 1, "{got:?}");
        assert_eq!(got[0].at, "docs/plan.md:4");
        assert!(got[0].what.starts_with("\"T-2 [x] Pay\" is done but keeps 3 lines"), "{}", got[0].what);
        let list = "- [x] ship login\n  it took a while\n  and more\n- [ ] pay\n- [x] one line\n";
        assert_eq!(file("TODO.md", list).iter().map(|f| f.at.as_str()).collect::<Vec<_>>(), ["TODO.md:1"]);
    }

    #[test]
    fn adr_implementation_sections_stay_short() {
        let long = format!("# 0013 Scale\n## Decisão\nx\n## Implementação\n{}", "- step\n".repeat(12));
        let got = file("docs/decisions/0013-scale.md", &long);
        assert_eq!(got.len(), 1);
        assert!(got[0].what.contains("has 12 lines, over 10"), "{}", got[0].what);
        assert!(file("docs/plan/0013-scale.md", &long).is_empty(), "only ADRs");
    }

    #[test]
    fn sizes_and_subjects() {
        let big = "x".repeat(usize::try_from(INSTRUCTIONS_BYTES).unwrap() + 1);
        assert!(file("apps/api/CLAUDE.md", &big)[0].what.contains("every call of every session"));
        assert!(file("README.md", &big).is_empty());
        assert!(subject("Short subject\n\nbody").is_none());
        assert!(subject(&"word ".repeat(20)).unwrap().what.starts_with("subject is 100 characters"));
        assert!(subject("# comment\n\nFix it\n").is_none());
    }
}
