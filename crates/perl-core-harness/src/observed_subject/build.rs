//! Strict construction and deterministic digests for observed runner
//! subjects (#12287).
//!
//! The join consumes one matrix-validated observed discovery receipt, one
//! independently reconstructed runner plan, one parent-bound effective-
//! invocation trace receipt, an exact producer subject identity, and (for
//! instrument-captured observations) the exact ordinary/instrumented transfer
//! relation. Inputs that cannot establish the joined claim identity are
//! rejected outright with named-field errors; content shortfalls that are
//! observable facts (missing, extra, duplicate, conflicting, partially
//! observed invocations; unbound transfer relations) become typed result
//! states and named diagnostics, never silent completions.

use crate::invocation_trace::model::{
    EffectiveInvocationRow, FieldStateCounts, InvocationObservationState, ProjectionRecord,
    TraceRowDisposition,
};
use crate::invocation_trace::validate::validate_invocation_trace_receipt;
use crate::observed_discovery::build::{
    sha256_json, validate_reference, validate_sha256_field, validate_target_id,
};
use crate::observed_discovery::model::LineFraming as FingerprintFrame;
use crate::observed_discovery::model::{
    DiscoveryObservationState, DiscoveryPayload, EvidenceClass, ReceiptFreshness,
};
use crate::observed_subject::model::{
    JoinWork, OBSERVED_RUNNER_SUBJECT_SCHEMA_VERSION, OBSERVED_SUBJECT_CLAIM_BOUNDARY,
    ObservedRunnerSubjectInput, ObservedRunnerSubjectPayload, ObservedRunnerSubjectRow,
    ObservedRunnerSubjectV1, ObservedSubjectBindings, ObservedSubjectState,
    OrdinaryInstrumentedEquivalenceIdentity, ProducerSubjectIdentity, ProjectedInvocation,
    SubjectDiagnostic, SubjectJoinDisposition,
};
use crate::runner_model::RunnerPlan;
use serde::Serialize;
use std::collections::BTreeMap;

/// Build one observed runner subject (#12287) from its exact input set under
/// the pinned target matrix authority.
pub fn build_observed_runner_subject(
    matrix: &crate::model::UpstreamTargetMatrix,
    input: &ObservedRunnerSubjectInput,
) -> Result<ObservedRunnerSubjectV1, String> {
    // Stage 1: shape-validate caller-supplied references.
    validate_producer_shape(&input.producer)?;
    if let Some(equivalence) = &input.equivalence {
        validate_equivalence_shape(equivalence)?;
    }

    // Stage 2: bind every receipt through its sanctioned fail-closed adapter.
    crate::observed_discovery::validate_observed_discovery_receipt(matrix, &input.discovery)?;
    let discovery = &input.discovery.payload;
    bind_producer(&input.producer, discovery)?;
    validate_plan_binding(matrix, &input.plan, discovery)?;
    validate_invocation_trace_receipt(&input.discovery, &input.trace)?;

    // Stage 3: perform the denominator arithmetic.
    let mut diagnostics: Vec<SubjectDiagnostic> = Vec::new();
    let mut work = base_work();
    work.plan_validations_accepted = 1;

    // Transfer relation first: its outcome gates the aggregate state but never
    // the arithmetic, so every shortfall stays visible at row level too.
    let (equivalence, equivalence_diagnostic, instrumented_without_equivalence) =
        transfer_relation(input);
    if let Some(diagnostic) = &equivalence_diagnostic {
        diagnostics.push(diagnostic.clone());
    }

    // Admission side: accepted members in original discovery order.
    let mut rows: Vec<ObservedRunnerSubjectRow> = Vec::new();
    work.discovery_rows_considered = discovery.rows.len() as u64;
    let mut upstream_state = None;
    if !discovery.state.is_complete() {
        upstream_state = upstream_inherited_state(discovery.state, true);
        diagnostics.push(SubjectDiagnostic {
            field: "discovery.state".to_string(),
            member_path: None,
            detail: format!(
                "observed discovery state is {:?}; the joined subject \
                             cannot be complete",
                discovery.state
            ),
        });
    }
    for row in &discovery.rows {
        match &row.disposition {
            crate::observed_discovery::model::MemberDisposition::Accepted => {
                work.discovery_accepted_rows += 1;
            }
            crate::observed_discovery::model::MemberDisposition::UnsupportedSourceForm => {
                work.unsupported_source_form_rows += 1;
                diagnostics.push(SubjectDiagnostic {
                    field: "discovery.rows".to_string(),
                    member_path: None,
                    detail: format!(
                        "discovery ordinal {} carries an unsupported source form and never \
                         enters membership",
                        row.ordinal
                    ),
                });
            }
            other => {
                if !matches!(
                    other,
                    crate::observed_discovery::model::MemberDisposition::DuplicateOfCanonical { .. }
                ) {
                    diagnostics.push(SubjectDiagnostic {
                        field: "discovery.rows".to_string(),
                        member_path: row.canonical_path().map(str::to_string),
                        detail: format!("discovery ordinal {} is {:?}", row.ordinal, other),
                    });
                }
            }
        }
    }

    let trace = &input.trace.payload;
    // An incomplete or truncated trace stream leaves the invocation side of
    // the denominator unknowable even when discovery is complete.
    let trace_stream_complete = trace.trace_decode.is_complete() && !trace.trace.truncated;
    if !trace_stream_complete && upstream_state.is_none() {
        upstream_state = Some(ObservedSubjectState::NotProven);
        diagnostics.push(SubjectDiagnostic {
            field: "trace.decode".to_string(),
            member_path: None,
            detail: format!(
                "effective-invocation observation stream is incomplete or truncated \
                 ({:?}, truncated={}); the invocation denominator cannot be proven",
                trace.trace_decode, trace.trace.truncated
            ),
        });
    }
    let (groups, complete_count, partial_count, mismatch_count) = grouped_projections(&trace.rows);
    work.invocation_rows_considered = trace.rows.len() as u64;
    work.complete_invocation_rows = complete_count;
    work.partial_invocation_rows = partial_count;
    work.subject_mismatch_rows = mismatch_count;

    // Row-level subject mismatches are named evidence, never silent members.
    for row in trace.rows.iter().filter(|row| {
        matches!(row.disposition, TraceRowDisposition::Accepted)
            && row.state == InvocationObservationState::SubjectMismatch
    }) {
        diagnostics.push(SubjectDiagnostic {
            field: "invocation_subject_bindings".to_string(),
            member_path: Some(row.subject.parent_member_path.clone()),
            detail: format!(
                "invocation row {} claims a subject that does not bind this joined membership",
                row.row_id
            ),
        });
    }

    let mut outcomes = JoinOutcomes {
        upstream: upstream_state,
        subject_mismatch_rows: mismatch_count,
        missing: 0,
        extra: 0,
        conflicting_members: 0,
        duplicate_only_members: 0,
        partial_fields: 0,
        instrumented_without_equivalence,
    };

    // Missing and per-member classification over admitted members.
    for row in discovery.rows.iter().filter(|row| row.is_accepted()) {
        let member_path = match row.canonical_path() {
            Some(path) => path,
            None => continue,
        };
        let claims: &[ProjectedInvocation] =
            groups.get(member_path).map(Vec::as_slice).unwrap_or(&[]);
        let (disposition, sequence, projection_digest, field_counts) = match claims {
            [] => {
                let partial_position = trace.rows.iter().position(|invocation| {
                    matches!(invocation.disposition, TraceRowDisposition::Accepted)
                        && invocation.state == InvocationObservationState::ObservedPartial
                        && invocation.subject.parent_member_path == member_path
                });
                if let Some(position) = partial_position {
                    let invocation_row = &trace.rows[position];
                    let first_missing_field = invocation_row
                        .fields
                        .first_not_observed()
                        .map(|key| key.wire_name().to_string())
                        .unwrap_or_else(|| "unknown_field".to_string());
                    outcomes.partial_fields += 1;
                    diagnostics.push(SubjectDiagnostic {
                        field: format!("fields.{first_missing_field}"),
                        member_path: Some(member_path.to_string()),
                        detail: "behavior-bearing field not observed; the row never projects \
                                 a canonical plan"
                            .to_string(),
                    });
                    (
                        SubjectJoinDisposition::PartialFields { first_missing_field },
                        Some(invocation_row.sequence),
                        None,
                        invocation_row.fields.state_counts(),
                    )
                } else {
                    outcomes.missing += 1;
                    diagnostics.push(SubjectDiagnostic {
                        field: "invocation_observations".to_string(),
                        member_path: Some(member_path.to_string()),
                        detail: "admitted member has no invocation observation".to_string(),
                    });
                    (
                        SubjectJoinDisposition::MissingInvocation,
                        None,
                        None,
                        FieldStateCounts::default(),
                    )
                }
            }
            [single] => {
                // A redundant partial observation beside a complete projection
                // for the same member is itself a shortfall: it must never be
                // silently absorbed by an otherwise one-to-one join.
                let redundant_partials = trace
                    .rows
                    .iter()
                    .filter(|invocation| {
                        matches!(invocation.disposition, TraceRowDisposition::Accepted)
                            && invocation.state == InvocationObservationState::ObservedPartial
                            && invocation.subject.parent_member_path == member_path
                    })
                    .count();
                if redundant_partials > 0 {
                    outcomes.partial_fields += 1;
                    diagnostics.push(SubjectDiagnostic {
                        field: "fields.redundant_partial".to_string(),
                        member_path: Some(member_path.to_string()),
                        detail: "accepted partial observation beside a complete projection; \
                                 the observation side is not exactly one-to-one"
                            .to_string(),
                    });
                }
                (
                    SubjectJoinDisposition::Joined,
                    Some(single.sequence),
                    Some(single.digest.clone()),
                    fields_for(&trace.rows, single.sequence),
                )
            }
            multiple => {
                let digests_equal =
                    multiple.windows(2).all(|pair| pair[0].digest == pair[1].digest);
                let sequences = multiple.iter().map(|claim| claim.sequence).collect::<Vec<u32>>();
                if digests_equal {
                    outcomes.duplicate_only_members += 1;
                } else {
                    outcomes.conflicting_members += 1;
                }
                diagnostics.push(SubjectDiagnostic {
                    field: "invocation_observations".to_string(),
                    member_path: Some(member_path.to_string()),
                    detail: if digests_equal {
                        "member claimed by multiple identical projections"
                    } else {
                        "member claimed by conflicting projections"
                    }
                    .to_string(),
                });
                let disposition = if digests_equal {
                    SubjectJoinDisposition::DuplicateInvocation { sequences }
                } else {
                    SubjectJoinDisposition::ConflictingInvocation { sequences }
                };
                (disposition, None, None, FieldStateCounts::default())
            }
        };
        rows.push(ObservedRunnerSubjectRow {
            member_path: member_path.to_string(),
            discovery_ordinal: Some(row.ordinal),
            discovery_raw_text: Some(row.raw_text.clone()),
            framing: Some(row.framing),
            normalized: row.normalized.clone(),
            invocation_sequence: sequence,
            projection_digest,
            field_counts,
            disposition,
            row_fingerprint: String::new(),
        });
    }

    // Extra invocations: complete projected members outside the admission.
    for (member_path, claims) in &groups {
        if discovery
            .rows
            .iter()
            .any(|row| row.is_accepted() && row.canonical_path() == Some(member_path.as_str()))
        {
            continue;
        }
        for claim in claims {
            outcomes.extra += 1;
            diagnostics.push(SubjectDiagnostic {
                field: "invocation_observations".to_string(),
                member_path: Some(member_path.clone()),
                detail: "complete projection outside the admitted membership".to_string(),
            });
            rows.push(ObservedRunnerSubjectRow {
                member_path: member_path.clone(),
                discovery_ordinal: None,
                discovery_raw_text: None,
                framing: None,
                normalized: None,
                invocation_sequence: Some(claim.sequence),
                projection_digest: Some(claim.digest.clone()),
                field_counts: fields_for(&trace.rows, claim.sequence),
                disposition: SubjectJoinDisposition::ExtraInvocation { sequence: claim.sequence },
                row_fingerprint: String::new(),
            });
        }
    }

    for fingerprint_target in rows.iter_mut() {
        fingerprint_target.row_fingerprint = row_fingerprint(fingerprint_target)?;
    }

    let state = aggregate_state(&outcomes);
    work.joined_rows =
        rows.iter().filter(|row| matches!(row.disposition, SubjectJoinDisposition::Joined)).count()
            as u64;
    work.missing_invocation_rows = outcomes.missing;
    work.extra_invocation_rows = outcomes.extra;
    work.duplicate_invocation_rows = outcomes.duplicate_only_members;
    work.conflicting_invocation_rows = outcomes.conflicting_members;

    let plan_digest_value = plan_digest(&input.plan)?;
    let payload = ObservedRunnerSubjectPayload {
        evidence_classes: join_evidence_classes(
            input.trace.payload.subject.instrumentation_id.is_some(),
        ),
        bindings: join_bindings(input, plan_digest_value),
        producer: input.producer.clone(),
        equivalence,
        scheduling: input.plan.scheduling.clone(),
        state,
        rows,
        diagnostics,
        work,
        limitations: required_limitations(),
        claim_boundary: OBSERVED_SUBJECT_CLAIM_BOUNDARY.to_string(),
    };
    Ok(ObservedRunnerSubjectV1 {
        schema_version: OBSERVED_RUNNER_SUBJECT_SCHEMA_VERSION.to_string(),
        payload_digest: observed_subject_payload_digest(&payload)?,
        payload,
    })
}

/// Deterministic SHA-256 over the canonical serialization of the payload.
pub fn observed_subject_payload_digest(
    payload: &ObservedRunnerSubjectPayload,
) -> Result<String, String> {
    let bytes = serde_json::to_vec(payload)
        .map_err(|error| format!("serializing observed runner subject payload: {error}"))?;
    Ok(crate::build::sha256_bytes(&bytes))
}

/// Re-check an assembled observed runner subject against its claimed input
/// set: the receipt is rejected unless it is byte-for-byte the subject these
/// inputs prove, making any tampering in a persisted receipt detectable.
pub fn check_observed_runner_subject(
    matrix: &crate::model::UpstreamTargetMatrix,
    input: &ObservedRunnerSubjectInput,
    receipt: &ObservedRunnerSubjectV1,
) -> Result<(), String> {
    crate::observed_subject::validate::validate_observed_runner_subject_shape(receipt)?;
    let rebuilt = build_observed_runner_subject(matrix, input)?;
    if rebuilt != *receipt {
        return Err("observed runner subject does not match the receipt these inputs reconstruct"
            .to_string());
    }
    Ok(())
}

/// Freshness of a joined subject relative to the current prepared tree
/// reference; identical law to the discovery and trace freshness surfaces.
pub fn observed_subject_freshness(
    receipt: &ObservedRunnerSubjectV1,
    current_prepared_tree_identity: &str,
) -> ReceiptFreshness {
    if receipt.payload.bindings.prepared_tree_identity == current_prepared_tree_identity {
        ReceiptFreshness::Current
    } else {
        ReceiptFreshness::Stale
    }
}

/// Typed inputs to the deterministic aggregation law.
pub(crate) struct JoinOutcomes {
    /// Derived failure inherited from upstream observation states, when any.
    pub upstream: Option<ObservedSubjectState>,
    /// Rows carrying `subject_mismatch` invocation states.
    pub subject_mismatch_rows: u64,
    /// Members without any invocation observation.
    pub missing: u64,
    /// Complete invocations outside the admitted membership.
    pub extra: u64,
    /// Members claimed by multiple distinct complete projections.
    pub conflicting_members: u64,
    /// Members claimed by multiple identical complete projections.
    pub duplicate_only_members: u64,
    /// Accepted-frame rows that cannot project a plan.
    pub partial_fields: u64,
    /// Instrument-captured without an exactly bound transfer relation.
    pub instrumented_without_equivalence: bool,
}

/// Deterministic aggregation precedence. Higher-priority categories dominate;
/// nothing below `complete_current` can ever collapse into it.
pub(crate) fn aggregate_state(outcomes: &JoinOutcomes) -> ObservedSubjectState {
    if let Some(upstream) = outcomes.upstream {
        return upstream;
    }
    if outcomes.subject_mismatch_rows > 0 {
        return ObservedSubjectState::SubjectMismatch;
    }
    if outcomes.missing > 0 {
        return ObservedSubjectState::PartialMissingInvocation;
    }
    if outcomes.extra > 0 {
        return ObservedSubjectState::PartialExtraInvocation;
    }
    if outcomes.conflicting_members > 0 || outcomes.duplicate_only_members > 0 {
        return ObservedSubjectState::PartialConflictingInvocation;
    }
    if outcomes.partial_fields > 0 {
        return ObservedSubjectState::PartialUnobservedFields;
    }
    if outcomes.instrumented_without_equivalence {
        return ObservedSubjectState::InstrumentedWithoutEquivalence;
    }
    ObservedSubjectState::CompleteCurrent
}

/// Map one upstream discovery state onto the joined-state family. Only a
/// complete ordinary observation can carry the join; cancelled captures,
/// instrument failures, malformed or truncated captures, and every other
/// shortfall stay explicitly typed.
pub(crate) fn upstream_inherited_state(
    discovery_state: DiscoveryObservationState,
    trace_stream_complete: bool,
) -> Option<ObservedSubjectState> {
    if !discovery_state.is_complete() {
        return Some(match discovery_state {
            DiscoveryObservationState::Cancelled => ObservedSubjectState::Cancelled,
            DiscoveryObservationState::InstrumentFailed => ObservedSubjectState::InstrumentFailure,
            _ => ObservedSubjectState::NotProven,
        });
    }
    if !trace_stream_complete {
        return Some(ObservedSubjectState::NotProven);
    }
    None
}

/// The mandatory limitation set, sorted.
pub(crate) fn required_limitations() -> Vec<String> {
    use crate::observed_subject::model::{
        LIMITATION_JOIN_NOT_EXECUTION, LIMITATION_NO_LOCAL_AUTHORITY,
        LIMITATION_REFERENCES_ARE_CALLER_SUPPLIED,
    };
    let mut limitations = vec![
        LIMITATION_JOIN_NOT_EXECUTION.to_string(),
        LIMITATION_REFERENCES_ARE_CALLER_SUPPLIED.to_string(),
        LIMITATION_NO_LOCAL_AUTHORITY.to_string(),
    ];
    limitations.sort();
    limitations
}

/// Structural digest of the independent runner plan.
pub(crate) fn plan_digest(plan: &RunnerPlan) -> Result<String, String> {
    sha256_json(plan)
}

/// Shape-validate the caller-supplied producer subject identity.
pub(crate) fn validate_producer_shape(producer: &ProducerSubjectIdentity) -> Result<(), String> {
    validate_reference(&producer.repository_commit, "repository commit", 40, 64, true)?;
    validate_reference(&producer.perl_ref, "perl ref", 1, 128, false)?;
    validate_reference(&producer.prepared_tree_identity, "prepared tree identity", 1, 128, false)?;
    validate_reference(&producer.host_perl_identity, "host perl identity", 1, 128, false)?;
    validate_sha256_field(&producer.matrix_fingerprint, "matrix fingerprint")?;
    validate_target_id(&producer.target_id)?;
    validate_sha256_field(&producer.target_contract_digest, "target contract digest")?;
    if let Some(variant) = &producer.variant_target_id {
        validate_target_id(variant)?;
    }
    validate_sha256_field(&producer.environment_sha256, "producer environment digest")?;
    if producer.working_directory.is_empty() || producer.working_directory.len() > 1024 {
        return Err(
            "producer working directory must be nonempty and at most 1024 characters".to_string()
        );
    }
    Ok(())
}

/// Shape-validate the supplied transfer-relation references.
pub(crate) fn validate_equivalence_shape(
    equivalence: &OrdinaryInstrumentedEquivalenceIdentity,
) -> Result<(), String> {
    validate_reference(&equivalence.instrumentation_id, "instrumentation id", 1, 128, false)?;
    validate_sha256_field(
        &equivalence.ordinary_runner_artifact_sha256,
        "ordinary runner artifact digest",
    )?;
    validate_sha256_field(
        &equivalence.instrumented_runner_artifact_sha256,
        "instrumented runner artifact digest",
    )?;
    validate_sha256_field(&equivalence.patch_subject_digest, "patch subject digest")?;
    Ok(())
}

/// Bind the producer identity field-by-field to the observed discovery
/// subject. Every disagreement names its field; none is repairable.
fn bind_producer(
    producer: &ProducerSubjectIdentity,
    discovery: &DiscoveryPayload,
) -> Result<(), String> {
    let subject = &discovery.subject;
    let disagreements = [
        ("repository_commit", producer.repository_commit != subject.repository_commit),
        ("perl_ref", producer.perl_ref != subject.perl_ref),
        (
            "prepared_tree_identity",
            producer.prepared_tree_identity != subject.prepared_tree_identity,
        ),
        ("host_perl_identity", producer.host_perl_identity != subject.host_perl_identity),
        ("matrix_fingerprint", producer.matrix_fingerprint != subject.matrix_fingerprint),
        ("target_id", producer.target_id != subject.target_id),
        (
            "target_contract_digest",
            producer.target_contract_digest != subject.target_contract_digest,
        ),
        ("variant_target_id", producer.variant_target_id != subject.variant_target_id),
        ("runner", producer.runner != discovery.invocation.runner),
        (
            "runner_artifact.canonical_path",
            producer.runner_artifact.canonical_path
                != discovery.invocation.runner_artifact.canonical_path,
        ),
        (
            "runner_artifact.content_sha256",
            producer.runner_artifact.content_sha256
                != discovery.invocation.runner_artifact.content_sha256,
        ),
        ("working_directory", producer.working_directory != discovery.invocation.working_directory),
        (
            "environment_sha256",
            producer.environment_sha256 != discovery.invocation.environment.sha256,
        ),
    ];
    if let Some((field, _)) = disagreements.iter().find(|(_, disagrees)| *disagrees) {
        return Err(format!(
            "producer subject field {field} disagrees with the observed discovery subject"
        ));
    }
    Ok(())
}

/// Revalidate the independent plan structurally and byte-bind it to the
/// observed discovery stream it claims to have been reconstructed from. The
/// final full-authority rebuild proves items, order, membership, and
/// scheduling are exactly what this matrix and these observed bytes produce;
/// a coherent forgery carrying the right digests cannot pass it.
fn validate_plan_binding(
    matrix: &crate::model::UpstreamTargetMatrix,
    plan: &RunnerPlan,
    discovery: &DiscoveryPayload,
) -> Result<(), String> {
    let subject = &discovery.subject;
    let disagreements = [
        ("matrix_fingerprint", plan.matrix_fingerprint != subject.matrix_fingerprint),
        ("target_id", plan.target_id != subject.target_id),
        ("target_contract_digest", plan.target_contract_digest != subject.target_contract_digest),
        ("runner", plan.runner != discovery.invocation.runner),
        ("discovery_frame", plan.discovery_frame != discovery.discovery_frame),
    ];
    if let Some((field, _)) = disagreements.iter().find(|(_, disagrees)| *disagrees) {
        return Err(format!(
            "runner plan field {field} disagrees with the observed discovery subject"
        ));
    }
    let raw_bytes = discovery.stdout.bytes()?;
    let raw_digest = crate::build::sha256_bytes(&raw_bytes);
    if plan.raw_discovery_digest != raw_digest {
        return Err(format!(
            "runner plan field raw_discovery_digest {} does not match the observed \
             discovery stdout digest {raw_digest}; the plan was not reconstructed \
             from this observed stream",
            plan.raw_discovery_digest
        ));
    }
    crate::build::validate_runner_plan_against(matrix, &raw_bytes, plan)
}

/// Assemble the agreed identity snapshot recorded by the join.
pub(crate) fn join_bindings(
    input: &ObservedRunnerSubjectInput,
    plan_digest_value: String,
) -> ObservedSubjectBindings {
    let discovery = &input.discovery.payload;
    let subject = &discovery.subject;
    ObservedSubjectBindings {
        repository_commit: subject.repository_commit.clone(),
        perl_ref: subject.perl_ref.clone(),
        prepared_tree_identity: subject.prepared_tree_identity.clone(),
        host_perl_identity: subject.host_perl_identity.clone(),
        matrix_fingerprint: subject.matrix_fingerprint.clone(),
        target_id: subject.target_id.clone(),
        target_contract_digest: subject.target_contract_digest.clone(),
        variant_target_id: subject.variant_target_id.clone(),
        runner: discovery.invocation.runner,
        runner_artifact: discovery.invocation.runner_artifact.clone(),
        working_directory: discovery.invocation.working_directory.clone(),
        environment_sha256: discovery.invocation.environment.sha256.clone(),
        discovery_frame: discovery.discovery_frame,
        discovery_process_nonce: discovery.terminal.process_nonce.clone(),
        trace_session_id: input.trace.payload.subject.trace_session_id.clone(),
        discovery_receipt_digest: input.discovery.payload_digest.clone(),
        trace_receipt_digest: input.trace.payload_digest.clone(),
        plan_digest: plan_digest_value,
    }
}

/// Evidence classes actually consumed by this join, sorted, deduplicated.
pub(crate) fn join_evidence_classes(instrument_captured: bool) -> Vec<EvidenceClass> {
    let mut classes = vec![EvidenceClass::ObservedUpstream];
    if instrument_captured {
        classes.push(EvidenceClass::InstrumentedUpstream);
    }
    classes.sort();
    classes.dedup();
    classes
}

/// Bind the transfer relation or record the exact failing field.
pub(crate) fn transfer_relation(
    input: &ObservedRunnerSubjectInput,
) -> (Option<OrdinaryInstrumentedEquivalenceIdentity>, Option<SubjectDiagnostic>, bool) {
    let trace = &input.trace.payload;
    match (&trace.subject.instrumentation_id, &input.equivalence) {
        (None, _) => (None, None, false),
        (Some(instrumentation_id), None) => (
            None,
            Some(SubjectDiagnostic {
                field: "equivalence".to_string(),
                member_path: None,
                detail: format!(
                    "observation is instrument-captured under {instrumentation_id} but no \
                     ordinary/instrumented transfer relation was supplied"
                ),
            }),
            true,
        ),
        (Some(instrumentation_id), Some(equivalence)) => {
            let expected = [
                ("instrumentation_id", equivalence.instrumentation_id == *instrumentation_id),
                (
                    "ordinary_runner_artifact_sha256",
                    equivalence.ordinary_runner_artifact_sha256
                        == input.discovery.payload.invocation.runner_artifact.content_sha256,
                ),
                (
                    "instrumented_runner_artifact_sha256",
                    equivalence.instrumented_runner_artifact_sha256
                        == trace.runner_artifact.content_sha256,
                ),
            ];
            if let Some((field, _)) = expected.iter().find(|(_, agrees)| !agrees) {
                (
                    None,
                    Some(SubjectDiagnostic {
                        field: format!("equivalence.{field}"),
                        member_path: None,
                        detail: format!(
                            "supplied transfer relation belongs to another instrumentation \
                             or artifact pair; expected to bind {instrumentation_id}"
                        ),
                    }),
                    true,
                )
            } else {
                (Some(equivalence.clone()), None, false)
            }
        }
    }
}

/// Classify the invocation side into per-member complete-projection groups.
/// Only accepted frames contribute; duplicates stay first-retained within the
/// trace contract and resurface here as duplicate/conflicting member claims.
pub(crate) fn grouped_projections(
    rows: &[EffectiveInvocationRow],
) -> (BTreeMap<String, Vec<ProjectedInvocation>>, u64, u64, u64) {
    let mut groups: BTreeMap<String, Vec<ProjectedInvocation>> = BTreeMap::new();
    let mut complete = 0u64;
    let mut partial = 0u64;
    let mut mismatched = 0u64;
    for row in rows {
        if !matches!(row.disposition, TraceRowDisposition::Accepted) {
            continue;
        }
        match row.state {
            InvocationObservationState::ObservedComplete => {
                complete += 1;
                if let ProjectionRecord::Projected { digest } = &row.projection {
                    groups.entry(row.subject.parent_member_path.clone()).or_default().push(
                        ProjectedInvocation { sequence: row.sequence, digest: digest.clone() },
                    );
                }
            }
            InvocationObservationState::ObservedPartial => partial += 1,
            InvocationObservationState::SubjectMismatch => mismatched += 1,
            _ => {}
        }
    }
    (groups, complete, partial, mismatched)
}

/// Typed field counts of one invocation row by sequence.
fn fields_for(rows: &[EffectiveInvocationRow], sequence: u32) -> FieldStateCounts {
    rows.iter()
        .find(|row| row.sequence == sequence)
        .map(|row| row.fields.state_counts())
        .unwrap_or_default()
}

/// Deterministic per-row fingerprint over every other field of the row.
pub(crate) fn row_fingerprint(row: &ObservedRunnerSubjectRow) -> Result<String, String> {
    #[derive(Serialize)]
    struct FingerprintBasis<'a> {
        member_path: &'a str,
        discovery_ordinal: &'a Option<u32>,
        discovery_raw_text: &'a Option<String>,
        framing: &'a Option<FingerprintFrame>,
        normalized: &'a Option<crate::runner_model::RunnerSourceItem>,
        invocation_sequence: &'a Option<u32>,
        projection_digest: &'a Option<String>,
        field_counts: &'a FieldStateCounts,
        disposition: &'a SubjectJoinDisposition,
    }
    let basis = FingerprintBasis {
        member_path: &row.member_path,
        discovery_ordinal: &row.discovery_ordinal,
        discovery_raw_text: &row.discovery_raw_text,
        framing: &row.framing,
        normalized: &row.normalized,
        invocation_sequence: &row.invocation_sequence,
        projection_digest: &row.projection_digest,
        field_counts: &row.field_counts,
        disposition: &row.disposition,
    };
    let bytes = serde_json::to_vec(&basis)
        .map_err(|error| format!("serializing joined row fingerprint basis: {error}"))?;
    Ok(crate::build::sha256_bytes(&bytes))
}

/// Re-derive state/accounting coherence from the retained payload rows alone.
///
/// A persisted receipt can always be re-digested after tampering, so its
/// self-traveled laws must include internal agreement between the aggregate
/// state, the work counters, and the retained rows. This closes relabel-to-
/// complete and emptied-receipt counterfeits without input access. Full
/// input-side proof remains `check_observed_runner_subject`, which rebuilds
/// the entire receipt from its exact inputs.
pub(crate) fn coherence_error(payload: &ObservedRunnerSubjectPayload) -> Result<(), String> {
    let rows = &payload.rows;
    fn count_of(
        rows: &[ObservedRunnerSubjectRow],
        pred: impl Fn(&SubjectJoinDisposition) -> bool,
    ) -> u64 {
        rows.iter().filter(|row| pred(&row.disposition)).count() as u64
    }
    let counters = [
        (
            "joined_rows",
            payload.work.joined_rows,
            count_of(rows, |d| matches!(d, SubjectJoinDisposition::Joined)),
        ),
        (
            "missing_invocation_rows",
            payload.work.missing_invocation_rows,
            count_of(rows, |d| matches!(d, SubjectJoinDisposition::MissingInvocation)),
        ),
        (
            "extra_invocation_rows",
            payload.work.extra_invocation_rows,
            count_of(rows, |d| matches!(d, SubjectJoinDisposition::ExtraInvocation { .. })),
        ),
        (
            "duplicate_invocation_rows",
            payload.work.duplicate_invocation_rows,
            count_of(rows, |d| matches!(d, SubjectJoinDisposition::DuplicateInvocation { .. })),
        ),
        (
            "conflicting_invocation_rows",
            payload.work.conflicting_invocation_rows,
            count_of(rows, |d| matches!(d, SubjectJoinDisposition::ConflictingInvocation { .. })),
        ),
    ];
    for (field, recorded, derived) in counters {
        if recorded != derived {
            return Err(format!(
                "work counter {field} records {recorded} but its retained rows derive \
                 {derived}; state and accounting contradict the payload"
            ));
        }
    }
    // Partial-field rows may exceed their joined-row markers when an accepted
    // partial observation sits beside a complete projection for the same
    // member, so only the direction is law here.
    let partial_field_rows =
        count_of(rows, |d| matches!(d, SubjectJoinDisposition::PartialFields { .. }));
    if payload.work.partial_invocation_rows < partial_field_rows {
        return Err(format!(
            "work counter partial_invocation_rows records {} below its retained {} \
             partial-field rows; state and accounting contradict the payload",
            payload.work.partial_invocation_rows, partial_field_rows
        ));
    }

    let all_joined =
        rows.iter().all(|row| matches!(row.disposition, SubjectJoinDisposition::Joined));
    if payload.state.is_complete() && (rows.is_empty() || !all_joined) {
        return Err("complete_current subjects retain at least one fully joined row".to_string());
    }
    Ok(())
}

/// Baseline work accounting with every structural-zero invariant at zero.
pub(crate) fn base_work() -> JoinWork {
    JoinWork::default()
}
