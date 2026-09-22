//! Generic, command-agnostic strategies. Always safe to apply.

use regex::Regex;
use std::sync::OnceLock;

pub const MAX_LINE_CHARS: usize = 400;
pub const MAX_LINES: usize = 300;
pub const HEAD_LINES: usize = 180;
pub const TAIL_LINES: usize = 100;
/// Signal lines rescued from the omitted middle of a capped output.
pub const MAX_RESCUED: usize = 40;

fn ansi_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\x1b\[[0-9;?]*[ -/]*[@-~]|\x1b\][^\x07]*\x07|\r").unwrap())
}

pub fn strip_ansi(s: &str) -> String {
    ansi_re().replace_all(s, "").into_owned()
}

/// Normalise whitespace: trailing spaces, tabs kept, blank runs -> one blank.
pub fn collapse_whitespace(lines: &[String]) -> Vec<String> {
    let mut out = Vec::with_capacity(lines.len());
    let mut last_blank = false;
    for l in lines {
        let t = l.trim_end();
        let blank = t.trim().is_empty();
        if blank && last_blank {
            continue;
        }
        last_blank = blank;
        out.push(t.to_string());
    }
    while out.last().is_some_and(|l| l.trim().is_empty()) {
        out.pop();
    }
    out
}

/// Consecutive identical lines become one line with a `(×N)` suffix.
pub fn dedup_runs(lines: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(lines.len());
    let mut i = 0;
    while i < lines.len() {
        let cur = &lines[i];
        let mut j = i + 1;
        while j < lines.len() && &lines[j] == cur {
            j += 1;
        }
        let n = j - i;
        if n >= 3 && !cur.trim().is_empty() {
            out.push(format!("{cur} (×{n})"));
        } else {
            for _ in 0..n {
                out.push(cur.clone());
            }
        }
        i = j;
    }
    out
}

pub fn truncate_long_lines(lines: &[String]) -> Vec<String> {
    lines
        .iter()
        .map(|l| {
            let n = l.chars().count();
            if n > MAX_LINE_CHARS {
                let head: String = l.chars().take(MAX_LINE_CHARS).collect();
                format!("{head}… [+{} chars]", n - MAX_LINE_CHARS)
            } else {
                l.clone()
            }
        })
        .collect()
}

/// Keep head and tail when output is very long. Errors usually live at
/// the tail, so the tail window is never dropped; errors in the middle
/// (a failing module in a long build log) are rescued by rule.
pub fn cap_lines(lines: &[String]) -> Vec<String> {
    if lines.len() <= MAX_LINES {
        return lines.to_vec();
    }
    let middle = &lines[HEAD_LINES..lines.len() - TAIL_LINES];
    let mut out = Vec::with_capacity(HEAD_LINES + TAIL_LINES + MAX_RESCUED * 2 + 1);
    out.extend_from_slice(&lines[..HEAD_LINES]);
    let mut skipped = 0;
    let mut rescued = 0;
    for l in middle {
        if rescued < MAX_RESCUED && super::fidelity::is_signal(l) {
            if skipped > 0 {
                out.push(omitted(skipped));
                skipped = 0;
            }
            out.push(l.clone());
            rescued += 1;
        } else {
            skipped += 1;
        }
    }
    if skipped > 0 {
        out.push(omitted(skipped));
    }
    out.extend_from_slice(&lines[lines.len() - TAIL_LINES..]);
    out
}

fn omitted(n: usize) -> String {
    format!("… [{n} lines omitted, full output in original]")
}

pub fn apply(text: &str) -> String {
    let clean = strip_ansi(text);
    let lines: Vec<String> = clean.lines().map(std::string::ToString::to_string).collect();
    let lines = collapse_whitespace(&lines);
    let lines = dedup_runs(&lines);
    let lines = truncate_long_lines(&lines);
    let lines = cap_lines(&lines);
    lines.join("\n")
}

/// `path:line:text` style output (grep, rg, eslint compact) grouped by file.
pub fn group_by_file(text: &str) -> Option<String> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"^([^:\s][^:]*?):(\d+)(?::(\d+))?:(.*)$").unwrap());
    let lines: Vec<&str> = text.lines().collect();
    let matched = lines.iter().filter(|l| re.is_match(l)).count();
    if lines.len() < 4 || matched * 10 < lines.len() * 7 {
        return None;
    }
    let mut out: Vec<String> = Vec::new();
    let mut current: Option<String> = None;
    for l in lines {
        if let Some(c) = re.captures(l) {
            let file = c.get(1).unwrap().as_str().to_string();
            if current.as_deref() != Some(&file) {
                out.push(format!("{file}:"));
                current = Some(file);
            }
            let line = c.get(2).unwrap().as_str();
            let body = c.get(4).unwrap().as_str().trim();
            out.push(format!("  {line}: {body}"));
        } else if !l.trim().is_empty() {
            current = None;
            out.push(l.to_string());
        }
    }
    Some(out.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dedups_runs_with_count() {
        let v: Vec<String> = ["a", "a", "a", "b", "b"].iter().map(std::string::ToString::to_string).collect();
        assert_eq!(dedup_runs(&v), vec!["a (×3)", "b", "b"]);
    }

    #[test]
    fn caps_long_output_keeping_tail() {
        let v: Vec<String> = (0..1000).map(|i| format!("line {i}")).collect();
        let out = cap_lines(&v);
        assert!(out.len() < 300);
        assert_eq!(out.last().unwrap(), "line 999");
        assert!(out.iter().any(|l| l.contains("lines omitted")));
    }

    #[test]
    fn cap_rescues_errors_from_the_middle() {
        let mut v: Vec<String> = (0..1000).map(|i| format!("step {i}")).collect();
        v[500] = "error: src/a.rs:3 broke".into();
        let out = cap_lines(&v);
        let i = out.iter().position(|l| l == "error: src/a.rs:3 broke").expect("rescued");
        assert!(out[i - 1].contains("lines omitted") && out[i + 1].contains("lines omitted"));
    }

    #[test]
    fn groups_grep_output() {
        let s = "src/a.rs:10:fn a() {}\nsrc/a.rs:20:fn b() {}\nsrc/b.rs:1:use x;\nsrc/b.rs:2:use y;";
        let g = group_by_file(s).unwrap();
        assert!(g.starts_with("src/a.rs:\n  10: fn a() {}"));
        assert!(g.contains("src/b.rs:\n  1: use x;"));
    }

    #[test]
    fn strips_ansi() {
        assert_eq!(strip_ansi("\x1b[31mred\x1b[0m"), "red");
    }
}
