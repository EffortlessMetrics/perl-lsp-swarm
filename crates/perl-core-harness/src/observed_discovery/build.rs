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
    // Single source of truth: the subject must carry this matrix's own
    // fingerprint, exactly as validation re-binds it later.
    let matrix_fingerprint = matrix.fingerprint()?;
    if input.subject.matrix_fingerprint != matrix_fingerprint {
        return Err(format!(
            "subject matrix fingerprint {} does not match the pinned matrix authority \
             {matrix_fingerprint}",
            input.subject.matrix_fingerprint
        ));
    }

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
    let bytes = value.as_bytes();
    value.starts_with('/')
        || value.starts_with('\\')
        || (bytes.first().is_some_and(|byte| byte.is_ascii_alphabetic())
            && bytes.get(1) == Some(&b':'))
}

pub(crate) fn validate_target_id(target_id: &str) -> Result<(), String> {
    if target_id.is_empty()
        || !target_id
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
    {
        return Err(format!("target id must match [a-z0-9_]+: {target_id}"));
    }
    Ok(())
}

pub(crate) fn validate_reference(
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
        if !value.bytes().all(crate::is_lower_case_hex_byte) {
            return Err(format!("{label} must be lower-case hexadecimal"));
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

pub(crate) fn validate_sha256_field(value: &str, label: &str) -> Result<(), String> {
    if !crate::is_canonical_sha256_hex(value) {
        return Err(format!(
            "{label} must be a 64-character hexadecimal digest ([0-9a-f] lower-case): {value}"
        ));
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

#[cfg(test)]
mod contract_tests {
    //! Focused unit proof for the strict input laws of the receipt builder:
    //! every validation helper is exercised directly on both sides of each
    //! branch, so the constructor's fail-closed seams keep a static test path
    //! even when the public surface only reports the first rejection.

    use super::{
        MAX_RAW_STREAM_BYTES, required_limitations, retained_envelope, validate_argument,
        validate_artifact_path, validate_capture_identity, validate_environment_part,
        validate_environment_value, validate_reference, validate_sha256_field, validate_target_id,
        validate_working_directory,
    };

    fn rejected_as<T>(outcome: Result<T, String>, fragment: &str) -> bool {
        match outcome {
            Err(message) => message.contains(fragment),
            Ok(_) => false,
        }
    }

    #[test]
    fn retained_envelope_binds_nonce_hex_and_truncation_or_refuses_oversize() {
        let retained = retained_envelope("nonce-1", b"t/base/if.t\n", false);
        assert!(retained.is_ok());
        let retained = retained.expect("small envelope retains");
        assert_eq!(retained.process_nonce, "nonce-1");
        assert_eq!(
            retained.bytes_hex,
            b"t/base/if.t\n".iter().map(|byte| format!("{byte:02x}")).collect::<String>()
        );
        assert!(!retained.truncated);
        let truncated = retained_envelope("nonce-1", b"t/base/if.t\n", true);
        assert!(truncated.is_ok_and(|envelope| envelope.truncated));
        let oversize = vec![b'a'; MAX_RAW_STREAM_BYTES + 1];
        assert!(rejected_as(
            retained_envelope("nonce-1", &oversize, true),
            "exceeds the retained bound"
        ));
    }

    #[test]
    fn capture_identity_requires_printable_bounded_nonce() {
        assert!(validate_capture_identity("capture-1").is_ok());
        assert!(validate_capture_identity(&"x".repeat(128)).is_ok());
        assert!(rejected_as(validate_capture_identity(""), "1-128 printable ASCII"));
        assert!(rejected_as(validate_capture_identity(&"x".repeat(129)), "1-128 printable ASCII"));
        assert!(rejected_as(validate_capture_identity("two words"), "1-128 printable ASCII"));
    }

    #[test]
    fn arguments_stay_relative_control_free_and_bounded() {
        assert!(validate_argument("--verbose").is_ok());
        assert!(validate_argument(&"a".repeat(4096)).is_ok());
        assert!(rejected_as(validate_argument(""), "nonempty and at most 4096"));
        assert!(rejected_as(validate_argument(&"a".repeat(4097)), "nonempty and at most 4096"));
        assert!(rejected_as(validate_argument("a\tb"), "must not contain control characters"));
        assert!(rejected_as(validate_argument("/abs/path"), "is absolute"));
        assert!(rejected_as(validate_argument("\\abs\\path"), "is absolute"));
        assert!(rejected_as(validate_argument("C:\\cwd"), "is absolute"));
        assert!(rejected_as(validate_argument("c:rel"), "is absolute"));
    }

    #[test]
    fn working_directories_stay_simple_relative_components() {
        assert!(validate_working_directory("t/base").is_ok());
        assert!(validate_working_directory(&"a".repeat(1024)).is_ok());
        assert!(rejected_as(validate_working_directory(""), "nonempty and at most 1024"));
        assert!(rejected_as(
            validate_working_directory(&"a".repeat(1025)),
            "nonempty and at most 1024"
        ));
        assert!(rejected_as(validate_working_directory("/root"), "is absolute"));
        assert!(rejected_as(validate_working_directory("a/./b"), "simple prepared-tree-relative"));
        assert!(rejected_as(validate_working_directory("a/../b"), "simple prepared-tree-relative"));
        assert!(rejected_as(validate_working_directory("a//b"), "simple prepared-tree-relative"));
    }

    #[test]
    fn runner_artifact_paths_stay_simple_relative_components() {
        assert!(validate_artifact_path("t/harness").is_ok());
        assert!(rejected_as(validate_artifact_path(""), "nonempty and at most 1024"));
        assert!(rejected_as(validate_artifact_path("C:/tools"), "is absolute"));
        assert!(rejected_as(validate_artifact_path("a/../b"), "simple prepared-tree-relative"));
        assert!(rejected_as(validate_artifact_path("a//b"), "simple prepared-tree-relative"));
    }

    #[test]
    fn target_ids_are_lowercase_identifier_shaped() {
        assert!(validate_target_id("perl_core_1").is_ok());
        assert!(rejected_as(validate_target_id(""), "[a-z0-9_]+"));
        assert!(rejected_as(validate_target_id("Perl"), "[a-z0-9_]+"));
        assert!(rejected_as(validate_target_id("with-dash"), "[a-z0-9_]+"));
    }

    #[test]
    fn references_enforce_their_declared_bounds_and_alphabet() {
        assert!(validate_reference(&"ab".repeat(20), "commit", 40, 64, true).is_ok());
        assert!(rejected_as(
            validate_reference(&"ab".repeat(19), "commit", 40, 64, true),
            "must be 40-64 characters"
        ));
        assert!(rejected_as(
            validate_reference(&"AB".repeat(20), "commit", 40, 64, true),
            "lower-case hexadecimal"
        ));
        assert!(validate_reference("v5.40.0", "perl ref", 1, 128, false).is_ok());
        assert!(rejected_as(
            validate_reference("has space", "perl ref", 1, 128, false),
            "printable ASCII without whitespace"
        ));
    }

    #[test]
    fn environment_parts_and_values_are_bounded_and_control_free() {
        assert!(validate_environment_part("PERL5LIB", "environment key", 128).is_ok());
        assert!(rejected_as(validate_environment_part("", "environment key", 128), "at most 128"));
        assert!(rejected_as(
            validate_environment_part(&"k".repeat(129), "environment key", 128),
            "at most 128"
        ));
        assert!(rejected_as(
            validate_environment_part("two words", "environment key", 128),
            "printable ASCII without whitespace"
        ));
        assert!(validate_environment_value("t/base").is_ok());
        assert!(rejected_as(
            validate_environment_value(&"v".repeat(1025)),
            "at most 1024 characters"
        ));
        assert!(rejected_as(
            validate_environment_value("a\u{7f}b"),
            "must not contain control characters"
        ));
    }

    #[test]
    fn sha256_fields_must_be_exactly_64_hex_characters() {
        assert!(validate_sha256_field(&"ab".repeat(32), "digest").is_ok());
        assert!(rejected_as(
            validate_sha256_field(&"ab".repeat(31), "digest"),
            "64-character hexadecimal digest"
        ));
        assert!(rejected_as(
            validate_sha256_field(&"zz".repeat(32), "digest"),
            "64-character hexadecimal digest"
        ));
    }

    #[test]
    fn mandatory_limitations_are_exactly_the_sorted_required_set() {
        let mut expected = vec![
            crate::observed_discovery::model::LIMITATION_MEMBERSHIP_NOT_SELECTED,
            crate::observed_discovery::model::LIMITATION_REFERENCES_ARE_CALLER_SUPPLIED,
            crate::observed_discovery::model::LIMITATION_NO_LOCAL_DISCOVERY,
        ];
        expected.sort_unstable();
        assert_eq!(required_limitations(), expected);
    }

    /// #7725: referenced raw-discovery identities must be spelled with the
    /// one canonical serialized form, lower-case hexadecimal.
    #[test]
    fn discovery_digests_accept_only_canonical_lower_case_hex() {
        assert!(validate_sha256_field(&"ab".repeat(32), "raw discovery digest").is_ok());
        assert!(rejected_as(
            validate_sha256_field(&"AB".repeat(32), "raw discovery digest"),
            "raw discovery digest"
        ));
        assert!(rejected_as(
            validate_sha256_field(&"aB".repeat(32), "raw discovery digest"),
            "raw discovery digest"
        ));
        assert!(rejected_as(
            validate_sha256_field(&"zz".repeat(32), "raw discovery digest"),
            "raw discovery digest"
        ));
        assert!(rejected_as(
            validate_sha256_field(&"ab".repeat(31), "raw discovery digest"),
            "raw discovery digest"
        ));
    }
}
