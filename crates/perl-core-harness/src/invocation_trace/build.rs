//! Strict constructors and deterministic digests for effective-invocation
//! trace receipts.
//!
//! A receipt is constructible only from supplied trace bytes whose parent
//! discovery receipt passes its own subject-binding validation, shares the
//! exact subject, owns the parent process identity, and stays free of trace
//! contamination in its ordinary result streams. Row-level subject mismatches
//! are recorded honestly as `subject_mismatch` rows; facts that make trace
//! identity un-establishable (oversize streams, foreign headers, disagreeing
//! parents, contaminated result streams) are rejected outright.

use crate::invocation_trace::adapter::{ExpectedInvocationBinding, project_effective_invocation};
use crate::invocation_trace::decode::{decode_trace_stream, derive_row_state, work_from_rows};
use crate::invocation_trace::model::{
    EffectiveInvocationTraceReceiptV1, INVOCATION_TRACE_CLAIM_BOUNDARY,
    LIMITATION_NO_RUNNER_INTERACTION, LIMITATION_OBSERVATION_NOT_EXECUTION,
    LIMITATION_PARENT_RECEIPT_CALLER_SUPPLIED, LIMITATION_PARTIAL_ROWS_NEVER_PLANS,
    MAX_TRACE_STREAM_BYTES, ObservedInvocationTraceInput, TRACE_CONTAMINATION_MARKERS,
    TracePayload, TraceStreamEnvelope, TraceSubjectIdentity, TraceWork,
    UPSTREAM_INVOCATION_TRACE_SCHEMA_VERSION,
};
use crate::observed_discovery::model::{
    EvidenceClass, ProcessCompletion, RunnerArtifactIdentity, UpstreamDiscoveryReceiptV1,
};
use crate::observed_discovery::validate::validate_receipt_subject_binding;
use crate::runner_model::RunnerKind;

/// Build one strict effective-invocation trace receipt from supplied bytes and
/// the exact parent discovery receipt.
pub fn build_invocation_trace_receipt(
    input: &ObservedInvocationTraceInput,
) -> Result<EffectiveInvocationTraceReceiptV1, String> {
    if !matches!(input.runner, RunnerKind::Test | RunnerKind::Harness) {
        return Err(format!(
            "runner {:?} is not an admitted upstream invocation route",
            input.runner
        ));
    }
    let parent = &input.parent_receipt;
    // The parent receipt must itself be coherent before it can lend identity.
    validate_receipt_subject_binding(parent)?;
    validate_subject_references(&input.subject)?;
    validate_trace_session_id(&input.subject.trace_session_id)?;
    validate_artifact(&input.runner_artifact)?;

    // Subject-relation checks performed during every strict construction:
    // parent-digest binding, subject equality, parent process identity, and
    // runner artifact/route binding. Trace contamination of the parent's
    // ordinary result streams voids the independent-transport contract.
    if input.subject.parent_receipt_digest != parent.payload_digest {
        return Err(format!(
            "trace subject binds parent receipt {} but the supplied parent digest is {}",
            input.subject.parent_receipt_digest, parent.payload_digest
        ));
    }
    if let Some(disagreement) = subject_disagreement(&input.subject, parent) {
        return Err(format!(
            "trace subject does not bind its parent discovery subject: {disagreement}"
        ));
    }
    if input.subject.parent_process_nonce != parent.payload.terminal.process_nonce {
        return Err(
            "trace parent process identity does not match the parent receipt terminal capture"
                .to_string(),
        );
    }
    // The traced process is the parent discovery process itself: its runner
    // route cannot differ from the route the parent receipt recorded.
    if input.runner != parent.payload.invocation.runner {
        return Err(format!(
            "trace runner {:?} does not match the parent discovery runner {:?}",
            input.runner, parent.payload.invocation.runner
        ));
    }
    if input.runner_artifact.canonical_path != input.runner.entrypoint() {
        return Err(format!(
            "runner artifact {} is not the entrypoint of runner {:?}",
            input.runner_artifact.canonical_path, input.runner
        ));
    }
    enforce_uncontaminated_result_streams(parent)?;

    if input.trace_bytes.len() > MAX_TRACE_STREAM_BYTES {
        return Err(format!(
            "trace stream of {} bytes exceeds the retained bound of {MAX_TRACE_STREAM_BYTES}; \
             the capture must be truncated and marked before receipt construction",
            input.trace_bytes.len()
        ));
    }

    let decoded = decode_trace_stream(&input.trace_bytes)?;
    // The decoded header must bind the receipt subject exactly: session,
    // parent process, and parent receipt identity cannot come from another
    // run.
    if let Some(header) = &decoded.header
        && (header.trace_session_id != input.subject.trace_session_id
            || header.parent_process_nonce != input.subject.parent_process_nonce
            || header.parent_receipt_digest != input.subject.parent_receipt_digest)
    {
        return Err("trace header does not bind the receipt subject identity".to_string());
    }
    let stream_complete = decoded.outcome.is_complete() && !input.trace_truncated;
    let completion = decoded
        .terminal
        .as_ref()
        .map(|terminal| terminal.completion)
        .unwrap_or(ProcessCompletion::Unknown);

    let mut rows = decoded.rows;
    let mut projections_accepted: u64 = 0;
    for row in &mut rows {
        let subject_consistent =
            row_subject_consistent(&row.subject, &input.subject, parent, input.runner);
        row.state = derive_row_state(
            row.disposition.is_accepted(),
            stream_complete,
            completion,
            subject_consistent,
            &row.fields,
        );
        let binding = ExpectedInvocationBinding::from_subject(&input.subject, &row.subject);
        let outcome = project_effective_invocation(row, &binding);
        if outcome.is_projected() {
            projections_accepted += 1;
        }
        row.projection = outcome.record();
    }
    let projections_attempted = rows.len() as u64;
    let work: TraceWork = work_from_rows(
        input.trace_bytes.len(),
        decoded.frames_consumed,
        &rows,
        projections_attempted,
        projections_accepted,
    );

    let payload = TracePayload {
        subject: input.subject.clone(),
        runner: input.runner,
        runner_artifact: input.runner_artifact.clone(),
        header: decoded.header.unwrap_or_else(empty_header_for),
        terminal: decoded.terminal,
        trace: TraceStreamEnvelope {
            bytes_hex: crate::observed_discovery::model::hex_encode(&input.trace_bytes),
            truncated: input.trace_truncated,
        },
        trace_decode: decoded.outcome,
        rows,
        work,
        limitations: required_limitations(),
        claim_boundary: INVOCATION_TRACE_CLAIM_BOUNDARY.to_string(),
    };
    Ok(EffectiveInvocationTraceReceiptV1 {
        schema_version: UPSTREAM_INVOCATION_TRACE_SCHEMA_VERSION.to_string(),
        evidence_class: EvidenceClass::InstrumentedUpstream,
        payload_digest: trace_payload_digest(&payload)?,
        payload,
    })
}

/// Deterministic SHA-256 over the canonical serialization of the payload.
pub fn trace_payload_digest(payload: &TracePayload) -> Result<String, String> {
    let bytes = serde_json::to_vec(payload)
        .map_err(|error| format!("serializing effective invocation trace payload: {error}"))?;
    Ok(crate::build::sha256_bytes(&bytes))
}

/// Re-check an existing receipt against its exact parent discovery receipt.
///
/// Read-only adapter surface for #12158/#12106 consumers: it never rebuilds
/// discovery or the trace; it revalidates every binding and reconstruction.
pub fn check_invocation_trace_against(
    parent: &UpstreamDiscoveryReceiptV1,
    receipt: &EffectiveInvocationTraceReceiptV1,
) -> Result<(), String> {
    crate::invocation_trace::validate::validate_invocation_trace_receipt(parent, receipt)
}

/// Freshness of a receipt relative to the current prepared tree reference.
pub fn trace_receipt_freshness(
    receipt: &EffectiveInvocationTraceReceiptV1,
    current_prepared_tree_identity: &str,
) -> crate::observed_discovery::model::ReceiptFreshness {
    if receipt.payload.subject.prepared_tree_identity == current_prepared_tree_identity {
        crate::observed_discovery::model::ReceiptFreshness::Current
    } else {
        crate::observed_discovery::model::ReceiptFreshness::Stale
    }
}

/// True when the row's claimed subject binds the receipt subject, the parent
/// receipt, and an accepted parent member. Equal spelling from another
/// receipt, member, process, runner, or preparation never satisfies it.
pub(crate) fn row_subject_consistent(
    row_subject: &crate::invocation_trace::model::RowSubjectBinding,
    subject: &TraceSubjectIdentity,
    parent: &UpstreamDiscoveryReceiptV1,
    runner: RunnerKind,
) -> bool {
    row_subject.trace_session_id == subject.trace_session_id
        && row_subject.parent_receipt_digest == parent.payload_digest
        && row_subject.runner == runner
        && row_subject.target_id == subject.target_id
        && row_subject.variant_target_id == subject.variant_target_id
        && row_subject.instrumentation_id == subject.instrumentation_id
        && parent.payload.rows.iter().any(|row| {
            row.is_accepted()
                && row.canonical_path() == Some(row_subject.parent_member_path.as_str())
        })
}

/// Fail-closed subject validation entry shared with the validator.
pub(crate) fn validate_subject(input_subject: &TraceSubjectIdentity) -> Result<(), String> {
    validate_subject_references(input_subject)
}
pub(crate) fn empty_header_for() -> crate::invocation_trace::model::TraceHeader {
    crate::invocation_trace::model::TraceHeader {
        schema_version: UPSTREAM_INVOCATION_TRACE_SCHEMA_VERSION.to_string(),
        trace_session_id: String::new(),
        parent_process_nonce: String::new(),
        parent_receipt_digest: String::new(),
        expected_row_count: 0,
        encoding: "utf-8".to_string(),
        newline: "lf".to_string(),
    }
}

/// The mandatory limitation set, sorted.
pub(crate) fn required_limitations() -> Vec<String> {
    let mut limitations = vec![
        LIMITATION_OBSERVATION_NOT_EXECUTION.to_string(),
        LIMITATION_PARENT_RECEIPT_CALLER_SUPPLIED.to_string(),
        LIMITATION_NO_RUNNER_INTERACTION.to_string(),
        LIMITATION_PARTIAL_ROWS_NEVER_PLANS.to_string(),
    ];
    limitations.sort();
    limitations
}

pub(crate) fn subject_disagreement(
    subject: &TraceSubjectIdentity,
    parent: &UpstreamDiscoveryReceiptV1,
) -> Option<String> {
    let parent_subject = &parent.payload.subject;
    let disagreements = [
        ("repository commit", subject.repository_commit != parent_subject.repository_commit),
        ("perl ref", subject.perl_ref != parent_subject.perl_ref),
        (
            "prepared tree identity",
            subject.prepared_tree_identity != parent_subject.prepared_tree_identity,
        ),
        ("host perl identity", subject.host_perl_identity != parent_subject.host_perl_identity),
        ("matrix fingerprint", subject.matrix_fingerprint != parent_subject.matrix_fingerprint),
        ("target id", subject.target_id != parent_subject.target_id),
        (
            "target contract digest",
            subject.target_contract_digest != parent_subject.target_contract_digest,
        ),
        ("variant target id", subject.variant_target_id != parent_subject.variant_target_id),
    ];
    disagreements
        .iter()
        .find(|(_, disagrees)| *disagrees)
        .map(|(label, _)| format!("{label} disagrees with the parent discovery subject"))
}

pub(crate) fn enforce_uncontaminated_result_streams(
    parent: &UpstreamDiscoveryReceiptV1,
) -> Result<(), String> {
    let stdout = parent.payload.stdout.bytes()?;
    let stderr = parent.payload.stderr.bytes()?;
    for marker in TRACE_CONTAMINATION_MARKERS {
        for (name, stream) in [("stdout", &stdout), ("stderr", &stderr)] {
            if find_subslice(stream, marker.as_bytes()) {
                return Err(format!(
                    "parent discovery {name} carries trace-frame bytes; the trace channel must \
                     stay independent of ordinary runner result streams"
                ));
            }
        }
    }
    Ok(())
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len().max(1)).any(|window| window == needle)
}

fn validate_trace_session_id(session: &str) -> Result<(), String> {
    if session.is_empty()
        || session.len() > 128
        || !session.bytes().all(|byte| (0x21..=0x7e).contains(&byte))
    {
        return Err("trace session id must be 1-128 printable ASCII characters".to_string());
    }
    Ok(())
}

pub(crate) fn validate_artifact(artifact: &RunnerArtifactIdentity) -> Result<(), String> {
    let path = &artifact.canonical_path;
    if path.is_empty() || path.len() > 1024 {
        return Err("runner artifact path must be nonempty and at most 1024 characters".to_string());
    }
    if path.starts_with('/') || path.starts_with('\\') || path.contains("..") {
        return Err(format!("runner artifact path {path} must stay simple and checkout-relative"));
    }
    if !crate::is_canonical_sha256_hex(&artifact.content_sha256) {
        return Err(format!(
            "runner artifact digest must be 64 hexadecimal characters ([0-9a-f] lower-case): {}",
            artifact.content_sha256
        ));
    }
    Ok(())
}

fn validate_subject_references(subject: &TraceSubjectIdentity) -> Result<(), String> {
    validate_reference(&subject.repository_commit, "repository commit", 40, 64, true)?;
    validate_reference(&subject.perl_ref, "perl ref", 1, 128, false)?;
    validate_reference(&subject.prepared_tree_identity, "prepared tree identity", 1, 128, false)?;
    validate_reference(&subject.host_perl_identity, "host perl identity", 1, 128, false)?;
    validate_sha256_field(&subject.matrix_fingerprint, "matrix fingerprint")?;
    validate_target_id(&subject.target_id)?;
    validate_sha256_field(&subject.target_contract_digest, "target contract digest")?;
    if let Some(variant) = &subject.variant_target_id {
        validate_target_id(variant)?;
    }
    if let Some(instrument) = &subject.instrumentation_id {
        validate_reference(instrument, "instrumentation id", 1, 128, false)?;
    }
    validate_sha256_field(&subject.parent_receipt_digest, "parent receipt digest")?;
    validate_reference(&subject.parent_process_nonce, "parent process nonce", 1, 128, false)?;
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
        if !value.bytes().all(crate::is_lower_case_hex_byte) {
            return Err(format!("{label} must be lower-case hexadecimal"));
        }
        return Ok(());
    }
    if !value.bytes().all(|byte| (0x21..=0x7e).contains(&byte)) {
        return Err(format!("{label} must be printable ASCII without whitespace"));
    }
    Ok(())
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

fn validate_sha256_field(value: &str, label: &str) -> Result<(), String> {
    if !crate::is_canonical_sha256_hex(value) {
        return Err(format!(
            "{label} must be a 64-character hexadecimal digest ([0-9a-f] lower-case): {value}"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod contract_tests {
    //! Focused unit proof for the strict input laws of the trace builder:
    //! every validation helper is exercised directly on both sides of each
    //! branch, so the constructor's fail-closed seams keep a static test path
    //! even when the public surface only reports the first rejection.

    use super::{
        RunnerArtifactIdentity, required_limitations, validate_artifact, validate_reference,
        validate_sha256_field, validate_target_id, validate_trace_session_id,
    };
    use crate::invocation_trace::model::SUBJECT_VALIDATIONS_PER_CONSTRUCTION;

    fn rejected_as<T>(outcome: Result<T, String>, fragment: &str) -> bool {
        match outcome {
            Err(message) => message.contains(fragment),
            Ok(_) => false,
        }
    }

    #[test]
    fn trace_session_ids_are_printable_and_bounded() {
        assert!(validate_trace_session_id("trace-session-1").is_ok());
        assert!(validate_trace_session_id(&"x".repeat(128)).is_ok());
        assert!(rejected_as(validate_trace_session_id(""), "1-128 printable ASCII"));
        assert!(rejected_as(validate_trace_session_id(&"x".repeat(129)), "1-128 printable"));
        assert!(rejected_as(validate_trace_session_id("two words"), "1-128 printable"));
    }

    #[test]
    fn target_ids_are_lowercase_identifier_shaped() {
        assert!(validate_target_id("component_base").is_ok());
        assert!(rejected_as(validate_target_id(""), "[a-z0-9_]+"));
        assert!(rejected_as(validate_target_id("Base"), "[a-z0-9_]+"));
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
        assert!(validate_reference("v5.42.2", "perl ref", 1, 128, false).is_ok());
        assert!(rejected_as(
            validate_reference("has space", "perl ref", 1, 128, false),
            "printable ASCII without whitespace"
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
            crate::invocation_trace::model::LIMITATION_OBSERVATION_NOT_EXECUTION,
            crate::invocation_trace::model::LIMITATION_PARENT_RECEIPT_CALLER_SUPPLIED,
            crate::invocation_trace::model::LIMITATION_NO_RUNNER_INTERACTION,
            crate::invocation_trace::model::LIMITATION_PARTIAL_ROWS_NEVER_PLANS,
        ];
        expected.sort_unstable();
        assert_eq!(required_limitations(), expected);
        assert_eq!(SUBJECT_VALIDATIONS_PER_CONSTRUCTION, 4);
    }

    /// #7725: trace-bound identities must be spelled with the one canonical
    /// serialized form, lower-case hexadecimal.
    #[test]
    fn trace_identities_accept_only_canonical_lower_case_hex() {
        assert!(validate_sha256_field(&"ab".repeat(32), "matrix fingerprint").is_ok());
        assert!(rejected_as(
            validate_sha256_field(&"AB".repeat(32), "matrix fingerprint"),
            "matrix fingerprint"
        ));
        assert!(rejected_as(
            validate_sha256_field(&"aB".repeat(32), "matrix fingerprint"),
            "matrix fingerprint"
        ));

        let artifact = RunnerArtifactIdentity {
            canonical_path: "t/TEST".to_string(),
            content_sha256: "aB".repeat(32),
        };
        let message = validate_artifact(&artifact).expect_err("mixed case must reject");
        assert!(message.contains("runner artifact digest"), "{message}");
        let lower_artifact = RunnerArtifactIdentity {
            canonical_path: "t/TEST".to_string(),
            content_sha256: "ab".repeat(32),
        };
        assert!(validate_artifact(&lower_artifact).is_ok());
    }
}
