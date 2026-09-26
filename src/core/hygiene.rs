//! Memory that has outgrown its use. Every item costs every session a
//! line of the brief, and past the brief's budget an item is not shown at
//! all. `relay compile --hygiene` names what to merge, check, shorten or
//! drop; it changes nothing itself.

use std::collections::BTreeSet;

use crate::core::memory::Item;
use crate::limits::brief::MEMORY_CHARS;
use crate::limits::hygiene::{OVERLONG_CHARS, SIMILAR};

/// What to do about one or two items.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Advice {
    /// Two items say nearly the same thing: keep one.
    Merge(usize, usize),
    /// Paths it is about changed since it was saved.
    Stale(usize, Vec<String>),
    Expired(usize),
    /// Longer than a line the brief can show.
    Overlong(usize),
    /// Items the brief's budget leaves out, oldest last: shown to nobody.
    Unseen(Vec<usize>),
}

fn words(title: &str) -> BTreeSet<String> {
    title.split(|c: char| !c.is_alphanumeric()).filter(|w| w.chars().count() >= 3).map(str::to_lowercase).collect()
}

/// Share of words two titles have in common (Jaccard).
fn similarity(a: &BTreeSet<String>, b: &BTreeSet<String>) -> f64 {
    let union = a.union(b).count();
    if union == 0 { 0.0 } else { a.intersection(b).count() as f64 / union as f64 }
}

/// Advice for `items`, in the order the brief lists them; `stale` and
/// `expired` as the brief computes them.
pub fn review(items: &[Item], stale: &[Vec<String>], now_iso: &str) -> Vec<Advice> {
    let mut out = Vec::new();
    let bags: Vec<BTreeSet<String>> = items.iter().map(|i| words(&i.title)).collect();
    for i in 0..items.len() {
        for j in i + 1..items.len() {
            if similarity(&bags[i], &bags[j]) >= SIMILAR {
                out.push(Advice::Merge(i, j));
            }
        }
    }
    for (i, it) in items.iter().enumerate() {
        if it.expired(now_iso) {
            out.push(Advice::Expired(i));
        } else if let Some(paths) = stale.get(i).filter(|p| !p.is_empty()) {
            out.push(Advice::Stale(i, paths.clone()));
        }
        if it.title.chars().count() > OVERLONG_CHARS {
            out.push(Advice::Overlong(i));
        }
    }
    let unseen = unseen(items);
    if !unseen.is_empty() {
        out.push(Advice::Unseen(unseen));
    }
    out
}

/// The items past the brief's memory budget, measured as the brief
/// lists them: `- kind: title`.
fn unseen(items: &[Item]) -> Vec<usize> {
    let mut used = "## Remembered\n".len();
    let mut out = Vec::new();
    for (i, it) in items.iter().enumerate() {
        used += format!("- {}: {}\n", it.kind, it.title).len();
        if used > MEMORY_CHARS {
            out.push(i);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::memory::Kind;

    fn item(title: &str) -> Item {
        Item {
            kind: Kind::Rule,
            path: format!("{title}.md").into(),
            title: title.into(),
            created: String::new(),
            sha: None,
            about: vec![],
            expires: None,
        }
    }

    #[test]
    fn near_duplicates_stale_overlong_and_unseen() {
        let mut items = vec![
            item("Run cargo test before every commit"),
            item("Run cargo test before each commit"),
            item("Amounts are in cents"),
            item(&"very long rule ".repeat(12)),
        ];
        items[2].expires = Some("2026-01-01T00:00:00Z".into());
        let stale = vec![vec![], vec!["src/pay.rs".to_string()], vec![], vec![]];
        let got = review(&items, &stale, "2026-09-26T00:00:00Z");
        assert!(got.contains(&Advice::Merge(0, 1)), "{got:?}");
        assert!(got.contains(&Advice::Stale(1, vec!["src/pay.rs".into()])), "{got:?}");
        assert!(got.contains(&Advice::Expired(2)), "{got:?}");
        assert!(got.contains(&Advice::Overlong(3)), "{got:?}");
        assert!(!got.iter().any(|a| matches!(a, Advice::Unseen(_))), "all fit: {got:?}");
        let many: Vec<Item> = (0..40).map(|i| item(&format!("Rule number {i} about a distinct topic {i}"))).collect();
        let Some(Advice::Unseen(left_out)) = review(&many, &[], "").pop() else { panic!("expected unseen") };
        assert_eq!(left_out.first(), Some(&14));
    }
}
