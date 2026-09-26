//! `relay brief <query>`: one page for one task, so an agent stops
//! reading whole plan docs to find it. The sections of the repo's
//! Markdown docs whose heading names the query (`T-253` finds its entry,
//! not `T-2530`), the remembered items that mention it, and the files it
//! will likely touch. Deterministic, no model, bounded.

use crate::core::outline::{self, Entry};
use crate::core::paths::Paths;
use crate::core::{index, likely, memory};
use crate::helpers::git as gitstate;
use crate::helpers::text::cut_lines;
use crate::helpers::truncate_chars;
use crate::limits::task_brief::{DOC_MAX_BYTES, MAX_CHARS, MIN_SECTION_CHARS, OTHER_MATCHES, SECTION_CHARS, SECTIONS};

/// A section of a doc that matched, and how well.
struct Hit {
    file: String,
    entry: Entry,
    body: String,
    /// The heading starts with the query (0), names it (1), or only the
    /// body does (2): `### T-253 · …` is the task, `### T-241 · … T-253`
    /// depends on it.
    rank: u8,
}

/// Whether `hay` mentions every word of `query`, case-insensitively and
/// as whole tokens: `T-25` is not in `T-253`.
pub fn mentions(hay: &str, query: &str) -> bool {
    let hay = hay.to_lowercase();
    query.split_whitespace().all(|w| {
        let w = w.to_lowercase();
        hay.match_indices(&w).any(|(i, _)| {
            let before = hay[..i].chars().next_back();
            let after = hay[i + w.len()..].chars().next();
            !before.is_some_and(char::is_alphanumeric) && !after.is_some_and(char::is_alphanumeric)
        })
    })
}

pub fn build(paths: &Paths, query: &str) -> String {
    let hits = sections(paths, query);
    let items: Vec<String> = memory::list(paths)
        .into_iter()
        .filter(|i| mentions(&i.title, query))
        .map(|i| format!("- {}: {}", i.kind, i.title))
        .collect();
    // The doc that describes the task is already on the page.
    let files: Vec<likely::Likely> =
        likely::files(paths, query).into_iter().filter(|l| !hits.iter().any(|h| h.file == l.file)).collect();
    compose(query, &hits, &items, &files)
}

fn compose(query: &str, hits: &[Hit], items: &[String], files: &[likely::Likely]) -> String {
    let mut out = format!("# relay brief: {query}\n");
    if hits.is_empty() && items.is_empty() && files.is_empty() {
        out.push_str("Nothing in the docs, memory or recent sessions names it. Try another id or fewer words.\n");
        return out;
    }
    let mut printed = 0;
    let mut more = Vec::new();
    for h in hits {
        let room = SECTION_CHARS.min(MAX_CHARS.saturating_sub(out.len() + 800));
        if printed == SECTIONS || room < MIN_SECTION_CHARS {
            if more.len() < OTHER_MATCHES {
                more.push(format!("{} L{}-{}", h.file, h.entry.start, h.entry.end));
            }
            continue;
        }
        printed += 1;
        out.push_str(&section(h, room));
    }
    if !more.is_empty() {
        out.push_str(&format!("\nAlso named in: {}\n", more.join(", ")));
    }
    if !items.is_empty() {
        out.push_str("\n## Remembered\n");
        out.push_str(&items.join("\n"));
        out.push('\n');
    }
    if !files.is_empty() {
        out.push_str("\n## Likely files\n");
        for f in files {
            out.push_str(&format!("- {f}\n"));
        }
    }
    out
}

/// One matching section under its file and range, cut to `room` with the
/// range of the rest.
fn section(h: &Hit, room: usize) -> String {
    let mut out = format!("\n## {} L{}-{}\n", h.file, h.entry.start, h.entry.end);
    match fit(&h.body, room) {
        (text, false) => out.push_str(&text),
        (text, true) => out.push_str(&format!(
            "{}\n… cut; the rest is Read offset {} limit {}",
            text.trim_end(),
            h.entry.start,
            h.entry.end + 1 - h.entry.start
        )),
    }
    out.push('\n');
    out
}

/// Whole lines of `text` that fit in `max` bytes, the line that does not
/// fit cut short, and whether anything was left out. Plan entries are
/// often one long line each.
fn fit(text: &str, max: usize) -> (String, bool) {
    let (mut head, cut) = cut_lines(text, max);
    if cut {
        let room = max.saturating_sub(head.len() + 1);
        let taken = head.lines().count();
        if let Some(next) = text.lines().skip(taken).find(|l| !l.is_empty()).filter(|_| room > 80) {
            head = format!("{}\n{}", head.trim_end(), truncate_chars(next, room * 9 / 10));
        }
    }
    (head, cut)
}

/// Sections whose heading names the query, most specific first; when no
/// heading does, the deepest section around each line that does.
fn sections(paths: &Paths, query: &str) -> Vec<Hit> {
    let mut docs: Vec<String> = gitstate::tracked_files(&paths.root);
    docs.extend(gitstate::dirty_files(&paths.root, crate::limits::claims::DIRTY_READ));
    docs.sort();
    docs.dedup();
    let mut hits = Vec::new();
    for rel in docs.iter().filter(|f| outline::is_markdown(f) && !f.starts_with(".relay/")) {
        let full = paths.root.join(rel);
        if std::fs::metadata(&full).map_or(true, |m| m.len() > DOC_MAX_BYTES) {
            continue;
        }
        let Some(idx) = index::get(paths, rel, &full) else { continue };
        // An id the index does not place in this doc: the doc is not read.
        let upper = query.trim().to_uppercase();
        if index::is_id(query)
            && !idx.ids.contains_key(&upper)
            && !idx.entries.iter().any(|e| mentions(&e.title, query))
        {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&full) else { continue };
        if !mentions(&text, query) {
            continue;
        }
        hits.extend(in_doc(rel, &text, &idx.entries, query));
    }
    hits.sort_by(|a, b| a.rank.cmp(&b.rank).then(b.entry.depth.cmp(&a.entry.depth)).then(a.file.cmp(&b.file)));
    hits
}

/// Whether the first word of `title`, past any list or number marks,
/// is the query's first word.
fn starts_with(title: &str, query: &str) -> bool {
    let title = title.trim_start_matches(|c: char| !c.is_alphanumeric());
    let first = query.split_whitespace().next().unwrap_or("");
    mentions(title.split_whitespace().next().unwrap_or(""), first)
}

fn in_doc(rel: &str, text: &str, entries: &[Entry], query: &str) -> Vec<Hit> {
    let lines: Vec<&str> = text.lines().collect();
    let body = |e: &Entry| lines[e.start - 1..e.end.min(lines.len())].join("\n");
    let hit = |e: &Entry, rank| Hit { file: rel.to_string(), entry: e.clone(), body: body(e), rank };
    let mut out: Vec<Hit> = entries
        .iter()
        .filter(|e| mentions(&e.title, query))
        .map(|e| hit(e, u8::from(!starts_with(&e.title, query))))
        .collect();
    // Then the deepest section around each other line that names it, once
    // each, outside the sections already named.
    let inside = |line: usize, hits: &[Hit]| hits.iter().any(|h| h.entry.start <= line && line <= h.entry.end);
    for (i, _) in lines.iter().enumerate().filter(|(_, l)| mentions(l, query)) {
        let line = i + 1;
        if inside(line, &out) {
            continue;
        }
        if let Some(e) = entries.iter().filter(|e| e.start <= line && line <= e.end).max_by_key(|e| e.depth) {
            out.push(hit(e, 2));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_id_matches_as_a_whole_token() {
        assert!(mentions("### T-253 · Filters", "T-253"));
        assert!(mentions("see t-253.", "T-253"));
        assert!(!mentions("### T-2530 · Other", "T-253"));
        assert!(!mentions("AT-253", "T-253"));
        assert!(mentions("Voice picker for the learner", "learner voice"));
        assert!(!mentions("Voice picker", "learner voice"));
    }

    #[test]
    fn the_heading_that_names_the_task_wins() {
        let doc = "# Plan\n## Phase 1\n### T-1 Login\nneeds T-2\n### T-2 Pay\nstripe\n## Phase 2\n";
        let at = |d: &str| outline::of(d, true);
        let hits = in_doc("docs/plan.md", doc, &at(doc), "T-2");
        let found: Vec<(usize, usize, u8)> = hits.iter().map(|h| (h.entry.start, h.entry.end, h.rank)).collect();
        assert_eq!(found, [(5, 6, 0), (3, 4, 2)], "the task, then the section that mentions it");
        let two = "### T-3 · Radar — T-2\nx\n### T-2 · Pay\ny\n";
        let dependent = in_doc("docs/plan.md", two, &at(two), "T-2");
        assert_eq!(dependent.iter().map(|h| h.rank).collect::<Vec<_>>(), [1, 0]);
        assert_eq!(hits[0].body, "### T-2 Pay\nstripe");
        let loose = in_doc("docs/plan.md", doc, &at(doc), "stripe");
        assert_eq!((loose[0].entry.title.as_str(), loose[0].rank), ("T-2 Pay", 2));
    }

    #[test]
    fn a_long_section_is_cut_with_the_range_to_read() {
        let body = "line of the task\n".repeat(2000);
        let hit = Hit {
            file: "docs/plan.md".into(),
            entry: Entry { depth: 3, title: "T-9".into(), start: 10, end: 2010 },
            body,
            rank: 0,
        };
        let out = compose("T-9", &[hit], &[], &[]);
        assert!(out.len() < MAX_CHARS, "{}", out.len());
        assert!(out.contains("… cut; the rest is Read offset 10 limit 2001"), "{out}");
    }
}
