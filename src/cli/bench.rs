use std::path::Path;

use crate::core::bench::{self, Sample, Totals};
use crate::core::paths::Paths;
use crate::helpers::{human_tokens, truncate_chars};

const WORST_SHOWN: usize = 5;

pub fn run(corpus: Option<&Path>, json: bool, min_recall: Option<f64>) -> anyhow::Result<i32> {
    let (source, samples) = if let Some(dir) = corpus {
        (format!("corpus {}", dir.display()), bench::from_corpus(dir)?)
    } else {
        let paths = Paths::from_cwd()?;
        ("stored originals, recompressed with current filters".to_string(), bench::from_store(&paths))
    };
    let (rows, total) = bench::totals(&samples);

    if json {
        let out = serde_json::json!({ "source": source, "filters": rows, "total": total, "samples": samples });
        println!("{}", serde_json::to_string_pretty(&out)?);
    } else if samples.is_empty() {
        println!("relay bench: no stored outputs yet; run commands through `relay x` or pass --corpus");
        return Ok(0);
    } else {
        print_report(&source, &rows, &total, &samples);
    }

    let failed = min_recall.is_some_and(|min| total.expect_kept < total.expect_total || total.signal_recall() < min);
    Ok(i32::from(failed))
}

fn print_report(source: &str, rows: &[Totals], total: &Totals, samples: &[Sample]) {
    println!("relay bench · {} outputs · {source}\n", total.samples);
    println!(
        "{:<12} {:>4}  {:>17}  {:>6}  {:>16}  {:>9}",
        "filter", "n", "tokens in → out", "saved", "signal kept", "refetched"
    );
    for r in rows.iter().chain(std::iter::once(total)) {
        if r.filter == "total" {
            println!("{}", "─".repeat(72));
        }
        println!(
            "{:<12} {:>4}  {:>17}  {:>5.0}%  {:>16}  {:>9}",
            r.filter,
            r.samples,
            format!("{} → {}", human_tokens(r.tokens_in), human_tokens(r.tokens_out)),
            r.saved_pct(),
            format!("{:.0}% ({}/{})", r.signal_recall() * 100.0, r.signal_kept, r.signal_total),
            r.refetched,
        );
    }
    if total.expect_total > 0 {
        println!("\nRequired lines kept: {}/{}", total.expect_kept, total.expect_total);
    }

    let mut worst: Vec<&Sample> = samples.iter().filter(|s| !s.missing.is_empty()).collect();
    worst.sort_by_key(|s| std::cmp::Reverse(s.missing.len()));
    if !worst.is_empty() {
        println!("\nSignal lost (see the original with `relay get <id>`):");
        for s in worst.iter().take(WORST_SHOWN) {
            println!("  {} `{}` [{}] lost {}:", s.name, truncate_chars(&s.cmd, 50), s.filter, s.missing.len());
            for m in s.missing.iter().take(2) {
                println!("    - {}", truncate_chars(m, 100));
            }
        }
    }
    println!("\nTokens are estimates (bytes/4). Signal = lines with errors, failures or file:line references.");
}
