//! A content hash for keys on disk: FNV-1a, 64 bits. Stable across Rust
//! versions and platforms (std's hasher is neither), and fast enough for
//! a hook; collisions only cost a missed saving, never a wrong answer
//! that cannot be undone, since every original stays on disk.

pub fn fnv64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// `fnv64` as 16 hex digits, for file names.
pub fn content_hash(bytes: &[u8]) -> String {
    format!("{:016x}", fnv64(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_the_reference_values() {
        assert_eq!(content_hash(b""), "cbf29ce484222325");
        assert_eq!(content_hash(b"a"), "af63dc4c8601ec8c");
        assert_ne!(content_hash(b"ab"), content_hash(b"ba"));
    }
}
