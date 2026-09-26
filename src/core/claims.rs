//! Live path claims: which session in this worktree is editing which
//! files, so parallel sessions in one checkout see each other before they
//! collide. Three sessions editing the same paths in one checkout once
//! spent about a million tokens on rebases and ports; a line in the brief
//! and a warning before the edit are cheaper.
//!
//! One JSON file per session under `<local>/claims/`, rewritten on each
//! edit the harness reports. No daemon and no lock: a claim is live while
//! its session edited within `limits::claims::LIVE` and has not ended, and
//! the file is deleted `limits::claims::KEEP` after its last edit.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::SystemTime;

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::core::paths::Paths;
use crate::core::spool;
use crate::helpers::{ago, iso, now_iso, parse_iso, truncate_chars, write_atomic};
use crate::limits::claims::{AREAS_SHOWN, BRIEF_CHARS, DIRTY_SHOWN, KEEP, LABEL_CHARS, LIVE};

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Claim {
    pub session: String,
    #[serde(default)]
    pub harness: String,
    /// The session's first ask, so a person recognises it.
    #[serde(default)]
    pub label: String,
    /// When the session last edited a file here.
    pub updated: String,
    #[serde(default)]
    pub ended: bool,
    /// Repo-relative path, and when this session last edited it.
    #[serde(default)]
    pub files: BTreeMap<String, String>,
    /// Paths this session was already warned about, so it hears it once.
    #[serde(default)]
    pub warned: Vec<String>,
}

impl Claim {
    pub fn is_live(&self, now: SystemTime) -> bool {
        !self.ended && self.updated >= iso(now.checked_sub(LIVE).unwrap_or(SystemTime::UNIX_EPOCH))
    }

    /// `1a2b3c4d (claude-code, "fix the filters")`
    pub fn name(&self) -> String {
        let mut s = short(&self.session).to_string();
        let mut about = Vec::new();
        if !self.harness.is_empty() {
            about.push(self.harness.clone());
        }
        if !self.label.is_empty() {
            about.push(format!("\"{}\"", self.label));
        }
        if !about.is_empty() {
            s.push_str(&format!(" ({})", about.join(", ")));
        }
        s
    }
}

fn short(session: &str) -> &str {
    &session[..8.min(session.len())]
}

fn dir(paths: &Paths) -> PathBuf {
    paths.local.join("claims")
}

fn file_for(paths: &Paths, session: &str) -> PathBuf {
    let safe: String =
        session.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect();
    dir(paths).join(format!("{safe}.json"))
}

fn load(paths: &Paths, session: &str) -> Option<Claim> {
    serde_json::from_str(&std::fs::read_to_string(file_for(paths, session)).ok()?).ok()
}

fn save(paths: &Paths, claim: &Claim) -> Result<()> {
    write_atomic(&file_for(paths, &claim.session), &serde_json::to_vec(claim)?)
}

/// A claim for `session`, new ones named from its spool: the harness and
/// the first thing it was asked.
fn load_or_new(paths: &Paths, session: &str) -> Claim {
    load(paths, session).unwrap_or_else(|| {
        let events = spool::read(paths, session);
        let field = |event: &str, key: &str| {
            events.iter().find(|e| e.event == event).and_then(|e| e.data[key].as_str()).unwrap_or("").to_string()
        };
        let label = field("prompt", "text").split_whitespace().collect::<Vec<_>>().join(" ");
        Claim {
            session: session.to_string(),
            harness: field("session_start", "harness"),
            label: truncate_chars(&label, LABEL_CHARS),
            updated: now_iso(),
            ..Claim::default()
        }
    })
}

/// Every claim still kept; older ones are deleted on the way.
pub fn all(paths: &Paths) -> Vec<Claim> {
    let Ok(rd) = std::fs::read_dir(dir(paths)) else { return Vec::new() };
    let cutoff = iso(SystemTime::now().checked_sub(KEEP).unwrap_or(SystemTime::UNIX_EPOCH));
    let mut out = Vec::new();
    for p in rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "json")) {
        match std::fs::read_to_string(&p).ok().and_then(|t| serde_json::from_str::<Claim>(&t).ok()) {
            Some(c) if c.updated >= cutoff => out.push(c),
            _ => {
                let _ = std::fs::remove_file(&p);
            }
        }
    }
    out.sort_by(|a, b| b.updated.cmp(&a.updated));
    out
}

/// `session` edited `file` (repo-relative) just now.
pub fn edited(paths: &Paths, session: &str, file: &str) -> Result<()> {
    let mut c = load_or_new(paths, session);
    let now = now_iso();
    c.files.insert(file.to_string(), now.clone());
    c.updated = now;
    c.ended = false;
    save(paths, &c)
}

/// The session ended: its paths are free, its uncommitted files still
/// carry its name.
pub fn end(paths: &Paths, session: &str) -> Result<()> {
    match load(paths, session) {
        Some(mut c) if !c.ended => {
            c.ended = true;
            save(paths, &c)
        }
        _ => Ok(()),
    }
}

/// Another live session that edited `file`, when `session` has not been
/// told about it yet; marks it told.
pub fn warn_once(paths: &Paths, session: &str, file: &str) -> Option<String> {
    let claims = all(paths);
    let (other, when) = conflict(&claims, session, file, SystemTime::now())?;
    let mut mine = load_or_new(paths, session);
    if mine.warned.iter().any(|w| w == file) {
        return None;
    }
    mine.warned.push(file.to_string());
    let _ = save(paths, &mine);
    Some(format!(
        "relay: session {} edited {file} {when} and is still working in this checkout. Re-read the file before \
         editing, keep to your own paths, or move this work to a worktree (`git worktree add`), or one of you \
         will spend the next hour on a rebase.",
        other.name()
    ))
}

/// The live claim other than `session`'s that edited `file` most
/// recently, and how long ago.
fn conflict<'a>(claims: &'a [Claim], session: &str, file: &str, now: SystemTime) -> Option<(&'a Claim, String)> {
    claims
        .iter()
        .filter(|c| c.session != session && c.is_live(now))
        .filter_map(|c| c.files.get(file).map(|t| (c, t)))
        .max_by(|a, b| a.1.cmp(b.1))
        .map(|(c, t)| (c, parse_iso(t).map_or_else(String::new, |t| ago(t, now))))
}

/// Paths grouped into areas: two or more files under the same first two
/// directories become `dir/**`, a lone file stays itself. Largest first.
pub fn areas(files: &[&str]) -> Vec<String> {
    let mut groups: BTreeMap<String, Vec<&str>> = BTreeMap::new();
    for f in files {
        let parts: Vec<&str> = f.split('/').collect();
        let key = parts[..parts.len().saturating_sub(1).min(2)].join("/");
        groups.entry(key).or_default().push(f);
    }
    let mut out: Vec<(usize, String)> = groups
        .into_iter()
        .flat_map(|(key, fs)| match fs.as_slice() {
            [one] => vec![(1, (*one).to_string())],
            _ if key.is_empty() => fs.iter().map(|f| (1, (*f).to_string())).collect(),
            _ => vec![(fs.len(), format!("{key}/**"))],
        })
        .collect();
    out.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    out.into_iter().map(|(_, a)| a).collect()
}

/// Each uncommitted file, under the session that edited it last.
fn owners<'a>(claims: &'a [Claim], dirty: &[String]) -> Vec<(&'a Claim, Vec<&'a str>)> {
    let mut by: BTreeMap<usize, Vec<&str>> = BTreeMap::new();
    for d in dirty {
        let owner = claims
            .iter()
            .enumerate()
            .filter_map(|(i, c)| c.files.get_key_value(d.as_str()).map(|(k, t)| (i, k.as_str(), t)))
            .max_by(|a, b| a.2.cmp(b.2));
        if let Some((i, k, _)) = owner {
            by.entry(i).or_default().push(k);
        }
    }
    by.into_iter().map(|(i, fs)| (&claims[i], fs)).collect()
}

/// The brief's section on the other sessions in this checkout: what each
/// live one is editing and which uncommitted files are whose. Empty when
/// there is nothing to say.
pub fn section(claims: &[Claim], dirty: &[String], me: Option<&str>, now: SystemTime) -> String {
    let others: Vec<Claim> = claims.iter().filter(|c| Some(c.session.as_str()) != me).cloned().collect();
    let owned = owners(&others, dirty);
    let mut lines = Vec::new();
    for c in &others {
        let dirty_here: Vec<&str> =
            owned.iter().find(|(o, _)| o.session == c.session).map(|(_, f)| f.clone()).unwrap_or_default();
        let active = c.is_live(now);
        if !active && dirty_here.is_empty() {
            continue;
        }
        let when = parse_iso(&c.updated).map_or_else(String::new, |t| ago(t, now));
        let mut line = if active {
            let files: Vec<&str> = c.files.keys().map(String::as_str).collect();
            format!("- {}, last edit {when}: editing {}", c.name(), listed(&areas(&files), AREAS_SHOWN))
        } else {
            format!("- {}, ended {when}", c.name())
        };
        if !dirty_here.is_empty() {
            let files: Vec<String> = dirty_here.iter().map(|s| (*s).to_string()).collect();
            line.push_str(&format!("; uncommitted: {}", listed(&files, DIRTY_SHOWN)));
        }
        lines.push(line);
    }
    if lines.is_empty() {
        return String::new();
    }
    let mut out = String::from("## Other sessions in this checkout\n");
    for l in lines {
        if out.len() + l.len() > BRIEF_CHARS {
            out.push_str("- …\n");
            break;
        }
        out.push_str(&l);
        out.push('\n');
    }
    out.push_str("_Leave their paths and uncommitted files alone, or work in a worktree._\n\n");
    out
}

fn listed(items: &[String], max: usize) -> String {
    let mut s = items.iter().take(max).cloned().collect::<Vec<_>>().join(", ");
    if items.len() > max {
        s.push_str(&format!(" (+{})", items.len() - max));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn claim(session: &str, updated: SystemTime, files: &[&str]) -> Claim {
        Claim {
            session: session.into(),
            harness: "claude-code".into(),
            label: "fix the filters".into(),
            updated: iso(updated),
            files: files.iter().map(|f| ((*f).to_string(), iso(updated))).collect(),
            ..Claim::default()
        }
    }

    #[test]
    fn files_group_into_areas() {
        let files = ["apps/backstage/a.tsx", "apps/backstage/src/b.tsx", "README.md", "src/x.rs"];
        assert_eq!(areas(&files), ["apps/backstage/**", "README.md", "src/x.rs"]);
        assert_eq!(areas(&["a.md", "b.md"]), ["a.md", "b.md"]);
    }

    #[test]
    fn only_a_live_other_session_conflicts() {
        let now = SystemTime::now();
        let claims = vec![
            claim("mine-1234", now, &["src/a.rs"]),
            claim("other-5678", now - Duration::from_secs(300), &["src/a.rs"]),
            claim("stale-9999", now - LIVE - Duration::from_secs(60), &["src/b.rs"]),
        ];
        let (c, when) = conflict(&claims, "mine-1234", "src/a.rs", now).unwrap();
        assert_eq!((c.session.as_str(), when.as_str()), ("other-5678", "5 min ago"));
        assert!(conflict(&claims, "mine-1234", "src/b.rs", now).is_none(), "stale claims do not block");
        let mut ended = claims[1].clone();
        ended.ended = true;
        assert!(conflict(&[ended], "mine-1234", "src/a.rs", now).is_none());
    }

    #[test]
    fn the_brief_names_live_sessions_and_who_owns_dirty_files() {
        let now = SystemTime::now();
        let mut done = claim("done-0000", now - Duration::from_secs(7200 * 3), &["docs/plan.md"]);
        done.ended = true;
        let claims = vec![
            claim("mine-1234", now, &["src/a.rs"]),
            claim("other-5678", now - Duration::from_secs(120), &["apps/backstage/a.tsx", "apps/backstage/b.tsx"]),
            done,
        ];
        let dirty = vec!["apps/backstage/a.tsx".into(), "docs/plan.md".into(), "src/a.rs".into()];
        let out = section(&claims, &dirty, Some("mine-1234"), now);
        assert!(
            out.contains(
                "- other-56 (claude-code, \"fix the filters\"), last edit 2 min ago: editing apps/backstage/**; uncommitted: apps/backstage/a.tsx\n"
            ),
            "{out}"
        );
        assert!(
            out.contains("- done-000 (claude-code, \"fix the filters\"), ended 6 h ago; uncommitted: docs/plan.md\n"),
            "{out}"
        );
        assert!(!out.contains("mine-123"), "{out}");
        assert_eq!(section(&claims[..1], &dirty, Some("mine-1234"), now), "");
    }
}
