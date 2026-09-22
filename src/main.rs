//! relay: context layer for coding agents.
//!
//! Layout
//! - `cli/`      thin command handlers
//! - `core/`     domain: storage tiers, output store, spool, exec, handoff, brief
//! - `compress/` output filters (generic + structured)
//! - `harness/`  one adapter per coding agent (claude, …)
//! - `helpers/`  small shared utilities

mod cli;
mod compress;
mod core;
mod harness;
mod helpers;

fn main() {
    match cli::run() {
        Ok(code) => std::process::exit(code),
        Err(e) => {
            eprintln!("relay: {e:#}");
            std::process::exit(1);
        }
    }
}
