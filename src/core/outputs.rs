//! Reversible output store. Every compressed command output keeps its
//! original under `<local>/outputs/<id>.out` with a `<id>.json` sidecar.
//! This is the raw material for handoffs and for `relay get`.

use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::core::paths::Paths;

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

fn meta_path(paths: &Paths, id: &str) -> PathBuf {
    paths.outputs().join(format!("{id}.json"))
}

fn raw_path(paths: &Paths, id: &str) -> PathBuf {
    paths.outputs().join(format!("{id}.out"))
}

pub fn store(paths: &Paths, meta: &OutputMeta, raw: &str) -> Result<()> {
    fs::create_dir_all(paths.outputs())?;
    fs::write(raw_path(paths, &meta.id), raw)?;
    fs::write(meta_path(paths, &meta.id), serde_json::to_vec_pretty(meta)?)?;
    Ok(())
}

pub fn get(paths: &Paths, id: &str) -> Result<(OutputMeta, String)> {
    let id = id.trim();
    if id.is_empty() || id.contains('/') || id.contains("..") {
        bail!("invalid output id");
    }
    let meta: OutputMeta = serde_json::from_slice(
        &fs::read(meta_path(paths, id)).with_context(|| format!("no output with id {id}"))?,
    )?;
    let raw = fs::read_to_string(raw_path(paths, id))?;
    Ok((meta, raw))
}

/// All stored metas, oldest first. Cheap enough for status and handoff
/// while the store is per worktree and pruned.
pub fn list(paths: &Paths) -> Vec<OutputMeta> {
    let mut v: Vec<OutputMeta> = Vec::new();
    if let Ok(rd) = fs::read_dir(paths.outputs()) {
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().and_then(|s| s.to_str()) != Some("json") {
                continue;
            }
            if let Ok(b) = fs::read(&p)
                && let Ok(m) = serde_json::from_slice::<OutputMeta>(&b)
            {
                v.push(m);
            }
        }
    }
    v.sort_by(|a, b| a.ts.cmp(&b.ts).then(a.id.cmp(&b.id)));
    v
}

pub fn for_session(paths: &Paths, session: &str) -> Vec<OutputMeta> {
    list(paths)
        .into_iter()
        .filter(|m| m.session.as_deref() == Some(session))
        .collect()
}
