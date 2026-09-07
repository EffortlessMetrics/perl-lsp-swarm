//! The domain-neutral `evidence_envelope.v1` outer record.
//!
//! [`EvidenceEnvelope`] is the outer shell every domain-specific evidence
//! payload (test receipts, coverage receipts, gate receipts, ...) is carried
//! inside. This crate does not know what any specific payload means; it only
//! owns the identity, provenance, completeness, and classification fields
//! that every payload kind needs answered the same way, plus the
//! deterministic [`crate::EnvelopeFingerprint`] derived from them.
//!
//! Receipt registries, validators, freshness rules, JSON schema projections,
//! and CLI tooling are deliberately out of scope for this crate (see the
//! controlling issue's non-goals) and live in sibling crates that depend on
//! these types.

use serde::{Deserialize, Deserializer, Serialize};

use crate::claim::{ClaimBoundary, Limitation};
use crate::classification::{RedactionClass, RetentionClass};
use crate::completeness::Completeness;
use crate::input::InputReference;
use crate::payload::PayloadIdentity;
use crate::producer::ProducerIdentity;
use crate::receipt::ReceiptId;
use crate::subject::EvidenceSubject;

/// Current schema version for [`EvidenceEnvelope`].
pub const EVIDENCE_ENVELOPE_SCHEMA_VERSION_V1: u32 = 1;

/// A versioned schema marker for the `evidence_envelope.v1` outer format.
///
/// # Fail-closed deserialization
///
/// Deserialization **rejects** any version this build does not support,
/// exactly as `perl-source-identity`'s `SourceIdentitySchemaVersion` does: an
/// unrecognized version is a serde error, not a value that silently flows
/// onward as though it were understood.
///
/// Constructing an unsupported version *in code* remains possible (the field
/// is public), so tests and version-negotiation logic can still reason about
/// versions this build does not accept on the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(transparent)]
pub struct EvidenceEnvelopeSchemaVersion(pub u32);

impl<'de> Deserialize<'de> for EvidenceEnvelopeSchemaVersion {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = u32::deserialize(deserializer)?;
        let version = Self(raw);
        if version.is_supported() {
            Ok(version)
        } else {
            Err(serde::de::Error::invalid_value(
                serde::de::Unexpected::Unsigned(u64::from(raw)),
                &"a supported evidence_envelope schema version (currently 1)",
            ))
        }
    }
}

impl EvidenceEnvelopeSchemaVersion {
    /// The current `evidence_envelope.v1` schema version.
    pub const V1: Self = Self(EVIDENCE_ENVELOPE_SCHEMA_VERSION_V1);

    /// Returns `true` if this version is one the current runtime recognizes.
    #[must_use]
    pub fn is_supported(&self) -> bool {
        self.0 == EVIDENCE_ENVELOPE_SCHEMA_VERSION_V1
    }

    /// Unwrap the raw integer version.
    #[must_use]
    pub fn as_u32(self) -> u32 {
        self.0
    }
}

impl std::fmt::Display for EvidenceEnvelopeSchemaVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "evidence_envelope.v{}", self.0)
    }
}

/// The `evidence_envelope.v1` outer evidence record.
///
/// # Completeness is always explicit
///
/// `completeness` and `inputs` are both required fields with no `serde`
/// default: an envelope that omits either fails to deserialize. This is
/// deliberate — a permissive decode that filled in
/// [`Completeness::Complete`] for a missing field would let an envelope with
/// zero recorded inputs be silently read as fully complete evidence, which is
/// exactly the failure mode this type exists to prevent.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceEnvelope {
    /// The schema version that produced this envelope.
    pub schema_version: EvidenceEnvelopeSchemaVersion,
    /// Durable identity for this specific receipt.
    pub receipt_id: ReceiptId,
    /// Identity of the domain-specific payload this envelope carries.
    pub payload: PayloadIdentity,
    /// Identity of the tool/process that produced this envelope.
    pub producer: ProducerIdentity,
    /// What (and which run) this envelope's evidence is about.
    pub subject: EvidenceSubject,
    /// Explicit completeness classification — never inferred from
    /// `inputs.len()`.
    pub completeness: Completeness,
    /// Redaction classification of the payload.
    pub redaction_class: RedactionClass,
    /// Retention classification of the payload.
    pub retention_class: RetentionClass,
    /// The explicit boundary of what this evidence establishes.
    pub claim_boundary: ClaimBoundary,
    /// Explicit known limitations on this evidence.
    pub limitations: Vec<Limitation>,
    /// Lineage edges to upstream receipts this envelope consumed as inputs.
    /// May legitimately be empty; [`EvidenceEnvelope::completeness`] — not
    /// this array's length — is the authority on whether that emptiness
    /// means "nothing was expected" or "something is missing".
    pub inputs: Vec<InputReference>,
}

impl EvidenceEnvelope {
    /// Returns `true` if the schema version is one this runtime understands.
    #[must_use]
    pub fn is_schema_supported(&self) -> bool {
        self.schema_version.is_supported()
    }

    /// Compute this envelope's deterministic [`crate::EnvelopeFingerprint`].
    #[must_use]
    pub fn fingerprint(&self) -> crate::EnvelopeFingerprint {
        crate::EnvelopeFingerprint::of(self)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;
    use crate::claim::{ClaimBoundary, Limitation};
    use crate::classification::{RedactionClass, RetentionClass};
    use crate::completeness::Completeness;
    use crate::input::InputReference;
    use crate::payload::PayloadIdentity;
    use crate::producer::ProducerIdentity;
    use crate::receipt::ReceiptId;
    use crate::subject::{EvidenceSubject, RunIdentity, RunSource};
    use perl_source_identity::{ContentDigest, ProjectId};

    fn sample_envelope() -> EvidenceEnvelope {
        EvidenceEnvelope {
            schema_version: EvidenceEnvelopeSchemaVersion::V1,
            receipt_id: ReceiptId::from_producer_and_key(
                &crate::producer::test_producer(),
                "receipt-1",
            ),
            payload: PayloadIdentity::new(
                "test-receipt",
                1,
                ContentDigest::of_bytes(b"payload bytes"),
            ),
            producer: ProducerIdentity::new(
                "perl-lsp-test-runner",
                "0.17.0",
                "abc123",
                "ci-build-1",
            ),
            subject: EvidenceSubject::new(
                ProjectId::from_canonical_name("acme/widget"),
                Some("base-sha".to_string()),
                Some("head-sha".to_string()),
                Some("candidate-sha".to_string()),
                None,
                RunIdentity::new(RunSource::Workflow, "run-1", 1),
            ),
            completeness: Completeness::Complete,
            redaction_class: RedactionClass::Public,
            retention_class: RetentionClass::Standard,
            claim_boundary: ClaimBoundary::new(
                vec!["parses without panicking".to_string()],
                vec!["does not prove semantic correctness".to_string()],
            ),
            limitations: vec![Limitation::general("sample limitation")],
            inputs: vec![InputReference::new(
                ReceiptId::from_producer_and_key(&crate::producer::test_producer(), "upstream-1"),
                ContentDigest::of_bytes(b"upstream bytes"),
            )],
        }
    }

    // ── Schema version tests ──────────────────────────────────────────────────

    #[test]
    fn schema_version_v1_is_supported() {
        assert!(EvidenceEnvelopeSchemaVersion::V1.is_supported());
    }

    #[test]
    fn unknown_schema_version_is_unsupported() {
        assert!(!EvidenceEnvelopeSchemaVersion(0).is_supported());
        assert!(!EvidenceEnvelopeSchemaVersion(99).is_supported());
    }

    #[test]
    fn schema_version_display() {
        assert_eq!(format!("{}", EvidenceEnvelopeSchemaVersion::V1), "evidence_envelope.v1");
    }

    #[test]
    fn envelope_is_schema_supported() {
        assert!(sample_envelope().is_schema_supported());
    }

    /// Fail-closed serde: a schema version this build does not implement must
    /// be rejected at decode time, not silently read as v1.
    #[test]
    fn envelope_rejects_unsupported_schema_version() {
        let json = serde_json::to_string(&sample_envelope()).expect("serialize");
        assert!(json.contains("\"schema_version\":1"), "baseline shape changed: {json}");

        for bad_version in ["0", "2", "99", "4294967295"] {
            let mutated =
                json.replace("\"schema_version\":1", &format!("\"schema_version\":{bad_version}"));
            assert_ne!(mutated, json, "mutation must actually apply");
            let parsed = serde_json::from_str::<EvidenceEnvelope>(&mutated);
            assert!(
                parsed.is_err(),
                "schema version {bad_version} must be rejected, got {parsed:?}"
            );
        }
    }

    // ── Serde round-trip ───────────────────────────────────────────────────────

    #[test]
    fn envelope_serde_round_trip() {
        let env = sample_envelope();
        let json = serde_json::to_string(&env).expect("serialize");
        let back: EvidenceEnvelope = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(env, back);
    }

    #[test]
    fn envelope_serde_round_trip_with_empty_inputs_and_limitations() {
        let mut env = sample_envelope();
        env.inputs.clear();
        env.limitations.clear();
        env.completeness = Completeness::NotApplicable;
        let json = serde_json::to_string(&env).expect("serialize");
        let back: EvidenceEnvelope = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(env, back);
    }

    // ── Completeness is explicit: never inferred from an absent array ─────────

    /// Removing `completeness` from the wire form must fail to decode. A
    /// wrong implementation that used `#[serde(default)]` (defaulting to
    /// `Completeness::Complete`) would decode this successfully with
    /// `completeness == Complete` despite an empty `inputs` array — exactly
    /// the silent-completeness failure mode this type must prevent. This
    /// test fails against that wrong implementation.
    #[test]
    fn envelope_rejects_missing_completeness_field() {
        let value = serde_json::to_value(sample_envelope()).expect("to_value");
        let mut map = value.as_object().expect("envelope is a JSON object").clone();
        assert!(map.remove("completeness").is_some(), "field must exist to be removed");
        let json = serde_json::to_string(&map).expect("serialize");
        assert!(
            serde_json::from_str::<EvidenceEnvelope>(&json).is_err(),
            "envelope missing `completeness` must not deserialize, and must never silently \
             become Complete"
        );
    }

    /// Removing `inputs` from the wire form must fail to decode rather than
    /// silently defaulting to an empty vec (which combined with a permissive
    /// completeness default would look identical to a genuinely complete,
    /// input-free envelope).
    #[test]
    fn envelope_rejects_missing_inputs_field() {
        let value = serde_json::to_value(sample_envelope()).expect("to_value");
        let mut map = value.as_object().expect("envelope is a JSON object").clone();
        assert!(map.remove("inputs").is_some(), "field must exist to be removed");
        let json = serde_json::to_string(&map).expect("serialize");
        assert!(
            serde_json::from_str::<EvidenceEnvelope>(&json).is_err(),
            "envelope missing `inputs` must not deserialize"
        );
    }

    /// An envelope with an explicit, present `completeness` and a genuinely
    /// empty `inputs` array is valid: emptiness is fine as long as it is
    /// explicit, not inferred.
    #[test]
    fn envelope_with_explicit_not_applicable_and_empty_inputs_is_valid() {
        let mut env = sample_envelope();
        env.completeness = Completeness::NotApplicable;
        env.inputs.clear();
        let json = serde_json::to_string(&env).expect("serialize");
        let back: EvidenceEnvelope = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.completeness, Completeness::NotApplicable);
        assert!(back.inputs.is_empty());
    }

    // ── Deterministic fingerprint ──────────────────────────────────────────────

    #[test]
    fn fingerprint_is_deterministic_across_repeated_computation() {
        let env = sample_envelope();
        let a = env.fingerprint();
        let b = env.fingerprint();
        assert_eq!(a, b, "same envelope contents must always produce the same fingerprint");
    }

    /// Negative control: without this, `fingerprint_is_deterministic_*` could
    /// pass vacuously against a constant-returning implementation.
    #[test]
    fn fingerprint_changes_when_a_field_changes() {
        let mut a = sample_envelope();
        let b_fp = {
            let mut b = sample_envelope();
            b.producer.version = "0.18.0".to_string();
            b.fingerprint()
        };
        let a_fp = a.fingerprint();
        assert_ne!(a_fp, b_fp, "changing producer.version must change the fingerprint");

        // Also vary a second, independent field to further rule out a
        // constant/degenerate implementation.
        a.subject.run.attempt += 1;
        assert_ne!(
            a.fingerprint(),
            a_fp,
            "changing subject.run.attempt must change the fingerprint"
        );
    }

    /// Fingerprint must be independent of `Vec` field insertion order for
    /// every canonicalized collection field.
    #[test]
    fn fingerprint_is_independent_of_inputs_insertion_order() {
        let mut forward = sample_envelope();
        forward.inputs = vec![
            InputReference::new(
                ReceiptId::from_producer_and_key(&crate::producer::test_producer(), "upstream-a"),
                ContentDigest::of_bytes(b"a"),
            ),
            InputReference::new(
                ReceiptId::from_producer_and_key(&crate::producer::test_producer(), "upstream-b"),
                ContentDigest::of_bytes(b"b"),
            ),
        ];
        let mut reversed = sample_envelope();
        reversed.inputs = forward.inputs.iter().rev().cloned().collect();

        assert_ne!(forward.inputs, reversed.inputs, "test setup must actually differ in Vec order");
        assert_eq!(
            forward.fingerprint(),
            reversed.fingerprint(),
            "inputs insertion order must not affect the fingerprint"
        );
    }

    #[test]
    fn fingerprint_is_independent_of_limitations_insertion_order() {
        let mut forward = sample_envelope();
        forward.limitations =
            vec![Limitation::general("limitation a"), Limitation::general("limitation b")];
        let mut reversed = sample_envelope();
        reversed.limitations = forward.limitations.iter().rev().cloned().collect();

        assert_ne!(forward.limitations, reversed.limitations);
        assert_eq!(
            forward.fingerprint(),
            reversed.fingerprint(),
            "limitations insertion order must not affect the fingerprint"
        );
    }

    #[test]
    fn fingerprint_is_independent_of_claim_boundary_list_order() {
        let mut forward = sample_envelope();
        forward.claim_boundary =
            ClaimBoundary::new(vec!["claim a".to_string(), "claim b".to_string()], vec![]);
        let mut reversed = sample_envelope();
        reversed.claim_boundary =
            ClaimBoundary::new(vec!["claim b".to_string(), "claim a".to_string()], vec![]);

        assert_ne!(forward.claim_boundary, reversed.claim_boundary);
        assert_eq!(
            forward.fingerprint(),
            reversed.fingerprint(),
            "claim_boundary.established order must not affect the fingerprint"
        );
    }

    /// A genuinely different multiset of inputs (not just reordered) must
    /// still change the fingerprint — canonicalization must not collapse
    /// distinct evidence into one fingerprint.
    #[test]
    fn fingerprint_distinguishes_different_input_sets() {
        let mut a = sample_envelope();
        a.inputs = vec![InputReference::new(
            ReceiptId::from_producer_and_key(&crate::producer::test_producer(), "upstream-a"),
            ContentDigest::of_bytes(b"a"),
        )];
        let mut b = sample_envelope();
        b.inputs = vec![InputReference::new(
            ReceiptId::from_producer_and_key(&crate::producer::test_producer(), "upstream-a"),
            ContentDigest::of_bytes(b"different"),
        )];
        assert_ne!(a.fingerprint(), b.fingerprint());
    }

    /// Fail-closed decode extends to unknown fields: an envelope carrying a
    /// field this build does not understand is rejected, never silently
    /// dropped. Without `deny_unknown_fields` every other test in this module
    /// still passes while the field vanishes on decode.
    #[test]
    fn envelope_rejects_unknown_field() {
        let value = serde_json::to_value(sample_envelope()).expect("to_value");
        let mut map = value.as_object().expect("envelope is a JSON object").clone();
        assert!(
            map.insert("future_field".to_string(), serde_json::Value::Bool(true)).is_none(),
            "the field must be genuinely new for this test to mean anything"
        );
        assert!(
            serde_json::from_str::<EvidenceEnvelope>(
                &serde_json::to_string(&map).expect("serialize")
            )
            .is_err(),
            "an unknown envelope field must be rejected, never ignored"
        );
    }
}
