/// Token estimate used everywhere in relay, always labelled as an estimate.
///
/// A linear model over cheap text features, fitted against a real BPE
/// tokenizer (`o200k_base`) on 6,000 shell outputs from coding-agent
/// transcripts: median error 6.5% per output and +0.4% bias in total,
/// against 12% and -10% for bytes/4. Bytes/4 misreads indentation and
/// alignment runs, which BPE encodes cheaply, and would make whitespace
/// cleanup look like savings.
pub fn est_tokens(s: &str) -> usize {
    let mut f = Features::default();
    // The sentinel closes a word, number or blank run at the end.
    for c in s.chars().chain(std::iter::once('\0')) {
        f.push(c);
    }
    f.tokens()
}

/// The text features the token model is fitted on, counted in one pass.
#[derive(Default)]
struct Features {
    words: usize,
    long_word_extra: usize,
    digit_triples: usize,
    punct: usize,
    space_runs: usize,
    newlines: usize,
    non_ascii: usize,
    word_len: usize,
    digit_len: usize,
    blank_len: usize,
}

impl Features {
    fn push(&mut self, c: char) {
        self.run_of_letters(c);
        self.run_of_digits(c);
        self.run_of_blanks(c);
        self.single(c);
    }

    fn run_of_letters(&mut self, c: char) {
        if c.is_ascii_alphabetic() {
            self.word_len += 1;
        } else if self.word_len > 0 {
            self.words += 1;
            self.long_word_extra += self.word_len.saturating_sub(8) / 6;
            self.word_len = 0;
        }
    }

    fn run_of_digits(&mut self, c: char) {
        if c.is_ascii_digit() {
            self.digit_len += 1;
        } else if self.digit_len > 0 {
            self.digit_triples += self.digit_len.div_ceil(3);
            self.digit_len = 0;
        }
    }

    fn run_of_blanks(&mut self, c: char) {
        if c == ' ' || c == '\t' {
            self.blank_len += 1;
        } else {
            self.space_runs += usize::from(self.blank_len >= 2);
            self.blank_len = 0;
        }
    }

    fn single(&mut self, c: char) {
        match c {
            '\n' => self.newlines += 1,
            '\0' => {}
            c if !c.is_ascii() => {
                self.non_ascii += 1;
                self.punct += usize::from(!c.is_alphanumeric() && !c.is_whitespace());
            }
            c if c.is_ascii_punctuation() && c != '_' => self.punct += 1,
            _ => {}
        }
    }

    fn tokens(&self) -> usize {
        const WORD: f64 = 1.387;
        const LONG_WORD_EXTRA: f64 = 1.154;
        const DIGIT_TRIPLE: f64 = 1.819;
        const PUNCT: f64 = 0.361;
        const SPACE_RUN: f64 = 0.886;
        const NEWLINE: f64 = 0.535;
        const NON_ASCII: f64 = 0.054;
        let est = WORD * self.words as f64
            + LONG_WORD_EXTRA * self.long_word_extra as f64
            + DIGIT_TRIPLE * self.digit_triples as f64
            + PUNCT * self.punct as f64
            + SPACE_RUN * self.space_runs as f64
            + NEWLINE * self.newlines as f64
            + NON_ASCII * self.non_ascii as f64;
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "non-negative and far below usize::MAX"
        )]
        let rounded = est.round() as usize;
        rounded
    }
}

/// Whole lines of `text` that fit in `max` bytes, and whether any were
/// left out. A first line longer than `max` is truncated instead.
pub fn cut_lines(text: &str, max: usize) -> (String, bool) {
    let mut out = String::new();
    for l in text.lines() {
        if out.len() + l.len() + 1 > max {
            if out.is_empty() {
                out = truncate_chars(l, max);
            }
            return (out, true);
        }
        out.push_str(l);
        out.push('\n');
    }
    (out.trim_end().to_string(), false)
}

/// Truncate to at most `max` chars, appending an ellipsis.
pub fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let cut: String = s.chars().take(max.saturating_sub(1)).collect();
    format!("{cut}…")
}

pub fn human_bytes(n: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = n as f64;
    let mut i = 0;
    while v >= 1024.0 && i < UNITS.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 { format!("{n} B") } else { format!("{v:.1} {}", UNITS[i]) }
}

/// `1 rule`, `3 rules`: `word` is the singular, pluralized with `s`.
pub fn count(n: usize, word: &str) -> String {
    if n == 1 { format!("1 {word}") } else { format!("{n} {word}s") }
}

pub fn human_tokens(n: usize) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1e6)
    } else if n >= 10_000 {
        format!("{:.1}k", n as f64 / 1e3)
    } else {
        n.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_estimate_tracks_content_not_whitespace() {
        assert_eq!(est_tokens(""), 0);
        assert_eq!(est_tokens("hello world"), 3);
        let flat = "if x {\nreturn y;\n}\n";
        let indented = "if x {\n                return y;\n}\n";
        assert!(est_tokens(indented) - est_tokens(flat) <= 1, "indentation is nearly free in BPE");
        assert!(est_tokens("12345678") > est_tokens("1"));
    }

    #[test]
    fn truncates_with_ellipsis() {
        assert_eq!(truncate_chars("hello", 10), "hello");
        assert_eq!(truncate_chars("hello world", 6), "hello…");
    }

    #[test]
    fn cut_lines_keeps_whole_lines() {
        assert_eq!(cut_lines("a\nbb\nccc", 5), ("a\nbb\n".to_string(), true));
        assert_eq!(cut_lines("a\nbb", 50), ("a\nbb".to_string(), false));
        assert_eq!(cut_lines("abcdefgh", 4), ("abc…".to_string(), true));
    }

    #[test]
    fn counts_agree_with_their_number() {
        assert_eq!(count(1, "rule"), "1 rule");
        assert_eq!(count(0, "rule"), "0 rules");
        assert_eq!(count(3, "handoff"), "3 handoffs");
    }
}
