use std::path::PathBuf;

use crate::core::bench::{self, Sample, Totals};
use crate::core::paths::Paths;
use crate::harness;
use crate::helpers::{human_tokens, truncate_chars};

const WORST_SHOWN: usize = 5;
const FAMILIES_SHOWN: usize = 20;

pub enum Source {
    Store,
    Corpus(PathBuf),
    /// Harness name and an optional transcript directory.
    History(String, Option<PathBuf>),
}

pub fn run(source: Source, json: bool, min_recall: Option<f64>) -> anyhow::Result<i32> {
    let (label, samples) = match source {
        Source::Corpus(dir) => (format!("corpus {}", dir.display()), bench::from_corpus(&dir)?),
        Source::History(name, dir) => {
            let h = harness::by_name(&name)?;
            let calls = h.shell_history(dir.as_deref())?;
            let label = format!("{} transcripts, current filters and hook policy", h.id());
            (label, bench::from_history(&calls, harness::protocol::hook::should_wrap))
        }
        Source::Store => {
            let paths = Paths::from_cwd()?;
            ("stored originals, recompressed with current filters".to_string(), bench::from_store(&paths))
        }
    };
    let (filters, total) = bench::totals(&samples, |s| &s.filter);
    let (families, _) = bench::totals(&samples, |s| &s.family);

    if json {
        let out = serde_json::json!({
            "source": label, "filters": filters, "families": families, "total": total, "samples": samples,
        });
        println!("{}", serde_json::to_string_pretty(&out)?);
    } else if samples.is_empty() {
        println!("relay bench: nothing to measure; run commands through `relay x`, or pass --corpus or --history");
        return Ok(0);
    } else {
        println!("relay bench · {} outputs · {label}\n", total.samples);
        print_filters(&filters, &total);
        if total.wrapped < total.samples {
            print_families(&families, &total);
        }
        print_losses(&samples, &total);
        println!("\nTokens are estimates (bytes/4). Signal = lines with errors, failures or file:line references.");
    }

    let failed = min_recall.is_some_and(|min| total.expect_kept < total.expect_total || total.signal_recall() < min);
    Ok(i32::from(failed))
}

fn in_out(t: &Totals, out: usize) -> String {
    format!("{} → {}", human_tokens(t.tokens_in), human_tokens(out))
}

fn recall(t: &Totals) -> String {
    format!("{:.0}% ({}/{})", t.signal_recall() * 100.0, t.signal_kept, t.signal_total)
}

fn print_filters(rows: &[Totals], total: &Totals) {
    println!("By filter, as if every call went through `relay x`:");
    println!(
        "{:<12} {:>6}  {:>17}  {:>6}  {:>18}  {:>9}",
        "filter", "n", "tokens in → out", "saved", "signal kept", "refetched"
    );
    for r in rows.iter().chain(std::iter::once(total)) {
        if r.key == "total" {
            println!("{}", "─".repeat(76));
        }
        println!(
            "{:<12} {:>6}  {:>17}  {:>5.0}%  {:>18}  {:>9}",
            r.key,
            r.samples,
            in_out(r, r.tokens_out),
            r.saved_pct(),
            recall(r),
            r.refetched
        );
    }
}

fn print_families(rows: &[Totals], total: &Totals) {
    println!("\nBy command, largest first. `now` = with the current hook policy (unwrapped calls pass untouched):");
    println!(
        "{:<16} {:>6} {:>8}  {:>9}  {:>9}  {:>9}  {:>18}",
        "command", "n", "wrapped", "tokens in", "saved all", "saved now", "signal kept"
    );
    for r in rows.iter().take(FAMILIES_SHOWN).chain(std::iter::once(total)) {
        if r.key == "total" {
            println!("{}", "─".repeat(86));
        }
        println!(
            "{:<16} {:>6} {:>7.0}%  {:>9}  {:>8.0}%  {:>8.0}%  {:>18}",
            truncate_chars(&r.key, 16),
            r.samples,
            r.wrapped as f64 * 100.0 / r.samples.max(1) as f64,
            human_tokens(r.tokens_in),
            r.saved_pct(),
            r.effective_saved_pct(),
            recall(r)
        );
    }
}

fn print_losses(samples: &[Sample], total: &Totals) {
    if total.expect_total > 0 {
        println!("\nRequired lines kept: {}/{}", total.expect_kept, total.expect_total);
    }
    let mut worst: Vec<&Sample> = samples.iter().filter(|s| !s.missing.is_empty()).collect();
    worst.sort_by_key(|s| std::cmp::Reverse(s.missing.len()));
    if worst.is_empty() {
        return;
    }
    println!("\nSignal lost ({} outputs), worst first:", worst.len());
    for s in worst.iter().take(WORST_SHOWN) {
        println!("  {} `{}` [{}] lost {}:", s.name, truncate_chars(&s.cmd, 60), s.filter, s.missing.len());
        for m in s.missing.iter().take(2) {
            println!("    - {}", truncate_chars(m, 100));
        }
    }
}
