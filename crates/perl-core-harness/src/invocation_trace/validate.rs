//! Fail-closed validation for effective-invocation trace receipts.
//!
//! Validation never trusts a self-reported digest or projection: it re-decodes
//! the retained trace bytes through the same strict decoder, re-derives every
//! row state, projection record, and work counter, recomputes the payload
//! digest, and re-binds the parent discovery receipt and its accepted
//! membership. Structural zeros are re-proven, never assumed.

use crate::invocation_trace::adapter::{ExpectedInvocationBinding, project_effective_invocation};
use crate::invocation_trace::build::{
    required_limitations, row_subject_consistent, trace_payload_digest, validate_subject,
};
use crate::invocation_trace::decode::{decode_trace_stream, derive_row_state, work_from_rows};
use crate::invocation_trace::model::{
    EffectiveInvocationTraceReceiptV1, InvocationObservationState, MAX_TRACE_STREAM_BYTES,
    TRACE_CONTAMINATION_MARKERS, UPSTREAM_INVOCATION_TRACE_SCHEMA_VERSION,
};
use crate::observed_discovery::model::{EvidenceClass, ProcessCompletion};
use crate::observed_discovery::validate::validate_receipt_subject_binding;
use crate::runner_model::RunnerKind;

/// Full acceptance validation: subject binding, parent re-binding, strict
/// trace re-decode, row reconstruction, projection re-derivation, and work
/// re-derivation.
pub fn validate_invocation_trace_receipt(
    parent: &crate::observed_discovery::model::UpstreamDiscoveryReceiptV1,
    receipt: &EffectiveInvocationTraceReceiptV1,
) -> Result<(), String> {
    validate_trace_receipt_subject_binding(receipt)?;
    let payload = &receipt.payload;

    // The supplied parent must itself be coherent and be exactly the receipt's
    // bound parent.
    validate_receipt_subject_binding(parent)?;
    if payload.subject.parent_receipt_digest != parent.payload_digest {
        return Err(format!(
            "receipt binds parent receipt {} but the supplied parent digest is {}",
            payload.subject.parent_receipt_digest, parent.payload_digest
        ));
    }
    if payload.subject.parent_process_nonce != parent.payload.terminal.process_nonce {
        return Err("receipt parent process identity does not match the parent terminal capture"
            .to_string());
    }
    if payload.subject.repository_commit != parent.payload.subject.repository_commit
        || payload.subject.prepared_tree_identity != parent.payload.subject.prepared_tree_identity
        || payload.subject.target_id != parent.payload.subject.target_id
        || payload.subject.target_contract_digest != parent.payload.subject.target_contract_digest
        || payload.subject.matrix_fingerprint != parent.payload.subject.matrix_fingerprint
        || payload.subject.variant_target_id != parent.payload.subject.variant_target_id
    {
        return Err(
            "receipt subject does not bind the supplied parent discovery subject".to_string()
        );
    }
    if payload.runner_artifact.canonical_path != payload.runner.entrypoint() {
        return Err(format!(
            "runner artifact {} is not the entrypoint of runner {:?}",
            payload.runner_artifact.canonical_path, payload.runner
        ));
    }
    enforce_uncontaminated(parent)?;

    let trace_bytes = payload.trace.bytes()?;
    if trace_bytes.len() > MAX_TRACE_STREAM_BYTES {
        return Err(format!(
            "retained trace envelope holds {} bytes beyond the {MAX_TRACE_STREAM_BYTES}-byte bound",
            trace_bytes.len()
        ));
    }
    let decoded = decode_trace_stream(&trace_bytes)?;
    if let Some(header) = &decoded.header {
        if header.trace_session_id != payload.subject.trace_session_id
            || header.parent_process_nonce != payload.subject.parent_process_nonce
            || header.parent_receipt_digest != payload.subject.parent_receipt_digest
        {
            return Err("retained header does not bind the receipt subject identity".to_string());
        }
        if *header != payload.header {
            return Err("retained header disagrees with the decoded trace bytes".to_string());
        }
    }
    if decoded.terminal != payload.terminal {
        return Err("retained terminal frame disagrees with the decoded trace bytes".to_string());
    }
    if decoded.outcome != payload.trace_decode {
        return Err("retained decode outcome disagrees with the decoded trace bytes".to_string());
    }

    let stream_complete = decoded.outcome.is_complete() && !payload.trace.truncated;
    let completion = decoded
        .terminal
        .as_ref()
        .map(|terminal| terminal.completion)
        .unwrap_or(ProcessCompletion::Unknown);

    if decoded.rows.len() != payload.rows.len() {
        return Err(format!(
            "retained trace bytes reconstruct {} rows; receipt records {}",
            decoded.rows.len(),
            payload.rows.len()
        ));
    }
    let mut projections_accepted: u64 = 0;
    for (index, (decoded_row, recorded_row)) in
        decoded.rows.iter().zip(payload.rows.iter()).enumerate()
    {
        let subject_consistent =
            row_subject_consistent(&decoded_row.subject, &payload.subject, parent, payload.runner);
        let expected_state = derive_row_state(
            decoded_row.disposition.is_accepted(),
            stream_complete,
            completion,
            subject_consistent,
            &decoded_row.fields,
        );
        if expected_state != recorded_row.state {
            return Err(format!(
                "row {index} records state {:?} but reconstructs {expected_state:?}",
                recorded_row.state
            ));
        }
        let binding =
            ExpectedInvocationBinding::from_subject(&payload.subject, &decoded_row.subject);
        let outcome = project_effective_invocation(
            &rehydrated_row(decoded_row, recorded_row.state),
            &binding,
        );
        if outcome.is_projected() {
            projections_accepted += 1;
        }
        if outcome.record() != recorded_row.projection {
            return Err(format!(
                "row {index} records projection {:?} but reconstructs {:?}",
                recorded_row.projection,
                outcome.record()
            ));
        }
    }
    let expected_work = work_from_rows(
        trace_bytes.len(),
        decoded.frames_consumed,
        &payload.rows,
        payload.rows.len() as u64,
        projections_accepted,
    );
    if expected_work != payload.work {
        return Err("recorded trace work disagrees with reconstructed observation work".to_string());
    }
    Ok(())
}

/// Subject-binding validation that does not require the parent receipt:
/// schema, evidence class, limitations, claim boundary, digest coherence,
/// structural zeros, and record-shape laws.
pub fn validate_trace_receipt_subject_binding(
    receipt: &EffectiveInvocationTraceReceiptV1,
) -> Result<(), String> {
    if receipt.schema_version != UPSTREAM_INVOCATION_TRACE_SCHEMA_VERSION {
        return Err(format!("unsupported invocation trace schema {}", receipt.schema_version));
    }
    if receipt.evidence_class != EvidenceClass::InstrumentedUpstream {
        return Err(format!(
            "invocation trace receipt cannot carry evidence class {:?}",
            receipt.evidence_class
        ));
    }
    if !matches!(receipt.payload.runner, RunnerKind::Test | RunnerKind::Harness) {
        return Err(format!(
            "runner {:?} is not an admitted upstream invocation route",
            receipt.payload.runner
        ));
    }
    validate_subject(&receipt.payload.subject)?;
    let payload = &receipt.payload;
    if payload.limitations != required_limitations() {
        return Err(
            "invocation trace receipts retain exactly their mandatory limitations".to_string()
        );
    }
    if payload.claim_boundary != crate::invocation_trace::model::INVOCATION_TRACE_CLAIM_BOUNDARY {
        return Err("invocation trace receipts retain their fixed claim boundary".to_string());
    }
    let recomputed = trace_payload_digest(payload)?;
    if recomputed != receipt.payload_digest {
        return Err("payload digest does not bind the recorded payload".to_string());
    }

    let trace_bytes = payload.trace.bytes()?;
    if payload.work.trace_bytes_consumed != trace_bytes.len() as u64 {
        return Err("recorded trace byte count disagrees with the retained envelope".to_string());
    }
    if payload.work.trace_rows_consumed != payload.rows.len() as u64 {
        return Err("recorded decoded-row count disagrees with retained rows".to_string());
    }
    for (index, row) in payload.rows.iter().enumerate() {
        let expected_fingerprint = crate::build::sha256_bytes(row.raw_line.as_bytes());
        if row.row_fingerprint != expected_fingerprint {
            return Err(format!("row {index} fingerprint does not bind its retained raw line"));
        }
        let state = row.state;
        if state == InvocationObservationState::Stale {
            return Err(format!(
                "row {index} carries a consumer-side stale state inside a receipt"
            ));
        }
        if state == InvocationObservationState::ObservedComplete
            && matches!(
                row.projection,
                crate::invocation_trace::model::ProjectionRecord::Rejected { .. }
            )
        {
            return Err(format!("complete row {index} must carry an accepted projection record"));
        }
    }
    if payload.work.source_reads != 0 {
        return Err("invocation trace performs no source reads".to_string());
    }
    if payload.work.filesystem_scans != 0 {
        return Err("invocation trace performs no filesystem scans".to_string());
    }
    if payload.work.runner_processes != 0 {
        return Err("invocation trace spawns no runner processes".to_string());
    }
    if payload.work.direct_probe_inputs != 0 {
        return Err("invocation trace consumes no direct-probe inputs".to_string());
    }
    if payload.work.canonical_plan_projections_attempted != payload.rows.len() as u64 {
        return Err("projection attempts must cover every retained row".to_string());
    }
    if payload.work.canonical_plan_projections_accepted
        + payload.work.canonical_plan_projections_rejected
        != payload.work.canonical_plan_projections_attempted
    {
        return Err("projection outcomes must partition the attempts".to_string());
    }
    Ok(())
}

/// Rehydrate a decoded row into the shape the pure adapter consumes, carrying
/// the recorded state so projection reproducibility is checked against the
/// same state the constructor derived.
fn rehydrated_row(
    decoded_row: &crate::invocation_trace::model::EffectiveInvocationRow,
    state: InvocationObservationState,
) -> crate::invocation_trace::model::EffectiveInvocationRow {
    let mut row = decoded_row.clone();
    row.state = state;
    row
}

fn enforce_uncontaminated(
    parent: &crate::observed_discovery::model::UpstreamDiscoveryReceiptV1,
) -> Result<(), String> {
    let stdout = parent.payload.stdout.bytes()?;
    let stderr = parent.payload.stderr.bytes()?;
    for marker in TRACE_CONTAMINATION_MARKERS {
        for (name, stream) in [("stdout", &stdout), ("stderr", &stderr)] {
            if stream.windows(marker.len()).any(|window| window == marker.as_bytes()) {
                return Err(format!(
                    "parent discovery {name} carries trace-frame bytes; the trace channel must \
                     stay independent of ordinary runner result streams"
                ));
            }
        }
    }
    Ok(())
}
