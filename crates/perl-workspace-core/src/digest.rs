//! Deterministic content digests.
//!
//! [`SourceDigest`] is a fixed, dependency-free FNV-1a 64-bit digest of source
//! bytes. It is used for content identity and change detection — deriving
//! stable IDs and detecting whether a file's content has changed between
//! indexing runs.
//!
//! It is deliberately **not** cryptographic. Do not use it for security
//! decisions or where an adversary controls the input and could seek
//! collisions. The algorithm is pinned (FNV-1a 64) so digests are stable and
//! reproducible across builds, platforms, and toolchain versions; swapping in a
//! cryptographic hash later is a localised change behind this type.

use serde::{Deserialize, Serialize};

const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// A deterministic, non-cryptographic 64-bit content digest (FNV-1a).
///
/// Two byte slices with identical content always produce equal digests, on any
/// platform and toolchain. The [`core::fmt::Display`] form is a fixed
/// 16-character lowercase hex string prefixed with `fnv1a64:` so the algorithm
/// is self-describing on the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct SourceDigest(u64);

impl SourceDigest {
    /// Compute the digest of `bytes`.
    #[must_use]
    pub fn of_bytes(bytes: &[u8]) -> Self {
        let mut hash = FNV_OFFSET_BASIS;
        for &byte in bytes {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(FNV_PRIME);
        }
        Self(hash)
    }

    /// Compute the digest of `text`'s UTF-8 bytes.
    #[must_use]
    pub fn of_str(text: &str) -> Self {
        Self::of_bytes(text.as_bytes())
    }

    /// The raw 64-bit digest value.
    #[must_use]
    pub fn value(&self) -> u64 {
        self.0
    }

    /// The self-describing wire form, e.g. `fnv1a64:0a1b2c3d4e5f6071`.
    #[must_use]
    pub fn to_hex(&self) -> String {
        format!("fnv1a64:{:016x}", self.0)
    }
}

impl core::fmt::Display for SourceDigest {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "fnv1a64:{:016x}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digest_is_deterministic() {
        let a = SourceDigest::of_str("package Foo;\n1;\n");
        let b = SourceDigest::of_str("package Foo;\n1;\n");
        assert_eq!(a, b);
    }

    #[test]
    fn different_content_differs() {
        let a = SourceDigest::of_str("package Foo;");
        let b = SourceDigest::of_str("package Bar;");
        assert_ne!(a, b);
    }

    #[test]
    fn empty_input_is_offset_basis() {
        assert_eq!(SourceDigest::of_bytes(&[]).value(), FNV_OFFSET_BASIS);
    }

    #[test]
    fn known_vector_ascii_a() {
        // FNV-1a 64 of the single byte 'a' (0x61) is a fixed, well-known value.
        // 0xcbf29ce484222325 ^ 0x61 = 0xcbf29ce484222344; * FNV_PRIME =>
        assert_eq!(SourceDigest::of_bytes(b"a").value(), 0xaf63_dc4c_8601_ec8c);
    }

    #[test]
    fn hex_form_is_stable_and_prefixed() {
        let d = SourceDigest::of_bytes(b"a");
        assert_eq!(d.to_hex(), "fnv1a64:af63dc4c8601ec8c");
        assert_eq!(d.to_string(), "fnv1a64:af63dc4c8601ec8c");
    }
}
