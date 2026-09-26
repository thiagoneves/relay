//! Whole-file reads of big text files. A 345 KB plan doc is about 100k
//! tokens, and each of twenty subagents reading it whole pays that again;
//! most needed one section. Such a read is turned down with the file's
//! outline and the line ranges to read instead. Ranged reads always go
//! through, and the file on disk stays the original.

use std::path::Path;

use crate::core::outline;
use crate::helpers::{est_tokens, human_bytes, human_tokens};
use crate::limits::reads::{GUARD_BYTES, OUTLINE_ENTRIES, OUTLINE_MAX_BYTES};

/// A read turned down, and what it would have cost.
#[derive(Debug, PartialEq, Eq)]
pub struct Guarded {
    /// What the agent reads instead.
    pub message: String,
    /// Estimated tokens of the whole file.
    pub file_tokens: usize,
}

/// Formats the harness renders from bytes rather than as text.
const NOT_TEXT: &[&str] = &[
    "png", "jpg", "jpeg", "gif", "webp", "bmp", "ico", "tiff", "svgz", "pdf", "ipynb", "zip", "gz", "wasm", "mp3",
    "mp4", "wav",
];

/// The outline to show instead of reading `file` whole, or `None` when
/// the read should go through: a range was asked for, or the file is
/// small, missing or not text. `rel` is how the file is named to the agent.
pub fn check(file: &Path, rel: &str, ranged: bool) -> Option<Guarded> {
    if ranged {
        return None;
    }
    let ext = file.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    if NOT_TEXT.contains(&ext.as_str()) {
        return None;
    }
    let size = std::fs::metadata(file).ok().filter(std::fs::Metadata::is_file)?.len();
    if size <= GUARD_BYTES || size > OUTLINE_MAX_BYTES {
        return None;
    }
    let bytes = std::fs::read(file).ok()?;
    if bytes[..bytes.len().min(8192)].contains(&0) {
        return None;
    }
    let text = String::from_utf8_lossy(&bytes);
    let file_tokens = est_tokens(&text);
    let entries = outline::of(&text, outline::is_markdown(rel));
    // The example range: the first of the deepest entries shown.
    let shown = outline::pick(&entries, OUTLINE_ENTRIES);
    let deepest = shown.iter().map(|e| e.depth).max().unwrap_or(1);
    let example = shown.iter().find(|e| e.depth == deepest).copied().unwrap_or(&entries[0]);
    let message = format!(
        "relay: {rel} is {} (~{} tokens, estimated), too big to read whole. Its outline, by line:\n\n{}\n\
         Read only the part you need: Read with offset and limit (offset {} and limit {} for \"{}\"), or Grep \
         for a word or id. `relay brief <id or words>` prints the matching sections of the plan docs. Ranged \
         reads always go through.",
        human_bytes(size),
        human_tokens(file_tokens),
        outline::render(&entries, OUTLINE_ENTRIES).trim_end(),
        example.start,
        example.end + 1 - example.start,
        example.title,
    );
    Some(Guarded { message, file_tokens })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("relay-ut-guard-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn a_big_doc_read_whole_gets_its_outline() {
        let d = dir("big");
        let body = "Lorem ipsum dolor sit amet, consectetur adipiscing elit.\n".repeat(40);
        let doc: String = (1..=40).map(|i| format!("## T-{i} Task {i}\n")).collect::<Vec<_>>().join(&body);
        let f = d.join("plan.md");
        std::fs::write(&f, format!("# Plan\n{doc}")).unwrap();
        let g = check(&f, "docs/plan.md", false).unwrap();
        assert!(g.message.starts_with("relay: docs/plan.md is "), "{}", g.message);
        assert!(g.message.contains("\nL2-42          T-1 Task 1\n"), "{}", g.message);
        assert!(g.message.contains("(offset 2 and limit 41 for \"T-1 Task 1\")"), "{}", g.message);
        assert!(g.file_tokens > 15_000, "{}", g.file_tokens);
        assert_eq!(check(&f, "docs/plan.md", true), None, "a ranged read goes through");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn small_binary_and_missing_files_go_through() {
        let d = dir("small");
        let small = d.join("a.md");
        std::fs::write(&small, "# A\n").unwrap();
        assert_eq!(check(&small, "a.md", false), None);
        let bin = d.join("blob.dat");
        std::fs::write(&bin, vec![0u8; 100_000]).unwrap();
        assert_eq!(check(&bin, "blob.dat", false), None);
        assert_eq!(check(&d.join("gone.md"), "gone.md", false), None);
        let _ = std::fs::remove_dir_all(&d);
    }
}
