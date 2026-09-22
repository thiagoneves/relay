//! Generic, command-agnostic strategies. `apply` is for tool output;
//! `apply_read` is for commands that print files the agent asked to
//! see, where every line must stay as it is in the file.

use regex::Regex;
use std::sync::OnceLock;

pub const MAX_LINE_CHARS: usize = 400;
/// Tool output longer than this is cut to head + tail + signal lines; the
/// original stays one `relay get` away. Sized on real agent history: 80 +
/// 40 saves ~9% of all shell output tokens, the old 180 + 100 about 2%.
pub const MAX_LINES: usize = 150;
pub const HEAD_LINES: usize = 80;
pub const TAIL_LINES: usize = 40;
/// Signal lines rescued from the omitted middle of a capped output.
pub const MAX_RESCUED: usize = 40;

fn ansi_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\x1b\[[0-9;?]*[ -/]*[@-~]|\x1b\][^\x07]*\x07").unwrap())
}

/// Remove colour codes and replay carriage returns: a progress bar that
/// redrew itself fifty times shows only its final state.
pub fn strip_ansi(s: &str) -> String {
    let s = ansi_re().replace_all(s, "");
    if !s.contains('\r') {
        return s.into_owned();
    }
    s.split('\n').map(|l| l.trim_end_matches('\r').rsplit('\r').next().unwrap_or("")).collect::<Vec<_>>().join("\n")
}

/// `cat -n` style gutters pad numbers to a fixed width; the padding costs
/// tokens and carries nothing. Applied only when most lines have a gutter,
/// so indented content that merely starts with a number is left alone.
pub fn trim_number_gutter(lines: &[String]) -> Vec<String> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"^ +(\d+)(\t| ?│)").unwrap());
    let filled: Vec<&String> = lines.iter().filter(|l| !l.trim().is_empty()).collect();
    let with_gutter = filled.iter().filter(|l| re.is_match(l)).count();
    if filled.len() < 5 || with_gutter * 10 < filled.len() * 8 {
        return lines.to_vec();
    }
    lines.iter().map(|l| re.replace(l, "$1$2").into_owned()).collect()
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

pub fn apply_read(text: &str) -> String {
    let lines: Vec<String> = strip_ansi(text).lines().map(str::to_string).collect();
    trim_number_gutter(&lines).join("\n")
}

pub fn apply(text: &str) -> String {
    let clean = strip_ansi(text);
    let lines: Vec<String> = clean.lines().map(std::string::ToString::to_string).collect();
    let lines = collapse_whitespace(&lines);
    let lines = trim_number_gutter(&lines);
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

    #[test]
    fn carriage_returns_keep_the_final_redraw() {
        assert_eq!(strip_ansi("10%\r50%\r100%\nok\r\n"), "100%\nok\n");
    }

    #[test]
    fn trims_gutters_only_when_most_lines_have_one() {
        let cat_n: Vec<String> = (1..=6).map(|i| format!("     {i}\tline {i}")).collect();
        assert_eq!(trim_number_gutter(&cat_n)[0], "1\tline 1");
        let yaml: Vec<String> =
            ["responses:", "  200:", "    ok", "  404:", "    missing", "x"].iter().map(|s| (*s).to_string()).collect();
        assert_eq!(trim_number_gutter(&yaml), yaml);
    }

    #[test]
    fn reads_keep_every_line_of_the_file() {
        let file = "a\n\n\n}\n}\n}\n  trailing  \n";
        assert_eq!(apply_read(file), "a\n\n\n}\n}\n}\n  trailing  ");
    }
}
