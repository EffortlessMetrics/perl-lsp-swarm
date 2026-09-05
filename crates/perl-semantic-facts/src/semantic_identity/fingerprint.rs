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
/// Independent second-lane basis (distinct odd constant) so the 128-bit
/// digest combines two decorrelated FNV-1a lanes.
const FNV_LANE_B_BASIS: u64 = 0x9e37_79b9_7f4a_7c15;

/// Incremental deterministic 128-bit fingerprint accumulator.
///
/// The digest is a deterministic comparison convenience, not the
/// authoritative identity: structural `Eq` on the full identity types
/// remains authoritative. Two independent 64-bit lanes make accidental
/// collisions negligible, and any residual collision can only make two
/// distinct identities compare fingerprint-equal — consumers must treat
/// fingerprint equality as a candidate match confirmed by structural
/// equality, never as proof of identity.
#[derive(Debug, Clone)]
pub struct SemanticIdentityFingerprint {
    lane_a: u64,
    lane_b: u64,
}

impl SemanticIdentityFingerprint {
    /// Start a fingerprint over the given schema tag.
    #[must_use]
    pub fn new(schema_tag: &str) -> Self {
        let mut acc = Self { lane_a: FNV_OFFSET_BASIS, lane_b: FNV_LANE_B_BASIS };
        acc.mix_str(schema_tag);
        acc
    }

    fn mix_byte(&mut self, byte: u8) {
        self.lane_a ^= u64::from(byte);
        self.lane_a = self.lane_a.wrapping_mul(FNV_PRIME);
        // Lane B mixes the complemented byte under a rotated accumulation so
        // the two lanes never agree by construction on differing inputs.
        self.lane_b ^= u64::from(!byte).rotate_left(17);
        self.lane_b = self.lane_b.wrapping_mul(FNV_PRIME);
    }

    fn mix_str(&mut self, value: &str) {
        // An actual-length prefix keeps concatenations unambiguous: content
        // cannot shift across a field boundary.
        for byte in u64::try_from(value.len()).unwrap_or(u64::MAX).to_be_bytes() {
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

    /// Render the accumulated fingerprint as lowercase hex (two lanes).
    #[must_use]
    pub fn finish(self) -> String {
        let mut out = String::with_capacity(32);
        for byte in self.lane_a.to_be_bytes() {
            let _ = write!(out, "{byte:02x}");
        }
        for byte in self.lane_b.to_be_bytes() {
            let _ = write!(out, "{byte:02x}");
        }
        out
    }
}
