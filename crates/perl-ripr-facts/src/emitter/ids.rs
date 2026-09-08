//! ID/digest recipes shared across the fact emitters.
//!
//! Every deterministic id/hash format the packet uses lives here once, so
//! [`owner_fact_id`] can never drift between the emitter that assigns an
//! `owner_id` ([`super::owners::emit_files_and_owners`]) and the emitters that
//! reconstruct/reference it ([`super::relations`], [`super::boundaries`]).

use sha2::{Digest, Sha256};

/// Build the canonical `owners[]` `owner_id` string for a declaration. The
/// single source of truth for the id shape, shared by `emit_files_and_owners`
/// (which emits the `owner` facts) and `resolve_package_owner_id` (which
/// rebuilds a package owner's id so a `relation` can reference it) — so the two
/// can never drift into the dangling cross-reference #3342 corrected.
pub(crate) fn owner_fact_id(
    relative_path: &str,
    kind: &str,
    qualified_name: &str,
    start_byte: usize,
    end_byte: usize,
) -> String {
    format!("owner:{relative_path}:{kind}:{qualified_name}:{start_byte}-{end_byte}")
}

/// Map a content_hash (u64) to a hex digest string for the packet.
pub(crate) fn content_hash_to_digest(hash: u64) -> String {
    format!("fnv64:{hash:016x}")
}

/// Simple FNV-1a hash for deterministic digests.
pub(crate) fn fnv1a_hash(text: &str) -> u64 {
    const FNV_OFFSET: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x100000001b3;
    let mut hash = FNV_OFFSET;
    for byte in text.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

pub(crate) fn content_sha256_digest(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    let hex: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
    format!("sha256:{hex}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_hash_to_digest_formats_hex() {
        assert_eq!(content_hash_to_digest(0), "fnv64:0000000000000000");
        assert_eq!(content_hash_to_digest(255), "fnv64:00000000000000ff");
        assert_eq!(content_hash_to_digest(0xcbf29ce484222325), "fnv64:cbf29ce484222325");
    }

    #[test]
    fn fnv1a_hash_is_deterministic() {
        assert_eq!(fnv1a_hash("hello"), fnv1a_hash("hello"));
        assert_ne!(fnv1a_hash("hello"), fnv1a_hash("world"));
    }
}
