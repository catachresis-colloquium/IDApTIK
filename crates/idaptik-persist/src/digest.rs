//! Digest helper for content-addressed definitions.
//!
//! FNV-1a 64-bit. Chosen because it needs no dependency, is trivially
//! reimplementable in any language that later wants to read this store, and is
//! deterministic across platforms.
//!
//! It is **not** a cryptographic hash. It is used for two things only:
//!
//! * deduplication — the same definition stored once, however many checkpoints
//!   embed it;
//! * corruption detection — [`crate::Store::verify`] recomputes it and reports
//!   a mismatch.
//!
//! A collision would mean two different definitions share a blob. At the sizes
//! involved (one authored scenario per sim kind) that is not a practical
//! concern, and if it ever became one the fix is a wider digest, not a
//! redesign. Nothing in this crate treats the digest as a security boundary.

const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// FNV-1a 64 over `bytes`, as 16 lowercase hex digits.
pub fn digest_hex(bytes: &[u8]) -> String {
    let mut hash = FNV_OFFSET_BASIS;
    for &b in bytes {
        hash ^= u64::from(b);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    format!("{hash:016x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_is_the_offset_basis() {
        assert_eq!(digest_hex(b""), "cbf29ce484222325");
    }

    /// The published FNV-1a 64 test vector. If this ever fails, the digest
    /// changed and every blob key in every existing store is invalid.
    #[test]
    fn known_vector() {
        assert_eq!(digest_hex(b"a"), "af63dc4c8601ec8c");
        assert_eq!(digest_hex(b"foobar"), "85944171f73967e8");
    }

    #[test]
    fn is_length_sensitive() {
        assert_ne!(digest_hex(b"ab"), digest_hex(b"ba"));
        assert_ne!(digest_hex(b"a"), digest_hex(b"aa"));
    }
}
