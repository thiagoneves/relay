//! relay: context layer for coding agents.

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
