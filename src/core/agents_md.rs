//! A pointer to relay's memory in the repo's agent instruction file, so an
//! agent without hooks (or without relay installed) still reads the rules,
//! gotchas and decisions, and is told how to add one. The block sits
//! between markers, so a second `relay init` updates it in place.

use std::path::PathBuf;

use anyhow::Result;

use crate::core::paths::Paths;
use crate::helpers::write_atomic;

const OPEN: &str = "<!-- relay -->";
const CLOSE: &str = "<!-- /relay -->";

fn block(shared: &str) -> String {
    format!(
        "{OPEN}\nBefore starting, read `{shared}/project.md`. Rules, gotchas and decisions live under `{shared}/`, \
one file each; a handoff someone shared is in `{shared}/handoffs/`. When you settle a decision, hit a gotcha \
or learn a project rule, save it: `relay remember decision|gotcha|rule \"<one line>\"`.\n{CLOSE}\n"
    )
}

/// `text` with relay's block in place: replaced when present, appended
/// when not. `None` when nothing would change.
pub fn with_block(text: &str, block: &str) -> Option<String> {
    let out = match (text.find(OPEN), text.find(CLOSE)) {
        // A formatter may reflow the block; only its words matter.
        (Some(a), Some(b)) if b > a && same_words(&text[a..b + CLOSE.len()], block) => return None,
        (Some(a), Some(b)) if b > a => {
            let end = b + CLOSE.len();
            let end = text[end..].strip_prefix('\n').map_or(end, |_| end + 1);
            format!("{}{block}{}", &text[..a], &text[end..])
        }
        _ if text.trim().is_empty() => block.to_string(),
        _ => format!("{}\n\n{block}", text.trim_end()),
    };
    (out != text).then_some(out)
}

fn same_words(a: &str, b: &str) -> bool {
    a.split_whitespace().eq(b.split_whitespace())
}

/// Write the block into AGENTS.md (created when missing) and into CLAUDE.md
/// when that exists. Returns the files changed.
pub fn ensure(paths: &Paths) -> Result<Vec<PathBuf>> {
    let block = block(&paths.rel(&paths.shared));
    let mut changed = Vec::new();
    for (name, create) in [("AGENTS.md", true), ("CLAUDE.md", false)] {
        let file = paths.root.join(name);
        if !file.exists() && !create {
            continue;
        }
        let text = std::fs::read_to_string(&file).unwrap_or_default();
        if let Some(updated) = with_block(&text, &block) {
            write_atomic(&file, updated.as_bytes())?;
            changed.push(file);
        }
    }
    Ok(changed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_block_is_appended_once_and_updated_in_place() {
        let b1 = block(".relay");
        assert_eq!(with_block("", &b1), Some(b1.clone()));
        let with = with_block("# Rules\n\nBe kind.\n", &b1).unwrap();
        assert_eq!(with, format!("# Rules\n\nBe kind.\n\n{b1}"));
        assert_eq!(with_block(&with, &b1), None);
        let reflowed =
            with.replace("<!-- relay -->\n", "<!-- relay -->\n\n").replace("one file each;", "one file each;\n");
        assert_eq!(with_block(&reflowed, &b1), None, "a reflowed block is the same block");
        let b2 = block(".git/relay/shared");
        let updated = with_block(&format!("{with}\n# After\n"), &b2).unwrap();
        assert!(updated.contains(".git/relay/shared/project.md") && !updated.contains("`.relay/`"), "{updated}");
        assert!(updated.ends_with("# After\n"), "{updated}");
    }
}
