//! Identity of the domain-specific payload an [`crate::EvidenceEnvelope`] carries.
//!
//! This crate is domain-neutral: it does not know what a "test receipt" or a
//! "coverage receipt" looks like. [`PayloadIdentity`] records just enough for
//! a domain-neutral consumer (a registry, a lineage graph) to know *what kind*
//! of payload is inside, *which version* of that payload's own schema
//! produced it, and *the exact bytes* of the payload via a reused
//! [`ContentDigest`] — without this crate parsing or validating the payload
//! itself.

use perl_source_identity::ContentDigest;
use serde::{Deserialize, Serialize};

/// Identity of one domain-specific evidence payload.
///
/// `schema_version` is the payload's *own* schema version — a concept
/// entirely independent of [`crate::EvidenceEnvelopeSchemaVersion`], which
/// versions only the outer envelope shape. This crate does not know which
/// payload schema versions are valid for a given `kind`; that validation
/// belongs to the domain-specific sibling crate that owns the payload schema
/// (deliberately excluded from this issue's scope).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PayloadIdentity {
    /// The domain-specific payload kind, e.g. `"test-receipt"` or
    /// `"coverage-receipt"`. An opaque, producer-defined string: this crate
    /// does not enumerate valid kinds.
    pub kind: String,
    /// The payload's own schema version, scoped to `kind`.
    pub schema_version: u32,
    /// The collision-resistant digest of the exact payload bytes.
    pub digest: ContentDigest,
}

impl PayloadIdentity {
    /// Construct a payload identity from its kind, schema version, and digest.
    #[must_use]
    pub fn new(kind: impl Into<String>, schema_version: u32, digest: ContentDigest) -> Self {
        Self { kind: kind.into(), schema_version, digest }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    fn sample() -> PayloadIdentity {
        PayloadIdentity::new("test-receipt", 3, ContentDigest::of_bytes(b"payload bytes"))
    }

    #[test]
    fn payload_identity_serde_round_trip() {
        let original = sample();
        let json = serde_json::to_string(&original).expect("serialize");
        let back: PayloadIdentity = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(original, back);
    }

    #[test]
    fn payload_identity_distinguishes_kind() {
        let a = PayloadIdentity::new("test-receipt", 1, ContentDigest::of_bytes(b"x"));
        let b = PayloadIdentity::new("coverage-receipt", 1, ContentDigest::of_bytes(b"x"));
        assert_ne!(a, b);
    }

    #[test]
    fn payload_identity_distinguishes_schema_version() {
        let a = PayloadIdentity::new("test-receipt", 1, ContentDigest::of_bytes(b"x"));
        let b = PayloadIdentity::new("test-receipt", 2, ContentDigest::of_bytes(b"x"));
        assert_ne!(a, b);
    }

    #[test]
    fn payload_identity_rejects_malformed_digest_at_serde_boundary() {
        let json = serde_json::to_string(&sample()).unwrap();
        let corrupted = json.replace("sha256:", "md5:");
        assert!(serde_json::from_str::<PayloadIdentity>(&corrupted).is_err());
    }
}
