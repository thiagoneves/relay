//! `.relay/` as an Open Knowledge Format bundle (okf.md, v0.2): a root
//! `index.md` that lists every concept, and a frontmatter migration for
//! items written before relay adopted the format. Anything that reads
//! OKF reads relay's memory; relay asks nothing extra of the user.

use anyhow::Result;

use crate::core::memory::{self, Kind};
use crate::core::paths::Paths;
use crate::helpers::{frontmatter, write_atomic};

pub const VERSION: &str = "0.2";

/// Rewrite `.relay/index.md` from what the bundle holds now.
pub fn refresh_index(paths: &Paths) -> Result<()> {
    let items = memory::list(paths);
    let handoffs = shared_handoffs(paths);
    let title = frontmatter::get_text(&std::fs::read_to_string(paths.project_file()).unwrap_or_default(), "title")
        .unwrap_or_else(|| "project".to_string());
    write_atomic(&paths.shared.join("index.md"), index(&title, &items, &handoffs).as_bytes())
}

fn index(title: &str, items: &[memory::Item], handoffs: &[(String, String)]) -> String {
    let mut b = format!("---\nokf_version: \"{VERSION}\"\n---\n\n# {title} · relay memory\n\n");
    b.push_str("## Project\n\n- [project.md](project.md): what a session reads first.\n\n");
    for kind in Kind::ALL {
        let of_kind: Vec<&memory::Item> = items.iter().filter(|i| i.kind == kind).collect();
        if of_kind.is_empty() {
            continue;
        }
        b.push_str(&format!("## {}s\n\n", kind.okf_type()));
        for it in of_kind {
            let file = it.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            b.push_str(&format!("- [{}]({}/{file})\n", it.title, kind_dir(kind)));
        }
        b.push('\n');
    }
    if !handoffs.is_empty() {
        b.push_str("## Handoffs\n\n");
        for (file, title) in handoffs {
            b.push_str(&format!("- [{title}](handoffs/{file})\n"));
        }
        b.push('\n');
    }
    b
}

fn kind_dir(kind: Kind) -> String {
    format!("{}s", kind.as_str())
}

/// Shared handoffs, newest first, as (file name, title).
fn shared_handoffs(paths: &Paths) -> Vec<(String, String)> {
    let Ok(rd) = std::fs::read_dir(paths.shared.join("handoffs")) else { return Vec::new() };
    let mut out: Vec<(String, String, String)> = rd
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "md"))
        .filter_map(|p| {
            let body = std::fs::read_to_string(&p).ok()?;
            let file = p.file_name()?.to_string_lossy().into_owned();
            let ended = frontmatter::get(&body, "ended").unwrap_or_default();
            let title = frontmatter::get_text(&body, "title").unwrap_or_else(|| file.clone());
            Some((ended, file, title))
        })
        .collect();
    out.sort_by(|a, b| b.0.cmp(&a.0));
    out.into_iter().map(|(_, f, t)| (f, t)).collect()
}

/// Give items written before OKF the fields it wants, in place. Returns
/// how many files changed.
pub fn migrate(paths: &Paths) -> Result<usize> {
    let mut changed = 0;
    let pf = paths.project_file();
    if let Ok(body) = std::fs::read_to_string(&pf)
        && let Some(updated) = migrated_project(&body)
    {
        write_atomic(&pf, updated.as_bytes())?;
        changed += 1;
    }
    for kind in Kind::ALL {
        let Ok(rd) = std::fs::read_dir(paths.shared.join(kind_dir(kind))) else { continue };
        for p in rd.flatten().map(|e| e.path()) {
            let Ok(body) = std::fs::read_to_string(&p) else { continue };
            if let Some(updated) = migrated(&body, kind) {
                write_atomic(&p, updated.as_bytes())?;
                changed += 1;
            }
        }
    }
    Ok(changed)
}

/// A project.md from before OKF: its title is the first heading.
fn migrated_project(body: &str) -> Option<String> {
    if frontmatter::get(body, "type").is_some() {
        return None;
    }
    let text = frontmatter::strip(body);
    let title = text.lines().find_map(|l| l.strip_prefix("# ")).unwrap_or("project").trim();
    let at = frontmatter::get(body, "generated").unwrap_or_default();
    Some(format!(
        "---\ntype: Project\ntitle: {}\ntimestamp: {at}\ngenerated: {{by: \"process:relay init\", at: {at}}}\n---\n\n{text}",
        frontmatter::quote(title)
    ))
}

/// `None` when `body` already carries a `type`.
fn migrated(body: &str, kind: Kind) -> Option<String> {
    if frontmatter::get(body, "type").is_some() {
        return None;
    }
    let text = frontmatter::strip(body);
    let title = text.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("");
    let created = frontmatter::get(body, "created").unwrap_or_default();
    let mut head = format!(
        "---\ntype: {}\ntitle: {}\ntimestamp: {created}\ngenerated: {{by: \"process:relay\", at: {created}}}\n",
        kind.okf_type(),
        frontmatter::quote(title)
    );
    for key in ["branch", "sha", "session", "paths"] {
        if let Some(v) = frontmatter::get(body, key) {
            head.push_str(&format!("{key}: {v}\n"));
        }
    }
    if let Some(e) = frontmatter::get(body, "expires") {
        head.push_str(&format!("stale_after: {e}\n"));
    }
    Some(format!("{head}---\n\n{text}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_items_gain_type_title_and_timestamp() {
        let old = "---\nkind: gotcha\ncreated: 2026-09-22T10:00:00Z\nsha: abc\npaths: src/a.rs\nexpires: 2027-01-01T00:00:00Z\n---\n\nHooks fail open\n";
        let new = migrated(old, Kind::Gotcha).unwrap();
        assert!(
            new.starts_with("---\ntype: Gotcha\ntitle: \"Hooks fail open\"\ntimestamp: 2026-09-22T10:00:00Z\n"),
            "{new}"
        );
        assert!(
            new.contains("sha: abc\npaths: src/a.rs\nstale_after: 2027-01-01T00:00:00Z\n---\n\nHooks fail open\n"),
            "{new}"
        );
        assert_eq!(migrated(&new, Kind::Gotcha), None);
    }

    #[test]
    fn an_old_project_file_becomes_a_project_concept() {
        let old = "---\ngenerated: 2026-09-22T07:44:13Z\nby: relay init\n---\n\n# relay\n\nBody.\n";
        let new = migrated_project(old).unwrap();
        assert!(new.starts_with("---\ntype: Project\ntitle: \"relay\"\ntimestamp: 2026-09-22T07:44:13Z\n"), "{new}");
        assert!(new.ends_with("---\n\n# relay\n\nBody.\n"), "{new}");
        assert_eq!(migrated_project(&new), None);
    }

    #[test]
    fn the_index_declares_the_version_and_lists_every_concept() {
        let item = memory::Item {
            kind: Kind::Rule,
            path: "x/rules/one.md".into(),
            title: "One rule".into(),
            created: String::new(),
            sha: None,
            about: vec![],
            expires: None,
        };
        let out = index("app", &[item], &[("s1.md".into(), "Handoff · main · 2026-09-22".into())]);
        assert!(out.starts_with("---\nokf_version: \"0.2\"\n---\n\n# app · relay memory\n"), "{out}");
        assert!(out.contains("## Rules\n\n- [One rule](rules/one.md)\n"), "{out}");
        assert!(out.contains("## Handoffs\n\n- [Handoff · main · 2026-09-22](handoffs/s1.md)\n"), "{out}");
    }
}
