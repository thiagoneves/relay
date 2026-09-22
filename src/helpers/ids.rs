use super::time::now_millis;

/// Short unique id with a prefix, e.g. `o_18f3a2c9b1e_3f2`.
/// Uniqueness within one machine comes from millis + pid, which is
/// enough for a per-worktree store.
pub fn new_id(prefix: &str) -> String {
    format!("{}_{:x}_{:x}", prefix, now_millis(), std::process::id() & 0xfff)
}
