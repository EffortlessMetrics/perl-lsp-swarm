//! Deterministic fingerprints for semantic identities.
//!
//! Fingerprints are canonical FNV-1a digests over ordered field text. They are
//! deterministic under input/map-order permutation because every composite
//! contributor is folded in a fixed order, and no host path, wall clock, or
//! process-local counter participates.

use std::fmt::Write as _;

/// 64-bit FNV-1a offset basis.
const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
/// 64-bit FNV-1a prime.
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// Incremental deterministic fingerprint accumulator.
#[derive(Debug, Clone)]
pub struct SemanticIdentityFingerprint {
    hash: u64,
}

impl SemanticIdentityFingerprint {
    /// Start a fingerprint over the given schema tag.
    #[must_use]
    pub fn new(schema_tag: &str) -> Self {
        let mut acc = Self { hash: FNV_OFFSET_BASIS };
        acc.mix_str(schema_tag);
        acc
    }

    fn mix_byte(&mut self, byte: u8) {
        self.hash ^= u64::from(byte);
        self.hash = self.hash.wrapping_mul(FNV_PRIME);
    }

    fn mix_str(&mut self, value: &str) {
        // A length prefix keeps concatenations unambiguous.
        for byte in 0u64.to_be_bytes() {
            self.mix_byte(byte);
        }
        for byte in value.as_bytes().iter() {
            self.mix_byte(*byte);
        }
    }

    /// Mix one ordered labeled field into the fingerprint.
    #[must_use]
    pub fn field(mut self, label: &str, value: &str) -> Self {
        self.mix_str(label);
        self.mix_str(value);
        self
    }

    /// Mix one discriminant (enum tag) into the fingerprint.
    #[must_use]
    pub fn discriminant(self, label: &str, tag: &str) -> Self {
        self.field(label, tag)
    }

    /// Mix an ordered sequence of pre-canonicalized items. Callers that hold
    /// unordered collections must sort their canonical item text first.
    #[must_use]
    pub fn ordered_field(self, label: &str, ordered_values: &[String]) -> Self {
        let joined = ordered_values.join("\u{1f}");
        self.field(label, &joined)
    }

    /// Render the accumulated fingerprint as lowercase hex.
    #[must_use]
    pub fn finish(self) -> String {
        let mut out = String::with_capacity(16);
        for byte in self.hash.to_be_bytes() {
            let _ = write!(out, "{byte:02x}");
        }
        out
    }
}
