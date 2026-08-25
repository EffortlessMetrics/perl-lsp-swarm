//! Deterministic canonical encoding and the hand-written stable hash.
//!
//! The fingerprint of a profile is FNV-1a 64 over a canonical byte stream in
//! which every semantic field is written exactly once under an explicit tag,
//! collections are walked in `BTree` order, and every variable-length item is
//! length-prefixed. `std`'s `DefaultHasher` is deliberately not used: it is
//! not stable across runs. Row insertion order therefore cannot influence
//! identity, while any change to any semantic field must.

/// FNV-1a 64-bit offset basis.
const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV-1a 64-bit prime.
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// Hand-written stable FNV-1a 64-bit hash over `bytes`.
pub(crate) fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash = FNV_OFFSET_BASIS;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

/// Length-prefixing canonical byte writer.
pub(crate) struct CanonWriter {
    buffer: Vec<u8>,
}

impl CanonWriter {
    /// New empty writer.
    pub(crate) const fn new() -> Self {
        Self { buffer: Vec::new() }
    }

    /// Writes one exact tag token (no payload).
    pub(crate) fn tag(&mut self, tag: &str) {
        self.write_length_prefixed(tag.as_bytes());
    }

    /// Writes `tag` plus a UTF-8 string payload.
    pub(crate) fn str_field(&mut self, tag: &str, value: &str) {
        self.tag(tag);
        self.write_length_prefixed(value.as_bytes());
    }

    /// Writes `tag` plus a little-endian `u64` payload.
    pub(crate) fn u64_field(&mut self, tag: &str, value: u64) {
        self.tag(tag);
        self.write_length_prefixed(&value.to_le_bytes());
    }

    fn write_length_prefixed(&mut self, bytes: &[u8]) {
        let len = bytes.len() as u64;
        self.buffer.extend_from_slice(&len.to_le_bytes());
        self.buffer.extend_from_slice(bytes);
    }

    /// Consumes the writer and hashes the accumulated canonical stream.
    pub(crate) fn finish_fingerprint(self) -> u64 {
        fnv1a64(&self.buffer)
    }
}

/// Types that participate in the profile's deterministic semantic fingerprint.
///
/// Implementations must write every semantic field exactly once; mutation
/// falsifiers in `xtask/tests/compiler_profile_contract.rs` fail when a field
/// kind is missing from its encoding.
pub(crate) trait CanonicalEncode {
    /// Appends this value's canonical form to `writer`.
    fn encode(&self, writer: &mut CanonWriter);
}
