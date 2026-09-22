//! Regression gate for compression quality on the checked-in corpus
//! (regenerate with `scripts/corpus.sh`). Savings may only go up; no
//! required line may ever be lost.

mod common;

use serde_json::Value;

const MIN_SAVED_PCT: f64 = 60.0;

#[test]
fn corpus_keeps_every_required_line_and_saves_tokens() {
    let corpus = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/corpus");
    let out = common::relay().args(["bench", "--json", "--min-recall", "1.0", "--corpus", corpus]).output().unwrap();
    let report: Value = serde_json::from_slice(&out.stdout).expect("bench --json");
    let total = &report["total"];

    let lost: Vec<String> = report["samples"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|s| !s["missing"].as_array().unwrap().is_empty())
        .map(|s| format!("{}: {}", s["name"], s["missing"]))
        .collect();
    assert!(lost.is_empty(), "signal lost:\n{}", lost.join("\n"));
    assert_eq!(out.status.code(), Some(0));

    for s in report["samples"].as_array().unwrap() {
        assert!(s["tokens_out"].as_u64() <= s["tokens_in"].as_u64(), "{} grew", s["name"]);
    }
    let tin = total["tokens_in"].as_f64().unwrap();
    let saved = (tin - total["tokens_out"].as_f64().unwrap()) * 100.0 / tin;
    assert!(saved >= MIN_SAVED_PCT, "saved {saved:.1}% < {MIN_SAVED_PCT}%");
}
