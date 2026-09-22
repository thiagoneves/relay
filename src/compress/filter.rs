//! The structured filters a command's output can go through, and what
//! each one does before the generic pipeline.

use super::{generic, git, listing, tests_runner};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Filter {
    GitStatus,
    GitDiff,
    GitLog,
    CargoTest,
    GoTest,
    Pytest,
    JsTest,
    /// Search results: grouped by file, never cut.
    Grep,
    LsLong,
    /// File contents the agent asked for: cleaned, never cut.
    Read,
    Generic,
}

impl Filter {
    pub fn name(self) -> &'static str {
        match self {
            Self::GitStatus => "git-status",
            Self::GitDiff => "git-diff",
            Self::GitLog => "git-log",
            Self::CargoTest => "cargo-test",
            Self::GoTest => "go-test",
            Self::Pytest => "pytest",
            Self::JsTest => "js-test",
            Self::Grep => "grep",
            Self::LsLong => "ls-long",
            Self::Read => "read",
            Self::Generic => "generic",
        }
    }

    pub(super) fn apply(self, raw: &str) -> String {
        match self {
            Self::Read => generic::apply_read(raw),
            Self::Grep => {
                let clean = generic::strip_ansi(raw);
                generic::apply_read(&generic::group_by_file(&clean).unwrap_or(clean))
            }
            _ => generic::apply(&self.structure(generic::strip_ansi(raw))),
        }
    }

    /// The filter's own pass over colour-free output.
    fn structure(self, clean: String) -> String {
        match self {
            Self::GitStatus => git::status(&clean),
            Self::GitDiff => git::diff(&clean),
            Self::GitLog => git::log(&clean),
            Self::CargoTest => tests_runner::cargo_test(&clean),
            Self::GoTest => tests_runner::go_test(&clean),
            Self::Pytest => tests_runner::pytest(&clean),
            Self::JsTest => tests_runner::js_test(&clean),
            Self::LsLong => listing::ls_long(&clean),
            Self::Read | Self::Grep | Self::Generic => clean,
        }
    }

    /// Whether every line of the output was asked for, so none may be cut.
    pub(super) fn keeps_every_line(self) -> bool {
        matches!(self, Self::Read | Self::Grep)
    }
}
