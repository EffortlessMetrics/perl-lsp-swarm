//! Deterministic identity for the full contents of an evidence envelope.
//!
//! [`EnvelopeFingerprint`] answers "is this the same evidence?" — a distinct
//! question from [`crate::ReceiptId`] ("which receipt is this?"). Two
//! envelopes with identical field values always produce the same
//! fingerprint, regardless of:
//!
//! - the order fields were set in code (Rust struct field order is not
//!   serialization order and is irrelevant here — the hash walks the fields
//!   in one fixed order defined by [`EnvelopeFingerprint::of`]);
//! - the order elements were inserted into any of the envelope's `Vec`
//!   fields (`claim_boundary.established`, `claim_boundary.not_established`,
//!   `limitations`, `inputs`) — each is treated as an unordered collection
//!   and canonically sorted before hashing.
//!
//! # Domain separation
//!
//! The fingerprint uses its own domain tag,
//! `perl-lsp:evidence-envelope-fingerprint:v1`, distinct from
//! `perl-source-identity`'s `perl-lsp:content-digest:v1` domain used by
//! [`perl_source_identity::ContentDigest`]. Domain separation means hashing
//! the *same* material under the two domains produces *different* digests —
//! an `EnvelopeFingerprint` can never collide with, or be mistaken for, a
//! `ContentDigest` even if a producer fed one crate's output into the other's
//! input by mistake.

use serde::{Deserialize, Deserializer, Serialize};
use sha2::{Digest as _, Sha256};

use crate::envelope::EvidenceEnvelope;
use crate::wire::{bytes_to_wire_hex, is_sha256_hex_body, length_prefixed};

/// Domain separator for [`EnvelopeFingerprint`] derivation.
///
/// Distinct from every domain tag `perl-source-identity` uses, so that no
/// material hashed under this domain can ever equal a value hashed under
/// `perl-source-identity`'s domains.
const FINGERPRINT_DOMAIN: &[u8] = b"perl-lsp:evidence-envelope-fingerprint:v1\0";

/// Wire prefix for [`EnvelopeFingerprint`].
const FINGERPRINT_PREFIX: &str = "envfp:sha256:";

/// Presence tag pushed before an `Option<String>` field: `0` for absent,
/// `1` for present (followed by the length-prefixed value). This keeps
/// `None` distinguishable from `Some(String::new())`.
const ABSENT: &[u8] = &[0];
const PRESENT: &[u8] = &[1];

/// Minimal domain-tagged hash accumulator used only by this module.
///
/// This intentionally does not reuse `perl-source-identity`'s internal
/// `DomainHasher`: that type is `pub(crate)` to that crate and not part of
/// its public API, so a distinct domain tag here needs its own (equally
/// small) accumulator rather than an unexported dependency.
struct FingerprintHasher(Sha256);

impl FingerprintHasher {
    fn new() -> Self {
        let mut h = Sha256::new();
        h.update(FINGERPRINT_DOMAIN);
        Self(h)
    }

    /// Append a length-prefixed byte field.
    fn push_field(&mut self, field: &[u8]) -> &mut Self {
        self.0.update(length_prefixed(field));
        self
    }

    /// Append a length-prefixed `Option<&str>` field with an explicit
    /// presence tag, so `None` and `Some("")` never collide.
    fn push_optional(&mut self, field: Option<&str>) -> &mut Self {
        match field {
            None => {
                self.0.update(ABSENT);
            }
            Some(v) => {
                self.0.update(PRESENT);
                self.push_field(v.as_bytes());
            }
        }
        self
    }

    /// Append a `u32` as 4 big-endian bytes.
    fn push_u32(&mut self, value: u32) -> &mut Self {
        self.0.update(value.to_be_bytes());
        self
    }

    fn finish(self) -> [u8; 32] {
        self.0.finalize().into()
    }
}

/// A deterministic, domain-separated fingerprint over one envelope's full
/// canonically-ordered contents.
///
/// # Wire format
///
/// `envfp:sha256:<64 lowercase hex digits>`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(transparent)]
pub struct EnvelopeFingerprint(String);

impl EnvelopeFingerprint {
    /// Compute the deterministic fingerprint of one envelope.
    ///
    /// See the module docs for the exact canonicalization rules applied to
    /// `Vec`-valued fields.
    #[must_use]
    pub fn of(envelope: &EvidenceEnvelope) -> Self {
        let mut h = FingerprintHasher::new();

        // 1. Schema version.
        h.push_u32(envelope.schema_version.as_u32());

        // 2. Receipt identity.
        h.push_field(envelope.receipt_id.as_wire().as_bytes());

        // 3. Payload identity.
        h.push_field(envelope.payload.kind.as_bytes());
        h.push_u32(envelope.payload.schema_version);
        h.push_field(envelope.payload.digest.as_wire().as_bytes());

        // 4. Producer identity.
        h.push_field(envelope.producer.name.as_bytes());
        h.push_field(envelope.producer.version.as_bytes());
        h.push_field(envelope.producer.source_sha.as_bytes());
        h.push_field(envelope.producer.build_identity.as_bytes());

        // 5. Subject.
        h.push_field(envelope.subject.project_id.as_wire().as_bytes());
        h.push_optional(envelope.subject.base_ref.as_deref());
        h.push_optional(envelope.subject.head_ref.as_deref());
        h.push_optional(envelope.subject.candidate_ref.as_deref());
        h.push_optional(envelope.subject.artifact_ref.as_deref());
        h.push_field(envelope.subject.run.source.fingerprint_tag().as_bytes());
        h.push_field(envelope.subject.run.run_id.as_bytes());
        h.push_u32(envelope.subject.run.attempt);

        // 6. Completeness / redaction / retention.
        //
        //    Hashed by their stable textual tags, never by `as u8`: these enums
        //    are `#[non_exhaustive]`, so a variant inserted anywhere but the end
        //    would shift every later discriminant and silently change the
        //    fingerprint of already-produced evidence whose wire form is
        //    unchanged. The tags are pinned by `fingerprint_is_stable_for_a_known_envelope`.
        h.push_field(envelope.completeness.fingerprint_tag().as_bytes());
        h.push_field(envelope.redaction_class.fingerprint_tag().as_bytes());
        h.push_field(envelope.retention_class.fingerprint_tag().as_bytes());

        // 7. Claim boundary — both lists are canonically sorted so element
        //    insertion order never affects the fingerprint.
        let mut established: Vec<&str> =
            envelope.claim_boundary.established.iter().map(String::as_str).collect();
        established.sort_unstable();
        h.push_field((established.len() as u32).to_be_bytes().as_slice());
        for item in established {
            h.push_field(item.as_bytes());
        }

        let mut not_established: Vec<&str> =
            envelope.claim_boundary.not_established.iter().map(String::as_str).collect();
        not_established.sort_unstable();
        h.push_field((not_established.len() as u32).to_be_bytes().as_slice());
        for item in not_established {
            h.push_field(item.as_bytes());
        }

        // 8. Limitations — canonically sorted (order-independent).
        let mut limitations: Vec<_> = envelope.limitations.clone();
        limitations.sort();
        h.push_field((limitations.len() as u32).to_be_bytes().as_slice());
        for limitation in &limitations {
            h.push_field(limitation.detail.as_bytes());
            h.push_optional(limitation.affected_claim.as_deref());
        }

        // 9. Inputs — canonically sorted lineage edges (order-independent).
        let mut inputs: Vec<_> = envelope.inputs.iter().map(|i| i.canonical_key()).collect();
        inputs.sort_unstable();
        h.push_field((inputs.len() as u32).to_be_bytes().as_slice());
        for (receipt, digest) in inputs {
            h.push_field(receipt.as_bytes());
            h.push_field(digest.as_bytes());
        }

        let raw = h.finish();
        Self(format!("{FINGERPRINT_PREFIX}{}", bytes_to_wire_hex(&raw)))
    }

    /// Parse a fingerprint from its wire representation.
    #[must_use]
    pub fn from_wire(s: &str) -> Option<Self> {
        s.strip_prefix(FINGERPRINT_PREFIX)
            .filter(|body| is_sha256_hex_body(body))
            .map(|_| Self(s.to_owned()))
    }

    /// The wire representation, e.g. `envfp:sha256:abc123...`.
    #[must_use]
    pub fn as_wire(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for EnvelopeFingerprint {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        Self::from_wire(&raw).ok_or_else(|| {
            serde::de::Error::invalid_value(
                serde::de::Unexpected::Str(&raw),
                &"an envelope fingerprint of the form `envfp:sha256:<64 lowercase hex digits>`",
            )
        })
    }
}

impl std::fmt::Display for EnvelopeFingerprint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;
    use perl_source_identity::ContentDigest;

    /// Domain separation: the same raw material hashed under this crate's
    /// fingerprint domain must not equal `ContentDigest::of_bytes` over the
    /// identical bytes. Both use SHA-256 with length-prefixed fields; the
    /// only difference is the domain tag, which is exactly the property this
    /// test isolates.
    #[test]
    fn fingerprint_domain_is_separated_from_content_digest_domain() {
        let material = b"identical material across both domains";

        let mut h = FingerprintHasher::new();
        h.push_field(material);
        let raw = h.finish();
        let fingerprint_wire = format!("sha256:{}", bytes_to_wire_hex(&raw));

        let content_digest = ContentDigest::of_bytes(material);

        assert_ne!(
            fingerprint_wire,
            content_digest.as_wire(),
            "identical material hashed under two domains must not collide"
        );
    }

    /// A fully-populated envelope whose every field is non-default, so the
    /// golden vector below actually covers the whole field walk.
    fn golden_envelope() -> EvidenceEnvelope {
        use crate::claim::{ClaimBoundary, Limitation};
        use crate::classification::{RedactionClass, RetentionClass};
        use crate::completeness::Completeness;
        use crate::envelope::EvidenceEnvelopeSchemaVersion;
        use crate::input::InputReference;
        use crate::payload::PayloadIdentity;
        use crate::producer::ProducerIdentity;
        use crate::receipt::ReceiptId;
        use crate::subject::{EvidenceSubject, RunIdentity, RunSource};
        use perl_source_identity::ProjectId;

        EvidenceEnvelope {
            schema_version: EvidenceEnvelopeSchemaVersion::V1,
            receipt_id: ReceiptId::from_canonical_key("golden-receipt"),
            payload: PayloadIdentity::new(
                "golden-kind",
                7,
                ContentDigest::of_bytes(b"golden payload bytes"),
            ),
            producer: ProducerIdentity::new("golden-producer", "1.2.3", "deadbeef", "build-9"),
            subject: EvidenceSubject::new(
                ProjectId::from_canonical_name("acme/golden"),
                Some("base-ref".to_string()),
                Some("head-ref".to_string()),
                Some("candidate-ref".to_string()),
                Some("artifact-ref".to_string()),
                RunIdentity::new(RunSource::Workflow, "run-42", 2),
            ),
            completeness: Completeness::Partial,
            redaction_class: RedactionClass::Redacted,
            retention_class: RetentionClass::Extended,
            claim_boundary: ClaimBoundary::new(
                vec!["established-a".to_string(), "established-b".to_string()],
                vec!["not-established-a".to_string()],
            ),
            limitations: vec![
                Limitation::general("general limitation"),
                Limitation::scoped("scoped limitation", "established-a"),
            ],
            inputs: vec![
                InputReference::new(
                    ReceiptId::from_canonical_key("input-1"),
                    ContentDigest::of_bytes(b"input one"),
                ),
                InputReference::new(
                    ReceiptId::from_canonical_key("input-2"),
                    ContentDigest::of_bytes(b"input two"),
                ),
            ],
        }
    }

    /// Golden vector pinning the exact fingerprint of a fully-populated
    /// envelope.
    ///
    /// This is the crate's protection against *silent* identity drift. The
    /// determinism and order-independence tests only compare fingerprints to
    /// each other, so they stay green under any change that is applied
    /// uniformly — reordering the field walk, renaming a
    /// `fingerprint_tag`, inserting an `#[non_exhaustive]` enum variant ahead
    /// of an existing one, or adding a field to the walk. Every one of those
    /// invalidates already-persisted fingerprints, so every one of them must
    /// fail here and be acknowledged as a breaking change rather than merged
    /// as a refactor.
    ///
    /// If this test fails, do not "fix" it by pasting in the new value until
    /// you have confirmed the change to envelope identity is intended.
    #[test]
    fn fingerprint_is_stable_for_a_known_envelope() {
        assert_eq!(
            golden_envelope().fingerprint().as_wire(),
            "envfp:sha256:764bb508f274c6e5157a2381d43dac8ad5dccc0f31476ed04b577c96bdc4a84a",
            "envelope fingerprint changed; see this test's doc comment before updating it"
        );
    }

    /// The golden envelope must exercise every branch the walk can take, or
    /// the vector above pins less than it appears to. In particular every
    /// `Option` subject field is `Some` and both `Limitation` shapes
    /// (general and scoped) are present.
    #[test]
    fn golden_envelope_exercises_every_optional_branch() {
        let e = golden_envelope();
        assert!(e.subject.base_ref.is_some());
        assert!(e.subject.head_ref.is_some());
        assert!(e.subject.candidate_ref.is_some());
        assert!(e.subject.artifact_ref.is_some());
        assert!(e.limitations.iter().any(|l| l.affected_claim.is_none()), "general limitation");
        assert!(e.limitations.iter().any(|l| l.affected_claim.is_some()), "scoped limitation");
        assert!(!e.claim_boundary.established.is_empty());
        assert!(!e.claim_boundary.not_established.is_empty());
        assert!(!e.inputs.is_empty());
    }

    #[test]
    fn from_wire_round_trips_a_computed_fingerprint() {
        // Constructing a full EvidenceEnvelope is exercised in envelope.rs;
        // here we only check the wire parser against a hand-built value.
        let hex = "0".repeat(64);
        let wire = format!("envfp:sha256:{hex}");
        let parsed = EnvelopeFingerprint::from_wire(&wire).expect("valid wire form must parse");
        assert_eq!(parsed.as_wire(), wire);
    }

    #[test]
    fn from_wire_rejects_malformed_input() {
        assert!(EnvelopeFingerprint::from_wire("sha256:short").is_none());
        assert!(EnvelopeFingerprint::from_wire("").is_none());
        assert!(
            EnvelopeFingerprint::from_wire(&format!("envfp:sha256:{}", "A".repeat(64))).is_none(),
            "uppercase hex must be rejected, not normalized"
        );
    }

    #[test]
    fn fingerprint_deserialization_is_validating() {
        for bad in ["\"not-a-fingerprint\"", "\"envfp:sha256:short\"", "\"\""] {
            assert!(
                serde_json::from_str::<EnvelopeFingerprint>(bad).is_err(),
                "deserialization must reject {bad}"
            );
        }
    }
}
