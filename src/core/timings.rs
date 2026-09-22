//! How long each hook took, so the promise that relay never slows the
//! harness down is a number `relay status` shows rather than a claim.
//! Measured inside relay, from reading the event to replying; process
//! start comes on top and is a few milliseconds.

use std::time::{Duration, SystemTime};

use crate::core::paths::Paths;
use crate::helpers::fs::append_capped;
use crate::helpers::{iso, now_iso};
use crate::limits::store::TIMINGS_BYTES;

/// Hook events grouped by the budget they must meet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Group {
    /// Runs on every tool call or prompt: must be instant.
    PerCall,
    /// Puts the brief in front of the model before the first reply.
    SessionStart,
    /// Builds the handoff after the session; the harness gives it seconds.
    SessionEnd,
}

impl Group {
    fn of(event: &str) -> Option<Self> {
        match event {
            "PreToolUse" | "PostToolUse" | "UserPromptSubmit" | "Stop" => Some(Self::PerCall),
            "SessionStart" => Some(Self::SessionStart),
            "SessionEnd" | "PreCompact" => Some(Self::SessionEnd),
            _ => None,
        }
    }

    pub fn budget(self) -> Duration {
        match self {
            Self::PerCall | Self::SessionStart => Duration::from_millis(50),
            Self::SessionEnd => Duration::from_secs(2),
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct Stat {
    pub group: Group,
    pub calls: usize,
    pub p95: Duration,
    pub max: Duration,
}

pub fn record(paths: &Paths, event: &str, took: Duration) {
    append_capped(&paths.timings_file(), &format!("{} {event} {}", now_iso(), took.as_micros()), TIMINGS_BYTES);
}

/// Hooks timed at or after `since`, per group, in `Group` order.
pub fn since(paths: &Paths, since: SystemTime) -> Vec<Stat> {
    let text = std::fs::read_to_string(paths.timings_file()).unwrap_or_default();
    summarize(&text, &iso(since))
}

fn summarize(text: &str, from: &str) -> Vec<Stat> {
    let mut samples: Vec<(Group, u64)> = text
        .lines()
        .filter_map(|l| {
            let mut parts = l.split(' ');
            let (at, event, micros) = (parts.next()?, parts.next()?, parts.next()?.parse().ok()?);
            (at >= from).then_some((Group::of(event)?, micros))
        })
        .collect();
    samples.sort_unstable();
    [Group::PerCall, Group::SessionStart, Group::SessionEnd]
        .into_iter()
        .filter_map(|g| stat(g, &samples.iter().filter(|(s, _)| *s == g).map(|(_, m)| *m).collect::<Vec<_>>()))
        .collect()
}

/// `micros` sorted ascending.
fn stat(group: Group, micros: &[u64]) -> Option<Stat> {
    let max = *micros.last()?;
    let p95 = micros[(micros.len() * 95).div_ceil(100).saturating_sub(1)];
    Some(Stat { group, calls: micros.len(), p95: Duration::from_micros(p95), max: Duration::from_micros(max) })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn p95_and_max_per_group_within_the_window() {
        let mut text = String::from("2026-09-01T00:00:00Z PreToolUse 900000\n");
        for i in 1..=100 {
            text.push_str(&format!("2026-09-22T10:00:00Z PostToolUse {}\n", i * 1000));
        }
        text.push_str("2026-09-22T10:00:00Z SessionEnd 300000\nnot a line\n");
        let stats = summarize(&text, "2026-09-15T00:00:00Z");
        assert_eq!(
            stats[0],
            Stat { group: Group::PerCall, calls: 100, p95: Duration::from_millis(95), max: Duration::from_millis(100) }
        );
        assert_eq!((stats[1].group, stats[1].calls), (Group::SessionEnd, 1));
        assert_eq!(stats.len(), 2, "the old PreToolUse is outside the window");
    }
}
