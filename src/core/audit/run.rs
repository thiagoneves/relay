//! One harness audited over a scope: pick its transcripts, read them
//! through the harness, and settle findings against today's config.

use std::path::Path;
use std::time::SystemTime;

use super::findings::Status;
use super::{Finding, Now, Report, SessionAudit, Transcript, newest, report, select};

/// What an audit needs from a harness. It lives here rather than on the
/// harness trait so `core` never depends on the adapters.
pub trait Source {
    fn transcripts(&self, root: Option<&Path>) -> Vec<Transcript>;
    fn audit_session(&self, transcript: &Path) -> Option<SessionAudit>;
    /// Hook commands in today's config, when the harness can tell.
    fn configured_hooks(&self, root: Option<&Path>) -> Option<Vec<String>>;
    /// Why `f` no longer applies, when today's config shows it.
    fn settled(&self, f: &Finding) -> Option<String>;
    /// Concrete steps to act on `f` in this harness.
    fn advise(&self, f: &Finding, r: &Report) -> Vec<String>;
}

pub struct Scope<'a> {
    /// The project; `None` audits every project.
    pub root: Option<&'a Path>,
    /// Only sessions started after this.
    pub since: Option<SystemTime>,
    /// Newest top-level sessions to read; their subagents come along.
    pub sessions: usize,
}

/// `None` when no session in scope could be read.
pub fn run(src: &dyn Source, scope: &Scope) -> Option<Report> {
    let all = src.transcripts(scope.root);
    // The newest session of the audited scope shows what its config loads
    // today. Another project's newest session would not list this
    // project's skills or MCP servers, and mark them removed.
    let newest = newest(&all);
    let recent: Vec<_> = all.into_iter().filter(|t| scope.since.is_none_or(|s| t.started >= s)).collect();
    let audits: Vec<_> = select(recent, scope.sessions).iter().filter_map(|t| read(src, t)).collect();
    if audits.is_empty() {
        return None;
    }
    let now = Now {
        hooks: src.configured_hooks(scope.root),
        newest: newest.as_ref().and_then(|t| src.audit_session(&t.path)),
        newest_started: newest.map(|t| t.started),
        root: scope.root.map(Path::to_path_buf),
    };
    let mut r = report(audits, &now);
    settle(src, &mut r);
    Some(r)
}

fn read(src: &dyn Source, t: &Transcript) -> Option<SessionAudit> {
    let mut a = src.audit_session(&t.path)?;
    a.seen = Some(t.modified);
    Some(a)
}

/// The harness's word on each finding: already fixed, and how to fix it.
fn settle(src: &dyn Source, r: &mut Report) {
    for f in &mut r.findings {
        if let Some(why) = src.settled(f) {
            f.status = Status::Gone;
            f.fix = format!("Already removed: {why}");
        }
    }
    let steps: Vec<Vec<String>> = r.findings.iter().map(|f| src.advise(f, r)).collect();
    for (f, s) in r.findings.iter_mut().zip(steps) {
        f.steps = s;
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::path::PathBuf;
    use std::time::{Duration, UNIX_EPOCH};

    use super::*;

    /// This project's session, and a newer one in another project.
    struct Fake {
        audited: RefCell<Vec<PathBuf>>,
    }

    fn t(name: &str, secs: u64) -> Transcript {
        let at = UNIX_EPOCH + Duration::from_secs(secs);
        Transcript { path: PathBuf::from(name), id: name.into(), parent: None, started: at, modified: at }
    }

    impl Source for Fake {
        fn transcripts(&self, root: Option<&Path>) -> Vec<Transcript> {
            match root {
                Some(_) => vec![t("this", 10)],
                None => vec![t("this", 10), t("other-project", 20)],
            }
        }
        fn audit_session(&self, path: &Path) -> Option<SessionAudit> {
            self.audited.borrow_mut().push(path.to_path_buf());
            Some(SessionAudit::default())
        }
        fn configured_hooks(&self, _: Option<&Path>) -> Option<Vec<String>> {
            None
        }
        fn settled(&self, _: &Finding) -> Option<String> {
            None
        }
        fn advise(&self, _: &Finding, _: &Report) -> Vec<String> {
            Vec::new()
        }
    }

    #[test]
    fn project_audit_judges_findings_against_its_own_newest_session() {
        let h = Fake { audited: RefCell::new(Vec::new()) };
        run(&h, &Scope { root: Some(Path::new("/repo")), since: None, sessions: 5 });
        assert!(!h.audited.borrow().contains(&PathBuf::from("other-project")), "{:?}", h.audited.borrow());
    }
}
