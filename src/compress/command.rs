//! Shell command shape: how a command line splits into simple commands
//! and which program each one runs. Every rule that looks at a command
//! (filter choice, read detection, wrapping) goes through `segments`, so
//! they all agree on where one command ends and the next begins.

/// What joins a segment to the one after it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Joint {
    /// `&&`
    And,
    /// `||`
    Or,
    /// `;` or a newline
    Seq,
    /// `|` or `|&`: this segment's output feeds the next one.
    Pipe,
    /// A lone `&`: this segment runs in the background.
    Background,
    /// Last segment of the command.
    End,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment<'a> {
    pub text: &'a str,
    pub then: Joint,
}

/// Split a command line on `&&`, `||`, `;`, `|`, a lone `&` and newlines,
/// outside quotes. Redirections (`2>&1`, `>&2`, `&>f`) are not
/// separators. Empty segments are dropped; the joint that ended one is
/// kept on the segment before it.
pub fn segments(cmd: &str) -> Vec<Segment<'_>> {
    let b = cmd.as_bytes();
    let mut out = Vec::new();
    let (mut start, mut i) = (0, 0);
    while i < b.len() {
        match b[i] {
            b'\\' => i += 2,
            q @ (b'\'' | b'"') => i = after_quote(b, i + 1, q),
            _ => match joint_at(b, i) {
                Some((joint, len)) => {
                    push(&mut out, &cmd[start..i], joint);
                    i += len;
                    start = i;
                }
                None => i += 1,
            },
        }
    }
    if start < b.len() {
        push(&mut out, &cmd[start..], Joint::End);
    }
    if let Some(last) = out.last_mut().filter(|s| s.then != Joint::Background) {
        last.then = Joint::End;
    }
    out
}

/// Index just past the quote `q` closing a string that starts at `i`.
fn after_quote(b: &[u8], mut i: usize, q: u8) -> usize {
    while i < b.len() {
        match b[i] {
            b'\\' if q == b'"' => i += 2,
            c if c == q => return i + 1,
            _ => i += 1,
        }
    }
    b.len()
}

/// The separator starting at `i`, and its length. `&>file`, `>&2` and
/// `2>&1` are redirections, not separators.
fn joint_at(b: &[u8], i: usize) -> Option<(Joint, usize)> {
    let next = b.get(i + 1).copied();
    match b[i] {
        b'&' if next == Some(b'&') => Some((Joint::And, 2)),
        b'&' if next == Some(b'>') || i > 0 && b[i - 1] == b'>' => None,
        b'&' => Some((Joint::Background, 1)),
        b'|' if next == Some(b'|') => Some((Joint::Or, 2)),
        b'|' if next == Some(b'&') => Some((Joint::Pipe, 2)),
        b'|' => Some((Joint::Pipe, 1)),
        b';' | b'\n' => Some((Joint::Seq, 1)),
        _ => None,
    }
}

/// An empty segment only moves its joint onto the one before:
/// in `a &\n b` the background marker wins over the newline.
fn push<'a>(out: &mut Vec<Segment<'a>>, text: &'a str, then: Joint) {
    let text = text.trim();
    if !text.is_empty() {
        out.push(Segment { text, then });
    } else if let Some(last) = out.last_mut().filter(|l| l.then != Joint::Background) {
        last.then = then;
    }
}

/// Split off leading `cd`, `pushd`, `popd` and `export` steps joined by
/// `&&` or `;`. Their effect has to land in the caller's shell, so only
/// the rest may run under `relay x`. Returns `("", cmd)` when there is no
/// such prefix or nothing follows it.
pub fn split_shell_state(cmd: &str) -> (&str, &str) {
    const STATE: &[&str] = &["cd", "pushd", "popd", "export"];
    for seg in segments(cmd) {
        let first = seg.text.split_whitespace().next().unwrap_or("");
        if STATE.contains(&first) && matches!(seg.then, Joint::And | Joint::Seq) {
            continue;
        }
        if STATE.contains(&first) {
            break;
        }
        // `segments` hands out subslices of `cmd`, so the offset is exact.
        let at = seg.text.as_ptr() as usize - cmd.as_ptr() as usize;
        return if at == 0 { ("", cmd) } else { cmd.split_at(at) };
    }
    ("", cmd)
}

/// Leading tokens of one simple command, skipping env assignments,
/// wrappers (`sudo`, `time`, `timeout 30`, `env`, `nohup`) and
/// `pnpm|yarn|npm exec`, so the first
/// token is the program that produces the output. Quotes around a token
/// are removed. At most four tokens.
pub fn head_tokens(cmd: &str) -> Vec<String> {
    const WRAPPERS: &[&str] = &["sudo", "rtk", "time", "env", "command", "exec", "nohup", "timeout"];
    let mut toks: Vec<String> = Vec::new();
    for raw in cmd.split_whitespace() {
        if toks.is_empty() {
            if raw.contains('=') && !raw.starts_with('-') {
                continue; // FOO=bar prefix
            }
            if WRAPPERS.contains(&raw) {
                continue;
            }
            // `timeout 30 cmd`: the duration is not the command.
            if raw.chars().next().is_some_and(|c| c.is_ascii_digit()) {
                continue;
            }
        }
        let t = raw.trim_matches(|c| c == '"' || c == '\'');
        if toks.len() == 1 && t == "exec" && matches!(toks[0].as_str(), "pnpm" | "yarn" | "npm") {
            toks.clear();
            continue;
        }
        toks.push(t.to_string());
        if toks.len() >= 4 {
            break;
        }
    }
    toks
}

/// One simple command as the filter rules see it: its words as typed,
/// and the leading tokens of the program that produces its output.
pub struct Simple<'a> {
    pub words: Vec<&'a str>,
    pub toks: Vec<String>,
}

impl<'a> Simple<'a> {
    pub fn new(text: &'a str) -> Self {
        Self { words: text.split_whitespace().collect(), toks: head_tokens(text) }
    }

    /// The program, without its directory; empty for an empty command.
    pub fn program(&self) -> &str {
        self.toks.first().map_or("", |t| program(t))
    }

    /// The `i`th leading token as typed, empty when there is none.
    pub fn arg(&self, i: usize) -> &str {
        self.toks.get(i).map_or("", String::as_str)
    }

    /// Whether any word is one of `flags`, alone or as `flag=value`.
    pub fn has_flag(&self, flags: &[&str]) -> bool {
        self.words.iter().any(|w| flags.iter().any(|f| w == f || w.starts_with(&format!("{f}="))))
    }
}

/// The program name without its directory: `./gradlew` → `gradlew`,
/// `/usr/bin/git` → `git`.
pub fn program(token: &str) -> &str {
    token.rsplit(['/', '\\']).next().unwrap_or(token)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn split(cmd: &str) -> Vec<(&str, Joint)> {
        segments(cmd).into_iter().map(|s| (s.text, s.then)).collect()
    }

    #[test]
    fn splits_on_every_separator() {
        assert_eq!(
            split("cd a && git diff || true; cargo test | tail -5\nls"),
            [
                ("cd a", Joint::And),
                ("git diff", Joint::Or),
                ("true", Joint::Seq),
                ("cargo test", Joint::Pipe),
                ("tail -5", Joint::Seq),
                ("ls", Joint::End),
            ]
        );
    }

    #[test]
    fn redirections_are_not_separators() {
        assert_eq!(split("cargo test 2>&1 | tee log"), [("cargo test 2>&1", Joint::Pipe), ("tee log", Joint::End)]);
        assert_eq!(split("make &> build.log"), [("make &> build.log", Joint::End)]);
        assert_eq!(split("echo x >&2"), [("echo x >&2", Joint::End)]);
        assert_eq!(split("make |& grep error"), [("make", Joint::Pipe), ("grep error", Joint::End)]);
    }

    #[test]
    fn a_lone_ampersand_is_background() {
        assert_eq!(split("npm run dev & sleep 3"), [("npm run dev", Joint::Background), ("sleep 3", Joint::End)]);
        assert_eq!(split("npm run dev &"), [("npm run dev", Joint::Background)]);
        assert_eq!(split("npm run dev &\ncurl x"), [("npm run dev", Joint::Background), ("curl x", Joint::End)]);
    }

    #[test]
    fn quotes_hide_separators() {
        assert_eq!(
            split(r#"rg "a|b" src; git log --format='%h; %s'"#),
            [(r#"rg "a|b" src"#, Joint::Seq), ("git log --format='%h; %s'", Joint::End)]
        );
        assert_eq!(split(r#"echo "say \"a;b\"" && ls"#), [(r#"echo "say \"a;b\"""#, Joint::And), ("ls", Joint::End)]);
        assert_eq!(split(r"echo a\;b"), [(r"echo a\;b", Joint::End)]);
    }

    #[test]
    fn empty_segments_are_dropped() {
        assert_eq!(split(";; ls ;"), [("ls", Joint::End)]);
        assert!(segments("   ").is_empty());
    }

    #[test]
    fn head_tokens_skip_wrappers_and_runners() {
        assert_eq!(head_tokens("RUST_LOG=debug sudo timeout 30 cargo test -q"), ["cargo", "test", "-q"]);
        assert_eq!(head_tokens("pnpm exec vitest run"), ["vitest", "run"]);
        assert_eq!(head_tokens("yarn exec jest"), ["jest"]);
        assert_eq!(head_tokens("pnpm run test"), ["pnpm", "run", "test"]);
    }

    #[test]
    fn shell_state_prefix_stays_outside() {
        assert_eq!(split_shell_state("cd /repo && cargo test"), ("cd /repo && ", "cargo test"));
        assert_eq!(
            split_shell_state("cd a; export X=1 && git status | head"),
            ("cd a; export X=1 && ", "git status | head")
        );
        assert_eq!(split_shell_state("cargo test && cd x"), ("", "cargo test && cd x"));
        assert_eq!(split_shell_state("cd /repo"), ("", "cd /repo"));
        assert_eq!(split_shell_state("cd a || git status"), ("", "cd a || git status"));
        assert_eq!(split_shell_state("cd 'my dir' && ls"), ("cd 'my dir' && ", "ls"));
    }

    #[test]
    fn program_drops_the_directory() {
        assert_eq!(program("./gradlew"), "gradlew");
        assert_eq!(program("/usr/bin/git"), "git");
        assert_eq!(program("git"), "git");
    }
}
