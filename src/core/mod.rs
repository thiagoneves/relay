//! Domain layer: what relay is, independent of any harness or CLI.
//!
//! - `paths`    two-tier storage layout (`.relay/` shared, `<gitdir>/relay/` local)
//! - `outputs`  reversible output store (originals behind compressed views)
//! - `spool`    append-only event log written by hooks
//! - `exec`     the `relay x` primitive: run, compress, store
//! - `handoff`  rule-based session handoff
//! - `brief`    session-start brief
//! - `memory`   durable items in `.relay/` (rules, gotchas, decisions)
//! - `bootstrap` project.md seeded by rule from the repo

pub mod bootstrap;
pub mod brief;
pub mod exec;
pub mod handoff;
pub mod memory;
pub mod outputs;
pub mod paths;
pub mod spool;
