//! Durable project memory in the committed tier: one item per file
//! under `.relay/{rules,decisions,gotchas}/`. Written by `relay remember`
//! (the agent, mid-session). Review happens in `git diff`; there is no
//! pending queue.

use std::path::PathBuf;

use anyhow::{Result, bail};

use crate::core::paths::Paths;
use crate::helpers::git as gitstate;
use crate::helpers::{now_iso, truncate_chars, write_atomic};

/// Kinds in the order the brief shows them: rules bind every session,
/// gotchas save the most time, decisions explain the code.
pub const KINDS: &[&str] = &["rule", "gotcha", "decision"];

pub struct Item {
    pub kind: String,
    pub path: PathBuf,
    pub title: String,
    pub created: String,
}

pub fn dir_for(paths: &Paths, kind: &str) -> PathBuf {
    paths.shared.join(format!("{kind}s"))
}

pub struct NewItem<'a> {
    pub kind: &'a str,
    pub text: &'a str,
    pub files: &'a [String],
    pub session: Option<&'a str>,
}

pub fn remember(paths: &Paths, item: &NewItem) -> Result<PathBuf> {
    if !KINDS.contains(&item.kind) {
        bail!("unknown kind `{}` (use: {})", item.kind, KINDS.join(", "));
    }
    let text = item.text.trim();
    if text.is_empty() {
        bail!("nothing to remember: text is empty");
    }
    let title = title_of(text);
    if let Some(dup) = list(paths).into_iter().find(|i| i.kind == item.kind && i.title == title) {
        bail!("already remembered in {}", paths.rel(&dup.path));
    }
    let dir = dir_for(paths, item.kind);
    let path = unique_path(&dir, &slug(&title));
    let git = gitstate::state(&paths.root);

    let mut b = String::from("---\n");
    b.push_str(&format!("kind: {}\ncreated: {}\n", item.kind, now_iso()));
    if !git.branch.is_empty() {
        b.push_str(&format!("branch: {}\n", git.branch));
    }
    if !git.sha.is_empty() {
        b.push_str(&format!("sha: {}\n", git.sha));
    }
    if let Some(s) = item.session {
        b.push_str(&format!("session: {s}\n"));
    }
    if !item.files.is_empty() {
        b.push_str(&format!("paths: {}\n", item.files.join(", ")));
    }
    b.push_str("---\n\n");
    b.push_str(text);
    b.push('\n');
    write_atomic(&path, b.as_bytes())?;
    Ok(path)
}

/// All items, newest first within each kind, kinds in `KINDS` order.
pub fn list(paths: &Paths) -> Vec<Item> {
    let mut out = Vec::new();
    for kind in KINDS {
        let mut items: Vec<Item> = Vec::new();
        let Ok(rd) = std::fs::read_dir(dir_for(paths, kind)) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().and_then(|s| s.to_str()) != Some("md") {
                continue;
            }
            let Ok(body) = std::fs::read_to_string(&p) else { continue };
            let created = crate::core::handoff::frontmatter(&body, "created").unwrap_or_default();
            let title = title_of(crate::core::handoff::strip_frontmatter(&body));
            if title.is_empty() {
                continue;
            }
            items.push(Item { kind: kind.to_string(), path: p, title, created });
        }
        items.sort_by(|a, b| b.created.cmp(&a.created).then(a.path.cmp(&b.path)));
        out.extend(items);
    }
    out
}

/// First non-empty line, without markdown heading marks, capped.
fn title_of(text: &str) -> String {
    let line = text.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("");
    truncate_chars(line.trim_start_matches('#').trim(), 160)
}

fn slug(title: &str) -> String {
    let mut s = String::new();
    for c in title.chars().flat_map(char::to_lowercase) {
        if c.is_ascii_alphanumeric() {
            s.push(c);
        } else if !s.ends_with('-') && !s.is_empty() {
            s.push('-');
        }
        if s.len() >= 60 {
            break;
        }
    }
    let s = s.trim_end_matches('-').to_string();
    if s.is_empty() { "item".into() } else { s }
}

fn unique_path(dir: &std::path::Path, stem: &str) -> PathBuf {
    let first = dir.join(format!("{stem}.md"));
    if !first.exists() {
        return first;
    }
    (2..).map(|n| dir.join(format!("{stem}-{n}.md"))).find(|p| !p.exists()).expect("unbounded range")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugs_are_short_and_ascii() {
        assert_eq!(slug("Use `cargo nextest`, not cargo test!"), "use-cargo-nextest-not-cargo-test");
        assert_eq!(slug("Não usar ç"), "n-o-usar");
        assert_eq!(slug("!!!"), "item");
        assert!(slug(&"a ".repeat(100)).len() <= 60);
    }

    #[test]
    fn title_is_first_line_without_heading() {
        assert_eq!(title_of("\n## Hooks fail open\nbecause…"), "Hooks fail open");
    }
}
