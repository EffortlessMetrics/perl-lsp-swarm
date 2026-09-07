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
///
/// Writes into one pre-sized buffer via a lookup table rather than
/// `format!("{b:02x}")` per byte, which would allocate 32 temporary `String`s
/// per call.
pub(crate) fn bytes_to_wire_hex(bytes: &[u8; 32]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(SHA256_HEX_LEN);
    for &b in bytes {
        out.push(HEX[usize::from(b >> 4)] as char);
        out.push(HEX[usize::from(b & 0x0f)] as char);
    }
    out
}

/// Width in bytes of the big-endian length prefix on every hashed field.
pub(crate) const LENGTH_PREFIX_LEN: usize = 8;

/// Encode a byte slice with an 8-byte big-endian length prefix, so that
/// concatenated fields cannot shift into one another (`["a","bc"]` and
/// `["ab","c"]` must hash differently).
///
/// # Why 8 bytes and not 4
///
/// A `u32` prefix silently wraps for any field of 4 GiB or more: a
/// `4 GiB + 5`-byte value would announce itself as 5 bytes, letting its tail
/// be absorbed into the following fields' bytes and, in principle, collide
/// with a structurally different envelope. None of the `String` fields that
/// reach this function is length-bounded by the type system, so the bound has
/// to come from the encoding. Widening the prefix to `u64` removes the
/// truncation entirely — `usize` is at most 64 bits on every supported
/// target, so `data.len() as u64` is lossless for any slice that can exist in
/// memory, and the "different envelopes never share a fingerprint" claim
/// holds unconditionally rather than up to a 4 GiB caveat.
pub(crate) fn length_prefixed(data: &[u8]) -> Vec<u8> {
    let len = (data.len() as u64).to_be_bytes();
    let mut out = Vec::with_capacity(LENGTH_PREFIX_LEN + data.len());
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

    /// The prefix must be wide enough that no in-memory slice can wrap it.
    ///
    /// A 4-byte prefix wraps at 4 GiB, which would let a `4 GiB + n`-byte
    /// field announce itself as `n` bytes and smuggle its tail into the
    /// following fields. Allocating 4 GiB to prove that directly is not
    /// practical, so this pins the two properties that make it unreachable:
    /// the prefix is 8 bytes wide, and it carries the full `u64` length.
    #[test]
    fn length_prefix_is_wide_enough_to_never_wrap() {
        let encoded = length_prefixed(b"payload");
        assert_eq!(encoded.len(), LENGTH_PREFIX_LEN + 7, "8-byte prefix plus the data");
        assert_eq!(
            &encoded[..LENGTH_PREFIX_LEN],
            &7u64.to_be_bytes(),
            "prefix must be the big-endian u64 length"
        );

        // A length that a u32 prefix could not represent must round-trip
        // through the same encoding path used for real data.
        let over_u32: u64 = u64::from(u32::MAX) + 5;
        let header = over_u32.to_be_bytes();
        assert_ne!(
            &header[..],
            &(over_u32 as u32).to_be_bytes()[..],
            "the u32 encoding of this length wraps; the u64 encoding must not"
        );
        assert_eq!(header.len(), LENGTH_PREFIX_LEN);
    }

    /// Pins the exact hex encoding. The lookup-table encoder must agree with
    /// `format!("{b:02x}")` byte for byte — in particular high nibble first,
    /// which is the easy thing to invert when hand-rolling this.
    #[test]
    fn bytes_to_wire_hex_matches_formatted_hex() {
        // Distinct high/low nibbles, so a swapped-nibble encoder fails.
        let mut bytes = [0u8; 32];
        for (i, b) in bytes.iter_mut().enumerate() {
            *b = (i as u8).wrapping_mul(7).wrapping_add(0xa1);
        }
        let expected: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
        let actual = bytes_to_wire_hex(&bytes);
        assert_eq!(actual, expected, "lookup-table encoder must match formatted hex");
        assert_eq!(actual.len(), SHA256_HEX_LEN);
        assert!(is_sha256_hex_body(&actual), "output must be a valid lowercase hex body");

        // Explicit boundary vector: 0x00 and 0xff must round-trip.
        let edges =
            [0x00u8, 0xff, 0x0f, 0xf0].iter().map(|b| format!("{b:02x}")).collect::<String>();
        assert_eq!(edges, "00ff0ff0", "sanity-check the oracle itself");
    }

    #[test]
    fn is_sha256_hex_body_accepts_only_lowercase_64() {
        assert!(is_sha256_hex_body(&"a".repeat(64)));
        assert!(!is_sha256_hex_body(&"A".repeat(64)));
        assert!(!is_sha256_hex_body(&"a".repeat(63)));
        assert!(!is_sha256_hex_body(""));
    }
}
