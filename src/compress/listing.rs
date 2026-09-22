//! Directory listings.

use std::sync::LazyLock;

use regex::Regex;

use super::Filter;
use super::command::Simple;

/// `ls` with a short flag cluster that includes `l`.
pub fn filter_for(s: &Simple) -> Option<Filter> {
    let long = s.toks.iter().any(|t| t.starts_with('-') && !t.starts_with("--") && t.contains('l'));
    (s.program() == "ls" && long).then_some(Filter::LsLong)
}

static LONG: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^([-dlcbps][rwxsStT-]{9}[@+.]?)\s+(\d+)\s+(\S+)\s+(\S+)\s+(.*)$").expect("valid regex")
});

/// `ls -l`: when every entry has the same owner and group, say it once
/// instead of on every line. Nothing else changes.
pub fn ls_long(text: &str) -> String {
    let rows: Vec<(&str, &str)> = text
        .lines()
        .filter_map(|l| LONG.captures(l))
        .map(|c| (c.get(3).unwrap().as_str(), c.get(4).unwrap().as_str()))
        .collect();
    let Some(&(owner, group)) = rows.first() else { return text.to_string() };
    if rows.len() < 3 || rows.iter().any(|r| *r != (owner, group)) {
        return text.to_string();
    }
    let mut out = vec![format!("(all entries: owner {owner}, group {group})")];
    out.extend(text.lines().map(|l| LONG.replace(l, "$1 $2 $5").into_owned()));
    out.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn states_a_shared_owner_once() {
        let ls = "total 16\ndrwxr-xr-x@  4 ana  staff  128 10 set 18:56 .\n-rw-r--r--   1 ana  staff  90 10 set 18:55 a.rs\n-rw-r--r--   1 ana  staff  12 10 set 18:55 b.rs";
        let out = ls_long(ls);
        assert!(out.starts_with("(all entries: owner ana, group staff)\ntotal 16\n"));
        assert!(out.contains("\n-rw-r--r-- 1 90 10 set 18:55 a.rs"));
        assert!(!out.contains("staff  90"));
    }

    #[test]
    fn keeps_mixed_owners() {
        let ls = "-rw-r--r-- 1 ana staff 1 x a\n-rw-r--r-- 1 root wheel 1 x b\n-rw-r--r-- 1 ana staff 1 x c";
        assert_eq!(ls_long(ls), ls);
    }
}
