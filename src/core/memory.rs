//! Durable project memory in the committed tier: one item per file
//! under `.relay/{rules,decisions,gotchas}/`. Written by `relay remember`
//! (the agent, mid-session). Review happens in `git diff`; there is no
//! pending queue.

use std::path::PathBuf;

use anyhow::Result;
use clap::ValueEnum;

use crate::core::paths::Paths;
use crate::helpers::frontmatter;
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
    /// The commit the item was saved on.
    pub sha: Option<String>,
    /// Repo paths the item is about, as given to `--path`.
    pub about: Vec<String>,
}

impl Item {
    /// The paths this item is about that appear in `changed`: files that
    /// moved on since the item was saved, so it may no longer hold. A
    /// directory counts when anything under it changed.
    pub fn changed<'a>(&'a self, changed: &[String]) -> Vec<&'a str> {
        self.about
            .iter()
            .map(|p| p.trim_start_matches("./").trim_end_matches('/'))
            .filter(|p| !p.is_empty())
            .filter(|p| changed.iter().any(|c| c == p || c.starts_with(&format!("{p}/"))))
            .collect()
    }
}

/// For each item, the paths it is about that changed since it was saved.
/// One git call per distinct commit, newest items first, up to
/// `max_commits`; items past that, or without paths, report nothing.
pub fn staleness(root: &std::path::Path, items: &[Item], max_commits: usize) -> Vec<Vec<String>> {
    let mut by_sha: std::collections::HashMap<&str, Option<Vec<String>>> = std::collections::HashMap::new();
    items
        .iter()
        .map(|it| {
            let Some(sha) = it.sha.as_deref().filter(|_| !it.about.is_empty()) else { return Vec::new() };
            if !by_sha.contains_key(sha) && by_sha.len() >= max_commits {
                return Vec::new();
            }
            let changed = by_sha.entry(sha).or_insert_with(|| crate::helpers::git::changed_since(root, sha));
            changed.as_deref().map(|c| it.changed(c).into_iter().map(str::to_string).collect()).unwrap_or_default()
        })
        .collect()
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

/// Why an item was not saved, for the CLI to explain.
#[derive(Debug)]
pub enum NotSaved {
    Empty,
    AlreadyRemembered(String),
}

impl std::fmt::Display for NotSaved {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty => f.write_str("nothing to remember: text is empty"),
            Self::AlreadyRemembered(at) => write!(f, "already remembered in {at}"),
        }
    }
}

impl std::error::Error for NotSaved {}

pub fn remember(paths: &Paths, item: &NewItem) -> Result<PathBuf> {
    let text = item.text.trim();
    if text.is_empty() {
        return Err(NotSaved::Empty.into());
    }
    let title = title_of(text);
    if let Some(dup) = list(paths).into_iter().find(|i| i.kind == item.kind && i.title == title) {
        return Err(NotSaved::AlreadyRemembered(paths.rel(&dup.path)).into());
    }
    let path = unique_path(&dir_for(paths, item.kind), &slug(&title));
    write_atomic(&path, render(item, &gitstate::state(&paths.root), &now_iso()).as_bytes())?;
    Ok(path)
}

/// The item file: where and when it was learned, then the text.
fn render(item: &NewItem, git: &gitstate::GitState, created: &str) -> String {
    let mut b = format!("---\nkind: {}\ncreated: {created}\n", item.kind);
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
    format!("{b}---\n\n{}\n", item.text.trim())
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
            let sha = frontmatter::get(&body, "sha").filter(|s| !s.is_empty());
            let about = frontmatter::get(&body, "paths")
                .map(|v| v.split(',').map(|p| p.trim().to_string()).filter(|p| !p.is_empty()).collect())
                .unwrap_or_default();
            items.push(Item { kind, path: p, title, created, sha, about });
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
    fn item_records_where_it_was_learned() {
        let git = gitstate::GitState { branch: "main".into(), sha: "abc1234".into(), dirty: vec![] };
        let item = NewItem { kind: Kind::Gotcha, text: " Hooks fail open \n", files: &[], session: Some("s1") };
        assert_eq!(
            render(&item, &git, "2026-09-22T10:00:00Z"),
            "---\nkind: gotcha\ncreated: 2026-09-22T10:00:00Z\nbranch: main\nsha: abc1234\nsession: s1\n---\n\nHooks fail open\n"
        );
    }

    #[test]
    fn title_is_first_line_without_heading() {
        assert_eq!(title_of("\n## Hooks fail open\nbecause…"), "Hooks fail open");
    }

    #[test]
    fn an_item_is_stale_when_a_path_it_is_about_changed() {
        let item = Item {
            kind: Kind::Gotcha,
            path: PathBuf::from("x.md"),
            title: "t".into(),
            created: String::new(),
            sha: Some("abc".into()),
            about: vec!["./src/pay.rs".into(), "src/compress/".into(), "docs".into()],
        };
        let changed = ["src/pay.rs".to_string(), "src/compress/git.rs".to_string(), "docsite/a.md".to_string()];
        assert_eq!(item.changed(&changed), ["src/pay.rs", "src/compress"]);
        assert!(item.changed(&["README.md".to_string()]).is_empty());
    }
}
