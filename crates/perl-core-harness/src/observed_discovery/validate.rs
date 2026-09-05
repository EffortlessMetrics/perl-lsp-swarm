//! Fail-closed validation for observed upstream discovery receipts.
//!
//! Validation never trusts a self-reported digest or membership projection:
//! it re-decodes the retained raw stdout bytes through the same strict
//! decoder, re-derives every work counter and the terminal observation state,
//! recomputes the payload digest, and re-binds target references against the
//! pinned target matrix.

use crate::build::{effective_selection, find_target};
use crate::model::UpstreamTargetMatrix;
use crate::observed_discovery::build::{
    discovery_payload_digest, required_limitations, sha256_json, validate_sha256_field,
};
use crate::observed_discovery::decode::{
    decode_malformed, decode_stream, derive_observation_state, work_from_rows,
};
use crate::observed_discovery::model::{
    DiscoveryPayload, EvidenceClass, MAX_RAW_STREAM_BYTES, MemberDisposition,
    OBSERVED_DISCOVERY_CLAIM_BOUNDARY, ObservedDiscoveryRow, RawStreamEnvelope,
    SUBJECT_VALIDATIONS_PER_CONSTRUCTION, UPSTREAM_DISCOVERY_SCHEMA_VERSION,
    UpstreamDiscoveryReceiptV1,
};
use crate::runner_model::RunnerKind;

/// Full acceptance validation: structural integrity plus raw-byte row
/// reconstruction plus matrix re-binding.
pub fn validate_observed_discovery_receipt(
    matrix: &UpstreamTargetMatrix,
    receipt: &UpstreamDiscoveryReceiptV1,
) -> Result<(), String> {
    validate_receipt_subject_binding(receipt)?;
    let payload = &receipt.payload;

    let entry = find_target(matrix, &payload.subject.target_id)?;
    let expected_contract_digest = sha256_json(&entry.contract)?;
    if expected_contract_digest != payload.subject.target_contract_digest {
        return Err(format!(
            "receipt target contract digest does not match target {}",
            payload.subject.target_id
        ));
    }
    if matrix.fingerprint()? != payload.subject.matrix_fingerprint {
        return Err("receipt matrix fingerprint does not match the supplied matrix".to_string());
    }
    if let Some(variant) = &payload.subject.variant_target_id {
        find_target(matrix, variant).map_err(|error| {
            format!("receipt names variant target that is absent from the matrix: {error}")
        })?;
    }
    let (selectors, script_forms) = effective_selection(matrix, entry)?;

    let stdout_bytes = payload.stdout.bytes()?;
    let decoded = decode_stream(&stdout_bytes, payload.discovery_frame, &selectors, &script_forms)?;
    require_same_rows(&decoded.rows, &payload.rows)?;
    if !decoded.outcome.is_complete() {
        // A malformed stream keeps its typed outcome and zero rows.
        if !payload.rows.is_empty() {
            return Err("malformed observed stream must record no reconstructed rows".to_string());
        }
    } else if decoded.rows.is_empty() {
        return Err("observed discovery stream contains no members".to_string());
    }

    let expected_work = work_from_rows(
        payload.stdout.byte_len(),
        payload.stderr.byte_len(),
        &decoded.rows,
        decoded.normalization_attempts,
        SUBJECT_VALIDATIONS_PER_CONSTRUCTION,
    );
    if expected_work != payload.work {
        return Err(
            "recorded decoder work disagrees with reconstructed observation work".to_string()
        );
    }

    let subject_consistent = artifact_matches(payload)
        && expected_contract_digest == payload.subject.target_contract_digest;
    let truncated = payload.stdout.truncated || payload.stderr.truncated;
    let all_rows_accepted = payload.rows.iter().all(|row| row.is_accepted());
    let derived_state = derive_observation_state(
        payload.terminal.completion,
        decode_malformed(&decoded.outcome, &decoded.rows),
        truncated,
        subject_consistent,
        all_rows_accepted,
    );
    if derived_state != payload.state {
        return Err(format!(
            "recorded observation state {:?} disagrees with derived state {derived_state:?}",
            payload.state
        ));
    }
    Ok(())
}

/// Subject-binding validation that does not require matrix authority:
/// digests, envelopes, capture identity, invocation leakage rules, work
/// counter zeros, and recorded-state coherence are all checked here.
pub fn validate_receipt_subject_binding(
    receipt: &UpstreamDiscoveryReceiptV1,
) -> Result<(), String> {
    if receipt.schema_version != UPSTREAM_DISCOVERY_SCHEMA_VERSION {
        return Err(format!("unsupported observed discovery schema {}", receipt.schema_version));
    }
    if receipt.evidence_class != EvidenceClass::ObservedUpstream {
        return Err(format!(
            "observed discovery receipt cannot carry evidence class {:?}",
            receipt.evidence_class
        ));
    }
    let payload = &receipt.payload;
    // The admitted-route law binds every receipt path, not only construction:
    // a deserialized receipt recording any other runner route fails closed
    // here before artifact binding can launder it.
    if !matches!(payload.invocation.runner, RunnerKind::Test | RunnerKind::Harness) {
        return Err(format!(
            "runner {:?} is not an admitted upstream discovery route",
            payload.invocation.runner
        ));
    }
    validate_payload_fields(payload)?;
    let recomputed = discovery_payload_digest(payload)?;
    if recomputed != receipt.payload_digest {
        return Err("payload digest does not bind the recorded payload".to_string());
    }

    let stdout_bytes = payload.stdout.bytes()?;
    let stderr_bytes = payload.stderr.bytes()?;
    enforce_stream_bound(stdout_bytes.len())?;
    enforce_stream_bound(stderr_bytes.len())?;
    enforce_capture_identity(&payload.stdout, &payload.stderr, &payload.terminal.process_nonce)?;

    if payload.work.filesystem_discovery_operations != 0 {
        return Err("observed discovery performs no filesystem discovery operations".to_string());
    }
    if payload.work.direct_probe_rows_consumed != 0 {
        return Err("observed discovery consumes no direct-probe rows".to_string());
    }
    if payload.work.terminal_subject_validations != SUBJECT_VALIDATIONS_PER_CONSTRUCTION {
        return Err(format!(
            "observed discovery records {} terminal/subject validations per construction",
            SUBJECT_VALIDATIONS_PER_CONSTRUCTION
        ));
    }
    if payload.work.raw_stdout_bytes != stdout_bytes.len() as u64
        || payload.work.raw_stderr_bytes != stderr_bytes.len() as u64
    {
        return Err("recorded raw stream sizes disagree with retained envelopes".to_string());
    }
    if payload.work.decoded_rows != payload.rows.len() as u64 {
        return Err("recorded decoded-row count disagrees with retained rows".to_string());
    }
    let counted_accepted = payload.rows.iter().filter(|row| row.is_accepted()).count() as u64;
    if payload.work.accepted_rows != counted_accepted {
        return Err("recorded accepted-row count disagrees with retained rows".to_string());
    }

    let truncated = payload.stdout.truncated || payload.stderr.truncated;
    let malformed = decode_malformed(&payload.stdout_decode, &payload.rows);
    let all_rows_accepted = payload.rows.iter().all(|row| row.is_accepted());
    let subject_consistent = artifact_matches(payload);
    let derived_state = derive_observation_state(
        payload.terminal.completion,
        malformed,
        truncated,
        subject_consistent,
        all_rows_accepted,
    );
    if derived_state != payload.state {
        return Err(format!(
            "recorded observation state {:?} disagrees with derived state {derived_state:?}",
            payload.state
        ));
    }
    Ok(())
}

fn validate_payload_fields(payload: &DiscoveryPayload) -> Result<(), String> {
    if payload.limitations != required_limitations() {
        return Err(
            "observed discovery receipts retain exactly their mandatory limitations".to_string()
        );
    }
    if payload.claim_boundary != OBSERVED_DISCOVERY_CLAIM_BOUNDARY {
        return Err("observed discovery receipts retain their fixed claim boundary".to_string());
    }
    let mut environment_canonical = String::new();
    for (key, value) in &payload.invocation.environment.variables {
        environment_canonical.push_str(key);
        environment_canonical.push('=');
        environment_canonical.push_str(value);
        environment_canonical.push('\n');
    }
    let expected = crate::build::sha256_bytes(environment_canonical.as_bytes());
    if expected != payload.invocation.environment.sha256 {
        return Err("environment identity digest does not bind the retained variables".to_string());
    }
    // The intake law binds deserialized receipts too, not only construction:
    // a caller that retags the artifact digest and recomputes the payload
    // digest cannot launder an alternate spelling through validation (#7725).
    validate_sha256_field(
        &payload.invocation.runner_artifact.content_sha256,
        "runner artifact content digest",
    )?;
    let sequential_from_zero = payload.rows.first().is_none_or(|row| row.ordinal == 0)
        && payload.rows.windows(2).all(|pair| pair[1].ordinal == pair[0].ordinal + 1);
    if !sequential_from_zero {
        return Err("observed rows must keep strictly sequential original order".to_string());
    }
    for row in &payload.rows {
        if row.discovery_frame != payload.discovery_frame {
            return Err(format!(
                "row {} carries frame {:?} outside the declared stream frame {:?}",
                row.ordinal, row.discovery_frame, payload.discovery_frame
            ));
        }
        let identity_required = matches!(
            row.disposition,
            MemberDisposition::Accepted
                | MemberDisposition::DuplicateOfCanonical { .. }
                | MemberDisposition::ConflictingCanonical { .. }
                | MemberDisposition::OutsideTargetSelection
        );
        if identity_required && row.normalized.is_none() {
            return Err(format!("row {} disposition requires a normalized identity", row.ordinal));
        }
        if matches!(row.disposition, MemberDisposition::MalformedRow) && row.normalized.is_some() {
            return Err(format!(
                "malformed row {} cannot carry a normalized identity",
                row.ordinal
            ));
        }
    }
    Ok(())
}

fn require_same_rows(
    expected: &[ObservedDiscoveryRow],
    recorded: &[ObservedDiscoveryRow],
) -> Result<(), String> {
    if expected.len() != recorded.len() {
        return Err(format!(
            "raw stdout bytes reconstruct {} rows; receipt records {}",
            expected.len(),
            recorded.len()
        ));
    }
    for (index, (expected_row, recorded_row)) in expected.iter().zip(recorded.iter()).enumerate() {
        if expected_row != recorded_row {
            return Err(format!(
                "reconstructed row {index} differs from the recorded row {}",
                recorded_row.ordinal
            ));
        }
    }
    Ok(())
}

fn artifact_matches(payload: &DiscoveryPayload) -> bool {
    payload.invocation.runner_artifact.canonical_path == payload.invocation.runner.entrypoint()
}

fn enforce_stream_bound(len: usize) -> Result<(), String> {
    if len > MAX_RAW_STREAM_BYTES {
        return Err(format!(
            "retained stream envelope holds {len} bytes beyond the {MAX_RAW_STREAM_BYTES}-byte bound"
        ));
    }
    Ok(())
}

fn enforce_capture_identity(
    stdout: &RawStreamEnvelope,
    stderr: &RawStreamEnvelope,
    terminal_nonce: &str,
) -> Result<(), String> {
    if stdout.process_nonce != stderr.process_nonce || stderr.process_nonce != terminal_nonce {
        return Err(
            "stdout, stderr, and terminal observations must share one process capture identity"
                .to_string(),
        );
    }
    Ok(())
}
