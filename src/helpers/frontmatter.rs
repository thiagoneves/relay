//! The `---` header on handoffs, memory items and project.md. Read line
//! by line so CRLF files (Git for Windows' default checkout) parse the
//! same as LF ones.

/// Value of `key: value` inside the header.
pub fn get(body: &str, key: &str) -> Option<String> {
    let mut lines = body.lines();
    if lines.next()?.trim_end() != "---" {
        return None;
    }
    let prefix = format!("{key}: ");
    for l in lines {
        if l.trim_end() == "---" {
            break;
        }
        if let Some(v) = l.strip_prefix(&prefix) {
            return Some(v.trim().to_string());
        }
    }
    None
}

/// A YAML double-quoted scalar for `s`, for values that may hold `:`,
/// `#` or quotes.
pub fn quote(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

/// `get`, with a double-quoted value unquoted.
pub fn get_text(body: &str, key: &str) -> Option<String> {
    get(body, key).map(|v| match v.strip_prefix('"').and_then(|r| r.strip_suffix('"')) {
        Some(inner) => inner.replace("\\\"", "\"").replace("\\\\", "\\"),
        None => v,
    })
}

/// The body after the header, or all of it when there is none.
pub fn strip(body: &str) -> &str {
    let mut lines = body.split_inclusive('\n');
    let Some(first) = lines.next().filter(|l| l.trim_end() == "---") else { return body };
    let mut offset = first.len();
    for line in lines {
        offset += line.len();
        if line.trim_end() == "---" {
            return body[offset..].trim_start();
        }
    }
    body
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lf_and_crlf_read_the_same() {
        for body in [
            "---\ncreated: 2026-09-22\n---\n\n# Hooks fail open\n",
            "---\r\ncreated: 2026-09-22\r\n---\r\n\r\n# Hooks fail open\r\n",
        ] {
            assert_eq!(get(body, "created").as_deref(), Some("2026-09-22"), "{body:?}");
            assert_eq!(strip(body).trim_end(), "# Hooks fail open", "{body:?}");
        }
    }

    #[test]
    fn no_header_means_whole_body() {
        assert_eq!(strip("# Title\n---\nx"), "# Title\n---\nx");
        assert_eq!(get("# Title", "created"), None);
    }

    #[test]
    fn quoted_values_round_trip() {
        let body = format!("---\ntitle: {}\n---\n", quote(r#"Say "hi": a\b"#));
        assert_eq!(get_text(&body, "title").as_deref(), Some(r#"Say "hi": a\b"#));
        assert_eq!(get_text("---\nplain: x\n---\n", "plain").as_deref(), Some("x"));
    }
}
