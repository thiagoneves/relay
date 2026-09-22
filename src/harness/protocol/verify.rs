//! Whether the harness showed the model the output relay swapped in. A
//! `PostToolUse` replacement that the harness ignores fails without an
//! error, so relay checks the transcript at the end of each session: a
//! replaced output reached the model only if its `relay get <id>` footer
//! is in the tool result the transcript recorded.

use std::collections::HashMap;

/// One output relay replaced: the harness's call id and the stored id
/// its footer names.
pub struct Replaced {
    pub tool_use_id: String,
    pub output_id: String,
}

/// Of the replacements whose tool result the transcript still holds, how
/// many were checked and how many the model never saw.
#[derive(Debug, PartialEq, Eq)]
pub struct Outcome {
    pub checked: usize,
    pub ignored: usize,
}

pub fn check(replaced: &[Replaced], results: &HashMap<String, String>) -> Outcome {
    let seen: Vec<bool> = replaced
        .iter()
        .filter_map(|r| results.get(&r.tool_use_id).map(|text| text.contains(&format!("relay get {}", r.output_id))))
        .collect();
    Outcome { checked: seen.len(), ignored: seen.iter().filter(|s| !**s).count() }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn replaced(tool: &str, output: &str) -> Replaced {
        Replaced { tool_use_id: tool.into(), output_id: output.into() }
    }

    #[test]
    fn counts_only_what_the_transcript_still_holds() {
        let results = HashMap::from([
            ("t1".to_string(), "row 400\n[relay 3k→900 tokens · original: relay get o_1]".to_string()),
            ("t2".to_string(), "row 1\nrow 2\n…the whole original".to_string()),
        ]);
        let all = [replaced("t1", "o_1"), replaced("t2", "o_2"), replaced("t3", "o_3")];
        assert_eq!(check(&all, &results), Outcome { checked: 2, ignored: 1 });
        assert_eq!(check(&[], &results), Outcome { checked: 0, ignored: 0 });
    }
}
