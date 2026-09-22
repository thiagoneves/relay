//! Structured filters for git output. They never parse anything they
//! cannot recognise: unknown shapes fall through untouched.

use crate::limits;

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

/// Where the diff parser is.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Part {
    /// Outside any file: commit headers and messages from `git show`.
    Outside,
    /// Between `diff --git` and the first hunk.
    FileHeader,
    /// Inside a hunk; the number is how many columns carry `+`/`-`
    /// (two for a combined diff of a merge).
    Hunk(usize),
    /// A `GIT binary patch` body.
    Binary,
}

fn is_file_start(l: &str) -> bool {
    l.starts_with("diff --git ") || l.starts_with("diff --cc ") || l.starts_with("diff --combined ")
}

/// `git diff` / `git show`: keep file names, what happened to each file
/// (new, deleted, renamed, binary), hunk headers and changed lines. Drop
/// context lines and the `index`/`---`/`+++` header. Cap changed lines
/// per file. Text that is not a patch comes back untouched.
pub fn diff(text: &str) -> String {
    if !text.lines().any(is_file_start) {
        return text.to_string();
    }
    let mut out = Vec::new();
    let mut part = Part::Outside;
    let mut in_file = 0usize;
    let mut skipped = 0usize;
    let flush_skipped = |out: &mut Vec<String>, skipped: &mut usize| {
        if *skipped > 0 {
            out.push(format!("  … [{} more changed lines in this file]", *skipped));
            *skipped = 0;
        }
    };
    for l in text.lines() {
        if is_file_start(l) {
            flush_skipped(&mut out, &mut skipped);
            in_file = 0;
            part = Part::FileHeader;
            let rest = l.split_once(' ').map_or(l, |(_, r)| r).split_once(' ').map_or(l, |(_, r)| r);
            let name = rest.split(" b/").nth(1).unwrap_or(rest);
            out.push(format!("── {name}"));
            continue;
        }
        if l.starts_with("@@") && part != Part::Outside {
            part = Part::Hunk(l.chars().take_while(|c| *c == '@').count().saturating_sub(1).max(1));
            out.push(hunk_header(l));
            continue;
        }
        match part {
            Part::FileHeader => {
                if let Some(marker) = header_marker(l) {
                    out.push(format!("  ({marker})"));
                } else if l.starts_with("GIT binary patch") {
                    out.push("  (binary patch)".into());
                    part = Part::Binary;
                } else if !is_header_noise(l) {
                    out.push(l.to_string());
                }
            }
            Part::Hunk(cols) => {
                let head: Vec<char> = l.chars().take(cols).collect();
                if l.is_empty() {
                    // An empty context line whose leading space was stripped.
                } else if head.len() == cols && head.iter().all(|c| matches!(c, ' ' | '+' | '-')) {
                    if head.iter().any(|c| *c != ' ') {
                        if in_file >= limits::compress::DIFF_LINES_PER_FILE {
                            skipped += 1;
                        } else {
                            in_file += 1;
                            out.push(l.to_string());
                        }
                    }
                    // Context lines are dropped: `relay get` has them.
                } else if !l.starts_with('\\') {
                    // Past the last hunk: the next commit of `git show a b`.
                    flush_skipped(&mut out, &mut skipped);
                    part = Part::Outside;
                    out.push(l.to_string());
                }
            }
            Part::Binary => {}
            Part::Outside => out.push(l.to_string()),
        }
    }
    flush_skipped(&mut out, &mut skipped);
    out.join("\n")
}

/// "@@ -1,2 +1,3 @@ fn main" -> "@@ fn main"; the ranges stay when there
/// is no context. Works for `@@@` combined headers too.
fn hunk_header(l: &str) -> String {
    let n = l.chars().take_while(|c| *c == '@').count();
    let marker = &l[..n];
    let rest = &l[n..];
    let (ranges, ctx) = rest.find(marker).map_or((rest.trim(), ""), |end| (rest[..end].trim(), rest[end + n..].trim()));
    format!("{marker} {}", if ctx.is_empty() { ranges } else { ctx })
}

/// What happened to the file, when the header says so.
fn header_marker(l: &str) -> Option<String> {
    if l.starts_with("new file mode") {
        Some("new file".into())
    } else if l.starts_with("deleted file mode") {
        Some("deleted".into())
    } else if let Some(from) = l.strip_prefix("rename from ") {
        Some(format!("renamed from {from}"))
    } else if let Some(from) = l.strip_prefix("copy from ") {
        Some(format!("copied from {from}"))
    } else if let Some(mode) = l.strip_prefix("new mode ") {
        Some(format!("mode {mode}"))
    } else if l.starts_with("Binary files ") && l.ends_with(" differ") {
        Some("binary".into())
    } else {
        None
    }
}

fn is_header_noise(l: &str) -> bool {
    ["index ", "--- ", "+++ ", "old mode ", "similarity index ", "dissimilarity index ", "rename to ", "copy to "]
        .iter()
        .any(|p| l.starts_with(p))
}

/// `git log` default format collapsed to one line per commit. Any line
/// outside that format (a patch, a stat, a graph) returns the text as is.
pub fn log(text: &str) -> String {
    const HEADERS: &[&str] = &["Merge: ", "AuthorDate: ", "Commit: ", "CommitDate: "];
    let mut out = Vec::new();
    let mut sha = String::new();
    let mut author = String::new();
    let mut date = String::new();
    let mut subject: Option<String> = None;
    let flush = |out: &mut Vec<String>, sha: &str, author: &str, date: &str, subject: &Option<String>| {
        if !sha.is_empty() {
            let s = subject.clone().unwrap_or_default();
            let short: String = sha.chars().take(7).collect();
            out.push(format!("{short} {date} {author} {s}").trim().to_string());
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
        } else if l.starts_with("    ") {
            if subject.is_none() && !l.trim().is_empty() {
                subject = Some(l.trim().to_string());
            }
        } else if !l.trim().is_empty() && !HEADERS.iter().any(|h| l.starts_with(h)) {
            return text.to_string();
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
    fn diff_keeps_changed_lines_that_look_like_headers() {
        let s = "diff --git a/m.sql b/m.sql\nindex 1..2 100644\n--- a/m.sql\n+++ b/m.sql\n@@ -1,3 +1,3 @@\n-- drop users table\n--- old comment\n+++ new comment\n select 1;\n";
        assert_eq!(diff(s), "── m.sql\n@@ -1,3 +1,3\n-- drop users table\n--- old comment\n+++ new comment");
    }

    #[test]
    fn diff_says_what_happened_to_each_file() {
        let s = "diff --git a/old.rs b/old.rs\ndeleted file mode 100644\nindex 1..0\n--- a/old.rs\n+++ /dev/null\n@@ -1 +0,0 @@\n-fn a() {}\n\
                 diff --git a/new.rs b/new.rs\nnew file mode 100644\nindex 0..1\n--- /dev/null\n+++ b/new.rs\n@@ -0,0 +1 @@\n+fn b() {}\n\
                 diff --git a/logo.png b/logo.png\nnew file mode 100644\nindex 0..1\nBinary files /dev/null and b/logo.png differ\n\
                 diff --git a/a.rs b/b.rs\nsimilarity index 100%\nrename from a.rs\nrename to b.rs\n\
                 diff --git a/run.sh b/run.sh\nold mode 100644\nnew mode 100755\n";
        assert_eq!(
            diff(s),
            "── old.rs\n  (deleted)\n@@ -1 +0,0\n-fn a() {}\n\
             ── new.rs\n  (new file)\n@@ -0,0 +1\n+fn b() {}\n\
             ── logo.png\n  (new file)\n  (binary)\n\
             ── b.rs\n  (renamed from a.rs)\n\
             ── run.sh\n  (mode 100755)"
        );
    }

    #[test]
    fn diff_keeps_the_commit_around_a_patch() {
        let s = "commit abc\nAuthor: Ana <a@x>\n\n    Fix\n\ndiff --git a/x b/x\n--- a/x\n+++ b/x\n@@ -1 +1 @@\n-a\n+b\ncommit def\nAuthor: Bo <b@x>\n";
        assert_eq!(
            diff(s),
            "commit abc\nAuthor: Ana <a@x>\n\n    Fix\n\n── x\n@@ -1 +1\n-a\n+b\ncommit def\nAuthor: Bo <b@x>"
        );
    }

    #[test]
    fn diff_of_a_merge_keeps_both_columns() {
        let s = "diff --cc x.rs\nindex 1,2..3\n--- a/x.rs\n+++ b/x.rs\n@@@ -1,1 -1,1 +1,1 @@@\n- a\n +b\n  same\n";
        assert_eq!(diff(s), "── x.rs\n@@@ -1,1 -1,1 +1,1\n- a\n +b");
    }

    #[test]
    fn text_that_is_not_a_patch_is_untouched() {
        let names = "src/a.rs\nsrc/b.rs\n-weird\n";
        assert_eq!(diff(names), names);
    }

    #[test]
    fn log_one_line_per_commit() {
        let s = "commit abcdef1234567\nAuthor: Ana <a@x>\nDate:   Mon Sep 22 10:00:00 2026 -0300\n\n    Fix thing\n\n    body\n\ncommit 1234567abcdef\nAuthor: Bo <b@x>\nDate:   Sun Sep 21 10:00:00 2026 -0300\n\n    Add thing\n";
        assert_eq!(log(s), "abcdef1 Sep 22 Ana Fix thing\n1234567 Sep 21 Bo Add thing");
    }

    #[test]
    fn log_with_patches_or_files_is_untouched() {
        let patch = "commit abc1000\nAuthor: A <a@x>\nDate:   Mon Sep 22 10:00:00 2026 -0300\n\n    Fix 1\n\ndiff --git a/x b/x\n--- a/x\n+++ b/x\n@@ -1 +1 @@\n-a\n+b\n";
        assert_eq!(log(patch), patch);
        let names =
            "commit abc1000\nAuthor: A <a@x>\nDate:   Mon Sep 22 10:00:00 2026 -0300\n\n    Fix 1\n\nsrc/a.rs\n";
        assert_eq!(log(names), names);
    }

    #[test]
    fn log_survives_a_non_ascii_sha() {
        assert!(log("commit aaéééééé\nAuthor: A <a@x>\n\n    x\n").starts_with("aaééééé "));
    }
}
