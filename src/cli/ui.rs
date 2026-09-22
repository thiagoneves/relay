//! How relay talks to people in a terminal. Every command prints through
//! this one look: the answer first, aligned fields for details, a mark
//! for each outcome and, when there is something to do, the next step.

use std::fmt;

use crate::helpers::term::{Color, Paint};

/// Where a command's messages go. The wrapper and setup notes use stderr
/// so they never mix with output a harness or script reads.
#[derive(Clone, Copy)]
pub struct Ui {
    paint: Paint,
    stderr: bool,
}

const LABEL_WIDTH: usize = 13;

impl Ui {
    pub fn stdout() -> Self {
        Self { paint: Paint::stdout(), stderr: false }
    }

    pub fn stderr() -> Self {
        Self { paint: Paint::stderr(), stderr: true }
    }

    fn line(self, text: &str) {
        if self.stderr {
            eprintln!("{text}");
        } else {
            println!("{text}");
        }
    }

    pub fn blank(self) {
        self.line("");
    }

    /// `relay <command> · <context>`, then a blank line.
    pub fn heading(self, title: &str, context: &str) {
        self.line(&format!("{} {}", self.paint.bold(title), self.paint.dim(&format!("· {context}"))));
        self.blank();
    }

    /// The one line to read when nothing else is: bold, then a blank line.
    pub fn headline(self, text: &str) {
        self.line(&format!("  {}", self.paint.bold(text)));
        self.blank();
    }

    pub fn field(self, label: &str, value: &str) {
        self.line(&format!("  {label:<LABEL_WIDTH$} {value}"));
    }

    pub fn ok(self, text: &str) {
        self.line(&format!("{} {text}", self.paint.color(Color::Green, "✓")));
    }

    pub fn warn(self, text: &str) {
        self.line(&format!("{} {text}", self.paint.color(Color::Yellow, "!")));
    }

    pub fn fail(self, text: &str) {
        self.line(&format!("{} {text}", self.paint.color(Color::Red, "✗")));
    }

    pub fn next(self, step: &str) {
        self.line(&format!("{} {step}", self.paint.color(Color::Cyan, "→")));
    }

    pub fn note(self, text: &str) {
        self.line(&self.paint.dim(text));
    }
}

/// An error the user can act on: what went wrong in plain words, and what
/// to do about it.
#[derive(Debug)]
pub struct Problem {
    pub what: String,
    pub next: Option<String>,
}

impl fmt::Display for Problem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.what)
    }
}

impl std::error::Error for Problem {}

pub fn problem(what: impl Into<String>, next: impl Into<String>) -> anyhow::Error {
    Problem { what: what.into(), next: Some(next.into()) }.into()
}

/// How a failed command ends: the problem, its next step when known.
pub fn report_error(e: &anyhow::Error) {
    let ui = Ui::stderr();
    if let Some(p) = e.downcast_ref::<Problem>() {
        ui.fail(&p.what);
        if let Some(next) = &p.next {
            ui.next(next);
        }
    } else {
        ui.fail(&format!("{e:#}"));
        if let Some(next) = io_next_step(e) {
            ui.next(next);
        }
    }
}

/// A next step for the file-system failures a user can fix themselves.
fn io_next_step(e: &anyhow::Error) -> Option<&'static str> {
    let io = e.chain().find_map(|c| c.downcast_ref::<std::io::Error>())?;
    match io.kind() {
        std::io::ErrorKind::PermissionDenied => Some(
            "Check that you can write to this repo's .git directory; `relay status` shows where relay keeps its data.",
        ),
        std::io::ErrorKind::StorageFull => {
            Some("Free some disk space; `relay purge` shows what relay stores for this worktree.")
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn io_failures_a_user_can_fix_get_a_next_step() {
        let denied = anyhow::Error::from(std::io::Error::from(std::io::ErrorKind::PermissionDenied)).context("mkdir x");
        assert!(io_next_step(&denied).is_some_and(|s| s.contains(".git")));
        let other = anyhow::Error::from(std::io::Error::from(std::io::ErrorKind::InvalidData));
        assert_eq!(io_next_step(&other), None);
    }
}
