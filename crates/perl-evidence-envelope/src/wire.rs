//! Shared wire-encoding primitives for this crate's own hashed identity types.
//!
//! `perl-source-identity` owns an equivalent set of helpers, but they are
//! private to that crate (`pub(crate)`), so [`ReceiptId`](crate::ReceiptId)
//! and [`EnvelopeFingerprint`](crate::EnvelopeFingerprint) — which need their
//! own domain tags distinct from anything `perl-source-identity` hashes —
//! carry a small local copy rather than depending on unexported internals.

/// Number of hex digits in a SHA-256 wire body.
pub(crate) const SHA256_HEX_LEN: usize = 64;

/// Returns `true` only for exactly 64 **lowercase** ASCII hex digits.
pub(crate) fn is_sha256_hex_body(s: &str) -> bool {
    s.len() == SHA256_HEX_LEN && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// Format 32 raw bytes as 64 lowercase hex digits.
pub(crate) fn bytes_to_wire_hex(bytes: &[u8; 32]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Encode a byte slice with a 4-byte big-endian length prefix, so that
/// concatenated fields cannot shift into one another (`["a","bc"]` and
/// `["ab","c"]` must hash differently).
pub(crate) fn length_prefixed(data: &[u8]) -> Vec<u8> {
    let len = (data.len() as u32).to_be_bytes();
    let mut out = Vec::with_capacity(4 + data.len());
    out.extend_from_slice(&len);
    out.extend_from_slice(data);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn length_prefixed_is_boundary_sensitive() {
        let mut a = length_prefixed(b"a");
        a.extend(length_prefixed(b"bc"));
        let mut b = length_prefixed(b"ab");
        b.extend(length_prefixed(b"c"));
        assert_ne!(a, b, "length prefixes must prevent field-boundary collisions");
    }

    #[test]
    fn is_sha256_hex_body_accepts_only_lowercase_64() {
        assert!(is_sha256_hex_body(&"a".repeat(64)));
        assert!(!is_sha256_hex_body(&"A".repeat(64)));
        assert!(!is_sha256_hex_body(&"a".repeat(63)));
        assert!(!is_sha256_hex_body(""));
    }
}
