//! Structured filters for git output. They never parse anything they
//! cannot recognise: unknown shapes fall through untouched.

/// `git status` (human format): drop hint lines and headers noise.
pub fn status(text: &str) -> String {
    let mut out = Vec::new();
    for l in text.lines() {
        let t = l.trim_end();
        let tt = t.trim_start();
        if tt.starts_with("(use \"git")
            || tt.starts_with("(commit or discard")
            || tt.starts_with("no changes added to commit")
            || tt.starts_with("nothing added to commit")
        {
            continue;
        }
        let mapped = match tt {
            "Changes not staged for commit:" => "Modified (unstaged):",
            "Changes to be committed:" => "Staged:",
            "Untracked files:" => "Untracked:",
            "Unmerged paths:" => "Conflicts:",
            _ => t,
        };
        if mapped.is_empty() {
            continue;
        }
        out.push(mapped.to_string());
    }
    out.join("\n")
}

/// `git diff`: keep file headers, hunk headers and changed lines. Drop
/// `index`, `---`/`+++` and mode lines. Cap changed lines per file.
pub fn diff(text: &str) -> String {
    const PER_FILE: usize = 80;
    let mut out = Vec::new();
    let mut in_file = 0usize;
    let mut skipped = 0usize;
    let flush_skipped = |out: &mut Vec<String>, skipped: &mut usize| {
        if *skipped > 0 {
            out.push(format!("  … [{} more changed lines in this file]", *skipped));
            *skipped = 0;
        }
    };
    for l in text.lines() {
        if let Some(rest) = l.strip_prefix("diff --git ") {
            flush_skipped(&mut out, &mut skipped);
            in_file = 0;
            let name = rest.split(" b/").nth(1).unwrap_or(rest);
            out.push(format!("── {name}"));
            continue;
        }
        if l.starts_with("index ")
            || l.starts_with("--- ")
            || l.starts_with("+++ ")
            || l.starts_with("old mode")
            || l.starts_with("new mode")
            || l.starts_with("similarity index")
            || l.starts_with("rename from")
            || l.starts_with("rename to")
        {
            continue;
        }
        if l.starts_with("@@") {
            // "@@ -1,2 +1,3 @@ fn main" -> "@@ fn main"; keep ranges when no context.
            let parts: Vec<&str> = l.splitn(3, "@@").collect();
            let ranges = parts.get(1).map_or("", |s| s.trim());
            let ctx = parts.get(2).map_or("", |s| s.trim());
            out.push(format!("@@ {}", if ctx.is_empty() { ranges } else { ctx }));
            continue;
        }
        if l.starts_with('+') || l.starts_with('-') {
            if in_file >= PER_FILE {
                skipped += 1;
                continue;
            }
            in_file += 1;
            out.push(l.to_string());
        }
        // Context lines are dropped: the agent can `relay get` the original.
    }
    flush_skipped(&mut out, &mut skipped);
    out.join("\n")
}

/// `git log` default format collapsed to one line per commit.
pub fn log(text: &str) -> String {
    let mut out = Vec::new();
    let mut sha = String::new();
    let mut author = String::new();
    let mut date = String::new();
    let mut subject: Option<String> = None;
    let flush = |out: &mut Vec<String>, sha: &str, author: &str, date: &str, subject: &Option<String>| {
        if !sha.is_empty() {
            let s = subject.clone().unwrap_or_default();
            out.push(format!("{} {} {} {}", &sha[..sha.len().min(7)], date, author, s).trim().to_string());
        }
    };
    for l in text.lines() {
        if let Some(rest) = l.strip_prefix("commit ") {
            flush(&mut out, &sha, &author, &date, &subject);
            sha = rest.split_whitespace().next().unwrap_or("").to_string();
            author.clear();
            date.clear();
            subject = None;
        } else if let Some(rest) = l.strip_prefix("Author: ") {
            author = rest.split(" <").next().unwrap_or(rest).trim().to_string();
        } else if let Some(rest) = l.strip_prefix("Date: ") {
            // "Mon Sep 22 10:11:12 2026 -0300" -> "Sep 22"
            let parts: Vec<&str> = rest.split_whitespace().collect();
            date = if parts.len() >= 3 { format!("{} {}", parts[1], parts[2]) } else { rest.trim().to_string() };
        } else if l.starts_with("    ") && subject.is_none() && !l.trim().is_empty() {
            subject = Some(l.trim().to_string());
        }
    }
    flush(&mut out, &sha, &author, &date, &subject);
    if out.is_empty() { text.to_string() } else { out.join("\n") }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_drops_hints() {
        let s = "On branch main\nChanges not staged for commit:\n  (use \"git add <file>...\" to update what will be committed)\n\tmodified:   src/a.rs\n\nno changes added to commit (use \"git add\" and/or \"git commit -a\")";
        let out = status(s);
        assert_eq!(out, "On branch main\nModified (unstaged):\n\tmodified:   src/a.rs");
    }

    #[test]
    fn diff_keeps_changes_only() {
        let s = "diff --git a/x.rs b/x.rs\nindex 1..2 100644\n--- a/x.rs\n+++ b/x.rs\n@@ -1,2 +1,2 @@ fn main\n ctx\n-old\n+new\n";
        assert_eq!(diff(s), "── x.rs\n@@ fn main\n-old\n+new");
    }

    #[test]
    fn log_one_line_per_commit() {
        let s = "commit abcdef1234567\nAuthor: Ana <a@x>\nDate:   Mon Sep 22 10:00:00 2026 -0300\n\n    Fix thing\n\n    body\n\ncommit 1234567abcdef\nAuthor: Bo <b@x>\nDate:   Sun Sep 21 10:00:00 2026 -0300\n\n    Add thing\n";
        assert_eq!(log(s), "abcdef1 Sep 22 Ana Fix thing\n1234567 Sep 21 Bo Add thing");
    }
}
