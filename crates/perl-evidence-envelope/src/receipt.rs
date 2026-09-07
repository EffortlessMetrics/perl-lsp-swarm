//! Durable identity for one evidence receipt.
//!
//! [`ReceiptId`] names *this* evidence envelope instance so that downstream
//! consumers (a registry, a lineage graph, a freshness rule) can refer to one
//! receipt without re-deriving or re-serializing its full contents. It is a
//! domain-separated SHA-256 digest of a caller-supplied canonical key, using
//! the same wire discipline as `perl-source-identity`'s ID newtypes.
//!
//! `ReceiptId` intentionally does not derive its value from the envelope's
//! contents: a receipt may be re-minted for evidence that reproduces
//! byte-identical payload bytes, and "which receipt is this" is a different
//! question from "is this the same payload".
//!
//! Note the direction of the relationship, which is easy to get backwards:
//! `receipt_id` is an **input to** [`crate::EnvelopeFingerprint`], not an
//! alternative to it. The fingerprint identifies the whole envelope record
//! including this ID, so two receipts minted for identical payload bytes have
//! different fingerprints. Payload sameness is answered by
//! [`PayloadIdentity::digest`](crate::PayloadIdentity), which is stable across
//! re-mints.

use serde::{Deserialize, Deserializer, Serialize};
use sha2::{Digest as _, Sha256};

use crate::producer::ProducerIdentity;
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
    /// Derive a receipt ID from the minting producer and a producer-local key.
    ///
    /// The `canonical_key` must be a stable identifier for this specific
    /// receipt-minting event, but it only has to be unique **within** the
    /// given producer: a monotonic producer-local counter or a per-run
    /// sequence number is a valid key. It must not encode host paths or other
    /// non-portable state.
    ///
    /// # Why the producer is part of the derivation
    ///
    /// Hashing the key alone would make producer-local keys collide across
    /// producers: two independent producers that each mint their first receipt
    /// as `"1"` would derive one durable ID, and a registry or lineage graph
    /// would silently conflate two unrelated receipts. Because a
    /// producer-local counter is exactly the kind of key this constructor
    /// invites, the namespace has to come from the derivation rather than from
    /// producer discipline.
    ///
    /// All four producer fields participate, each length-prefixed, so
    /// producers differing only in version or build identity still mint
    /// distinct IDs. This mirrors `perl-source-identity`, where
    /// `WorkspaceRootId` is derived from its `ProjectId` plus a root key
    /// rather than from the root key alone.
    #[must_use]
    pub fn from_producer_and_key(producer: &ProducerIdentity, canonical_key: &str) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(RECEIPT_ID_DOMAIN);
        hasher.update(length_prefixed(producer.name.as_bytes()));
        hasher.update(length_prefixed(producer.version.as_bytes()));
        hasher.update(length_prefixed(producer.source_sha.as_bytes()));
        hasher.update(length_prefixed(producer.build_identity.as_bytes()));
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
        let a = ReceiptId::from_producer_and_key(&crate::producer::test_producer(), "run-42");
        let b = ReceiptId::from_producer_and_key(&crate::producer::test_producer(), "run-42");
        assert_eq!(a, b);
    }

    #[test]
    fn different_keys_produce_different_ids() {
        let a = ReceiptId::from_producer_and_key(&crate::producer::test_producer(), "run-42");
        let b = ReceiptId::from_producer_and_key(&crate::producer::test_producer(), "run-43");
        assert_ne!(a, b);
    }

    /// The defect this constructor's shape exists to prevent: two producers
    /// that each mint their first receipt with a producer-local key must not
    /// derive one durable ID. A derivation that hashed the key alone would
    /// fail this.
    #[test]
    fn equal_local_keys_under_different_producers_do_not_collide() {
        let a = ProducerIdentity::new("producer-a", "1.0.0", "sha-a", "build-a");
        let b = ProducerIdentity::new("producer-b", "1.0.0", "sha-b", "build-b");
        assert_ne!(
            ReceiptId::from_producer_and_key(&a, "1"),
            ReceiptId::from_producer_and_key(&b, "1"),
            "producer-local keys must be namespaced by their producer"
        );
    }

    /// Every producer field participates, so producers differing only in
    /// version or build identity still mint distinct IDs.
    #[test]
    fn each_producer_field_changes_the_receipt_id() {
        let base = ProducerIdentity::new("p", "1.0.0", "sha", "build");
        let id = ReceiptId::from_producer_and_key(&base, "k");
        for other in [
            ProducerIdentity::new("p2", "1.0.0", "sha", "build"),
            ProducerIdentity::new("p", "1.0.1", "sha", "build"),
            ProducerIdentity::new("p", "1.0.0", "sha2", "build"),
            ProducerIdentity::new("p", "1.0.0", "sha", "build2"),
        ] {
            assert_ne!(
                id,
                ReceiptId::from_producer_and_key(&other, "k"),
                "a changed producer field must change the receipt ID"
            );
        }
    }

    /// Length prefixing must keep producer/key boundaries from shifting:
    /// ("ab", "c") and ("a", "bc") are different producers and keys.
    #[test]
    fn producer_and_key_boundaries_do_not_shift() {
        let p1 = ProducerIdentity::new("ab", "", "", "");
        let p2 = ProducerIdentity::new("a", "", "", "");
        assert_ne!(
            ReceiptId::from_producer_and_key(&p1, "c"),
            ReceiptId::from_producer_and_key(&p2, "bc"),
            "field boundaries must not be ambiguous"
        );
    }

    #[test]
    fn receipt_id_wire_has_prefix() {
        let id = ReceiptId::from_producer_and_key(&crate::producer::test_producer(), "run-42");
        assert!(id.as_wire().starts_with("receipt:sha256:"));
    }

    #[test]
    fn receipt_id_display_matches_wire() {
        let id = ReceiptId::from_producer_and_key(&crate::producer::test_producer(), "run-42");
        assert_eq!(format!("{id}"), id.as_wire());
    }

    #[test]
    fn receipt_id_wire_round_trips() {
        let id = ReceiptId::from_producer_and_key(&crate::producer::test_producer(), "run-42");
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
        let good = ReceiptId::from_producer_and_key(&crate::producer::test_producer(), "run-42");
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
