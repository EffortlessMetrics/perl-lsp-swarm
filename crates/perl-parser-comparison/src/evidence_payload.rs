//! Exact durable parser-comparison evidence references and terminal payloads.
//!
//! This module owns reusable domain payloads only. It does not select a run
//! population, certify complete execution, derive pairwise differentials, or
//! establish repository candidate, producer, freshness, retention, or
//! publication authority.

use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::{
    BoundedText, ConformanceOutcome, DiagnosticSummary, EvidenceValueError, HarnessFailure,
    HarnessOutcome, InstrumentState, MismatchClass, MismatchDetail, ObservationDisposition,
    ObservationPlane, ObserverId, ReviewedExpectationId, ScoredComparison, SemanticFingerprint,
    StableId, SubjectDisposition, SubjectExecution, SubjectRole,
};

/// Version of one durable subject-execution payload.
pub const SUBJECT_EXECUTION_EVIDENCE_SCHEMA_VERSION: &str =
    "parser_comparison_subject_execution.v1";
/// Version of one durable subject-observation payload.
pub const SUBJECT_OBSERVATION_EVIDENCE_SCHEMA_VERSION: &str =
    "parser_comparison_subject_observation.v1";
/// Version of one durable subject-conformance payload.
pub const SUBJECT_CONFORMANCE_EVIDENCE_SCHEMA_VERSION: &str =
    "parser_comparison_subject_conformance.v1";

/// Exact kind of a referenced parser-comparison evidence payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum EvidenceKind {
    /// Canonical source/case payload owned by the corpus authority.
    SourceCase,
    /// Exact parser-subject manifest.
    SubjectManifest,
    /// Exact observer manifest.
    ObserverManifest,
    /// Independently reviewed case obligation.
    CaseObligation,
    /// One terminal subject execution payload.
    SubjectExecution,
    /// One terminal subject observation payload.
    SubjectObservation,
    /// One terminal reviewed conformance payload.
    SubjectConformance,
}

impl EvidenceKind {
    /// Stable wire name of this evidence kind.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SourceCase => "source_case",
            Self::SubjectManifest => "subject_manifest",
            Self::ObserverManifest => "observer_manifest",
            Self::CaseObligation => "case_obligation",
            Self::SubjectExecution => "subject_execution",
            Self::SubjectObservation => "subject_observation",
            Self::SubjectConformance => "subject_conformance",
        }
    }
}

/// Checked SHA-256 semantic digest.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SemanticDigest(String);

impl SemanticDigest {
    /// Parse one `sha256:<64 lowercase hex>` digest.
    pub fn new(value: impl Into<String>) -> Result<Self, EvidencePayloadError> {
        let value = value.into();
        let Some(hex) = value.strip_prefix("sha256:") else {
            return Err(EvidencePayloadError::InvalidSemanticDigest(value));
        };
        if hex.len() != 64
            || !hex.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(EvidencePayloadError::InvalidSemanticDigest(value));
        }
        Ok(Self(value))
    }

    /// Compute a digest over exact canonical semantic bytes.
    pub fn from_bytes(bytes: &[u8]) -> Self {
        let digest = Sha256::digest(bytes);
        let mut value = String::with_capacity(71);
        value.push_str("sha256:");
        const HEX: &[u8; 16] = b"0123456789abcdef";
        for byte in digest {
            value.push(char::from(HEX[usize::from(byte >> 4)]));
            value.push(char::from(HEX[usize::from(byte & 0x0f)]));
        }
        Self(value)
    }

    /// Borrow the full digest including its algorithm prefix.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Borrow the lowercase hexadecimal digest body.
    pub fn hex(&self) -> &str {
        self.0.strip_prefix("sha256:").unwrap_or_default()
    }
}

impl fmt::Display for SemanticDigest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Exact reference to canonical semantic evidence bytes owned elsewhere.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EvidenceRef {
    kind: EvidenceKind,
    schema_version: StableId,
    semantic_id: StableId,
    semantic_digest: SemanticDigest,
}

impl EvidenceRef {
    /// Construct one exact checked evidence reference.
    pub fn new(
        kind: EvidenceKind,
        schema_version: StableId,
        semantic_id: StableId,
        semantic_digest: SemanticDigest,
    ) -> Self {
        Self { kind, schema_version, semantic_id, semantic_digest }
    }

    /// Referenced payload kind.
    pub const fn kind(&self) -> EvidenceKind {
        self.kind
    }

    /// Referenced payload schema version.
    pub const fn schema_version(&self) -> &StableId {
        &self.schema_version
    }

    /// Stable semantic identifier of the referenced payload.
    pub const fn semantic_id(&self) -> &StableId {
        &self.semantic_id
    }

    /// Exact digest of the referenced canonical semantic bytes.
    pub const fn semantic_digest(&self) -> &SemanticDigest {
        &self.semantic_digest
    }
}

/// Exact canonical source/case reference used by subject execution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceCaseRef {
    case_id: StableId,
    authority: EvidenceRef,
    content_digest: SemanticDigest,
}

impl SourceCaseRef {
    /// Construct an exact source/case reference.
    pub fn new(
        case_id: StableId,
        authority: EvidenceRef,
        content_digest: SemanticDigest,
    ) -> Result<Self, EvidencePayloadError> {
        require_kind(&authority, EvidenceKind::SourceCase)?;
        Ok(Self { case_id, authority, content_digest })
    }

    /// Stable canonical case ID.
    pub const fn case_id(&self) -> &StableId {
        &self.case_id
    }

    /// Canonical source/case authority reference.
    pub const fn authority(&self) -> &EvidenceRef {
        &self.authority
    }

    /// Exact source-content digest.
    pub const fn content_digest(&self) -> &SemanticDigest {
        &self.content_digest
    }
}

/// Exact parser-subject manifest reference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubjectManifestRef {
    authority: EvidenceRef,
    role: SubjectRole,
}

impl SubjectManifestRef {
    /// Construct an exact subject-manifest reference.
    pub fn new(authority: EvidenceRef, role: SubjectRole) -> Result<Self, EvidencePayloadError> {
        require_kind(&authority, EvidenceKind::SubjectManifest)?;
        Ok(Self { authority, role })
    }

    /// Exact subject-manifest authority reference.
    pub const fn authority(&self) -> &EvidenceRef {
        &self.authority
    }

    /// Exact role bound by the subject manifest.
    pub const fn role(&self) -> SubjectRole {
        self.role
    }
}

/// Exact typed observer-manifest reference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObserverManifestRef {
    authority: EvidenceRef,
    observer_id: ObserverId,
    plane: ObservationPlane,
}

impl ObserverManifestRef {
    /// Construct an exact observer-manifest reference.
    pub fn new(
        authority: EvidenceRef,
        observer_id: ObserverId,
        plane: ObservationPlane,
    ) -> Result<Self, EvidencePayloadError> {
        require_kind(&authority, EvidenceKind::ObserverManifest)?;
        Ok(Self { authority, observer_id, plane })
    }

    /// Exact observer-manifest authority reference.
    pub const fn authority(&self) -> &EvidenceRef {
        &self.authority
    }

    /// Stable observer ID.
    pub const fn observer_id(&self) -> &ObserverId {
        &self.observer_id
    }

    /// Exact observation plane owned by this observer manifest.
    pub const fn plane(&self) -> &ObservationPlane {
        &self.plane
    }
}

/// Exact independently reviewed obligation reference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObligationRef {
    authority: EvidenceRef,
    obligation_id: ReviewedExpectationId,
    observer: ObserverManifestRef,
}

impl ObligationRef {
    /// Construct an exact reviewed-obligation reference.
    pub fn new(
        authority: EvidenceRef,
        obligation_id: ReviewedExpectationId,
        observer: ObserverManifestRef,
    ) -> Result<Self, EvidencePayloadError> {
        require_kind(&authority, EvidenceKind::CaseObligation)?;
        Ok(Self { authority, obligation_id, observer })
    }

    /// Exact reviewed-obligation authority reference.
    pub const fn authority(&self) -> &EvidenceRef {
        &self.authority
    }

    /// Stable reviewed obligation ID.
    pub const fn obligation_id(&self) -> &ReviewedExpectationId {
        &self.obligation_id
    }

    /// Exact compatible observer required by this obligation.
    pub const fn observer(&self) -> &ObserverManifestRef {
        &self.observer
    }
}

/// Publication/privacy disposition of one bounded diagnostic attachment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum AttachmentPrivacy {
    /// Attachment may be included in public evidence.
    Public,
    /// Attachment must be redacted before public projection.
    Redacted,
    /// Attachment is private and may not enter public artifacts.
    Private,
}

impl AttachmentPrivacy {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Public => "public",
            Self::Redacted => "redacted",
            Self::Private => "private",
        }
    }
}

/// Bounded non-authoritative diagnostic attachment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoundedAttachment {
    kind: StableId,
    text: BoundedText,
    privacy: AttachmentPrivacy,
}

impl BoundedAttachment {
    /// Construct a bounded diagnostic attachment.
    pub fn new(kind: StableId, text: BoundedText, privacy: AttachmentPrivacy) -> Self {
        Self { kind, text, privacy }
    }

    /// Stable attachment kind.
    pub const fn kind(&self) -> &StableId {
        &self.kind
    }

    /// Bounded retained text and omission accounting.
    pub const fn text(&self) -> &BoundedText {
        &self.text
    }

    /// Publication/privacy disposition.
    pub const fn privacy(&self) -> AttachmentPrivacy {
        self.privacy
    }
}

/// One exact terminal `case × subject` execution payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubjectExecutionEvidence {
    source_case: SourceCaseRef,
    subject_manifest: SubjectManifestRef,
    harness: HarnessOutcome,
    subject_disposition: Option<SubjectDisposition>,
    instrument_state: InstrumentState,
    diagnostics: DiagnosticSummary,
    observations: BTreeMap<ObservationPlane, ObservationDisposition>,
    attachments: Vec<BoundedAttachment>,
    semantic_digest: SemanticDigest,
}

impl SubjectExecutionEvidence {
    /// Project one validated generic execution into durable exact evidence.
    pub fn new(
        source_case: SourceCaseRef,
        subject_manifest: SubjectManifestRef,
        execution: &SubjectExecution,
        mut attachments: Vec<BoundedAttachment>,
    ) -> Result<Self, EvidencePayloadError> {
        if subject_manifest.role() != execution.subject() {
            return Err(EvidencePayloadError::SubjectRoleMismatch);
        }
        sort_attachments(&mut attachments);
        let semantic_value = execution_semantic_value(
            &source_case,
            &subject_manifest,
            execution.harness(),
            execution.subject_disposition(),
            execution.instrument_state(),
            execution.diagnostics(),
            execution.observations(),
        );
        let semantic_digest = digest_value(&semantic_value)?;
        Ok(Self {
            source_case,
            subject_manifest,
            harness: execution.harness(),
            subject_disposition: execution.subject_disposition().cloned(),
            instrument_state: execution.instrument_state(),
            diagnostics: *execution.diagnostics(),
            observations: execution.observations().clone(),
            attachments,
            semantic_digest,
        })
    }

    /// Exact source/case reference.
    pub const fn source_case(&self) -> &SourceCaseRef {
        &self.source_case
    }

    /// Exact subject-manifest reference.
    pub const fn subject_manifest(&self) -> &SubjectManifestRef {
        &self.subject_manifest
    }

    /// Harness/process terminal state.
    pub const fn harness(&self) -> HarnessOutcome {
        self.harness
    }

    /// Subject-owned terminal disposition after completed execution.
    pub fn subject_disposition(&self) -> Option<&SubjectDisposition> {
        self.subject_disposition.as_ref()
    }

    /// Terminal instrument state.
    pub const fn instrument_state(&self) -> InstrumentState {
        self.instrument_state
    }

    /// Bounded diagnostic/recovery summary.
    pub const fn diagnostics(&self) -> &DiagnosticSummary {
        &self.diagnostics
    }

    /// Terminal dispositions for the requested observation planes.
    pub const fn observations(&self) -> &BTreeMap<ObservationPlane, ObservationDisposition> {
        &self.observations
    }

    /// Terminal disposition for one requested observation plane, when present.
    pub fn observation(&self, plane: &ObservationPlane) -> Option<ObservationDisposition> {
        self.observations.get(plane).copied()
    }

    /// Non-authoritative bounded attachments.
    pub fn attachments(&self) -> &[BoundedAttachment] {
        &self.attachments
    }

    /// Semantic digest excluding explicitly non-authoritative attachments.
    pub const fn semantic_digest(&self) -> &SemanticDigest {
        &self.semantic_digest
    }

    /// Exact reference to this execution payload.
    pub fn evidence_ref(&self) -> Result<EvidenceRef, EvidencePayloadError> {
        evidence_ref_for(
            EvidenceKind::SubjectExecution,
            SUBJECT_EXECUTION_EVIDENCE_SCHEMA_VERSION,
            &self.semantic_digest,
        )
    }

    /// Deterministic canonical semantic JSON used by [`Self::semantic_digest`].
    pub fn canonical_semantic_json(&self) -> Result<String, EvidencePayloadError> {
        canonical_json(&execution_semantic_value(
            &self.source_case,
            &self.subject_manifest,
            self.harness,
            self.subject_disposition.as_ref(),
            self.instrument_state,
            &self.diagnostics,
            &self.observations,
        ))
    }

    /// Deterministic complete payload JSON including bounded attachments.
    pub fn canonical_payload_json(&self) -> Result<String, EvidencePayloadError> {
        canonical_json(&json!({
            "schema_version": SUBJECT_EXECUTION_EVIDENCE_SCHEMA_VERSION,
            "source_case": source_case_value(&self.source_case),
            "subject_manifest": subject_manifest_value(&self.subject_manifest),
            "harness": harness_name(self.harness),
            "subject_disposition": self.subject_disposition.as_ref().map(subject_disposition_name),
            "instrument_state": instrument_state_name(self.instrument_state),
            "diagnostics": diagnostics_value(&self.diagnostics),
            "observations": observation_entries_value(&self.observations),
            "attachments": attachments_value(&self.attachments),
            "semantic_digest": self.semantic_digest.as_str(),
        }))
    }

    /// Recompute and validate this payload's semantic identity.
    pub fn validate(&self) -> Result<(), EvidencePayloadError> {
        validate_stored_digest(
            &self.semantic_digest,
            &execution_semantic_value(
                &self.source_case,
                &self.subject_manifest,
                self.harness,
                self.subject_disposition.as_ref(),
                self.instrument_state,
                &self.diagnostics,
                &self.observations,
            ),
        )
    }
}

/// One exact terminal `execution × observer` observation payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubjectObservationEvidence {
    execution: EvidenceRef,
    observer_manifest: ObserverManifestRef,
    mode: StableId,
    disposition: ObservationDisposition,
    fingerprint: Option<SemanticFingerprint>,
    limitation_reason: Option<StableId>,
    attachments: Vec<BoundedAttachment>,
    semantic_digest: SemanticDigest,
}

impl SubjectObservationEvidence {
    /// Construct one exact terminal observation payload.
    pub fn new(
        execution: &SubjectExecutionEvidence,
        observer_manifest: ObserverManifestRef,
        mode: StableId,
        disposition: ObservationDisposition,
        fingerprint: Option<SemanticFingerprint>,
        limitation_reason: Option<StableId>,
        mut attachments: Vec<BoundedAttachment>,
    ) -> Result<Self, EvidencePayloadError> {
        validate_observation_payload(
            execution,
            &observer_manifest,
            disposition,
            fingerprint.as_ref(),
            limitation_reason.as_ref(),
        )?;
        sort_attachments(&mut attachments);
        let execution_ref = execution.evidence_ref()?;
        let semantic_value = observation_semantic_value(
            &execution_ref,
            &observer_manifest,
            &mode,
            disposition,
            fingerprint.as_ref(),
            limitation_reason.as_ref(),
        );
        let semantic_digest = digest_value(&semantic_value)?;
        Ok(Self {
            execution: execution_ref,
            observer_manifest,
            mode,
            disposition,
            fingerprint,
            limitation_reason,
            attachments,
            semantic_digest,
        })
    }

    /// Exact subject-execution payload reference.
    pub const fn execution(&self) -> &EvidenceRef {
        &self.execution
    }

    /// Exact observer-manifest reference.
    pub const fn observer_manifest(&self) -> &ObserverManifestRef {
        &self.observer_manifest
    }

    /// Exact applicability/range/fresh-edit mode identity.
    pub const fn mode(&self) -> &StableId {
        &self.mode
    }

    /// Terminal observation disposition.
    pub const fn disposition(&self) -> ObservationDisposition {
        self.disposition
    }

    /// Canonical normalized observation fingerprint, when observed.
    pub const fn fingerprint(&self) -> Option<&SemanticFingerprint> {
        self.fingerprint.as_ref()
    }

    /// Typed stable reason for limited or unavailable evidence.
    pub const fn limitation_reason(&self) -> Option<&StableId> {
        self.limitation_reason.as_ref()
    }

    /// Non-authoritative bounded attachments.
    pub fn attachments(&self) -> &[BoundedAttachment] {
        &self.attachments
    }

    /// Semantic digest excluding explicitly non-authoritative attachments.
    pub const fn semantic_digest(&self) -> &SemanticDigest {
        &self.semantic_digest
    }

    /// Exact reference to this observation payload.
    pub fn evidence_ref(&self) -> Result<EvidenceRef, EvidencePayloadError> {
        evidence_ref_for(
            EvidenceKind::SubjectObservation,
            SUBJECT_OBSERVATION_EVIDENCE_SCHEMA_VERSION,
            &self.semantic_digest,
        )
    }

    /// Deterministic canonical semantic JSON used by [`Self::semantic_digest`].
    pub fn canonical_semantic_json(&self) -> Result<String, EvidencePayloadError> {
        canonical_json(&observation_semantic_value(
            &self.execution,
            &self.observer_manifest,
            &self.mode,
            self.disposition,
            self.fingerprint.as_ref(),
            self.limitation_reason.as_ref(),
        ))
    }

    /// Deterministic complete payload JSON including bounded attachments.
    pub fn canonical_payload_json(&self) -> Result<String, EvidencePayloadError> {
        canonical_json(&json!({
            "schema_version": SUBJECT_OBSERVATION_EVIDENCE_SCHEMA_VERSION,
            "execution": evidence_ref_value(&self.execution),
            "observer_manifest": observer_manifest_value(&self.observer_manifest),
            "mode": self.mode.as_str(),
            "disposition": observation_disposition_name(self.disposition),
            "fingerprint": self.fingerprint.as_ref().map(SemanticFingerprint::as_str),
            "limitation_reason": self.limitation_reason.as_ref().map(StableId::as_str),
            "attachments": attachments_value(&self.attachments),
            "semantic_digest": self.semantic_digest.as_str(),
        }))
    }

    /// Recompute and validate this payload's semantic identity.
    pub fn validate(&self) -> Result<(), EvidencePayloadError> {
        require_kind(&self.execution, EvidenceKind::SubjectExecution)?;
        validate_stored_digest(
            &self.semantic_digest,
            &observation_semantic_value(
                &self.execution,
                &self.observer_manifest,
                &self.mode,
                self.disposition,
                self.fingerprint.as_ref(),
                self.limitation_reason.as_ref(),
            ),
        )
    }
}

/// One exact observation scored against one exact reviewed obligation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubjectConformanceEvidence {
    observation: EvidenceRef,
    obligation: ObligationRef,
    outcome: ConformanceOutcome,
    expected_fingerprint: Option<SemanticFingerprint>,
    actual_fingerprint: Option<SemanticFingerprint>,
    mismatch: Option<MismatchDetail>,
    reason: Option<StableId>,
    attachments: Vec<BoundedAttachment>,
    semantic_digest: SemanticDigest,
}

impl SubjectConformanceEvidence {
    /// Construct decisive conformance from a validated scored comparison.
    pub fn scored(
        observation: &SubjectObservationEvidence,
        obligation: ObligationRef,
        comparison: &ScoredComparison,
        mut attachments: Vec<BoundedAttachment>,
    ) -> Result<Self, EvidencePayloadError> {
        validate_scored_conformance(observation, &obligation, comparison)?;
        sort_attachments(&mut attachments);
        let observation_ref = observation.evidence_ref()?;
        let expected_fingerprint = comparison.expected_fingerprint().cloned();
        let actual_fingerprint = comparison.actual_fingerprint().cloned();
        let mismatch = comparison.mismatch_detail().cloned();
        let semantic_value = conformance_semantic_value(
            &observation_ref,
            &obligation,
            comparison.outcome(),
            expected_fingerprint.as_ref(),
            actual_fingerprint.as_ref(),
            mismatch.as_ref(),
            None,
        );
        let semantic_digest = digest_value(&semantic_value)?;
        Ok(Self {
            observation: observation_ref,
            obligation,
            outcome: comparison.outcome(),
            expected_fingerprint,
            actual_fingerprint,
            mismatch,
            reason: None,
            attachments,
            semantic_digest,
        })
    }

    /// Construct a reviewed non-decisive conformance result.
    ///
    /// `Unscored` is intentionally rejected: an unscored observation must not
    /// fabricate an obligation or conformance payload.
    pub fn non_decisive(
        observation: &SubjectObservationEvidence,
        obligation: ObligationRef,
        outcome: ConformanceOutcome,
        reason: StableId,
        mut attachments: Vec<BoundedAttachment>,
    ) -> Result<Self, EvidencePayloadError> {
        if !matches!(outcome, ConformanceOutcome::Unknown | ConformanceOutcome::NotProven) {
            return Err(EvidencePayloadError::InvalidNonDecisiveConformance(outcome));
        }
        validate_obligation_observer(observation, &obligation)?;
        sort_attachments(&mut attachments);
        let observation_ref = observation.evidence_ref()?;
        let semantic_value = conformance_semantic_value(
            &observation_ref,
            &obligation,
            outcome,
            None,
            None,
            None,
            Some(&reason),
        );
        let semantic_digest = digest_value(&semantic_value)?;
        Ok(Self {
            observation: observation_ref,
            obligation,
            outcome,
            expected_fingerprint: None,
            actual_fingerprint: None,
            mismatch: None,
            reason: Some(reason),
            attachments,
            semantic_digest,
        })
    }

    /// Exact subject-observation payload reference.
    pub const fn observation(&self) -> &EvidenceRef {
        &self.observation
    }

    /// Exact reviewed obligation reference.
    pub const fn obligation(&self) -> &ObligationRef {
        &self.obligation
    }

    /// Reviewed conformance outcome.
    pub const fn outcome(&self) -> ConformanceOutcome {
        self.outcome
    }

    /// Expected reviewed fingerprint for decisive conformance.
    pub const fn expected_fingerprint(&self) -> Option<&SemanticFingerprint> {
        self.expected_fingerprint.as_ref()
    }

    /// Actual observed fingerprint for decisive conformance.
    pub const fn actual_fingerprint(&self) -> Option<&SemanticFingerprint> {
        self.actual_fingerprint.as_ref()
    }

    /// Typed mismatch detail, present only for mismatch.
    pub const fn mismatch(&self) -> Option<&MismatchDetail> {
        self.mismatch.as_ref()
    }

    /// Typed reason for a non-decisive result.
    pub const fn reason(&self) -> Option<&StableId> {
        self.reason.as_ref()
    }

    /// Non-authoritative bounded attachments.
    pub fn attachments(&self) -> &[BoundedAttachment] {
        &self.attachments
    }

    /// Semantic digest excluding explicitly non-authoritative attachments.
    pub const fn semantic_digest(&self) -> &SemanticDigest {
        &self.semantic_digest
    }

    /// Exact reference to this conformance payload.
    pub fn evidence_ref(&self) -> Result<EvidenceRef, EvidencePayloadError> {
        evidence_ref_for(
            EvidenceKind::SubjectConformance,
            SUBJECT_CONFORMANCE_EVIDENCE_SCHEMA_VERSION,
            &self.semantic_digest,
        )
    }

    /// Deterministic canonical semantic JSON used by [`Self::semantic_digest`].
    pub fn canonical_semantic_json(&self) -> Result<String, EvidencePayloadError> {
        canonical_json(&conformance_semantic_value(
            &self.observation,
            &self.obligation,
            self.outcome,
            self.expected_fingerprint.as_ref(),
            self.actual_fingerprint.as_ref(),
            self.mismatch.as_ref(),
            self.reason.as_ref(),
        ))
    }

    /// Deterministic complete payload JSON including bounded attachments.
    pub fn canonical_payload_json(&self) -> Result<String, EvidencePayloadError> {
        canonical_json(&json!({
            "schema_version": SUBJECT_CONFORMANCE_EVIDENCE_SCHEMA_VERSION,
            "observation": evidence_ref_value(&self.observation),
            "obligation": obligation_ref_value(&self.obligation),
            "outcome": conformance_outcome_name(self.outcome),
            "expected_fingerprint": self.expected_fingerprint.as_ref().map(SemanticFingerprint::as_str),
            "actual_fingerprint": self.actual_fingerprint.as_ref().map(SemanticFingerprint::as_str),
            "mismatch": self.mismatch.as_ref().map(mismatch_value),
            "reason": self.reason.as_ref().map(StableId::as_str),
            "attachments": attachments_value(&self.attachments),
            "semantic_digest": self.semantic_digest.as_str(),
        }))
    }

    /// Recompute and validate this payload's semantic identity.
    pub fn validate(&self) -> Result<(), EvidencePayloadError> {
        require_kind(&self.observation, EvidenceKind::SubjectObservation)?;
        validate_stored_digest(
            &self.semantic_digest,
            &conformance_semantic_value(
                &self.observation,
                &self.obligation,
                self.outcome,
                self.expected_fingerprint.as_ref(),
                self.actual_fingerprint.as_ref(),
                self.mismatch.as_ref(),
                self.reason.as_ref(),
            ),
        )
    }
}

/// Generate the canonical machine schema for the three accepted payloads.
///
/// The schema is generated directly from the same field contract exercised by
/// payload serialization tests; there is no second checked-in field list.
pub fn parser_comparison_evidence_schema_json() -> Result<String, EvidencePayloadError> {
    canonical_pretty_json(&json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "$id": "https://github.com/EffortlessMetrics/perl-lsp-swarm/schemas/parser-comparison-evidence-v1",
        "title": "Parser comparison exact terminal evidence",
        "description": "Finite vocabularies, advertised terminal-state contradictions, and nested evidence-reference roles are constrained here. This document uses the Draft 2020-12 dialect marker but claims only the repository-supported validation subset; it is not independent full-specification interoperability proof. Constructor validation remains authoritative for producer identity, freshness, digest recomputation, observer/obligation binding, and terminal rules not expressible in this bounded JSON projection. External producer-owned reference versions remain bounded stable IDs because this domain cell does not own their registries; domain-owned terminal payload references are bound to their exact version constants.",
        "x-perl-lsp-validation-profile": "draft-2020-12-supported-subset-v1",
        "oneOf": [
            {"$ref": "#/$defs/subject_execution"},
            {"$ref": "#/$defs/subject_observation"},
            {"$ref": "#/$defs/subject_conformance"}
        ],
        "$defs": {
            "semantic_digest": {
                "type": "string",
                "pattern": "^sha256:[0-9a-f]{64}$"
            },
            "stable_id": {
                "type": "string",
                "pattern": STABLE_ID_PATTERN
            },
            "semantic_fingerprint": {
                "type": "string",
                "minLength": 1,
                "maxLength": 1024,
                "pattern": "^[^\\u0000-\\u001f\\u007f]*$"
            },
            "evidence_ref": object_schema(
                &["kind", "schema_version", "semantic_id", "semantic_digest"],
                json!({
                    "kind": {"enum": [
                        "source_case", "subject_manifest", "observer_manifest", "case_obligation",
                        "subject_execution", "subject_observation", "subject_conformance"
                    ]},
                    "schema_version": {"$ref": "#/$defs/stable_id"},
                    "semantic_id": {"$ref": "#/$defs/stable_id"},
                    "semantic_digest": {"$ref": "#/$defs/semantic_digest"}
                }),
            ),
            "source_case_authority_ref": evidence_ref_for_kind_schema(EvidenceKind::SourceCase),
            "subject_manifest_authority_ref": evidence_ref_for_kind_schema(EvidenceKind::SubjectManifest),
            "observer_manifest_authority_ref": evidence_ref_for_kind_schema(EvidenceKind::ObserverManifest),
            "case_obligation_authority_ref": evidence_ref_for_kind_schema(EvidenceKind::CaseObligation),
            "subject_execution_ref": evidence_ref_for_kind_schema(EvidenceKind::SubjectExecution),
            "subject_observation_ref": evidence_ref_for_kind_schema(EvidenceKind::SubjectObservation),
            "source_case_ref": object_schema(
                &["case_id", "authority", "content_digest"],
                json!({
                    "case_id": {"$ref": "#/$defs/stable_id"},
                    "authority": {"$ref": "#/$defs/source_case_authority_ref"},
                    "content_digest": {"$ref": "#/$defs/semantic_digest"}
                }),
            ),
            "subject_manifest_ref": object_schema(
                &["authority", "role"],
                json!({
                    "authority": {"$ref": "#/$defs/subject_manifest_authority_ref"},
                    "role": {"enum": [
                        "current_upstream_tree_sitter", "historical_tree_sitter_c", "experimental_pest",
                        "native_recursive_descent", "native_tree_sitter_facade"
                    ]}
                }),
            ),
            "observer_manifest_ref": object_schema(
                &["authority", "observer_id", "plane"],
                json!({
                    "authority": {"$ref": "#/$defs/observer_manifest_authority_ref"},
                    "observer_id": {"$ref": "#/$defs/stable_id"},
                    "plane": registered_or_enum_schema(&[
                        "structure", "source_geometry", "recovery", "body_ownership",
                        "incremental_final_state", "query_or_highlight"
                    ])
                }),
            ),
            "obligation_ref": object_schema(
                &["authority", "obligation_id", "observer"],
                json!({
                    "authority": {"$ref": "#/$defs/case_obligation_authority_ref"},
                    "obligation_id": {"$ref": "#/$defs/stable_id"},
                    "observer": {"$ref": "#/$defs/observer_manifest_ref"}
                }),
            ),
            "attachment": object_schema(
                &["kind", "text", "original_bytes", "omitted_bytes", "privacy"],
                json!({
                    "kind": {"$ref": "#/$defs/stable_id"},
                    "text": {"type": "string"},
                    "original_bytes": {"type": "integer", "minimum": 0},
                    "omitted_bytes": {"type": "integer", "minimum": 0},
                    "privacy": {"enum": ["public", "redacted", "private"]}
                }),
            ),
            "diagnostic_summary": object_schema(
                &["diagnostic_count", "recovery_observed", "error_node_observed"],
                json!({
                    "diagnostic_count": {"type": "integer", "minimum": 0},
                    "recovery_observed": {"type": "boolean"},
                    "error_node_observed": {"type": "boolean"}
                }),
            ),
            "observation_entry": object_schema(
                &["plane", "disposition"],
                json!({
                    "plane": registered_or_enum_schema(&[
                        "structure", "source_geometry", "recovery", "body_ownership",
                        "incremental_final_state", "query_or_highlight"
                    ]),
                    "disposition": {"enum": [
                        "observed", "observed_with_limitations", "unsupported", "not_applicable",
                        "not_observable", "not_proven"
                    ]}
                }),
            ),
            "mismatch_detail": object_schema(
                &["class", "first_divergence"],
                json!({
                    "class": registered_or_enum_schema(&[
                        "wrong_kind", "wrong_parent_or_field", "wrong_order_or_ownership",
                        "wrong_value_or_payload", "wrong_range_or_geometry",
                        "wrong_recovery_or_terminal_state", "silently_empty", "wrong_but_plausible"
                    ]),
                    "first_divergence": {
                        "type": "string",
                        "minLength": 1,
                        "maxLength": 256
                    }
                }),
            ),
            "subject_execution": cross_field_object_schema(
                &[
                    "schema_version", "source_case", "subject_manifest", "harness",
                    "subject_disposition", "instrument_state", "diagnostics", "observations", "attachments",
                    "semantic_digest"
                ],
                json!({
                    "schema_version": {"const": SUBJECT_EXECUTION_EVIDENCE_SCHEMA_VERSION},
                    "source_case": {"$ref": "#/$defs/source_case_ref"},
                    "subject_manifest": {"$ref": "#/$defs/subject_manifest_ref"},
                    "harness": {"enum": [
                        "completed", "failed:not_run", "failed:setup_failed", "failed:cancelled",
                        "failed:timed_out", "failed:crashed_or_signalled", "failed:output_limited",
                        "failed:worker_protocol_failed", "failed:supervisor_failed"
                    ]},
                    "subject_disposition": {"oneOf": [
                        {"type": "null"},
                        registered_or_enum_schema(&[
                            "accepted_clean", "accepted_recovered", "rejected", "unsupported",
                            "cancelled", "budget_exhausted", "catastrophic"
                        ])
                    ]},
                    "instrument_state": {"enum": [
                        "complete", "partial", "unavailable", "failed", "truncated", "schema_mismatch"
                    ]},
                    "diagnostics": {"$ref": "#/$defs/diagnostic_summary"},
                    "observations": {"type": "array", "items": {"$ref": "#/$defs/observation_entry"}},
                    "attachments": {"type": "array", "items": {"$ref": "#/$defs/attachment"}},
                    "semantic_digest": {"$ref": "#/$defs/semantic_digest"}
                }),
                json!([
                    {"required": ["harness", "subject_disposition"], "properties": {
                        "harness": {"const": "completed"},
                        "subject_disposition": {"type": "string"}
                    }},
                    {"required": ["harness", "subject_disposition", "instrument_state"], "properties": {
                        "harness": {"enum": [
                            "failed:not_run", "failed:setup_failed", "failed:cancelled",
                            "failed:timed_out", "failed:crashed_or_signalled", "failed:output_limited",
                            "failed:worker_protocol_failed", "failed:supervisor_failed"
                        ]},
                        "subject_disposition": {"const": null},
                        "instrument_state": {"enum": [
                            "partial", "unavailable", "failed", "truncated", "schema_mismatch"
                        ]}
                    }}
                ]),
            ),
            "subject_observation": cross_field_object_schema(
                &[
                    "schema_version", "execution", "observer_manifest", "mode", "disposition",
                    "fingerprint", "limitation_reason", "attachments", "semantic_digest"
                ],
                json!({
                    "schema_version": {"const": SUBJECT_OBSERVATION_EVIDENCE_SCHEMA_VERSION},
                    "execution": {"$ref": "#/$defs/subject_execution_ref"},
                    "observer_manifest": {"$ref": "#/$defs/observer_manifest_ref"},
                    "mode": {"$ref": "#/$defs/stable_id"},
                    "disposition": {"enum": [
                        "observed", "observed_with_limitations", "unsupported", "not_applicable",
                        "not_observable", "not_proven"
                    ]},
                    "fingerprint": {"oneOf": [{"$ref": "#/$defs/semantic_fingerprint"}, {"type": "null"}]},
                    "limitation_reason": {"oneOf": [{"$ref": "#/$defs/stable_id"}, {"type": "null"}]},
                    "attachments": {"type": "array", "items": {"$ref": "#/$defs/attachment"}},
                    "semantic_digest": {"$ref": "#/$defs/semantic_digest"}
                }),
                json!([
                    {"required": ["disposition", "fingerprint", "limitation_reason"], "properties": {
                        "disposition": {"const": "observed"},
                        "fingerprint": {"type": "string"},
                        "limitation_reason": {"type": "null"}
                    }},
                    {"required": ["disposition", "fingerprint", "limitation_reason"], "properties": {
                        "disposition": {"const": "observed_with_limitations"},
                        "fingerprint": {"type": "string"},
                        "limitation_reason": {"type": "string"}
                    }},
                    {"required": ["disposition", "fingerprint", "limitation_reason"], "properties": {
                        "disposition": {"enum": [
                            "unsupported", "not_applicable", "not_observable", "not_proven"
                        ]},
                        "fingerprint": {"type": "null"},
                        "limitation_reason": {"type": "string"}
                    }}
                ]),
            ),
            "subject_conformance": cross_field_object_schema(
                &[
                    "schema_version", "observation", "obligation", "outcome",
                    "expected_fingerprint", "actual_fingerprint", "mismatch", "reason",
                    "attachments", "semantic_digest"
                ],
                json!({
                    "schema_version": {"const": SUBJECT_CONFORMANCE_EVIDENCE_SCHEMA_VERSION},
                    "observation": {"$ref": "#/$defs/subject_observation_ref"},
                    "obligation": {"$ref": "#/$defs/obligation_ref"},
                    "outcome": {"enum": [
                        "matches_expected", "mismatch", "unscored", "unknown", "not_proven"
                    ]},
                    "expected_fingerprint": {"oneOf": [{"$ref": "#/$defs/semantic_fingerprint"}, {"type": "null"}]},
                    "actual_fingerprint": {"oneOf": [{"$ref": "#/$defs/semantic_fingerprint"}, {"type": "null"}]},
                    "mismatch": {"oneOf": [
                        {"type": "null"},
                        {"$ref": "#/$defs/mismatch_detail"}
                    ]},
                    "reason": {"oneOf": [{"$ref": "#/$defs/stable_id"}, {"type": "null"}]},
                    "attachments": {"type": "array", "items": {"$ref": "#/$defs/attachment"}},
                    "semantic_digest": {"$ref": "#/$defs/semantic_digest"}
                }),
                json!([
                    {"required": [
                        "outcome", "expected_fingerprint", "actual_fingerprint", "mismatch", "reason"
                    ], "properties": {
                        "outcome": {"const": "matches_expected"},
                        "expected_fingerprint": {"type": "string"},
                        "actual_fingerprint": {"type": "string"},
                        "mismatch": {"type": "null"},
                        "reason": {"type": "null"}
                    }},
                    {"required": [
                        "outcome", "expected_fingerprint", "actual_fingerprint", "mismatch", "reason"
                    ], "properties": {
                        "outcome": {"const": "mismatch"},
                        "expected_fingerprint": {"type": "string"},
                        "actual_fingerprint": {"type": "string"},
                        "mismatch": {"$ref": "#/$defs/mismatch_detail"},
                        "reason": {"type": "null"}
                    }},
                    {"required": ["outcome", "expected_fingerprint", "actual_fingerprint", "mismatch", "reason"], "properties": {
                        "outcome": {"enum": ["unknown", "not_proven"]},
                        "expected_fingerprint": {"type": "null"},
                        "actual_fingerprint": {"type": "null"},
                        "mismatch": {"type": "null"},
                        "reason": {"type": "string"}
                    }}
                ]),
            )
        }
    }))
}

fn registered_or_enum_schema(values: &[&str]) -> Value {
    json!({
        "oneOf": [
            {"enum": values},
            {"type": "string", "pattern": REGISTERED_ID_PATTERN}
        ]
    })
}

const STABLE_ID_PATTERN: &str = "^[a-z0-9][a-z0-9._-]{0,127}$";
const REGISTERED_ID_PATTERN: &str = "^registered:[a-z0-9][a-z0-9._-]{0,127}$";

fn evidence_ref_for_kind_schema(kind: EvidenceKind) -> Value {
    let schema_version = match kind {
        EvidenceKind::SubjectExecution => {
            json!({"const": SUBJECT_EXECUTION_EVIDENCE_SCHEMA_VERSION})
        }
        EvidenceKind::SubjectObservation => {
            json!({"const": SUBJECT_OBSERVATION_EVIDENCE_SCHEMA_VERSION})
        }
        EvidenceKind::SubjectConformance => {
            json!({"const": SUBJECT_CONFORMANCE_EVIDENCE_SCHEMA_VERSION})
        }
        EvidenceKind::SourceCase
        | EvidenceKind::SubjectManifest
        | EvidenceKind::ObserverManifest
        | EvidenceKind::CaseObligation => json!({"$ref": "#/$defs/stable_id"}),
    };
    object_schema(
        &["kind", "schema_version", "semantic_id", "semantic_digest"],
        json!({
            "kind": {"const": kind.as_str()},
            "schema_version": schema_version,
            "semantic_id": {"$ref": "#/$defs/stable_id"},
            "semantic_digest": {"$ref": "#/$defs/semantic_digest"}
        }),
    )
}

fn cross_field_object_schema(required: &[&str], properties: Value, one_of: Value) -> Value {
    let mut schema = object_schema(required, properties);
    schema["oneOf"] = one_of;
    schema
}

/// Fail-closed durable evidence construction/validation error.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum EvidencePayloadError {
    /// Evidence reference kind does not match the typed wrapper.
    WrongEvidenceKind {
        /// Required kind.
        expected: EvidenceKind,
        /// Actual kind.
        actual: EvidenceKind,
    },
    /// Semantic digest did not use the exact accepted format.
    InvalidSemanticDigest(String),
    /// Exact subject role disagrees with the execution payload.
    SubjectRoleMismatch,
    /// Observation/result combination is internally contradictory.
    InvalidObservationDisposition,
    /// An observation payload was created for a plane absent from execution evidence.
    ObservationPlaneMismatch,
    /// Decisive conformance requires an exactly observed plane.
    ConformanceRequiresObservedPlane,
    /// Obligation and observation use different observer identities or planes.
    ObligationObserverMismatch,
    /// Scored comparison identity disagrees with the exact obligation/observer.
    ScoredComparisonIdentityMismatch,
    /// Scored actual fingerprint disagrees with the observation payload.
    ScoredObservationFingerprintMismatch,
    /// Non-decisive constructor received a decisive or unscored outcome.
    InvalidNonDecisiveConformance(ConformanceOutcome),
    /// Stored semantic digest differs from canonical semantic bytes.
    SemanticDigestMismatch,
    /// Checked stable evidence value was invalid.
    EvidenceValue(EvidenceValueError),
    /// Canonical JSON serialization failed.
    Serialization(String),
}

impl fmt::Display for EvidencePayloadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WrongEvidenceKind { expected, actual } => write!(
                formatter,
                "expected {} evidence reference, got {}",
                expected.as_str(),
                actual.as_str()
            ),
            Self::InvalidSemanticDigest(value) => {
                write!(formatter, "invalid semantic digest: {value}")
            }
            Self::SubjectRoleMismatch => formatter
                .write_str("subject manifest role disagrees with generic subject execution role"),
            Self::InvalidObservationDisposition => formatter.write_str(
                "observation disposition, fingerprint, limitation, or execution state is invalid",
            ),
            Self::ObservationPlaneMismatch => formatter.write_str(
                "observation plane is absent or has a different terminal disposition in execution evidence",
            ),
            Self::ConformanceRequiresObservedPlane => {
                formatter.write_str("decisive conformance requires an exactly observed plane")
            }
            Self::ObligationObserverMismatch => formatter.write_str(
                "reviewed obligation observer or plane disagrees with observation evidence",
            ),
            Self::ScoredComparisonIdentityMismatch => formatter.write_str(
                "scored comparison observer, obligation, or plane identity disagrees with evidence",
            ),
            Self::ScoredObservationFingerprintMismatch => formatter.write_str(
                "scored comparison actual fingerprint disagrees with observation evidence",
            ),
            Self::InvalidNonDecisiveConformance(outcome) => write!(
                formatter,
                "conformance outcome {outcome:?} is not valid for reviewed non-decisive evidence"
            ),
            Self::SemanticDigestMismatch => formatter
                .write_str("stored semantic digest disagrees with canonical semantic bytes"),
            Self::EvidenceValue(error) => error.fmt(formatter),
            Self::Serialization(message) => {
                write!(formatter, "canonical evidence serialization failed: {message}")
            }
        }
    }
}

impl Error for EvidencePayloadError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::EvidenceValue(error) => Some(error),
            _ => None,
        }
    }
}

impl From<EvidenceValueError> for EvidencePayloadError {
    fn from(error: EvidenceValueError) -> Self {
        Self::EvidenceValue(error)
    }
}

fn require_kind(
    reference: &EvidenceRef,
    expected: EvidenceKind,
) -> Result<(), EvidencePayloadError> {
    if reference.kind() == expected {
        Ok(())
    } else {
        Err(EvidencePayloadError::WrongEvidenceKind { expected, actual: reference.kind() })
    }
}

fn evidence_ref_for(
    kind: EvidenceKind,
    schema_version: &str,
    digest: &SemanticDigest,
) -> Result<EvidenceRef, EvidencePayloadError> {
    let schema_version = StableId::new(schema_version)?;
    let semantic_id = StableId::new(format!("{}.{}", kind.as_str(), digest.hex()))?;
    Ok(EvidenceRef::new(kind, schema_version, semantic_id, digest.clone()))
}

fn sort_attachments(attachments: &mut [BoundedAttachment]) {
    attachments.sort_by(|left, right| {
        left.kind
            .cmp(&right.kind)
            .then_with(|| left.privacy.cmp(&right.privacy))
            .then_with(|| left.text.as_str().cmp(right.text.as_str()))
            .then_with(|| left.text.original_bytes().cmp(&right.text.original_bytes()))
            .then_with(|| left.text.omitted_bytes().cmp(&right.text.omitted_bytes()))
    });
}

fn validate_observation_payload(
    execution: &SubjectExecutionEvidence,
    observer_manifest: &ObserverManifestRef,
    disposition: ObservationDisposition,
    fingerprint: Option<&SemanticFingerprint>,
    limitation_reason: Option<&StableId>,
) -> Result<(), EvidencePayloadError> {
    if execution.observation(observer_manifest.plane()) != Some(disposition) {
        return Err(EvidencePayloadError::ObservationPlaneMismatch);
    }
    let execution_completed = execution.harness() == HarnessOutcome::Completed;
    let valid = match disposition {
        ObservationDisposition::Observed => {
            execution_completed
                && execution.instrument_state() == InstrumentState::Complete
                && fingerprint.is_some()
                && limitation_reason.is_none()
        }
        ObservationDisposition::ObservedWithLimitations => {
            execution_completed
                && matches!(
                    execution.instrument_state(),
                    InstrumentState::Complete
                        | InstrumentState::Partial
                        | InstrumentState::Truncated
                )
                && fingerprint.is_some()
                && limitation_reason.is_some()
        }
        ObservationDisposition::Unsupported
        | ObservationDisposition::NotApplicable
        | ObservationDisposition::NotObservable
        | ObservationDisposition::NotProven => fingerprint.is_none() && limitation_reason.is_some(),
    };
    if valid { Ok(()) } else { Err(EvidencePayloadError::InvalidObservationDisposition) }
}

fn validate_obligation_observer(
    observation: &SubjectObservationEvidence,
    obligation: &ObligationRef,
) -> Result<(), EvidencePayloadError> {
    if observation.observer_manifest() == obligation.observer() {
        Ok(())
    } else {
        Err(EvidencePayloadError::ObligationObserverMismatch)
    }
}

fn validate_scored_conformance(
    observation: &SubjectObservationEvidence,
    obligation: &ObligationRef,
    comparison: &ScoredComparison,
) -> Result<(), EvidencePayloadError> {
    validate_obligation_observer(observation, obligation)?;
    if observation.disposition() != ObservationDisposition::Observed {
        return Err(EvidencePayloadError::ConformanceRequiresObservedPlane);
    }
    if !matches!(
        comparison.outcome(),
        ConformanceOutcome::MatchesExpected | ConformanceOutcome::Mismatch
    ) || comparison.observer_id() != Some(obligation.observer().observer_id())
        || comparison.expectation_id() != Some(obligation.obligation_id())
        || comparison.plane() != obligation.observer().plane()
    {
        return Err(EvidencePayloadError::ScoredComparisonIdentityMismatch);
    }
    if comparison.actual_fingerprint() != observation.fingerprint() {
        return Err(EvidencePayloadError::ScoredObservationFingerprintMismatch);
    }
    Ok(())
}

fn execution_semantic_value(
    source_case: &SourceCaseRef,
    subject_manifest: &SubjectManifestRef,
    harness: HarnessOutcome,
    subject_disposition: Option<&SubjectDisposition>,
    instrument_state: InstrumentState,
    diagnostics: &DiagnosticSummary,
    observations: &BTreeMap<ObservationPlane, ObservationDisposition>,
) -> Value {
    json!({
        "schema_version": SUBJECT_EXECUTION_EVIDENCE_SCHEMA_VERSION,
        "source_case": source_case_value(source_case),
        "subject_manifest": subject_manifest_value(subject_manifest),
        "harness": harness_name(harness),
        "subject_disposition": subject_disposition.map(subject_disposition_name),
        "instrument_state": instrument_state_name(instrument_state),
        "diagnostics": diagnostics_value(diagnostics),
        "observations": observation_entries_value(observations),
    })
}

fn observation_semantic_value(
    execution: &EvidenceRef,
    observer_manifest: &ObserverManifestRef,
    mode: &StableId,
    disposition: ObservationDisposition,
    fingerprint: Option<&SemanticFingerprint>,
    limitation_reason: Option<&StableId>,
) -> Value {
    json!({
        "schema_version": SUBJECT_OBSERVATION_EVIDENCE_SCHEMA_VERSION,
        "execution": evidence_ref_value(execution),
        "observer_manifest": observer_manifest_value(observer_manifest),
        "mode": mode.as_str(),
        "disposition": observation_disposition_name(disposition),
        "fingerprint": fingerprint.map(SemanticFingerprint::as_str),
        "limitation_reason": limitation_reason.map(StableId::as_str),
    })
}

fn conformance_semantic_value(
    observation: &EvidenceRef,
    obligation: &ObligationRef,
    outcome: ConformanceOutcome,
    expected_fingerprint: Option<&SemanticFingerprint>,
    actual_fingerprint: Option<&SemanticFingerprint>,
    mismatch: Option<&MismatchDetail>,
    reason: Option<&StableId>,
) -> Value {
    json!({
        "schema_version": SUBJECT_CONFORMANCE_EVIDENCE_SCHEMA_VERSION,
        "observation": evidence_ref_value(observation),
        "obligation": obligation_ref_value(obligation),
        "outcome": conformance_outcome_name(outcome),
        "expected_fingerprint": expected_fingerprint.map(SemanticFingerprint::as_str),
        "actual_fingerprint": actual_fingerprint.map(SemanticFingerprint::as_str),
        "mismatch": mismatch.map(mismatch_value),
        "reason": reason.map(StableId::as_str),
    })
}

fn evidence_ref_value(reference: &EvidenceRef) -> Value {
    json!({
        "kind": reference.kind().as_str(),
        "schema_version": reference.schema_version().as_str(),
        "semantic_id": reference.semantic_id().as_str(),
        "semantic_digest": reference.semantic_digest().as_str(),
    })
}

fn source_case_value(reference: &SourceCaseRef) -> Value {
    json!({
        "case_id": reference.case_id().as_str(),
        "authority": evidence_ref_value(reference.authority()),
        "content_digest": reference.content_digest().as_str(),
    })
}

fn subject_manifest_value(reference: &SubjectManifestRef) -> Value {
    json!({
        "authority": evidence_ref_value(reference.authority()),
        "role": subject_role_name(reference.role()),
    })
}

fn observer_manifest_value(reference: &ObserverManifestRef) -> Value {
    json!({
        "authority": evidence_ref_value(reference.authority()),
        "observer_id": reference.observer_id().as_str(),
        "plane": observation_plane_name(reference.plane()),
    })
}

fn obligation_ref_value(reference: &ObligationRef) -> Value {
    json!({
        "authority": evidence_ref_value(reference.authority()),
        "obligation_id": reference.obligation_id().as_str(),
        "observer": observer_manifest_value(reference.observer()),
    })
}

fn diagnostics_value(summary: &DiagnosticSummary) -> Value {
    json!({
        "diagnostic_count": summary.diagnostic_count(),
        "recovery_observed": summary.recovery_observed(),
        "error_node_observed": summary.error_node_observed(),
    })
}

fn observation_entries_value(
    observations: &BTreeMap<ObservationPlane, ObservationDisposition>,
) -> Value {
    Value::Array(
        observations
            .iter()
            .map(|(plane, disposition)| {
                json!({
                    "plane": observation_plane_name(plane),
                    "disposition": observation_disposition_name(*disposition),
                })
            })
            .collect(),
    )
}

fn attachments_value(attachments: &[BoundedAttachment]) -> Value {
    Value::Array(
        attachments
            .iter()
            .map(|attachment| {
                json!({
                    "kind": attachment.kind().as_str(),
                    "text": attachment.text().as_str(),
                    "original_bytes": attachment.text().original_bytes(),
                    "omitted_bytes": attachment.text().omitted_bytes(),
                    "privacy": attachment.privacy().as_str(),
                })
            })
            .collect(),
    )
}

fn mismatch_value(mismatch: &MismatchDetail) -> Value {
    json!({
        "class": mismatch_class_name(mismatch.class()),
        "first_divergence": mismatch.first_divergence().as_str(),
    })
}

fn harness_name(outcome: HarnessOutcome) -> String {
    match outcome {
        HarnessOutcome::Completed => "completed".to_owned(),
        HarnessOutcome::Failed(failure) => format!("failed:{}", harness_failure_name(failure)),
    }
}

fn harness_failure_name(failure: HarnessFailure) -> &'static str {
    match failure {
        HarnessFailure::NotRun => "not_run",
        HarnessFailure::SetupFailed => "setup_failed",
        HarnessFailure::Cancelled => "cancelled",
        HarnessFailure::TimedOut => "timed_out",
        HarnessFailure::CrashedOrSignalled => "crashed_or_signalled",
        HarnessFailure::OutputLimited => "output_limited",
        HarnessFailure::WorkerProtocolFailed => "worker_protocol_failed",
        HarnessFailure::SupervisorFailed => "supervisor_failed",
    }
}

fn subject_disposition_name(disposition: &SubjectDisposition) -> String {
    match disposition {
        SubjectDisposition::AcceptedClean => "accepted_clean".to_owned(),
        SubjectDisposition::AcceptedRecovered => "accepted_recovered".to_owned(),
        SubjectDisposition::Rejected => "rejected".to_owned(),
        SubjectDisposition::Unsupported => "unsupported".to_owned(),
        SubjectDisposition::Cancelled => "cancelled".to_owned(),
        SubjectDisposition::BudgetExhausted => "budget_exhausted".to_owned(),
        SubjectDisposition::Catastrophic => "catastrophic".to_owned(),
        SubjectDisposition::Registered(id) => format!("registered:{}", id.as_str()),
    }
}

fn instrument_state_name(state: InstrumentState) -> &'static str {
    match state {
        InstrumentState::Complete => "complete",
        InstrumentState::Partial => "partial",
        InstrumentState::Unavailable => "unavailable",
        InstrumentState::Failed => "failed",
        InstrumentState::Truncated => "truncated",
        InstrumentState::SchemaMismatch => "schema_mismatch",
    }
}

fn observation_disposition_name(disposition: ObservationDisposition) -> &'static str {
    match disposition {
        ObservationDisposition::Observed => "observed",
        ObservationDisposition::ObservedWithLimitations => "observed_with_limitations",
        ObservationDisposition::Unsupported => "unsupported",
        ObservationDisposition::NotApplicable => "not_applicable",
        ObservationDisposition::NotObservable => "not_observable",
        ObservationDisposition::NotProven => "not_proven",
    }
}

fn conformance_outcome_name(outcome: ConformanceOutcome) -> &'static str {
    match outcome {
        ConformanceOutcome::MatchesExpected => "matches_expected",
        ConformanceOutcome::Mismatch => "mismatch",
        ConformanceOutcome::Unscored => "unscored",
        ConformanceOutcome::Unknown => "unknown",
        ConformanceOutcome::NotProven => "not_proven",
    }
}

fn subject_role_name(role: SubjectRole) -> &'static str {
    match role {
        SubjectRole::CurrentUpstreamTreeSitter => "current_upstream_tree_sitter",
        SubjectRole::HistoricalTreeSitterC => "historical_tree_sitter_c",
        SubjectRole::ExperimentalPest => "experimental_pest",
        SubjectRole::NativeRecursiveDescent => "native_recursive_descent",
        SubjectRole::NativeTreeSitterFacade => "native_tree_sitter_facade",
    }
}

fn observation_plane_name(plane: &ObservationPlane) -> String {
    match plane {
        ObservationPlane::Structure => "structure".to_owned(),
        ObservationPlane::SourceGeometry => "source_geometry".to_owned(),
        ObservationPlane::Recovery => "recovery".to_owned(),
        ObservationPlane::BodyOwnership => "body_ownership".to_owned(),
        ObservationPlane::IncrementalFinalState => "incremental_final_state".to_owned(),
        ObservationPlane::QueryOrHighlight => "query_or_highlight".to_owned(),
        ObservationPlane::Registered(id) => format!("registered:{}", id.as_str()),
    }
}

fn mismatch_class_name(class: &MismatchClass) -> String {
    match class {
        MismatchClass::WrongKind => "wrong_kind".to_owned(),
        MismatchClass::WrongParentOrField => "wrong_parent_or_field".to_owned(),
        MismatchClass::WrongOrderOrOwnership => "wrong_order_or_ownership".to_owned(),
        MismatchClass::WrongValueOrPayload => "wrong_value_or_payload".to_owned(),
        MismatchClass::WrongRangeOrGeometry => "wrong_range_or_geometry".to_owned(),
        MismatchClass::WrongRecoveryOrTerminalState => {
            "wrong_recovery_or_terminal_state".to_owned()
        }
        MismatchClass::SilentlyEmpty => "silently_empty".to_owned(),
        MismatchClass::WrongButPlausible => "wrong_but_plausible".to_owned(),
        MismatchClass::Registered(id) => format!("registered:{}", id.as_str()),
    }
}

fn object_schema(required: &[&str], properties: Value) -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": required,
        "properties": properties,
    })
}

fn digest_value(value: &Value) -> Result<SemanticDigest, EvidencePayloadError> {
    canonical_json(value).map(|canonical| SemanticDigest::from_bytes(canonical.as_bytes()))
}

fn validate_stored_digest(
    stored: &SemanticDigest,
    semantic_value: &Value,
) -> Result<(), EvidencePayloadError> {
    if &digest_value(semantic_value)? == stored {
        Ok(())
    } else {
        Err(EvidencePayloadError::SemanticDigestMismatch)
    }
}

fn canonical_json(value: &Value) -> Result<String, EvidencePayloadError> {
    serde_json::to_string(&sorted_json_value(value))
        .map_err(|error| EvidencePayloadError::Serialization(error.to_string()))
}

fn canonical_pretty_json(value: &Value) -> Result<String, EvidencePayloadError> {
    serde_json::to_string_pretty(&sorted_json_value(value))
        .map_err(|error| EvidencePayloadError::Serialization(error.to_string()))
}

/// Recursively sort JSON object keys before deterministic serialization.
///
/// This is deterministic JSON for this repository, not RFC 8785 canonical
/// JSON: it does not claim the full RFC 8785 number or string canonicalization
/// contract.
fn sorted_json_value(value: &Value) -> Value {
    match value {
        Value::Object(object) => {
            let mut entries = object.iter().collect::<Vec<_>>();
            entries.sort_unstable_by(|(left, _), (right, _)| left.cmp(right));
            let mut sorted = serde_json::Map::new();
            for (key, value) in entries {
                sorted.insert(key.clone(), sorted_json_value(value));
            }
            Value::Object(sorted)
        }
        Value::Array(values) => {
            Value::Array(values.iter().map(sorted_json_value).collect::<Vec<_>>())
        }
        _ => value.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic_json_sorts_independently_constructed_nested_objects() {
        let first = json!({
            "z": {"b": 2, "a": [{"d": 4, "c": 3}]},
            "a": 1,
        });
        let second = json!({
            "a": 1,
            "z": {"a": [{"c": 3, "d": 4}], "b": 2},
        });

        assert_eq!(
            canonical_json(&first).expect("first JSON should serialize"),
            canonical_json(&second).expect("second JSON should serialize")
        );
        assert_eq!(
            canonical_pretty_json(&first).expect("first pretty JSON should serialize"),
            canonical_pretty_json(&second).expect("second pretty JSON should serialize")
        );
        assert_eq!(
            canonical_json(&first).expect("JSON should serialize"),
            r#"{"a":1,"z":{"a":[{"c":3,"d":4}],"b":2}}"#
        );
    }

    fn schema_enum_contains(schema: &Value, expected: &str) -> bool {
        if schema
            .get("enum")
            .and_then(Value::as_array)
            .is_some_and(|values| values.iter().any(|value| value.as_str() == Some(expected)))
        {
            return true;
        }
        schema.as_object().is_some_and(|object| {
            object.values().any(|value| schema_enum_contains(value, expected))
        }) || schema
            .as_array()
            .is_some_and(|values| values.iter().any(|value| schema_enum_contains(value, expected)))
    }

    #[test]
    fn mapper_outputs_remain_in_machine_schema_vocabularies() -> Result<(), Box<dyn Error>> {
        let schema_text = parser_comparison_evidence_schema_json()?;
        let schema: Value = serde_json::from_str(&schema_text)?;
        let custom = StableId::new("custom")?;
        let finite = [
            subject_role_name(SubjectRole::CurrentUpstreamTreeSitter).to_owned(),
            subject_role_name(SubjectRole::HistoricalTreeSitterC).to_owned(),
            subject_role_name(SubjectRole::ExperimentalPest).to_owned(),
            subject_role_name(SubjectRole::NativeRecursiveDescent).to_owned(),
            subject_role_name(SubjectRole::NativeTreeSitterFacade).to_owned(),
            instrument_state_name(InstrumentState::Complete).to_owned(),
            instrument_state_name(InstrumentState::Partial).to_owned(),
            instrument_state_name(InstrumentState::Unavailable).to_owned(),
            instrument_state_name(InstrumentState::Failed).to_owned(),
            instrument_state_name(InstrumentState::Truncated).to_owned(),
            instrument_state_name(InstrumentState::SchemaMismatch).to_owned(),
            observation_disposition_name(ObservationDisposition::Observed).to_owned(),
            observation_disposition_name(ObservationDisposition::ObservedWithLimitations)
                .to_owned(),
            observation_disposition_name(ObservationDisposition::Unsupported).to_owned(),
            observation_disposition_name(ObservationDisposition::NotApplicable).to_owned(),
            observation_disposition_name(ObservationDisposition::NotObservable).to_owned(),
            observation_disposition_name(ObservationDisposition::NotProven).to_owned(),
            conformance_outcome_name(ConformanceOutcome::MatchesExpected).to_owned(),
            conformance_outcome_name(ConformanceOutcome::Mismatch).to_owned(),
            conformance_outcome_name(ConformanceOutcome::Unscored).to_owned(),
            conformance_outcome_name(ConformanceOutcome::Unknown).to_owned(),
            conformance_outcome_name(ConformanceOutcome::NotProven).to_owned(),
            harness_name(HarnessOutcome::Completed),
            harness_name(HarnessOutcome::Failed(HarnessFailure::TimedOut)),
            subject_disposition_name(&SubjectDisposition::AcceptedClean),
            mismatch_class_name(&MismatchClass::WrongKind),
            observation_plane_name(&ObservationPlane::Structure),
        ];
        for value in finite {
            assert!(schema_enum_contains(&schema, &value), "mapper output drifted: {value}");
        }

        for value in [
            subject_disposition_name(&SubjectDisposition::Registered(custom.clone())),
            mismatch_class_name(&MismatchClass::Registered(custom.clone())),
            observation_plane_name(&ObservationPlane::Registered(custom)),
        ] {
            assert!(value.starts_with("registered:"));
        }
        Ok(())
    }
}
