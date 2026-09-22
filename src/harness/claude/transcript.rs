//! Claude Code transcripts (`<config>/projects/<project>/<session>.jsonl`),
//! read only: where they live, and the shell calls in them (an assistant
//! `tool_use` named `Bash`, answered later by a user `tool_result`).

use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::core::audit::Transcript;
use crate::core::bench::ShellCall;
use crate::harness::jsonl;

/// Claude Code names a project's transcript dir after its path with every
/// non-alphanumeric character turned into `-`.
pub fn project_slug(root: &Path) -> String {
    crate::helpers::fs::path_slug(root)
}

/// Transcript dirs under `projects` for the project at `root`: its own,
/// and those of sessions started in a directory below it. A dir's name
/// is lossy (every separator became `-`), so membership is decided by
/// the `cwd` recorded inside one of its transcripts.
pub fn project_dirs(projects: &Path, root: &Path) -> Vec<PathBuf> {
    let own = projects.join(project_slug(root));
    let Ok(rd) = std::fs::read_dir(projects) else { return vec![own] };
    let mut dirs: Vec<PathBuf> = rd
        .flatten()
        .map(|e| e.path())
        .filter(|d| *d != own && d.is_dir())
        .filter(|d| recorded_cwd(d).is_some_and(|cwd| jsonl::in_project(&cwd, root)))
        .collect();
    dirs.insert(0, own);
    dirs
}

/// The `cwd` of the sessions in a transcript dir, from the first lines
/// of any one of them that records it.
fn recorded_cwd(dir: &Path) -> Option<PathBuf> {
    std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "jsonl"))
        .find_map(|p| cwd_in(jsonl::head_lines(&p, 40)?))
}

fn cwd_in(mut lines: impl Iterator<Item = String>) -> Option<PathBuf> {
    lines.find_map(|l| {
        if !l.contains("\"cwd\"") {
            return None;
        }
        serde_json::from_str::<Value>(&l).ok()?["cwd"].as_str().map(PathBuf::from)
    })
}

/// `<dir>/<session>.jsonl`, and `<dir>/<session>/subagents/*.jsonl` for
/// the subagents that session spawned.
pub fn sessions_in(dir: &Path) -> Vec<Transcript> {
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(dir) else { return out };
    for e in rd.flatten() {
        let p = e.path();
        let Some(stem) = p.file_stem().and_then(|s| s.to_str()).map(str::to_string) else { continue };
        if p.extension().is_some_and(|x| x == "jsonl") {
            out.push(transcript(p, stem, None));
        } else if let Ok(subs) = std::fs::read_dir(p.join("subagents")) {
            for s in subs.flatten().map(|s| s.path()).filter(|s| s.extension().is_some_and(|x| x == "jsonl")) {
                let id = s.file_stem().and_then(|x| x.to_str()).unwrap_or("").to_string();
                out.push(transcript(s, id, Some(stem.clone())));
            }
        }
    }
    out
}

fn transcript(path: PathBuf, id: String, parent: Option<String>) -> Transcript {
    let now = std::time::SystemTime::now();
    Transcript::from_file(path.clone(), id.clone(), parent.clone()).unwrap_or(Transcript {
        path,
        id,
        parent,
        started: now,
        modified: now,
    })
}

/// Tool results for `ids` from the end of a transcript, as the model got
/// them.
pub fn tool_results(path: &Path, ids: &[&str]) -> HashMap<String, String> {
    let mut found = HashMap::new();
    let Some(lines) = jsonl::tail_lines(path, crate::limits::store::TRANSCRIPT_TAIL_BYTES) else { return found };
    for line in lines.filter(|l| l.contains("\"tool_result\"")) {
        let Ok(v) = serde_json::from_str::<Value>(&line) else { continue };
        for b in v["message"]["content"].as_array().into_iter().flatten() {
            if let Some(id) = b["tool_use_id"].as_str().filter(|id| ids.contains(id)) {
                found.insert(id.to_string(), jsonl::text_of(&b["content"]));
            }
        }
    }
    found
}

pub fn shell_calls(dir: &Path) -> Vec<ShellCall> {
    let mut files = jsonl::files_under(dir);
    files.sort();
    let mut calls = Vec::new();
    for f in files {
        read_file(&f, &mut calls);
    }
    calls
}

fn read_file(path: &Path, calls: &mut Vec<ShellCall>) {
    let Ok(f) = std::fs::File::open(path) else { return };
    let mut pending: HashMap<String, String> = HashMap::new();
    for line in BufReader::new(f).lines().map_while(Result::ok) {
        // Most lines are prose; skip them before paying for a JSON parse.
        if !line.contains("\"tool_use\"") && !line.contains("\"tool_result\"") {
            continue;
        }
        let Ok(v) = serde_json::from_str::<Value>(&line) else { continue };
        let Some(blocks) = v["message"]["content"].as_array() else { continue };
        for b in blocks {
            match b["type"].as_str() {
                Some("tool_use") if b["name"] == "Bash" => {
                    if let (Some(id), Some(cmd)) = (b["id"].as_str(), b["input"]["command"].as_str()) {
                        pending.insert(id.to_string(), cmd.to_string());
                    }
                }
                Some("tool_result") => {
                    if let Some(cmd) = b["tool_use_id"].as_str().and_then(|id| pending.remove(id)) {
                        let failed = b["is_error"].as_bool() == Some(true);
                        calls.push(ShellCall { cmd, output: jsonl::text_of(&b["content"]), failed });
                    }
                }
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pairs_tool_use_with_its_result() {
        let dir = std::env::temp_dir().join(format!("relay-transcript-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("proj")).unwrap();
        let lines = [
            r#"{"type":"user","message":{"content":"hi"}}"#,
            r#"{"type":"assistant","message":{"content":[{"type":"tool_use","id":"t1","name":"Bash","input":{"command":"git status"}}]}}"#,
            r#"{"type":"assistant","message":{"content":[{"type":"tool_use","id":"t2","name":"Read","input":{"file_path":"x"}}]}}"#,
            r#"{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"t2","content":"file"}]}}"#,
            r#"{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"t1","content":[{"type":"text","text":"On branch main"}]}]}}"#,
        ];
        std::fs::write(dir.join("proj/s.jsonl"), lines.join("\n")).unwrap();
        let calls = shell_calls(&dir);
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!((calls[0].cmd.as_str(), calls[0].output.as_str()), ("git status", "On branch main"));
    }

    #[test]
    fn project_dirs_include_sessions_started_below_the_root() {
        let base = std::env::temp_dir().join(format!("relay-projdirs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let root = base.join("repo");
        let projects = base.join("projects");
        let session = |cwd: &Path| {
            let d = projects.join(project_slug(cwd));
            std::fs::create_dir_all(&d).unwrap();
            let line = format!(r#"{{"type":"user","cwd":"{}","message":{{"content":"hi"}}}}"#, cwd.display());
            std::fs::write(d.join("s.jsonl"), format!("{{\"type\":\"summary\"}}\n{line}\n")).unwrap();
            d
        };
        let own = session(&root);
        let sub = session(&root.join("packages/web"));
        // Same slug prefix, different project.
        session(&base.join("repo-old"));
        let dirs = project_dirs(&projects, &root);
        std::fs::remove_dir_all(&base).unwrap();
        assert_eq!(dirs, [own, sub]);
    }
}
