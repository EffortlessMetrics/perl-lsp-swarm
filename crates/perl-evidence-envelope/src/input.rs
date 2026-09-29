//! Lineage edges: references from one envelope to the receipts it consumed.
//!
//! [`InputReference`] records lineage as **data only** — a pair identifying
//! which upstream receipt was consumed and what its content looked like at
//! the time. This crate performs no cycle detection or graph traversal over
//! these edges; a consumer that needs to walk or validate the lineage graph
//! (and detect cycles in it) is a deliberately separate sibling concern.

use perl_source_identity::ContentDigest;
use serde::{Deserialize, Serialize};

use crate::receipt::ReceiptId;

/// One lineage edge: a reference to an upstream receipt this envelope
/// consumed as an input.
///
/// Two `InputReference` values are equal only when both the receipt identity
/// and the digest agree — the digest matters because the same receipt ID
/// could in principle be re-emitted with different content, and lineage
/// should distinguish "same receipt, same content" from "same receipt,
/// different content" (e.g. a corrected re-run).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InputReference {
    /// Identity of the upstream receipt consumed as an input.
    pub input_receipt_id: ReceiptId,
    /// Digest of the upstream receipt's content at the time it was consumed.
    pub input_digest: ContentDigest,
}

impl InputReference {
    /// Construct an input reference from its receipt identity and digest.
    #[must_use]
    pub fn new(input_receipt_id: ReceiptId, input_digest: ContentDigest) -> Self {
        Self { input_receipt_id, input_digest }
    }

    /// The canonical sort/hash key used to make a collection of
    /// [`InputReference`] values order-independent (see
    /// [`crate::EnvelopeFingerprint`]).
    #[must_use]
    pub(crate) fn canonical_key(&self) -> (&str, &str) {
        (self.input_receipt_id.as_wire(), self.input_digest.as_wire())
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    fn sample() -> InputReference {
        InputReference::new(
            ReceiptId::from_producer_run_and_key(
                &crate::producer::test_producer(),
                &crate::subject::test_run(),
                "upstream-run-1",
            ),
            ContentDigest::of_bytes(b"upstream bytes"),
        )
    }

    #[test]
    fn input_reference_serde_round_trip() {
        let original = sample();
        let json = serde_json::to_string(&original).expect("serialize");
        let back: InputReference = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(original, back);
    }

    #[test]
    fn different_receipt_ids_produce_different_references() {
        let a = sample();
        let b = InputReference::new(
            ReceiptId::from_producer_run_and_key(
                &crate::producer::test_producer(),
                &crate::subject::test_run(),
                "upstream-run-2",
            ),
            ContentDigest::of_bytes(b"upstream bytes"),
        );
        assert_ne!(a, b);
    }

    #[test]
    fn different_digests_produce_different_references() {
        let a = sample();
        let b = InputReference::new(
            ReceiptId::from_producer_run_and_key(
                &crate::producer::test_producer(),
                &crate::subject::test_run(),
                "upstream-run-1",
            ),
            ContentDigest::of_bytes(b"different bytes"),
        );
        assert_ne!(a, b);
    }
}
