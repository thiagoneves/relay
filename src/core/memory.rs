//! Durable project memory in the committed tier: one item per file
//! under `.relay/{rules,decisions,gotchas}/`. Written by `relay remember`
//! (the agent, mid-session). Review happens in `git diff`; there is no
//! pending queue.

use std::path::PathBuf;

use anyhow::{Result, bail};
use clap::ValueEnum;

use crate::core::frontmatter;
use crate::core::paths::Paths;
use crate::helpers::git as gitstate;
use crate::helpers::{now_iso, truncate_chars, write_atomic};

/// What an item is. Declaration order is the order the brief shows
/// them: rules bind every session, gotchas save the most time,
/// decisions explain the code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, ValueEnum)]
pub enum Kind {
    Rule,
    Gotcha,
    Decision,
}

impl Kind {
    pub const ALL: [Kind; 3] = [Kind::Rule, Kind::Gotcha, Kind::Decision];

    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Rule => "rule",
            Kind::Gotcha => "gotcha",
            Kind::Decision => "decision",
        }
    }

    fn dir_name(self) -> String {
        format!("{}s", self.as_str())
    }
}

impl std::fmt::Display for Kind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

pub struct Item {
    pub kind: Kind,
    pub path: PathBuf,
    pub title: String,
    pub created: String,
}

pub fn dir_for(paths: &Paths, kind: Kind) -> PathBuf {
    paths.shared.join(kind.dir_name())
}

pub struct NewItem<'a> {
    pub kind: Kind,
    pub text: &'a str,
    pub files: &'a [String],
    pub session: Option<&'a str>,
}

pub fn remember(paths: &Paths, item: &NewItem) -> Result<PathBuf> {
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

/// All items, newest first within each kind, kinds in `Kind::ALL` order.
pub fn list(paths: &Paths) -> Vec<Item> {
    let mut out = Vec::new();
    for kind in Kind::ALL {
        let mut items: Vec<Item> = Vec::new();
        let Ok(rd) = std::fs::read_dir(dir_for(paths, kind)) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().and_then(|s| s.to_str()) != Some("md") {
                continue;
            }
            let Ok(body) = std::fs::read_to_string(&p) else { continue };
            let created = frontmatter::get(&body, "created").unwrap_or_default();
            let title = title_of(frontmatter::strip(&body));
            if title.is_empty() {
                continue;
            }
            items.push(Item { kind, path: p, title, created });
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
    let mut n = 2;
    loop {
        let p = dir.join(format!("{stem}-{n}.md"));
        if !p.exists() {
            return p;
        }
        n += 1;
    }
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
    fn crlf_item_lists_by_its_title() {
        let root = std::env::temp_dir().join(format!("relay-ut-memory-crlf-{}", std::process::id()));
        let paths = Paths { shared: root.join(".relay"), local: root.join("local"), root: root.clone(), in_git: false };
        let dir = dir_for(&paths, Kind::ALL[0]);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("hooks.md"), "---\r\ncreated: 2026-09-22\r\n---\r\n\r\n# Hooks fail open\r\n").unwrap();
        let items = list(&paths);
        assert_eq!(items[0].title, "Hooks fail open");
        assert_eq!(items[0].created, "2026-09-22");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn title_is_first_line_without_heading() {
        assert_eq!(title_of("\n## Hooks fail open\nbecause…"), "Hooks fail open");
    }
}
