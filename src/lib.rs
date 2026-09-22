//! relay: context layer for coding agents. Compresses what goes in,
//! remembers what matters, lives in your repo.
//!
//! The binary is a thin shell over [`run`]. The compression engine is
//! also usable on its own through [`compress()`].

mod cli;
mod compress;
mod core;
mod harness;
mod helpers;
mod limits;

pub use cli::run;
pub use compress::{Compressed, Filter, compress};
