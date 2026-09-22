//! What a hook answers, apart from how a harness wants it written. The
//! hook decides a `Reply`; each adapter renders it in its own dialect.
//! `claude` is the dialect Claude Code introduced and Codex adopted.

use serde_json::{Value, json};

use super::policy::Rewritten;

#[derive(Debug, Clone, PartialEq)]
pub enum Reply {
    /// Nothing to add or change.
    Nothing,
    /// Text for the model's context (the brief at session start).
    Context(String),
    /// Run this command instead of the agent's.
    Rewrite(Rewritten),
    /// Show the model this tool response instead of the one it produced.
    ReplaceOutput(Value),
}

/// The reason a harness shows for a rewrite relay approved.
pub const APPROVED_BECAUSE: &str = "relay: a read or routine dev task";

pub fn claude(reply: &Reply) -> Option<String> {
    match reply {
        Reply::Nothing => None,
        // Plain stdout on SessionStart becomes context for the model.
        Reply::Context(text) => Some(text.clone()),
        Reply::Rewrite(r) => {
            let mut out = json!({ "hookEventName": "PreToolUse", "updatedInput": { "command": r.command } });
            if r.approve {
                out["permissionDecision"] = "allow".into();
                out["permissionDecisionReason"] = APPROVED_BECAUSE.into();
            }
            Some(json!({ "hookSpecificOutput": out }).to_string())
        }
        Reply::ReplaceOutput(updated) => Some(
            json!({ "hookSpecificOutput": { "hookEventName": "PostToolUse", "updatedToolOutput": updated } })
                .to_string(),
        ),
    }
}
