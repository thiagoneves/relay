//! The handoff as markdown, from values only: no disk, no clock, no git.

use super::Tail;
use super::summary::Summary;
use crate::helpers::git::GitState;
use crate::helpers::text::cut_lines;
use crate::helpers::truncate_chars;
use crate::limits::handoff as cap;

pub struct Header<'a> {
    pub session: &'a str,
    pub reason: &'a str,
    pub ended: &'a str,
}

pub fn render(h: &Header, s: &Summary, t: &Tail, git: &GitState) -> String {
    let mut b = frontmatter(h, s, t, git);
    let branch = if git.branch.is_empty() { String::new() } else { format!("{} · ", git.branch) };
    b.push_str(&format!("# Handoff · {branch}{}\n\n", &h.ended[..10.min(h.ended.len())]));
    render_stop(&mut b, s, t);
    render_conversation(&mut b, s, t);
    render_work(&mut b, s);
    render_git(&mut b, git);
    b
}

/// OKF concept fields first (type, title, timestamp), then relay's own.
fn frontmatter(h: &Header, s: &Summary, t: &Tail, git: &GitState) -> String {
    let day = &h.ended[..10.min(h.ended.len())];
    let title =
        if git.branch.is_empty() { format!("Handoff · {day}") } else { format!("Handoff · {} · {day}", git.branch) };
    format!(
        "---\ntype: Handoff\ntitle: {}\ntimestamp: {}\nsession: {}\nharness: {}\nbranch: {}\nsha: {}\ndirty: {}\nstarted: {}\nended: {}\nreason: {}\n{}---\n\n",
        crate::helpers::frontmatter::quote(&title),
        h.ended,
        h.session,
        s.harness,
        git.branch,
        git.sha,
        git.dirty.len(),
        s.started,
        h.ended,
        h.reason,
        if t.headless { "headless: true\n" } else { "" }
    )
}

/// The transcript's last reply when there is one; else the truncated
/// reply the `Stop` hook recorded.
fn render_stop(b: &mut String, s: &Summary, t: &Tail) {
    if let Some(last) = t.replies.last() {
        b.push_str(&format!("## Where it stopped\n{}\n\n", excerpt(last, cap::STOPPED_CHARS)));
    } else if let Some(last) = &s.last_reply {
        b.push_str(&format!("## Last reply\n{}\n\n", truncate_chars(last, cap::LAST_REPLY_CHARS)));
    }
}

fn render_conversation(b: &mut String, s: &Summary, t: &Tail) {
    section(b, "Decisions", t.decisions.iter().map(|d| truncate_chars(d, cap::DECISION_CHARS)));
    let skip = s.prompts.len().saturating_sub(6);
    section(b, "Asked", s.prompts.iter().skip(skip).map(|p| truncate_chars(p, cap::PROMPT_CHARS)));
    let earlier = t.replies.len().saturating_sub(1);
    let shown = &t.replies[earlier.saturating_sub(cap::EARLIER_REPLIES)..earlier];
    section(b, "Earlier replies", shown.iter().map(|r| first_paragraph(r, cap::EARLIER_REPLY_CHARS)));
    if let Some(plan) = &t.plan {
        b.push_str(&format!("## Plan\n{}\n\n", excerpt(plan, cap::PLAN_CHARS)));
    }
}

fn render_work(b: &mut String, s: &Summary) {
    let files = s.files.iter().take(cap::FILES).map(|(f, n)| if *n > 1 { format!("{f} (×{n})") } else { f.clone() });
    section(b, "Files touched", files);
    section(b, "Read first", s.read_first.iter().cloned());
    section(b, "Remembered", s.remembered.iter().cloned());
    section(b, "Failing at end", s.failing.iter().cloned());
    section(b, "Commands", s.commands.iter().cloned());
}

/// Agent text inside the handoff: its headings become bold lines, so they
/// never read as handoff sections, and a cut never leaves a fence open.
fn excerpt(text: &str, max: usize) -> String {
    let flat: Vec<String> = text
        .trim()
        .lines()
        .map(|l| match l.trim_start_matches('#') {
            rest if rest.len() < l.len() && rest.starts_with(' ') => format!("**{}**", rest.trim()),
            _ => l.to_string(),
        })
        .collect();
    let (mut out, cut) = cut_lines(&flat.join("\n"), max);
    if !out.ends_with('\n') {
        out.push('\n');
    }
    if out.matches("```").count() % 2 == 1 {
        out.push_str("```\n");
    }
    if cut {
        out.push_str("…\n");
    }
    out.trim_end().to_string()
}

/// The opening of a reply, on one line.
fn first_paragraph(text: &str, max: usize) -> String {
    let para = text.trim().split("\n\n").next().unwrap_or("");
    truncate_chars(&para.split_whitespace().collect::<Vec<_>>().join(" "), max)
}

fn section(b: &mut String, title: &str, items: impl Iterator<Item = String>) {
    let mut items = items.peekable();
    if items.peek().is_none() {
        return;
    }
    b.push_str(&format!("## {title}\n"));
    for it in items {
        b.push_str(&format!("- {it}\n"));
    }
    b.push('\n');
}

fn render_git(b: &mut String, git: &GitState) {
    let branch = if git.branch.is_empty() { "(no branch)" } else { &git.branch };
    let sha = if git.sha.is_empty() { "(no commits)" } else { &git.sha };
    b.push_str(&format!("## Git\n{branch} @ {sha}"));
    if git.dirty.is_empty() {
        b.push_str(", clean\n");
        return;
    }
    b.push_str(&format!(", {} dirty:\n", git.dirty.len()));
    for d in git.dirty.iter().take(cap::DIRTY) {
        b.push_str(&format!("- {d}\n"));
    }
    if git.dirty.len() > cap::DIRTY {
        b.push_str(&format!("- … +{}\n", git.dirty.len() - cap::DIRTY));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn summary() -> Summary {
        Summary {
            started: "2026-09-22T09:00:00Z".into(),
            harness: "claude-code".into(),
            prompts: vec!["add retry".into()],
            last_reply: None,
            files: vec![("src/pay.rs".into(), 2)],
            read_first: vec![],
            remembered: vec![],
            commands: vec![],
            failing: vec![],
        }
    }

    #[test]
    fn renders_without_disk_or_clock() {
        let git = GitState { branch: "main".into(), sha: "abc1234".into(), dirty: vec![], user: None };
        let t = Tail { replies: vec!["Retry added.".into()], ..Tail::default() };
        let h = Header { session: "s1", reason: "exit", ended: "2026-09-22T10:00:00Z" };
        let out = render(&h, &summary(), &t, &git);
        assert!(
            out.starts_with(
                "---\ntype: Handoff\ntitle: \"Handoff · main · 2026-09-22\"\ntimestamp: 2026-09-22T10:00:00Z\nsession: s1\nharness: claude-code\nbranch: main\n"
            ),
            "{out}"
        );
        assert!(out.contains("# Handoff · main · 2026-09-22\n\n## Where it stopped\nRetry added.\n\n"), "{out}");
        assert!(out.contains("## Files touched\n- src/pay.rs (×2)\n"), "{out}");
        assert!(out.ends_with("## Git\nmain @ abc1234, clean\n"), "{out}");
    }

    #[test]
    fn agent_headings_never_read_as_sections() {
        let out = excerpt("## Done\nAll green.\n# Next\nship", 200);
        assert_eq!(out, "**Done**\nAll green.\n**Next**\nship");
    }

    #[test]
    fn a_cut_closes_an_open_code_fence() {
        let text = format!("Run:\n```\n{}\n```\nafter", "x".repeat(50));
        let out = excerpt(&text, 30);
        assert_eq!(out.matches("```").count(), 2, "{out}");
        assert!(out.ends_with('…'));
    }

    #[test]
    fn earlier_replies_are_their_opening_on_one_line() {
        assert_eq!(first_paragraph("Fixed the\nbuild.\n\nDetails follow.", 100), "Fixed the build.");
    }
}
