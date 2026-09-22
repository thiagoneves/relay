//! How to act on an audit finding in Claude Code: which plugin, folder,
//! command or menu holds what the audit found.

use std::collections::BTreeMap;
use std::path::Path;

use crate::core::audit::{Finding, Kind, Report};
use crate::core::paths::tilde;

pub fn steps(f: &Finding, r: &Report, config: &Path) -> Vec<String> {
    if f.is_history() {
        return Vec::new();
    }
    match f.kind {
        Kind::Skills => skills(r, config),
        Kind::Mcp | Kind::ToolNames => mcp(r),
        Kind::Instructions => vec!["`/memory` in Claude Code opens it".into()],
        Kind::HookOutput | Kind::FailingHooks => {
            vec!["`/hooks` in Claude Code shows which settings file or plugin defines it".into()]
        }
        Kind::Agents => vec![format!(
            "Delete the ones you do not use from {} or .claude/agents/ (`/agents` lists them)",
            tilde(&config.join("agents"))
        )],
        Kind::AutoReview | Kind::Other => Vec::new(),
    }
}

/// Unused skills grouped by where they come from: a plugin (`plugin:skill`),
/// the user's skills folder, or Claude Code itself.
fn skills(r: &Report, config: &Path) -> Vec<String> {
    let mut plugins: BTreeMap<&str, usize> = BTreeMap::new();
    let mut own = Vec::new();
    let mut builtin = 0;
    for s in &r.skills_unused {
        if let Some((plugin, _)) = s.split_once(':') {
            *plugins.entry(plugin).or_default() += 1;
        } else if config.join("skills").join(s).exists() {
            own.push(s.as_str());
        } else {
            builtin += 1;
        }
    }
    let mut out = Vec::new();
    for (p, n) in plugins {
        out.push(format!("Plugin {p}: {n} not used in these sessions · `/plugin` → Installed → disable"));
    }
    if !own.is_empty() {
        out.push(format!(
            "{}: {} not used in these sessions ({}) · move out the ones you no longer need",
            tilde(&config.join("skills")),
            own.len(),
            preview(&own, 4)
        ));
    }
    if builtin > 0 {
        out.push(format!("{builtin} ship with Claude Code and cannot be removed"));
    }
    out
}

/// Servers whose tools were never called in the audited sessions.
fn mcp(r: &Report) -> Vec<String> {
    let used: Vec<String> = r
        .costs
        .iter()
        .filter_map(|c| c.source.strip_prefix("Tool results: MCP "))
        .map(|s| key(s.strip_prefix("plugin_").unwrap_or(s)))
        .collect();
    let (mut connectors, mut local) = (Vec::new(), Vec::new());
    for s in r.mcp_servers.iter().filter(|s| !used.iter().any(|u| u.contains(&key(s)) || key(s).contains(u))) {
        match s.strip_prefix("claude.ai ") {
            Some(name) => connectors.push(name),
            None => local.push(s.as_str()),
        }
    }
    let mut out = Vec::new();
    if !local.is_empty() {
        out.push(format!(
            "Unused here: {} · `claude mcp remove <name>`, or add it only in the projects that use it (`claude mcp add -s project`)",
            local.join(", ")
        ));
    }
    if !connectors.is_empty() {
        out.push(format!(
            "claude.ai connectors unused here: {} · turn them off in claude.ai → Settings → Connectors",
            connectors.join(", ")
        ));
    }
    out
}

fn key(s: &str) -> String {
    s.chars().filter(char::is_ascii_alphanumeric).collect::<String>().to_lowercase()
}

fn preview(items: &[&str], n: usize) -> String {
    let shown = items.iter().take(n).copied().collect::<Vec<_>>().join(", ");
    if items.len() > n { format!("{shown}, …") } else { shown }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::audit::findings::Status;
    use crate::core::audit::{Cost, Severity};

    fn finding(kind: Kind) -> Finding {
        Finding {
            severity: Severity::Waste,
            kind,
            status: Status::Active,
            headline: String::new(),
            subject: None,
            fix: String::new(),
            steps: Vec::new(),
            resent: 1,
            last_seen: None,
        }
    }

    #[test]
    fn groups_unused_skills_by_plugin() {
        let r = Report {
            skills_unused: vec!["ui:pro".into(), "ui:max".into(), "update-config".into()],
            ..Report::default()
        };
        let s = steps(&finding(Kind::Skills), &r, Path::new("/nonexistent"));
        assert_eq!(s[0], "Plugin ui: 2 not used in these sessions · `/plugin` → Installed → disable");
        assert!(s[1].starts_with("1 ship with Claude Code"));
    }

    #[test]
    fn names_only_mcp_servers_never_called() {
        let r = Report {
            mcp_servers: vec!["github".into(), "claude.ai Magnific".into(), "serena".into()],
            costs: vec![Cost { source: "Tool results: MCP serena".into(), ..Cost::default() }],
            ..Report::default()
        };
        let s = steps(&finding(Kind::Mcp), &r, Path::new("/x"));
        assert!(s[0].starts_with("Unused here: github ·"), "{s:?}");
        assert!(s[1].contains("Magnific"));
    }
}
