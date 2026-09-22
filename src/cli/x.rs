use std::io::Write;

use crate::core::exec;
use crate::helpers::shell;

pub fn run(cmd: &str, raw: bool, session: Option<&str>) -> anyhow::Result<i32> {
    let out = exec::run(cmd, raw, session)?;
    let mut stdout = std::io::stdout().lock();
    if !out.printed.is_empty() {
        let _ = writeln!(stdout, "{}", out.printed);
    }
    let _ = stdout.flush();
    Ok(out.exit)
}

/// One argument is a command line already (what the hook passes); several
/// are argv, so each keeps its boundaries when handed to the shell.
pub fn command_line(args: &[String]) -> String {
    if let [one] = args {
        return one.clone();
    }
    let plain = |a: &str| !a.is_empty() && a.chars().all(|c| c.is_ascii_alphanumeric() || "-_./=:@,+%".contains(c));
    args.iter().map(|a| if plain(a) { a.clone() } else { shell::quote(a) }).collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(args: &[&str]) -> String {
        command_line(&args.iter().map(ToString::to_string).collect::<Vec<_>>())
    }

    #[test]
    fn keeps_argument_boundaries() {
        assert_eq!(line(&["cargo test && git status"]), "cargo test && git status");
        assert_eq!(line(&["grep", "foo bar", "src"]), "grep 'foo bar' src");
        assert_eq!(line(&["git", "log", "--format=%h"]), "git log --format=%h");
    }
}
