//! Every size budget relay works within. Tuning relay means editing this
//! file; the modules that apply a budget never hard-code one.

/// What the model sees of a command's output.
pub mod compress {
    /// Longer lines are cut, with a marker.
    pub const MAX_LINE_CHARS: usize = 400;
    /// Output longer than this is cut to head + tail + signal lines; the
    /// original stays one `relay get` away. Sized on real agent history:
    /// 80 + 40 saves ~9% of all shell output tokens, the old 180 + 100
    /// about 2%.
    pub const MAX_LINES: usize = 150;
    pub const HEAD_LINES: usize = 80;
    pub const TAIL_LINES: usize = 40;
    /// Signal lines rescued from the omitted middle of a capped output.
    pub const MAX_RESCUED: usize = 40;
    /// Changed lines kept per file in a `git diff`.
    pub const DIFF_LINES_PER_FILE: usize = 80;
}

/// What relay keeps on disk.
pub mod store {
    use std::time::Duration;

    /// Shorter outputs are printed as is, without a footer, and not stored.
    pub const MIN_OUTPUT_BYTES: usize = 200;
    /// Originals older than this are deleted; handoffs cite recent ones.
    pub const KEEP_OUTPUTS: Duration = Duration::from_secs(30 * 24 * 3600);
    /// How far from the end a transcript tail read starts. `SessionEnd`
    /// has a 2 s budget and transcripts reach gigabytes; the handoff only
    /// needs the last turns.
    pub const TRANSCRIPT_TAIL_BYTES: u64 = 32 * 1024 * 1024;
    /// Hook timings are cut back to their newest half past this size.
    pub const TIMINGS_BYTES: u64 = 128 * 1024;
    /// The log is cut back to its newest half past this size.
    pub const LOG_BYTES: u64 = 256 * 1024;
    /// Longest prompt, reply and command a spool event records.
    pub const EVENT_PROMPT_CHARS: usize = 600;
    pub const EVENT_REPLY_CHARS: usize = 400;
    pub const EVENT_COMMAND_CHARS: usize = 300;
}

/// The `SessionStart` brief, roughly 600 tokens in all.
pub mod brief {
    pub const MAX_CHARS: usize = 2400;
    pub const PROJECT_CHARS: usize = 1000;
    pub const MEMORY_CHARS: usize = 700;
    /// Distinct commits checked for stale items: one git call each.
    pub const STALE_COMMITS: usize = 8;
}

/// The handoff a session leaves for the next one.
pub mod handoff {
    pub const COMMANDS: usize = 12;
    pub const FAILING: usize = 5;
    /// Files read for orientation before the first edit.
    pub const READ_FIRST: usize = 8;
    /// Closing messages kept from the transcript, newest last.
    pub const REPLIES: usize = 8;
    /// Of those, how many show besides the last one.
    pub const EARLIER_REPLIES: usize = 3;
    pub const DECISIONS: usize = 8;
    pub const FILES: usize = 15;
    /// Uncommitted files listed.
    pub const DIRTY: usize = 10;
    /// The "where it stopped" section.
    pub const STOPPED_CHARS: usize = 1200;
    /// The last reply, when no transcript tail is available.
    pub const LAST_REPLY_CHARS: usize = 400;
    pub const EARLIER_REPLY_CHARS: usize = 240;
    pub const PLAN_CHARS: usize = 800;
    pub const DECISION_CHARS: usize = 200;
    pub const PROMPT_CHARS: usize = 220;
    pub const COMMAND_CHARS: usize = 80;
}

/// `relay status` and `relay log`.
pub mod status {
    use std::time::Duration;

    /// How far back `relay status` counts failures.
    pub const FAILURE_WINDOW: Duration = Duration::from_secs(7 * 24 * 3600);
    /// Lines `relay log` shows by default.
    pub const LOG_LINES: usize = 20;
}

/// `relay audit`.
pub mod audit {
    /// Sources below this share of all context sent are not a finding.
    pub const MIN_SHARE: f64 = 0.005;
    pub const SOURCES_SHOWN: usize = 12;
    /// Width of the context bar, in characters.
    pub const BAR_WIDTH: usize = 40;
}

/// `relay bench`.
pub mod bench {
    pub const WORST_SHOWN: usize = 5;
    pub const FAMILIES_SHOWN: usize = 20;
}
