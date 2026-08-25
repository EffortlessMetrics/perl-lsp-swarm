//! Strict constructors and deterministic digests for observed upstream
//! discovery receipts.
//!
//! A receipt is constructible only from a terminal process/raw-output envelope
//! whose authority is an admitted upstream runner route. Subject-relation
//! mismatches are recorded honestly as `subject_mismatch` observations; facts
//! that make receipt identity un-establishable (oversize or non-hex streams,
//! disagreeing capture identities, empty discovery, absolute-path leakage)
//! are rejected outright.

use crate::build::{effective_selection, find_target, sha256_bytes};
use crate::model::UpstreamTargetMatrix;
use crate::observed_discovery::decode::{
    decode_malformed, decode_stream, derive_observation_state,
};
use crate::observed_discovery::model::{
    DiscoveryPayload, EnvironmentIdentity, EvidenceClass, InvocationObservation,
    MAX_RAW_STREAM_BYTES, OBSERVED_DISCOVERY_CLAIM_BOUNDARY, ObservedDiscoveryInput,
    RawStreamEnvelope, SUBJECT_VALIDATIONS_PER_CONSTRUCTION, TerminalObservation,
    UPSTREAM_DISCOVERY_SCHEMA_VERSION, UpstreamDiscoveryReceiptV1, hex_encode,
};
use crate::runner_model::RunnerKind;
use serde::Serialize;
use std::collections::BTreeMap;

/// Build one strict observed-discovery receipt from a supplied envelope and
/// the pinned target matrix authority for its claimed target.
pub fn build_observed_discovery_receipt(
    matrix: &UpstreamTargetMatrix,
    input: &ObservedDiscoveryInput,
) -> Result<UpstreamDiscoveryReceiptV1, String> {
    if !matches!(input.runner, RunnerKind::Test | RunnerKind::Harness) {
        return Err(format!(
            "runner {:?} is not an admitted upstream discovery route",
            input.runner
        ));
    }
    validate_capture_identity(&input.process_nonce)?;
    let stdout =
        retained_envelope(&input.process_nonce, &input.stdout_bytes, input.stdout_truncated)?;
    let stderr =
        retained_envelope(&input.process_nonce, &input.stderr_bytes, input.stderr_truncated)?;
    validate_reference(&input.subject.repository_commit, "repository commit", 40, 64, true)?;
    validate_reference(&input.subject.perl_ref, "perl ref", 1, 128, false)?;
    validate_reference(
        &input.subject.prepared_tree_identity,
        "prepared tree identity",
        1,
        128,
        false,
    )?;
    validate_reference(&input.subject.host_perl_identity, "host perl identity", 1, 128, false)?;
    validate_sha256_field(&input.subject.matrix_fingerprint, "matrix fingerprint")?;
    validate_target_id(&input.subject.target_id)?;
    validate_sha256_field(&input.subject.target_contract_digest, "target contract digest")?;
    if let Some(variant) = &input.subject.variant_target_id {
        validate_target_id(variant)?;
    }
    if let Some(instrument) = &input.subject.instrumentation_id {
        validate_reference(instrument, "instrumentation id", 1, 128, false)?;
    }
    validate_artifact_path(&input.runner_artifact.canonical_path)?;
    validate_sha256_field(&input.runner_artifact.content_sha256, "runner artifact digest")?;
    for argument in &input.argv {
        validate_argument(argument)?;
    }
    validate_working_directory(&input.working_directory)?;
    let environment = environment_identity(&input.environment)?;

    let entry = find_target(matrix, &input.subject.target_id)?;
    let (selectors, script_forms) = effective_selection(matrix, entry)?;

    // Subject-relation checks performed during every strict construction:
    // capture-identity pairing (above), runner-artifact binding, and target
    // contract digest agreement.
    let subject_consistent =
        artifact_matches_runner(input) && contract_digest_matches(input, entry);

    let decoded =
        decode_stream(&input.stdout_bytes, input.discovery_frame, &selectors, &script_forms)?;
    if decoded.rows.is_empty() && decoded.outcome.is_complete() {
        return Err("observed discovery stream contains no members".to_string());
    }
    let all_rows_accepted = decoded.rows.iter().all(|row| row.is_accepted());
    let truncated = stdout.truncated || stderr.truncated;
    let state = derive_observation_state(
        input.completion,
        decode_malformed(&decoded.outcome, &decoded.rows),
        truncated,
        subject_consistent,
        all_rows_accepted,
    );
    let work = crate::observed_discovery::decode::work_from_rows(
        stdout.byte_len(),
        stderr.byte_len(),
        &decoded.rows,
        decoded.normalization_attempts,
        SUBJECT_VALIDATIONS_PER_CONSTRUCTION,
    );

    let payload = DiscoveryPayload {
        subject: input.subject.clone(),
        invocation: InvocationObservation {
            runner: input.runner,
            runner_artifact: input.runner_artifact.clone(),
            argv: input.argv.clone(),
            working_directory: input.working_directory.clone(),
            environment,
        },
        discovery_frame: input.discovery_frame,
        terminal: TerminalObservation {
            process_nonce: input.process_nonce.clone(),
            completion: input.completion,
        },
        stdout,
        stderr,
        stdout_decode: decoded.outcome,
        rows: decoded.rows,
        state,
        work,
        limitations: required_limitations(),
        claim_boundary: OBSERVED_DISCOVERY_CLAIM_BOUNDARY.to_string(),
    };
    Ok(UpstreamDiscoveryReceiptV1 {
        schema_version: UPSTREAM_DISCOVERY_SCHEMA_VERSION.to_string(),
        evidence_class: EvidenceClass::ObservedUpstream,
        payload_digest: discovery_payload_digest(&payload)?,
        payload,
    })
}

/// Deterministic SHA-256 over the canonical serialization of the payload.
pub fn discovery_payload_digest(payload: &DiscoveryPayload) -> Result<String, String> {
    let bytes = serde_json::to_vec(payload)
        .map_err(|error| format!("serializing observed discovery payload: {error}"))?;
    Ok(sha256_bytes(&bytes))
}

/// Re-check an existing receipt against the pinned target matrix it claims.
///
/// Read-only adapter surface for #7737/#12158/#12105 consumers: it never
/// rebuilds discovery from the filesystem; it re-binds the recorded target
/// references to matrix authority and confirms every accepted row is inside
/// the declared selection.
pub fn check_observed_discovery_against(
    matrix: &UpstreamTargetMatrix,
    receipt: &UpstreamDiscoveryReceiptV1,
) -> Result<(), String> {
    crate::observed_discovery::validate::validate_observed_discovery_receipt(matrix, receipt)
}

/// Freshness of a receipt relative to the current prepared tree reference.
pub fn receipt_freshness(
    receipt: &UpstreamDiscoveryReceiptV1,
    current_prepared_tree_identity: &str,
) -> crate::observed_discovery::model::ReceiptFreshness {
    if receipt.payload.subject.prepared_tree_identity == current_prepared_tree_identity {
        crate::observed_discovery::model::ReceiptFreshness::Current
    } else {
        crate::observed_discovery::model::ReceiptFreshness::Stale
    }
}

fn retained_envelope(
    process_nonce: &str,
    bytes: &[u8],
    truncated: bool,
) -> Result<RawStreamEnvelope, String> {
    if bytes.len() > MAX_RAW_STREAM_BYTES {
        return Err(format!(
            "raw stream of {} bytes exceeds the retained bound of {MAX_RAW_STREAM_BYTES}; \
             the capture must be truncated and marked before receipt construction",
            bytes.len()
        ));
    }
    Ok(RawStreamEnvelope {
        process_nonce: process_nonce.to_string(),
        bytes_hex: hex_encode(bytes),
        truncated,
    })
}

fn artifact_matches_runner(input: &ObservedDiscoveryInput) -> bool {
    input.runner_artifact.canonical_path == input.runner.entrypoint()
}

fn contract_digest_matches(
    input: &ObservedDiscoveryInput,
    entry: &crate::model::TargetMatrixEntry,
) -> bool {
    match sha256_json(&entry.contract) {
        Ok(digest) => digest == input.subject.target_contract_digest,
        Err(_) => false,
    }
}

fn environment_identity(
    variables: &BTreeMap<String, String>,
) -> Result<EnvironmentIdentity, String> {
    for (key, value) in variables {
        validate_environment_part(key, "environment key", 128)?;
        validate_environment_value(value)?;
    }
    let mut canonical = String::new();
    for (key, value) in variables {
        canonical.push_str(key);
        canonical.push('=');
        canonical.push_str(value);
        canonical.push('\n');
    }
    Ok(EnvironmentIdentity {
        variables: variables.clone(),
        sha256: sha256_bytes(canonical.as_bytes()),
    })
}

pub(crate) fn required_limitations() -> Vec<String> {
    let mut limitations = vec![
        crate::observed_discovery::model::LIMITATION_MEMBERSHIP_NOT_SELECTED.to_string(),
        crate::observed_discovery::model::LIMITATION_REFERENCES_ARE_CALLER_SUPPLIED.to_string(),
        crate::observed_discovery::model::LIMITATION_NO_LOCAL_DISCOVERY.to_string(),
    ];
    limitations.sort();
    limitations
}

fn validate_capture_identity(nonce: &str) -> Result<(), String> {
    if nonce.is_empty() || nonce.len() > 128 || !nonce.bytes().all(is_graphic_ascii) {
        return Err("process nonce must be 1-128 printable ASCII characters".to_string());
    }
    Ok(())
}

fn validate_argument(argument: &str) -> Result<(), String> {
    if argument.is_empty() || argument.len() > 4096 {
        return Err("argv entries must be nonempty and at most 4096 characters".to_string());
    }
    if argument.chars().any(|character| character.is_control()) {
        return Err("argv entries must not contain control characters".to_string());
    }
    if looks_absolute(argument) {
        return Err(format!(
            "argv entry {argument} is absolute; observed invocation identity must stay \
             checkout-root independent"
        ));
    }
    Ok(())
}

fn validate_working_directory(directory: &str) -> Result<(), String> {
    if directory.is_empty() || directory.len() > 1024 {
        return Err("working directory must be nonempty and at most 1024 characters".to_string());
    }
    if looks_absolute(directory) {
        return Err(format!(
            "working directory {directory} is absolute; observed invocation identity must \
             stay checkout-root independent"
        ));
    }
    for component in directory.split('/') {
        if component.is_empty() || component == "." || component == ".." {
            return Err(format!(
                "working directory must be simple prepared-tree-relative components: {directory}"
            ));
        }
    }
    Ok(())
}

fn validate_artifact_path(path: &str) -> Result<(), String> {
    if path.is_empty() || path.len() > 1024 {
        return Err("runner artifact path must be nonempty and at most 1024 characters".to_string());
    }
    if looks_absolute(path) {
        return Err(format!(
            "runner artifact path {path} is absolute; observed subject identity must stay \
             checkout-root independent"
        ));
    }
    for component in path.split('/') {
        if component.is_empty() || component == "." || component == ".." {
            return Err(format!(
                "runner artifact path must be simple prepared-tree-relative components: {path}"
            ));
        }
    }
    Ok(())
}

fn looks_absolute(value: &str) -> bool {
    value.starts_with('/') || value.starts_with('\\') || value.as_bytes().get(1) == Some(&b':')
}

fn validate_target_id(target_id: &str) -> Result<(), String> {
    if target_id.is_empty()
        || !target_id
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
    {
        return Err(format!("target id must match [a-z0-9_]+: {target_id}"));
    }
    Ok(())
}

fn validate_reference(
    value: &str,
    label: &str,
    min_len: usize,
    max_len: usize,
    lowercase_hex: bool,
) -> Result<(), String> {
    if value.len() < min_len || value.len() > max_len {
        return Err(format!("{label} must be {min_len}-{max_len} characters"));
    }
    if lowercase_hex {
        if !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(format!("{label} must be hexadecimal"));
        }
        return Ok(());
    }
    if !value.bytes().all(is_graphic_ascii) {
        return Err(format!("{label} must be printable ASCII without whitespace"));
    }
    Ok(())
}

fn validate_environment_part(value: &str, label: &str, max_len: usize) -> Result<(), String> {
    if value.is_empty() || value.len() > max_len {
        return Err(format!("{label} must be nonempty and at most {max_len} characters"));
    }
    if !value.bytes().all(is_graphic_ascii) {
        return Err(format!("{label} must be printable ASCII without whitespace"));
    }
    Ok(())
}

fn validate_environment_value(value: &str) -> Result<(), String> {
    if value.len() > 1024 {
        return Err("environment values must be at most 1024 characters".to_string());
    }
    if value.chars().any(|character| character.is_control()) {
        return Err("environment values must not contain control characters".to_string());
    }
    Ok(())
}

fn validate_sha256_field(value: &str, label: &str) -> Result<(), String> {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{label} must be a 64-character hexadecimal digest: {value}"));
    }
    Ok(())
}

fn is_graphic_ascii(byte: u8) -> bool {
    (0x21..=0x7e).contains(&byte)
}

pub(crate) fn sha256_json<T: Serialize>(value: &T) -> Result<String, String> {
    let bytes = serde_json::to_vec(value)
        .map_err(|error| format!("serializing observed discovery authority: {error}"))?;
    Ok(sha256_bytes(&bytes))
}
