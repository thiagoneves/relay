//! Seed `.relay/project.md` for an existing repo using only rules:
//! manifest name, README lead, agent instruction files, languages,
//! hot files and recent commits. An LLM pass is optional and explicit.

use std::path::Path;

use anyhow::Result;

use crate::core::paths::Paths;
use crate::helpers::git;
use crate::helpers::{now_iso, write_atomic};

pub fn project_md(paths: &Paths) -> String {
    render(&Facts::read(&paths.root), &now_iso())
}

/// What the rules can tell about a repo without an LLM.
struct Facts {
    name: String,
    readme: String,
    langs: Vec<String>,
    agent_files: Vec<&'static str>,
    hot: Vec<(String, usize)>,
    recent: Vec<String>,
}

impl Facts {
    fn read(root: &Path) -> Self {
        let agent_files = ["CLAUDE.md", "AGENTS.md", "GEMINI.md", ".cursorrules"];
        Self {
            name: detect_name(root),
            readme: readme_lead(root),
            langs: languages(&git::tracked_files(root)),
            agent_files: agent_files.into_iter().filter(|f| root.join(f).exists()).collect(),
            hot: git::hot_files(root, 300, 8),
            recent: git::recent_commits(root, 5),
        }
    }
}

fn render(f: &Facts, generated: &str) -> String {
    let mut b =
        format!("---\ngenerated: {generated}\nby: relay init (rules only, edit freely)\n---\n\n# {}\n\n", f.name);
    if !f.readme.is_empty() {
        b.push_str(&format!("{}\n\n", f.readme));
    }
    if !f.langs.is_empty() {
        b.push_str(&format!("**Stack:** {}\n\n", f.langs.join(", ")));
    }
    if !f.agent_files.is_empty() {
        b.push_str(&format!("**Agent instructions:** {} (read them first)\n\n", f.agent_files.join(", ")));
    }
    list(&mut b, "**Most changed files:**", f.hot.iter().map(|(f, n)| format!("{f} ({n})")));
    list(&mut b, "**Recent commits:**", f.recent.iter().cloned());
    b.push_str("## Conventions\n\n_Add the rules a new contributor needs on day one. Keep it short; decisions and gotchas go in their own files._\n");
    b
}

fn list(b: &mut String, title: &str, items: impl Iterator<Item = String>) {
    let items: Vec<String> = items.collect();
    if items.is_empty() {
        return;
    }
    b.push_str(title);
    b.push('\n');
    for it in items {
        b.push_str(&format!("- {it}\n"));
    }
    b.push('\n');
}

const TERSE: &str = "\n## Answers\n\n<!-- relay terse -->\nAnswer in as few words as the point needs. Lead with the answer, then the \
reason. Don't restate what the user said or what the code already shows.\n";

/// Add the concise-answers section to project.md, once. Opt-in: it
/// changes how the agent writes, which is the user's call.
pub fn ensure_terse(paths: &Paths) -> Result<bool> {
    let pf = paths.project_file();
    let text = std::fs::read_to_string(&pf).unwrap_or_default();
    if text.contains("<!-- relay terse -->") {
        return Ok(false);
    }
    write_atomic(&pf, format!("{}\n{TERSE}", text.trim_end()).as_bytes())?;
    Ok(true)
}

/// Create `.relay/` with a project.md. Never overwrites an existing one.
pub fn ensure_shared(paths: &Paths) -> Result<bool> {
    std::fs::create_dir_all(&paths.shared)?;
    if paths.in_git && !paths.memory_local {
        ensure_lf(&paths.root)?;
    }
    let pf = paths.project_file();
    if pf.exists() {
        return Ok(false);
    }
    write_atomic(&pf, project_md(paths).as_bytes())?;
    Ok(true)
}

const LF_RULE: &str = ".relay/** text eol=lf";

/// Keep `.relay/` LF on every checkout. Git for Windows converts to CRLF
/// by default, and the same item then reads differently per machine.
fn ensure_lf(root: &Path) -> Result<()> {
    let path = root.join(".gitattributes");
    let existing = std::fs::read_to_string(&path).unwrap_or_default();
    if existing.lines().any(|l| l.trim_start().starts_with(".relay/")) {
        return Ok(());
    }
    let sep = if existing.is_empty() || existing.ends_with('\n') { "" } else { "\n" };
    write_atomic(&path, format!("{existing}{sep}{LF_RULE}\n").as_bytes())
}

fn detect_name(root: &Path) -> String {
    if let Ok(s) = std::fs::read_to_string(root.join("package.json"))
        && let Ok(v) = serde_json::from_str::<serde_json::Value>(&s)
        && let Some(n) = v["name"].as_str()
    {
        return n.to_string();
    }
    if let Ok(s) = std::fs::read_to_string(root.join("Cargo.toml")) {
        for l in s.lines() {
            if let Some(rest) = l.trim().strip_prefix("name = ") {
                return rest.trim_matches('"').to_string();
            }
        }
    }
    root.file_name().and_then(|s| s.to_str()).unwrap_or("project").to_string()
}

/// First paragraph of the README that is not a heading or badge line.
fn readme_lead(root: &Path) -> String {
    let candidates = ["README.md", "readme.md", "README", "README.rst"];
    let Some(text) = candidates.iter().find_map(|f| std::fs::read_to_string(root.join(f)).ok()) else {
        return String::new();
    };
    let mut para: Vec<&str> = Vec::new();
    for l in text.lines() {
        let t = l.trim();
        if t.is_empty() {
            if !para.is_empty() {
                break;
            }
            continue;
        }
        if t.starts_with('#') || t.starts_with("[![") || t.starts_with('<') || t.starts_with('!') {
            continue;
        }
        para.push(t);
    }
    crate::helpers::truncate_chars(&para.join(" "), 400)
}

fn languages(files: &[String]) -> Vec<String> {
    let mut counts: std::collections::HashMap<&str, usize> = std::collections::HashMap::default();
    for f in files {
        let lang = match f.rsplit('.').next().unwrap_or("") {
            "rs" => "Rust",
            "ts" | "tsx" => "TypeScript",
            "js" | "jsx" | "mjs" | "cjs" => "JavaScript",
            "py" => "Python",
            "go" => "Go",
            "rb" => "Ruby",
            "java" | "kt" => "JVM",
            "swift" => "Swift",
            "cs" => "C#",
            "php" => "PHP",
            "c" | "h" | "cpp" | "hpp" | "cc" => "C/C++",
            "ex" | "exs" => "Elixir",
            "dart" => "Dart",
            _ => continue,
        };
        *counts.entry(lang).or_default() += 1;
    }
    let mut v: Vec<(&str, usize)> = counts.into_iter().collect();
    v.sort_by_key(|x| std::cmp::Reverse(x.1));
    v.into_iter().take(4).map(|(l, _)| l.to_string()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_only_what_it_found() {
        let f = Facts {
            name: "app".into(),
            readme: String::new(),
            langs: vec!["Rust".into()],
            agent_files: vec![],
            hot: vec![("src/main.rs".into(), 3)],
            recent: vec![],
        };
        let out = render(&f, "2026-09-22T10:00:00Z");
        assert!(out.starts_with("---\ngenerated: 2026-09-22T10:00:00Z\n"), "{out}");
        assert!(
            out.contains("# app\n\n**Stack:** Rust\n\n**Most changed files:**\n- src/main.rs (3)\n\n## Conventions"),
            "{out}"
        );
    }

    #[test]
    fn lf_rule_is_added_once_after_existing_lines() {
        let root = std::env::temp_dir().join(format!("relay-ut-gitattributes-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join(".gitattributes"), "*.png binary").unwrap();
        ensure_lf(&root).unwrap();
        ensure_lf(&root).unwrap();
        let text = std::fs::read_to_string(root.join(".gitattributes")).unwrap();
        assert_eq!(text, format!("*.png binary\n{LF_RULE}\n"));
        let _ = std::fs::remove_dir_all(&root);
    }
}
