//! Reversible output store. Every compressed command output keeps its
//! original under `<local>/outputs/<id>.out` with a `<id>.json` sidecar.
//! This is the raw material for handoffs and for `relay get`.
//!
//! Spill: some harnesses run tool commands in a sandbox where `.git/`
//! is read-only (Codex). When the local tier cannot be written, the
//! original goes to a per-project dir under the system temp dir and is
//! absorbed into the local tier by the next hook, which runs outside
//! the sandbox. The spill is a transient buffer, not a third tier.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::core::paths::Paths;
use crate::helpers::write_atomic;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputMeta {
    pub id: String,
    pub ts: String,
    #[serde(default)]
    pub session: Option<String>,
    pub cwd: String,
    pub cmd: String,
    pub exit: i32,
    pub filter: String,
    pub bytes_in: usize,
    pub bytes_out: usize,
    pub tokens_in: usize,
    pub tokens_out: usize,
}

impl OutputMeta {
    pub fn saved(&self) -> usize {
        self.tokens_in.saturating_sub(self.tokens_out)
    }
}

/// Where originals spill when the local tier is not writable.
pub fn spill_dir(paths: &Paths) -> PathBuf {
    let slug: String =
        paths.root.display().to_string().chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
    std::env::temp_dir().join("relay").join(slug).join("outputs")
}

/// The original lands before its sidecar, each by rename: a reader that
/// sees a `.json` can rely on the complete `.out` beside it.
fn write_pair(dir: &Path, meta: &OutputMeta, raw: &str) -> Result<()> {
    write_atomic(&dir.join(format!("{}.out", meta.id)), raw.as_bytes())?;
    write_atomic(&dir.join(format!("{}.json", meta.id)), &serde_json::to_vec_pretty(meta)?)?;
    Ok(())
}

#[derive(Debug, PartialEq, Eq)]
pub enum Stored {
    Local,
    Spilled,
}

pub fn store(paths: &Paths, meta: &OutputMeta, raw: &str) -> Result<Stored> {
    match write_pair(&paths.outputs(), meta, raw) {
        Ok(()) => Ok(Stored::Local),
        Err(local_err) => match write_pair(&spill_dir(paths), meta, raw) {
            Ok(()) => Ok(Stored::Spilled),
            Err(spill_err) => bail!("local store: {local_err}; spill: {spill_err}"),
        },
    }
}

fn find_meta(paths: &Paths, id: &str) -> Option<PathBuf> {
    [paths.outputs(), spill_dir(paths)].into_iter().map(|d| d.join(format!("{id}.json"))).find(|p| p.exists())
}

pub fn get(paths: &Paths, id: &str) -> Result<(OutputMeta, String)> {
    let id = id.trim();
    if id.is_empty() || id.contains(['/', '\\']) || id.contains("..") {
        bail!("invalid output id");
    }
    let meta_path = find_meta(paths, id).with_context(|| format!("no output with id {id}"))?;
    let meta: OutputMeta = serde_json::from_slice(&fs::read(&meta_path)?)?;
    let raw = fs::read_to_string(meta_path.with_extension("out"))?;
    Ok((meta, raw))
}

/// Log that an original was read back. A high refetch rate for a filter
/// means its compressed view dropped something the agent needed.
pub fn record_fetch(paths: &Paths, id: &str) -> Result<()> {
    use std::io::Write;
    let line = serde_json::json!({ "ts": crate::helpers::now_iso(), "id": id });
    let mut f = fs::OpenOptions::new().create(true).append(true).open(paths.local.join("fetches.jsonl"))?;
    writeln!(f, "{line}")?;
    Ok(())
}

pub fn fetched_ids(paths: &Paths) -> std::collections::HashSet<String> {
    let Ok(text) = fs::read_to_string(paths.local.join("fetches.jsonl")) else {
        return std::collections::HashSet::default();
    };
    text.lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter_map(|v| v["id"].as_str().map(str::to_string))
        .collect()
}

/// Move spilled originals into the local tier. Called from hooks and
/// the wrapper, which run outside any sandbox. Only complete pairs move
/// (a `.json` is written last), original first, so the local tier never
/// holds a sidecar without its original. Returns pairs moved.
pub fn absorb_spill(paths: &Paths) -> usize {
    let spill = spill_dir(paths);
    let Ok(rd) = fs::read_dir(&spill) else { return 0 };
    if fs::create_dir_all(paths.outputs()).is_err() {
        return 0;
    }
    let mut moved = 0;
    for e in rd.flatten() {
        let json = e.path();
        if json.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        let out = json.with_extension("out");
        if !out.exists() {
            continue;
        }
        if move_file(&out, &paths.outputs()) && move_file(&json, &paths.outputs()) {
            moved += 1;
        }
    }
    moved
}

/// Rename, or copy under a temp name and rename when the spill dir is on
/// another filesystem, so a partial copy is never visible.
fn move_file(from: &Path, dir: &Path) -> bool {
    let Some(name) = from.file_name() else { return false };
    let to = dir.join(name);
    if fs::rename(from, &to).is_ok() {
        return true;
    }
    fs::read(from).is_ok_and(|b| write_atomic(&to, &b).is_ok()) && fs::remove_file(from).is_ok()
}

fn read_metas(dir: &Path, into: &mut Vec<OutputMeta>) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        if let Ok(b) = fs::read(&p)
            && let Ok(m) = serde_json::from_slice::<OutputMeta>(&b)
            && !into.iter().any(|x| x.id == m.id)
        {
            into.push(m);
        }
    }
}

/// All stored metas (local + spill), oldest first. Cheap enough for
/// status and handoff while the store is per worktree.
pub fn list(paths: &Paths) -> Vec<OutputMeta> {
    let mut v: Vec<OutputMeta> = Vec::new();
    read_metas(&paths.outputs(), &mut v);
    read_metas(&spill_dir(paths), &mut v);
    v.sort_by(|a, b| a.ts.cmp(&b.ts).then(a.id.cmp(&b.id)));
    v
}

pub fn for_session(paths: &Paths, session: &str) -> Vec<OutputMeta> {
    list(paths).into_iter().filter(|m| m.session.as_deref() == Some(session)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(name: &str) -> Paths {
        let root = std::env::temp_dir().join(format!("relay-ut-outputs-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        Paths { shared: root.join(".relay"), local: root.join("local"), root, in_git: false }
    }

    fn meta(id: &str) -> OutputMeta {
        OutputMeta {
            id: id.into(),
            ts: "2026-09-22T10:00:00Z".into(),
            session: Some("s".into()),
            cwd: String::new(),
            cmd: "cargo test".into(),
            exit: 0,
            filter: "generic".into(),
            bytes_in: 3,
            bytes_out: 3,
            tokens_in: 1,
            tokens_out: 1,
        }
    }

    #[test]
    fn absorb_leaves_a_pair_whose_sidecar_is_not_written_yet() {
        let p = paths("absorb");
        let spill = spill_dir(&p);
        let _ = fs::remove_dir_all(&spill);
        fs::create_dir_all(&spill).unwrap();
        // Mid-write: original present, sidecar not yet renamed in.
        fs::write(spill.join("o_half.out"), "partial").unwrap();
        write_pair(&spill, &meta("o_full"), "whole").unwrap();

        assert_eq!(absorb_spill(&p), 1);
        assert!(p.outputs().join("o_full.out").exists() && p.outputs().join("o_full.json").exists());
        assert!(!p.outputs().join("o_half.out").exists());
        assert!(spill.join("o_half.out").exists());
        assert_eq!(get(&p, "o_full").unwrap().1, "whole");
        let _ = fs::remove_dir_all(&spill);
        let _ = fs::remove_dir_all(&p.root);
    }
}
