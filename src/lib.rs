//! relay: context layer for coding agents. Compresses what goes in,
//! remembers what matters, lives in your repo.
//!
//! The binary is a thin shell over [`main`]. The compression engine is
//! also usable on its own through [`compress()`].

mod cli;
mod compress;
mod core;
mod harness;
mod helpers;
mod limits;
mod machine;

pub use cli::main;
pub use compress::{Compressed, Filter, compress};
