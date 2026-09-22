//! The hook dialect introduced by Claude Code and adopted by Codex:
//! JSON event on stdin, `hooks.json`-style registration file. Adapters
//! that speak it only need to say where their file lives.

pub mod hook;
pub mod hooks_json;
pub mod permission;
