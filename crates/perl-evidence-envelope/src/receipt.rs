//! Durable identity for one evidence receipt.
//!
//! [`ReceiptId`] names *this* evidence envelope instance so that downstream
//! consumers (a registry, a lineage graph, a freshness rule) can refer to one
//! receipt without re-deriving or re-serializing its full contents. It is a
//! domain-separated SHA-256 digest of a caller-supplied canonical key, using
//! the same wire discipline as `perl-source-identity`'s ID newtypes.
//!
//! `ReceiptId` intentionally does not derive its value from the envelope's
//! own [`crate::EnvelopeFingerprint`]: a receipt may be re-minted (new
//! `ReceiptId`) for the same underlying fingerprint (e.g. a re-run that
//! reproduces byte-identical evidence), and the two identities answer
//! different questions — "which receipt is this" versus "is this evidence
//! identical to that evidence".

use serde::{Deserialize, Deserializer, Serialize};
use sha2::{Digest as _, Sha256};

use crate::wire::{bytes_to_wire_hex, is_sha256_hex_body, length_prefixed};

/// Domain separator for [`ReceiptId`] derivation.
const RECEIPT_ID_DOMAIN: &[u8] = b"perl-lsp:evidence-receipt-id:v1\0";

/// Wire prefix for [`ReceiptId`].
const RECEIPT_ID_PREFIX: &str = "receipt:sha256:";

/// Durable identity for one evidence receipt (one envelope instance).
///
/// # Wire format
///
/// `receipt:sha256:<64 lowercase hex digits>`. Uppercase hex and any other
/// prefix are rejected rather than normalized, matching the wire discipline
/// used throughout `perl-source-identity`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(transparent)]
pub struct ReceiptId(String);

impl ReceiptId {
    /// Derive a receipt ID from a caller-supplied canonical key.
    ///
    /// The `canonical_key` must be a stable, producer-defined identifier for
    /// this specific receipt-minting event (e.g. a UUID, a monotonic
    /// producer-local counter rendered as a string, or a composite of
    /// producer name and emission timestamp). It must not encode host paths
    /// or other non-portable state.
    #[must_use]
    pub fn from_canonical_key(canonical_key: &str) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(RECEIPT_ID_DOMAIN);
        hasher.update(length_prefixed(canonical_key.as_bytes()));
        let raw: [u8; 32] = hasher.finalize().into();
        Self(format!("{RECEIPT_ID_PREFIX}{}", bytes_to_wire_hex(&raw)))
    }

    /// Parse a receipt ID from its wire representation.
    ///
    /// Returns `None` unless the string is exactly
    /// `receipt:sha256:<64 lowercase hex digits>`.
    #[must_use]
    pub fn from_wire(s: &str) -> Option<Self> {
        s.strip_prefix(RECEIPT_ID_PREFIX)
            .filter(|body| is_sha256_hex_body(body))
            .map(|_| Self(s.to_owned()))
    }

    /// The wire representation, e.g. `receipt:sha256:abc123...`.
    #[must_use]
    pub fn as_wire(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for ReceiptId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        Self::from_wire(&raw).ok_or_else(|| {
            serde::de::Error::invalid_value(
                serde::de::Unexpected::Str(&raw),
                &"a receipt ID of the form `receipt:sha256:<64 lowercase hex digits>`",
            )
        })
    }
}

impl std::fmt::Display for ReceiptId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    #[test]
    fn receipt_id_is_deterministic() {
        let a = ReceiptId::from_canonical_key("run-42");
        let b = ReceiptId::from_canonical_key("run-42");
        assert_eq!(a, b);
    }

    #[test]
    fn different_keys_produce_different_ids() {
        let a = ReceiptId::from_canonical_key("run-42");
        let b = ReceiptId::from_canonical_key("run-43");
        assert_ne!(a, b);
    }

    #[test]
    fn receipt_id_wire_has_prefix() {
        let id = ReceiptId::from_canonical_key("run-42");
        assert!(id.as_wire().starts_with("receipt:sha256:"));
    }

    #[test]
    fn receipt_id_display_matches_wire() {
        let id = ReceiptId::from_canonical_key("run-42");
        assert_eq!(format!("{id}"), id.as_wire());
    }

    #[test]
    fn receipt_id_wire_round_trips() {
        let id = ReceiptId::from_canonical_key("run-42");
        assert_eq!(ReceiptId::from_wire(id.as_wire()).as_ref(), Some(&id));
    }

    #[test]
    fn receipt_id_rejects_malformed_wire() {
        assert!(ReceiptId::from_wire("sha256:short").is_none());
        assert!(ReceiptId::from_wire("").is_none());
        assert!(
            ReceiptId::from_wire(&format!("receipt:sha256:{}", "A".repeat(64))).is_none(),
            "uppercase hex must be rejected, not normalized"
        );
    }

    #[test]
    fn receipt_id_deserialization_is_validating() {
        let good = ReceiptId::from_canonical_key("run-42");
        let json = serde_json::to_string(&good).expect("serialize");
        let back: ReceiptId = serde_json::from_str(&json).expect("valid wire must parse");
        assert_eq!(good, back);

        for bad in ["\"not-a-receipt\"", "\"receipt:sha256:short\"", "\"\""] {
            assert!(
                serde_json::from_str::<ReceiptId>(bad).is_err(),
                "deserialization must reject {bad}"
            );
        }
    }
}
