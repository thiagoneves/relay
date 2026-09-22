//! Small shared helpers, one concern per file. Nothing here knows about
//! relay's storage layout or commands.

pub mod fs;
pub mod git;
pub mod ids;
pub mod text;
pub mod time;

pub use fs::{dir_size, write_atomic};
pub use ids::new_id;
pub use text::{est_tokens, human_bytes, human_tokens, truncate_chars};
pub use time::{now_iso, now_millis};
