//! The hook dialect introduced by Claude Code and adopted by Codex: JSON
//! events on stdin and a `hooks.json`-style registration file. Adapters
//! that speak it only need to say where their file lives.

mod dev_tasks;
pub mod hook;
pub mod hooks_json;
mod permission;
pub mod policy;
mod record;
pub mod reply;
pub mod verify;
