//! Credentials masked out of text relay keeps and shows again later
//! (prompts, commands, replies), since a handoff puts that text back into
//! the next session's context. Stored command output is left as printed:
//! `relay get` must return it unchanged.

use std::borrow::Cow;
use std::sync::LazyLock;

use regex::Regex;

const MASK: &str = "***";

static RULES: LazyLock<Vec<(Regex, &'static str)>> = LazyLock::new(|| {
    [
        // `Authorization: Bearer x`, `-H "Authorization: token x"`
        (r"(?i)(authorization:\s*(?:bearer|basic|token)\s+)[^\s'\x22]+", "${1}***"),
        (r"(?i)(\bbearer\s+)[A-Za-z0-9._~+/=-]{8,}", "${1}***"),
        // `--password x`, `token=x`, `API_KEY: x`
        (
            r#"(?i)((?:api[_-]?key|access[_-]?key|secret|token|password|passwd|pwd)\s*[=:]\s*|--(?:api-key|token|password|secret)[= ])["']?[^\s"'&]+"#,
            "${1}***",
        ),
        // user:pass@ in URLs
        (r"(://)[^/\s:@]+:[^/\s@]+@", "${1}***@"),
        // Well-known token shapes.
        (r"\b(?:ghp|gho|ghs|ghu|ghr|github_pat)_[A-Za-z0-9_]{20,}", MASK),
        (r"\bsk-[A-Za-z0-9_-]{20,}", MASK),
        (r"\bxox[abprs]-[A-Za-z0-9-]{10,}", MASK),
        (r"\bAKIA[0-9A-Z]{16}\b", MASK),
        (r"\bAIza[0-9A-Za-z_-]{30,}", MASK),
        // A JWT, and the body of a PEM private key.
        (r"\beyJ[A-Za-z0-9_-]{8,}\.eyJ[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}", MASK),
        (r"-----BEGIN [A-Z ]*PRIVATE KEY-----[\s\S]*?-----END [A-Z ]*PRIVATE KEY-----", "-----PRIVATE KEY ***-----"),
    ]
    .into_iter()
    .map(|(re, to)| (Regex::new(re).expect("valid regex"), to))
    .collect()
});

pub fn redact(text: &str) -> Cow<'_, str> {
    let mut out = Cow::Borrowed(text);
    for (re, to) in RULES.iter() {
        if re.is_match(&out) {
            out = Cow::Owned(re.replace_all(&out, *to).into_owned());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masks_credentials_and_keeps_the_rest() {
        for (input, want) in [
            (r#"curl -H "Authorization: Bearer abc.def.ghi" api/x"#, r#"curl -H "Authorization: Bearer ***" api/x"#),
            ("export GITHUB_TOKEN=ghp_0123456789abcdefghijABCD", "export GITHUB_TOKEN=***"),
            ("psql postgres://app:s3cret@db/app", "psql postgres://***@db/app"),
            ("tool --password hunter2 --verbose", "tool --password *** --verbose"),
            ("key sk-proj_0123456789abcdefghij0123", "key ***"),
            ("aws AKIAABCDEFGHIJKLMNOP", "aws ***"),
            ("jwt eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxIn0.abcdefghijk", "jwt ***"),
            ("-----BEGIN RSA PRIVATE KEY-----\nMIIE\n-----END RSA PRIVATE KEY-----", "-----PRIVATE KEY ***-----"),
        ] {
            assert_eq!(redact(input), want, "{input}");
        }
    }

    #[test]
    fn leaves_ordinary_commands_alone() {
        for plain in ["cargo test -- --nocapture", "git log --format=%h", "grep -rn token src/", "echo key: value"] {
            assert!(matches!(redact(plain), Cow::Borrowed(_)), "{plain}");
        }
    }
}
