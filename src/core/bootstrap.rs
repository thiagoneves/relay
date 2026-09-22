//! Seed `.relay/project.md` for an existing repo using only rules:
//! manifest name, README lead, agent instruction files, languages,
//! hot files and recent commits. An LLM pass is optional and explicit.

use std::path::Path;

use anyhow::Result;

use crate::core::paths::Paths;
use crate::helpers::git;
use crate::helpers::{now_iso, write_atomic};

pub fn project_md(paths: &Paths) -> String {
    let root = &paths.root;
    let name = detect_name(root);
    let readme = readme_lead(root);
    let files = git::tracked_files(root);
    let langs = languages(&files);
    let hot = git::hot_files(root, 300, 8);
    let recent = git::recent_commits(root, 5);
    let agent_files: Vec<&str> =
        ["CLAUDE.md", "AGENTS.md", "GEMINI.md", ".cursorrules"].into_iter().filter(|f| root.join(f).exists()).collect();

    let mut b = String::new();
    b.push_str("---\n");
    b.push_str(&format!("generated: {}\nby: relay init (rules only, edit freely)\n", now_iso()));
    b.push_str("---\n\n");
    b.push_str(&format!("# {name}\n\n"));
    if !readme.is_empty() {
        b.push_str(&format!("{readme}\n\n"));
    }
    if !langs.is_empty() {
        b.push_str(&format!("**Stack:** {}\n\n", langs.join(", ")));
    }
    if !agent_files.is_empty() {
        b.push_str(&format!("**Agent instructions:** {} (read them first)\n\n", agent_files.join(", ")));
    }
    if !hot.is_empty() {
        b.push_str("**Most changed files:**\n");
        for (f, n) in &hot {
            b.push_str(&format!("- {f} ({n})\n"));
        }
        b.push('\n');
    }
    if !recent.is_empty() {
        b.push_str("**Recent commits:**\n");
        for c in &recent {
            b.push_str(&format!("- {c}\n"));
        }
        b.push('\n');
    }
    b.push_str("## Conventions\n\n_Add the rules a new contributor needs on day one. Keep it short; decisions and gotchas go in their own files._\n");
    b
}

/// Create `.relay/` with a project.md. Never overwrites an existing one.
pub fn ensure_shared(paths: &Paths) -> Result<bool> {
    std::fs::create_dir_all(&paths.shared)?;
    if paths.in_git {
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
    v.sort_by(|a, b| b.1.cmp(&a.1));
    v.into_iter().take(4).map(|(l, _)| l.to_string()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

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
