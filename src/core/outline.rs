//! The outline of a text file: Markdown headings, or a code file's
//! top-level items, each with the lines it spans. Cheap line scanning, no
//! parser: it only has to tell an agent which range to read.

use serde::{Deserialize, Serialize};

use crate::helpers::truncate_chars;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    /// Heading level for Markdown (1 for `#`); 1 for a code item.
    pub depth: usize,
    pub title: String,
    /// First and last line, 1-based and inclusive.
    pub start: usize,
    pub end: usize,
}

const TITLE_CHARS: usize = 100;

pub fn is_markdown(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    [".md", ".markdown", ".mdx"].iter().any(|x| lower.ends_with(x))
}

/// The outline of `text`, read as Markdown when `markdown`, as code
/// otherwise; fixed chunks when neither finds any structure.
pub fn of(text: &str, markdown: bool) -> Vec<Entry> {
    let lines: Vec<&str> = text.lines().collect();
    let found = if markdown { headings(&lines) } else { blocks(&lines) };
    if found.is_empty() { chunks(&lines) } else { found }
}

/// ATX headings outside fenced code. A section ends where the next
/// heading of the same or a higher level starts.
fn headings(lines: &[&str]) -> Vec<Entry> {
    let mut out: Vec<Entry> = Vec::new();
    let mut fence: Option<&str> = None;
    for (i, l) in lines.iter().enumerate() {
        let t = l.trim_start();
        if let Some(f) = ["```", "~~~"].into_iter().find(|f| t.starts_with(f)) {
            fence = match fence {
                Some(open) if open == f => None,
                None => Some(f),
                other => other,
            };
            continue;
        }
        if fence.is_some() || l.starts_with("    ") {
            continue;
        }
        let depth = t.chars().take_while(|c| *c == '#').count();
        if !(1..=6).contains(&depth) || !t[depth..].starts_with(' ') {
            continue;
        }
        let title = t[depth..].trim().trim_end_matches('#').trim();
        out.push(Entry { depth, title: truncate_chars(title, TITLE_CHARS), start: i + 1, end: lines.len() });
    }
    for i in 0..out.len() {
        let next = out[i + 1..].iter().find(|e| e.depth <= out[i].depth).map(|e| e.start - 1);
        out[i].end = next.unwrap_or(lines.len());
    }
    out
}

/// Top-level items in any language: a line at column 0 that opens an
/// indented block below it. An item runs until the next one starts,
/// less the comments and attributes that lead into that one.
fn blocks(lines: &[&str]) -> Vec<Entry> {
    let indented = |l: &str| l.starts_with([' ', '\t']) && !l.trim().is_empty();
    let mut starts = Vec::new();
    for (i, l) in lines.iter().enumerate() {
        if l.is_empty() || l.starts_with([' ', '\t', '}', ')', ']']) {
            continue;
        }
        let next = lines[i + 1..].iter().find(|n| !n.trim().is_empty());
        if next.is_some_and(|n| indented(n)) {
            starts.push(i);
        }
    }
    let mut out = Vec::new();
    for (k, &i) in starts.iter().enumerate() {
        let end = starts.get(k + 1).map_or(lines.len(), |&n| last_before(lines, n));
        out.push(Entry { depth: 1, title: truncate_chars(lines[i].trim_end(), TITLE_CHARS), start: i + 1, end });
    }
    out
}

/// The last line of an item that ends just before line index `next`:
/// blank lines, comments and attributes that lead into `next` are not
/// part of it.
fn last_before(lines: &[&str], next: usize) -> usize {
    let mut end = next;
    while end > 0 {
        let l = lines[end - 1].trim_start();
        let leads = l.is_empty() || ["//", "#", "/*", "*", "@", "--"].iter().any(|p| l.starts_with(p));
        if !leads || lines[end - 1].starts_with([' ', '\t']) {
            break;
        }
        end -= 1;
    }
    end.max(1)
}

/// Fixed ranges, each named by its first non-blank line.
fn chunks(lines: &[&str]) -> Vec<Entry> {
    const LINES: usize = 400;
    (0..lines.len())
        .step_by(LINES)
        .map(|start| {
            let end = (start + LINES).min(lines.len());
            let title = lines[start..end].iter().find(|l| !l.trim().is_empty()).map_or("", |l| l.trim());
            Entry { depth: 1, title: truncate_chars(title, TITLE_CHARS), start: start + 1, end }
        })
        .collect()
}

/// At most `max` entries, in file order: every level that fits whole,
/// then the first entries of the next level under each parent, so a long
/// plan shows its phases and a few tasks of each rather than the first
/// eighty tasks.
pub fn pick(entries: &[Entry], max: usize) -> Vec<&Entry> {
    let upto = |d: usize| entries.iter().filter(|e| e.depth <= d).count();
    let deepest = entries.iter().map(|e| e.depth).max().unwrap_or(1);
    let Some(depth) = (1..=deepest).rev().find(|d| upto(*d) <= max) else {
        return entries.iter().take(max).collect();
    };
    // Each entry one level deeper, numbered within its parent.
    let mut nth = vec![usize::MAX; entries.len()];
    let mut seen = 0;
    for (i, e) in entries.iter().enumerate() {
        if e.depth <= depth {
            seen = 0;
        } else if e.depth == depth + 1 {
            nth[i] = seen;
            seen += 1;
        }
    }
    let with = |k: usize| upto(depth) + nth.iter().filter(|n| **n < k).count();
    let mut k = 0;
    while depth < deepest && with(k + 1) <= max && with(k + 1) > with(k) {
        k += 1;
    }
    entries.iter().zip(&nth).filter(|(e, n)| e.depth <= depth || **n < k).map(|(e, _)| e).collect()
}

/// `pick`ed entries, one per line with its range, and how many were left
/// out.
pub fn render(entries: &[Entry], max: usize) -> String {
    let shown = pick(entries, max);
    let min = shown.iter().map(|e| e.depth).min().unwrap_or(1);
    let mut out = String::new();
    for e in &shown {
        let range = format!("L{}-{}", e.start, e.end);
        out.push_str(&format!("{range:<13}{}{}\n", "  ".repeat(e.depth - min), e.title));
    }
    let hidden = entries.len() - shown.len();
    if hidden > 0 {
        out.push_str(&format!("… {hidden} more entries, deeper or later; Grep for a word to find them\n"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markdown_sections_end_at_the_next_peer() {
        let md = "# Plan\nintro\n## T-1 Login\ntext\n```\n# not a heading\n```\n## T-2 Pay\nmore\n# Appendix\nx\n";
        let got = of(md, true);
        let spans: Vec<(usize, &str, usize, usize)> =
            got.iter().map(|e| (e.depth, e.title.as_str(), e.start, e.end)).collect();
        assert_eq!(spans, [(1, "Plan", 1, 9), (2, "T-1 Login", 3, 7), (2, "T-2 Pay", 8, 9), (1, "Appendix", 10, 11)]);
    }

    #[test]
    fn code_items_are_blocks_at_column_zero() {
        let rs = "use std::fs;\n\n/// Doc.\npub fn a() {\n    1\n}\n\n#[test]\nfn b() {\n    2\n}\n";
        let got = of(rs, false);
        let spans: Vec<(&str, usize, usize)> = got.iter().map(|e| (e.title.as_str(), e.start, e.end)).collect();
        assert_eq!(spans, [("pub fn a() {", 4, 6), ("fn b() {", 9, 11)]);
    }

    #[test]
    fn text_without_structure_is_cut_in_chunks() {
        let text = "word\n".repeat(900);
        let got = of(&text, false);
        assert_eq!(got.iter().map(|e| (e.start, e.end)).collect::<Vec<_>>(), [(1, 400), (401, 800), (801, 900)]);
    }

    #[test]
    fn a_long_outline_keeps_the_top_levels() {
        let mut md = String::from("# Plan\n");
        for i in 0..50 {
            md.push_str(&format!("## Phase {i}\n### T-{i}\nbody\n"));
        }
        let out = render(&of(&md, true), 60);
        assert!(out.starts_with("L1-151       Plan\nL2-4           Phase 0\n"), "{out}");
        assert!(!out.contains("T-3"), "{out}");
        assert!(out.ends_with("… 50 more entries, deeper or later; Grep for a word to find them\n"), "{out}");
        // Room for more: the first few children of each parent.
        let mut md = String::from("# Plan\n");
        for p in 0..5 {
            md.push_str(&format!("## Phase {p}\n"));
            for t in 0..20 {
                md.push_str(&format!("### T-{p}{t:02}\nbody\n"));
            }
        }
        let shown: Vec<String> = pick(&of(&md, true), 30).iter().map(|e| e.title.clone()).collect();
        assert_eq!(shown.len(), 26, "{shown:?}");
        assert_eq!(&shown[..7], ["Plan", "Phase 0", "T-000", "T-001", "T-002", "T-003", "Phase 1"]);
    }
}
