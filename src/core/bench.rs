//! Compression benchmark. Replays originals through the current filters
//! and reports, per filter, what was saved and what signal survived.
//!
//! Sources: the local output store (this worktree's real history), a
//! corpus directory of `<name>.cmd` + `<name>.out` fixtures with an
//! optional `<name>.keep` listing lines that must survive verbatim, and
//! shell calls read from a harness's own transcripts.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context, Result, bail};
use serde::Serialize;

use crate::compress::{self, fidelity, fidelity::Fidelity};
use crate::core::outputs;
use crate::core::paths::Paths;
use crate::helpers::est_tokens;

/// One shell command an agent ran and the output it saw.
pub struct ShellCall {
    pub cmd: String,
    pub output: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Sample {
    pub name: String,
    pub cmd: String,
    pub family: String,
    pub filter: String,
    /// Whether the hook policy routes this command through `relay x`.
    /// Unwrapped calls reach the agent uncompressed.
    pub wrapped: bool,
    pub tokens_in: usize,
    pub tokens_out: usize,
    pub signal_kept: usize,
    pub signal_total: usize,
    pub expect_kept: usize,
    pub expect_total: usize,
    pub missing: Vec<String>,
    pub refetched: bool,
}

#[derive(Debug, Default, Clone, Serialize)]
pub struct Totals {
    pub key: String,
    pub samples: usize,
    pub wrapped: usize,
    pub tokens_in: usize,
    /// As if every call went through `relay x`.
    pub tokens_out: usize,
    /// With the current hook policy: unwrapped calls cost `tokens_in`.
    pub tokens_out_effective: usize,
    pub signal_kept: usize,
    pub signal_total: usize,
    pub expect_kept: usize,
    pub expect_total: usize,
    pub refetched: usize,
}

impl Totals {
    pub fn saved_pct(&self) -> f64 {
        pct(self.tokens_in.saturating_sub(self.tokens_out), self.tokens_in)
    }

    pub fn effective_saved_pct(&self) -> f64 {
        pct(self.tokens_in.saturating_sub(self.tokens_out_effective), self.tokens_in)
    }

    pub fn signal_recall(&self) -> f64 {
        Fidelity { kept: self.signal_kept, total: self.signal_total, missing: Vec::new() }.recall()
    }

    fn add(&mut self, s: &Sample) {
        self.samples += 1;
        self.wrapped += usize::from(s.wrapped);
        self.tokens_in += s.tokens_in;
        self.tokens_out += s.tokens_out;
        self.tokens_out_effective += if s.wrapped { s.tokens_out } else { s.tokens_in };
        self.signal_kept += s.signal_kept;
        self.signal_total += s.signal_total;
        self.expect_kept += s.expect_kept;
        self.expect_total += s.expect_total;
        self.refetched += usize::from(s.refetched);
    }
}

fn pct(part: usize, whole: usize) -> f64 {
    if whole == 0 { 0.0 } else { part as f64 * 100.0 / whole as f64 }
}

fn sample(name: String, cmd: &str, raw: &str, expect: &[String], refetched: bool, wrapped: bool) -> Sample {
    let c = compress::compress(cmd, raw);
    let signal = fidelity::measure(raw, &c.text);
    let expected = fidelity::check(expect, &c.text);
    let mut missing = expected.missing;
    for m in signal.missing {
        if !missing.contains(&m) {
            missing.push(m);
        }
    }
    Sample {
        name,
        cmd: cmd.to_string(),
        family: compress::family(cmd),
        filter: c.filter.to_string(),
        wrapped,
        tokens_in: est_tokens(raw),
        tokens_out: est_tokens(&c.text) + if c.shortened { compress::FOOTER_TOKENS } else { 0 },
        signal_kept: signal.kept,
        signal_total: signal.total,
        expect_kept: expected.kept,
        expect_total: expected.total,
        missing,
        refetched,
    }
}

pub fn from_store(paths: &Paths) -> Vec<Sample> {
    let fetched = outputs::fetched_ids(paths);
    outputs::list(paths)
        .into_iter()
        .filter_map(|m| {
            let (_, raw) = outputs::get(paths, &m.id).ok()?;
            Some(sample(m.id.clone(), &m.cmd, &raw, &[], fetched.contains(&m.id), true))
        })
        .collect()
}

pub fn from_corpus(dir: &Path) -> Result<Vec<Sample>> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .with_context(|| format!("cannot read corpus {}", dir.display()))?
        .flatten()
        .filter_map(|e| {
            let p = e.path();
            (p.extension()? == "cmd").then(|| p.file_stem()?.to_str().map(str::to_string)).flatten()
        })
        .collect();
    if names.is_empty() {
        bail!("no <name>.cmd fixtures in {}", dir.display());
    }
    names.sort();
    names
        .into_iter()
        .map(|name| {
            let read = |ext: &str| std::fs::read_to_string(dir.join(format!("{name}.{ext}")));
            let cmd = read("cmd")?.trim().to_string();
            let raw = read("out").with_context(|| format!("{name}.out missing"))?;
            let expect: Vec<String> = read("keep")
                .unwrap_or_default()
                .lines()
                .map(str::trim)
                .filter(|l| !l.is_empty() && !l.starts_with('#'))
                .map(str::to_string)
                .collect();
            Ok(sample(name, &cmd, &raw, &expect, false, true))
        })
        .collect()
}

pub fn from_history(calls: &[ShellCall], wraps: impl Fn(&str) -> bool) -> Vec<Sample> {
    calls
        .iter()
        .enumerate()
        .map(|(i, c)| sample(format!("h{i}"), &c.cmd, &c.output, &[], false, wraps(&c.cmd)))
        .collect()
}

/// Totals grouped by `key` (filter or family), largest input first, plus
/// the overall total.
pub fn totals(samples: &[Sample], key: impl Fn(&Sample) -> &str) -> (Vec<Totals>, Totals) {
    let mut by: BTreeMap<&str, Totals> = BTreeMap::new();
    let mut all = Totals { key: "total".into(), ..Totals::default() };
    for s in samples {
        let k = key(s);
        by.entry(k).or_insert_with(|| Totals { key: k.to_string(), ..Totals::default() }).add(s);
        all.add(s);
    }
    let mut rows: Vec<Totals> = by.into_values().collect();
    rows.sort_by_key(|t| std::cmp::Reverse(t.tokens_in));
    (rows, all)
}
