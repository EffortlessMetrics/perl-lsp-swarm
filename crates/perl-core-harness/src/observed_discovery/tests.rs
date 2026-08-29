//! Fixtures and falsifiers for observed upstream-discovery receipts (#12281).
//!
//! Positive fixtures cover nested `op/hook`, root-external `lib/dist/ext/cpan`
//! populations, `.t` and `test.pl` forms, duplicates, stable original order,
//! LF/CRLF framing, and a clean exact `t/TEST`-shaped stream. Falsifiers prove
//! injected drift, relabelling, subject substitution, and self-asserted
//! digests are rejected fail-closed.

use crate::io::read_matrix;
use crate::model::{TargetMatrixEntry, UpstreamTargetMatrix};
use crate::observed_discovery::model::{
    DiscoveryObservationState, EvidenceClass, LineFraming, MemberDisposition,
    ObservedDiscoveryInput, ProcessCompletion, ReceiptFreshness, UPSTREAM_DISCOVERY_SCHEMA_VERSION,
    UpstreamDiscoveryReceiptV1,
};
use crate::observed_discovery::{
    build_observed_discovery_receipt, check_observed_discovery_against, discovery_payload_digest,
    receipt_freshness, validate_observed_discovery_receipt, validate_receipt_subject_binding,
};
use crate::runner_model::{DiscoveryFrame, RunnerKind};
use color_eyre::eyre::{Result, bail, eyre};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn ensure(outcome: Result<(), String>) -> Result<()> {
    outcome.map_err(|error| eyre!(error))
}

fn ensure_digest(digest: Result<String, String>) -> Result<String> {
    digest.map_err(|error| eyre!(error))
}

fn repo_file(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").join(relative)
}

fn matrix() -> Result<UpstreamTargetMatrix> {
    read_matrix(&repo_file(".ci/perl-core-harness/upstream-targets-5.42.2.v1"))
}

fn find_entry<'m>(
    matrix: &'m UpstreamTargetMatrix,
    target_id: &str,
) -> Result<&'m TargetMatrixEntry> {
    matrix
        .targets
        .iter()
        .find(|entry| entry.contract.target_id == target_id)
        .ok_or_else(|| color_eyre::eyre::eyre!("matrix has no target {target_id}"))
}

fn contract_digest(entry: &TargetMatrixEntry) -> Result<String> {
    let bytes = serde_json::to_vec(&entry.contract)?;
    Ok(crate::build::sha256_bytes(&bytes))
}

fn sha_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|byte| format!("{byte:02x}")).collect()
}

fn subject_for(
    matrix: &UpstreamTargetMatrix,
    target_id: &str,
) -> Result<crate::observed_discovery::model::DiscoverySubjectIdentity> {
    let entry = find_entry(matrix, target_id)?;
    Ok(crate::observed_discovery::model::DiscoverySubjectIdentity {
        repository_commit: "a".repeat(40),
        perl_ref: "perl-5.42.2".to_string(),
        prepared_tree_identity: "prepared-tree-generation-1".to_string(),
        host_perl_identity: "host-perl-5.42.2".to_string(),
        matrix_fingerprint: matrix.fingerprint().map_err(|error| eyre!(error))?,
        target_id: target_id.to_string(),
        target_contract_digest: contract_digest(entry)?,
        variant_target_id: None,
        instrumentation_id: None,
    })
}

fn base_input(
    matrix: &UpstreamTargetMatrix,
    target_id: &str,
    stdout: &[u8],
) -> Result<ObservedDiscoveryInput> {
    Ok(ObservedDiscoveryInput {
        subject: subject_for(matrix, target_id)?,
        runner: RunnerKind::Test,
        runner_artifact: crate::observed_discovery::model::RunnerArtifactIdentity {
            canonical_path: "t/TEST".to_string(),
            content_sha256: sha_hex(b"t/TEST"),
        },
        argv: vec!["./perl".to_string(), "../t/TEST".to_string(), "--dumptests".to_string()],
        working_directory: "t".to_string(),
        environment: BTreeMap::from([("LC_ALL".to_string(), "C".to_string())]),
        discovery_frame: DiscoveryFrame::CanonicalRepositoryPath,
        completion: ProcessCompletion::ExitStatus { code: 0 },
        process_nonce: "capture-0001".to_string(),
        stdout_bytes: stdout.to_vec(),
        stdout_truncated: false,
        stderr_bytes: Vec::new(),
        stderr_truncated: false,
    })
}

fn build(
    matrix: &UpstreamTargetMatrix,
    input: &ObservedDiscoveryInput,
) -> Result<UpstreamDiscoveryReceiptV1> {
    build_observed_discovery_receipt(matrix, input).map_err(|error| color_eyre::eyre::eyre!(error))
}

fn assert_rejected_where(condition_reason: &str, outcome: Result<(), String>) -> Result<()> {
    let Err(_error) = outcome else {
        bail!("expected rejection for {condition_reason}");
    };
    Ok(())
}

// ---------------------------------------------------------------------------
// Positive fixtures
// ---------------------------------------------------------------------------

#[test]
fn clean_t_test_shaped_stream_is_complete_and_order_stable() -> Result<()> {
    let matrix = matrix()?;
    let input = base_input(&matrix, "component_base", b"t/base/if.t\r\nt/base/cond.t\n")?;
    let receipt = build(&matrix, &input)?;
    assert_eq!(receipt.schema_version, UPSTREAM_DISCOVERY_SCHEMA_VERSION);
    assert_eq!(receipt.evidence_class, EvidenceClass::ObservedUpstream);
    assert_eq!(receipt.payload.state, DiscoveryObservationState::ObservedComplete);
    assert!(receipt.payload.state.is_complete());
    let rows = &receipt.payload.rows;
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].raw_text, "t/base/if.t");
    assert_eq!(rows[0].framing, LineFraming::Crlf);
    assert_eq!(rows[0].ordinal, 0);
    assert_eq!(rows[1].framing, LineFraming::Lf);
    assert_eq!(rows[0].disposition, MemberDisposition::Accepted);
    assert_eq!(
        rows[0].normalized.as_ref().map(|item| item.canonical_path.as_str()),
        Some("t/base/if.t")
    );
    ensure(validate_observed_discovery_receipt(&matrix, &receipt))?;
    ensure(check_observed_discovery_against(&matrix, &receipt))?;
    Ok(())
}

#[test]
fn final_row_without_newline_keeps_eof_framing() -> Result<()> {
    let matrix = matrix()?;
    let input = base_input(&matrix, "component_base", b"t/base/if.t\nt/base/cond.t")?;
    let receipt = build(&matrix, &input)?;
    assert_eq!(receipt.payload.rows[1].framing, LineFraming::Eof);
    assert_eq!(receipt.payload.state, DiscoveryObservationState::ObservedComplete);
    Ok(())
}

#[test]
fn nested_op_hook_target_accepts_nested_member() -> Result<()> {
    let matrix = matrix()?;
    let input = base_input(&matrix, "component_op_hook", b"t/op/hook/hook.t\n")?;
    let receipt = build(&matrix, &input)?;
    assert_eq!(receipt.payload.state, DiscoveryObservationState::ObservedComplete);
    assert_eq!(receipt.payload.rows[0].canonical_path(), Some("t/op/hook/hook.t"));
    Ok(())
}

#[test]
fn root_external_populations_accept_dot_t_and_test_pl() -> Result<()> {
    let matrix = matrix()?;
    let cases = [
        ("manifest_root_lib", "lib/Foo/test.pl\ndist/Foo/t/basic.t\n", "lib/Foo/test.pl"),
        ("manifest_dist", "dist/Foo/basic.t\next/re/t/qr.t\n", "dist/Foo/basic.t"),
        ("manifest_ext", "ext/re/t/basic.t\ncpan/Foo/t/basic.t\n", "ext/re/t/basic.t"),
        ("manifest_cpan", "cpan/Foo/test.pl\nlib/Foo/basic.t\n", "cpan/Foo/test.pl"),
    ];
    for (target_id, stream, accepted_path) in cases {
        // Only rows inside each population are accepted; the cross-population
        // spellings stay recorded as out-of-target observations.
        let input = base_input(&matrix, target_id, stream.as_bytes())?;
        let receipt = build(&matrix, &input)?;
        assert_eq!(receipt.payload.work.decoded_rows, 2, "{target_id}");
        assert_eq!(receipt.payload.work.accepted_rows, 1, "{target_id}");
        assert_eq!(receipt.payload.work.out_of_target_rows, 1, "{target_id}");
        assert_eq!(receipt.payload.state, DiscoveryObservationState::ObservedPartial);
        let accepted =
            receipt.payload.rows.iter().find(|row| row.is_accepted()).ok_or_else(|| {
                color_eyre::eyre::eyre!("{target_id} must retain an accepted row")
            })?;
        assert_eq!(accepted.canonical_path(), Some(accepted_path), "{target_id}");
        ensure(validate_observed_discovery_receipt(&matrix, &receipt))?;
    }

    // Both source forms are admitted on manifest populations.
    let both_forms =
        build(&matrix, &base_input(&matrix, "manifest_cpan", b"cpan/A/t/x.t\ncpan/B/test.pl\n")?)?;
    assert_eq!(both_forms.payload.state, DiscoveryObservationState::ObservedComplete);
    Ok(())
}

#[test]
fn serialization_is_deterministic_across_spellings_and_builds() -> Result<()> {
    let matrix = matrix()?;
    let input = base_input(&matrix, "component_base", b"t/base/if.t\nt/base/cond.t\n")?;
    let first = build(&matrix, &input)?;
    let second = build(&matrix, &input)?;
    assert_eq!(first, second);
    assert_eq!(serde_json::to_vec(&first)?, serde_json::to_vec(&second)?);
    let compact = serde_json::to_vec(&first)?;
    let pretty = serde_json::to_vec_pretty(&first)?;
    let compact_receipt: UpstreamDiscoveryReceiptV1 = serde_json::from_slice(&compact)?;
    let pretty_receipt: UpstreamDiscoveryReceiptV1 = serde_json::from_slice(&pretty)?;
    assert_eq!(
        ensure_digest(discovery_payload_digest(&compact_receipt.payload))?,
        ensure_digest(discovery_payload_digest(&pretty_receipt.payload))?
    );
    // Environment insertion order cannot change canonical bytes.
    let mut reordered = input.clone();
    reordered.environment = BTreeMap::from([
        ("LC_ALL".to_string(), "C".to_string()),
        ("PERL5OPT".to_string(), "-Ilib".to_string()),
    ]);
    let with_env = build(&matrix, &reordered)?;
    let mut flipped = reordered.clone();
    flipped.environment = flipped.environment.clone().into_iter().rev().collect();
    let rebuilt = build(&matrix, &flipped)?;
    assert_eq!(with_env.payload_digest, rebuilt.payload_digest);
    Ok(())
}

#[test]
fn adapters_report_current_and_stale_freshness_without_discovery() -> Result<()> {
    let matrix = matrix()?;
    let input = base_input(&matrix, "component_base", b"t/base/if.t\n")?;
    let receipt = build(&matrix, &input)?;
    assert_eq!(
        receipt_freshness(&receipt, "prepared-tree-generation-1"),
        ReceiptFreshness::Current
    );
    assert_eq!(receipt_freshness(&receipt, "prepared-tree-generation-2"), ReceiptFreshness::Stale);
    ensure(validate_receipt_subject_binding(&receipt))?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Falsifier 1: declared bytes relabelled observed_upstream
// ---------------------------------------------------------------------------

#[test]
fn declared_evidence_class_never_validates_as_observed() -> Result<()> {
    let matrix = matrix()?;
    let receipt = build(&matrix, &base_input(&matrix, "component_base", b"t/base/if.t\n")?)?;
    let mut value = serde_json::to_value(&receipt)?;
    value["evidence_class"] = json!("declared_input");
    let relabelled: UpstreamDiscoveryReceiptV1 = serde_json::from_value(value)?;
    assert_rejected_where(
        "declared bytes relabelled observed",
        validate_receipt_subject_binding(&relabelled),
    )?;
    assert_rejected_where(
        "declared bytes relabelled observed against matrix",
        validate_observed_discovery_receipt(&matrix, &relabelled),
    )?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Falsifier 2: foreign schema versions and frame-less rows
// ---------------------------------------------------------------------------

#[test]
fn foreign_schema_version_and_frameless_rows_are_rejected() -> Result<()> {
    let matrix = matrix()?;
    let receipt = build(&matrix, &base_input(&matrix, "component_base", b"t/base/if.t\n")?)?;

    let mut value = serde_json::to_value(&receipt)?;
    value["schema_version"] = json!("perl_core_harness.runner_plan.v1");
    let historical: UpstreamDiscoveryReceiptV1 = serde_json::from_value(value)?;
    assert_rejected_where("runner_plan.v1 schema", validate_receipt_subject_binding(&historical))?;

    let mut value = serde_json::to_value(&receipt)?;
    if value["payload"]["rows"][0]["discovery_frame"].is_null() {
        bail!("row frame must serialize");
    }
    let row_object = value["payload"]["rows"][0]
        .as_object_mut()
        .ok_or_else(|| color_eyre::eyre::eyre!("row must be an object"))?;
    row_object.remove("discovery_frame");
    let frameless = serde_json::from_value::<UpstreamDiscoveryReceiptV1>(value);
    assert!(frameless.is_err(), "frame-less rows must not deserialize");
    Ok(())
}

// ---------------------------------------------------------------------------
// Falsifiers 3+4: frame collisions never share one source identity
// ---------------------------------------------------------------------------

#[test]
fn sibling_t_frame_spellings_do_not_collapse() -> Result<()> {
    let matrix = matrix()?;
    let input = base_input(&matrix, "component_base", b"lib/Foo/x.t\n../lib/Foo/x.t\n")?
        .clone_at_frame(DiscoveryFrame::RunnerTDirectoryRelative);
    let receipt = build(&matrix, &input)?;
    let first = receipt.payload.rows[0].normalized.as_ref().map(|item| item.canonical_path.clone());
    let second =
        receipt.payload.rows[1].normalized.as_ref().map(|item| item.canonical_path.clone());
    assert_eq!(first.as_deref(), Some("t/lib/Foo/x.t"));
    assert_eq!(second.as_deref(), Some("lib/Foo/x.t"));
    assert_ne!(first, second);
    Ok(())
}

#[test]
fn same_spelling_under_different_frames_has_distinct_identity() -> Result<()> {
    let matrix = matrix()?;
    let from_root = build(&matrix, &base_input(&matrix, "component_base", b"lib/Foo/x.t\n")?)?;
    let from_t = build(
        &matrix,
        &base_input(&matrix, "component_base", b"lib/Foo/x.t\n")?
            .clone_at_frame(DiscoveryFrame::RunnerTDirectoryRelative),
    )?;
    assert_ne!(from_root.payload.rows[0].canonical_path(), from_t.payload.rows[0].canonical_path());
    assert_ne!(from_root.payload_digest, from_t.payload_digest);
    Ok(())
}

// ---------------------------------------------------------------------------
// Falsifier 5: changed raw/order keeps digest load-bearing
// ---------------------------------------------------------------------------

#[test]
fn equal_membership_with_changed_raw_or_order_changes_the_digest() -> Result<()> {
    let matrix = matrix()?;
    let straight =
        build(&matrix, &base_input(&matrix, "component_base", b"t/base/if.t\nt/base/cond.t\n")?)?;
    let swapped =
        build(&matrix, &base_input(&matrix, "component_base", b"t/base/cond.t\nt/base/if.t\n")?)?;
    let renamed =
        build(&matrix, &base_input(&matrix, "component_base", b"t/base/if.t\r\nt/base/cond.t\n")?)?;
    let membership = |receipt: &UpstreamDiscoveryReceiptV1| -> Vec<String> {
        let mut paths = receipt
            .payload
            .rows
            .iter()
            .filter_map(|row| row.canonical_path().map(str::to_string))
            .collect::<Vec<_>>();
        paths.sort();
        paths
    };
    assert_eq!(membership(&straight), membership(&swapped));
    assert_eq!(membership(&straight), membership(&renamed));
    assert_ne!(straight.payload_digest, swapped.payload_digest);
    assert_ne!(straight.payload_digest, renamed.payload_digest);

    // Reordering retained rows while keeping the old digest is rejected.
    let mut value = serde_json::to_value(&straight)?;
    let reordered_row = value["payload"]["rows"][1].clone();
    value["payload"]["rows"][1] = value["payload"]["rows"][0].clone();
    value["payload"]["rows"][0] = reordered_row;
    let tampered: UpstreamDiscoveryReceiptV1 = serde_json::from_value(value)?;
    assert_rejected_where(
        "row reorder under stale digest",
        validate_receipt_subject_binding(&tampered),
    )?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Falsifier 6: duplicates are retained, never deduplicated
// ---------------------------------------------------------------------------

#[test]
fn duplicated_member_is_retained_with_duplicate_disposition() -> Result<()> {
    let matrix = matrix()?;
    let receipt =
        build(&matrix, &base_input(&matrix, "component_base", b"t/base/if.t\nt/base/if.t\n")?)?;
    assert_eq!(receipt.payload.rows.len(), 2);
    assert_eq!(receipt.payload.work.accepted_rows, 1);
    assert_eq!(receipt.payload.work.duplicate_rows, 1);
    assert_eq!(
        receipt.payload.rows[1].disposition,
        MemberDisposition::DuplicateOfCanonical { canonical_path: "t/base/if.t".to_string() }
    );
    assert_eq!(receipt.payload.state, DiscoveryObservationState::ObservedPartial);
    ensure(validate_observed_discovery_receipt(&matrix, &receipt))?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Falsifier 7: out-of-target and unsupported members are retained
// ---------------------------------------------------------------------------

#[test]
fn out_of_target_and_unsupported_members_are_preserved() -> Result<()> {
    let matrix = matrix()?;
    let input = base_input(
        &matrix,
        "component_base",
        b"t/base/if.t\nt/op/hook/hook.t\nt/base/readme.txt\n",
    )?;
    let receipt = build(&matrix, &input)?;
    assert_eq!(receipt.payload.rows[1].disposition, MemberDisposition::OutsideTargetSelection);
    assert!(receipt.payload.rows[1].normalized.is_some());
    assert_eq!(receipt.payload.rows[2].disposition, MemberDisposition::UnsupportedSourceForm);
    assert_eq!(receipt.payload.work.out_of_target_rows, 1);
    assert_eq!(receipt.payload.work.unsupported_source_form_rows, 1);
    assert_eq!(receipt.payload.state, DiscoveryObservationState::ObservedPartial);
    ensure(validate_observed_discovery_receipt(&matrix, &receipt))?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Falsifier 8: complete-looking prefixes with failed terminals
// ---------------------------------------------------------------------------

#[test]
fn complete_prefix_with_failed_terminal_never_completes() -> Result<()> {
    let matrix = matrix()?;
    let clean = b"t/base/if.t\nt/base/cond.t\n";
    let cases = [
        (ProcessCompletion::ExitStatus { code: 2 }, DiscoveryObservationState::RunnerFailed),
        (ProcessCompletion::Signalled { signal: 15 }, DiscoveryObservationState::RunnerFailed),
        (
            ProcessCompletion::TimedOut { deadline_millis: 1000 },
            DiscoveryObservationState::TimedOut,
        ),
        (ProcessCompletion::Cancelled, DiscoveryObservationState::Cancelled),
        (ProcessCompletion::InstrumentFailed, DiscoveryObservationState::InstrumentFailed),
        (ProcessCompletion::Unknown, DiscoveryObservationState::NotProven),
    ];
    for (completion, expected_state) in cases {
        let mut input = base_input(&matrix, "component_base", clean)?;
        input.completion = completion;
        let receipt = build(&matrix, &input)?;
        assert_eq!(receipt.payload.state, expected_state, "{completion:?}");
        assert!(!receipt.payload.state.is_complete());
    }

    let mut truncated = base_input(&matrix, "component_base", clean)?;
    truncated.stdout_truncated = true;
    let receipt = build(&matrix, &truncated)?;
    assert_eq!(receipt.payload.state, DiscoveryObservationState::OutputTruncated);

    // A valid prefix followed by malformed trailing output is malformed.
    let mut broken = base_input(&matrix, "component_base", b"t/base/if.t\n")?;
    broken.stdout_bytes = b"t/base/if.t\n\x00bad\n".to_vec();
    let receipt = build(&matrix, &broken)?;
    assert_eq!(receipt.payload.state, DiscoveryObservationState::MalformedOutput);
    assert!(matches!(
        receipt.payload.stdout_decode,
        crate::observed_discovery::model::StreamDecodeOutcome::Complete
    ));
    assert_eq!(receipt.payload.work.malformed_rows, 1);
    Ok(())
}

#[test]
fn invalid_utf8_stream_is_malformed_not_repaired() -> Result<()> {
    let matrix = matrix()?;
    let mut input = base_input(&matrix, "component_base", b"t/base/if.t\n")?;
    input.stdout_bytes = Vec::from([b't', b'/', 0xff, b'\n']);
    let receipt = build(&matrix, &input)?;
    assert_eq!(receipt.payload.state, DiscoveryObservationState::MalformedOutput);
    assert!(receipt.payload.rows.is_empty());
    assert_eq!(receipt.payload.work.raw_stdout_bytes, 4);
    ensure(validate_receipt_subject_binding(&receipt))?;
    Ok(())
}

/// The decoder never repairs whitespace drift: a row carrying leading or
/// trailing whitespace is malformed, not trimmed into an accepted member, so
/// drifted runner output cannot silently satisfy discovery.
#[test]
fn whitespace_drifted_rows_are_malformed_not_trimmed_into_members() -> Result<()> {
    let matrix = matrix()?;
    // Leading-space drift on an otherwise accepted member.
    let mut drifted = base_input(&matrix, "component_base", b"t/base/if.t\n")?;
    drifted.stdout_bytes = b" t/base/if.t\n".to_vec();
    let receipt = build(&matrix, &drifted)?;
    assert_eq!(receipt.payload.state, DiscoveryObservationState::MalformedOutput);
    assert_eq!(receipt.payload.rows.len(), 1);
    assert!(matches!(receipt.payload.rows[0].disposition, MemberDisposition::MalformedRow));
    assert!(receipt.payload.rows[0].normalized.is_none());
    ensure(validate_receipt_subject_binding(&receipt))?;
    ensure(validate_observed_discovery_receipt(&matrix, &receipt))?;

    // Trailing-space drift on the final row keeps the same law.
    let mut trailing = base_input(&matrix, "component_base", b"t/base/if.t\n")?;
    trailing.stdout_bytes = b"t/base/if.t \n".to_vec();
    let receipt = build(&matrix, &trailing)?;
    assert_eq!(receipt.payload.state, DiscoveryObservationState::MalformedOutput);
    assert!(matches!(receipt.payload.rows[0].disposition, MemberDisposition::MalformedRow));
    ensure(validate_observed_discovery_receipt(&matrix, &receipt))?;

    // The undrifted spelling still passes: the law is about whitespace, not
    // about the member itself.
    let clean = base_input(&matrix, "component_base", b"t/base/if.t\n")?;
    let receipt = build(&matrix, &clean)?;
    assert_eq!(receipt.payload.state, DiscoveryObservationState::ObservedComplete);
    Ok(())
}

// ---------------------------------------------------------------------------
// Falsifier 9: swapped or copied stream identities
// ---------------------------------------------------------------------------

#[test]
fn swapped_streams_and_foreign_capture_identity_are_detected() -> Result<()> {
    let matrix = matrix()?;
    let receipt = build(&matrix, &base_input(&matrix, "component_base", b"t/base/if.t\n")?)?;

    let mut value = serde_json::to_value(&receipt)?;
    let stderr_envelope = value["payload"]["stderr"].clone();
    value["payload"]["stderr"] = value["payload"]["stdout"].clone();
    value["payload"]["stdout"] = stderr_envelope;
    let swapped: UpstreamDiscoveryReceiptV1 = serde_json::from_value(value)?;
    assert_rejected_where("stdout/stderr swap", validate_receipt_subject_binding(&swapped))?;

    let mut value = serde_json::to_value(&receipt)?;
    value["payload"]["terminal"]["process_nonce"] = json!("capture-9999");
    let copied: UpstreamDiscoveryReceiptV1 = serde_json::from_value(value)?;
    assert_rejected_where(
        "terminal copied from another process",
        validate_receipt_subject_binding(&copied),
    )?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Falsifier 10: another subject cannot fill the receipt
// ---------------------------------------------------------------------------

#[test]
fn mismatched_runner_artifact_records_subject_mismatch() -> Result<()> {
    let matrix = matrix()?;
    let mut input = base_input(&matrix, "component_base", b"t/base/if.t\n")?;
    input.runner_artifact.canonical_path = "t/harness".to_string();
    let receipt = build(&matrix, &input)?;
    assert_eq!(receipt.payload.state, DiscoveryObservationState::SubjectMismatch);
    assert!(!receipt.payload.state.is_complete());
    ensure(validate_observed_discovery_receipt(&matrix, &receipt))?;
    Ok(())
}

#[test]
fn foreign_target_or_matrix_binding_is_rejected() -> Result<()> {
    let matrix = matrix()?;
    let other_entry = find_entry(&matrix, "component_re")?;
    let foreign_digest = contract_digest(other_entry)?;

    let receipt = build(&matrix, &base_input(&matrix, "component_base", b"t/base/if.t\n")?)?;
    let mut value = serde_json::to_value(&receipt)?;
    value["payload"]["subject"]["target_contract_digest"] = json!(foreign_digest);
    let mut forged: UpstreamDiscoveryReceiptV1 = serde_json::from_value(value)?;
    forged.payload_digest = ensure_digest(discovery_payload_digest(&forged.payload))?;
    assert_rejected_where(
        "foreign target contract digest",
        validate_observed_discovery_receipt(&matrix, &forged),
    )?;

    let mut value = serde_json::to_value(&receipt)?;
    value["payload"]["subject"]["matrix_fingerprint"] = json!("f".repeat(64));
    let mut forged: UpstreamDiscoveryReceiptV1 = serde_json::from_value(value)?;
    forged.payload_digest = ensure_digest(discovery_payload_digest(&forged.payload))?;
    assert_rejected_where(
        "foreign matrix fingerprint",
        validate_observed_discovery_receipt(&matrix, &forged),
    )?;

    let direct_fallback = {
        let mut input = base_input(&matrix, "component_base", b"t/base/if.t\n")?;
        input.runner = RunnerKind::DirectFallback;
        input.runner_artifact.canonical_path = "perl-core-harness".to_string();
        input
    };
    assert!(build_observed_discovery_receipt(&matrix, &direct_fallback).is_err());
    Ok(())
}

// ---------------------------------------------------------------------------
// Falsifier 11: no local filesystem walk or direct probe may supply members
// ---------------------------------------------------------------------------

#[test]
fn fabricated_local_work_counters_are_rejected() -> Result<()> {
    let matrix = matrix()?;
    let receipt = build(&matrix, &base_input(&matrix, "component_base", b"t/base/if.t\n")?)?;

    for (field, value, needle) in [
        ("filesystem_discovery_operations", json!(3u64), "filesystem discovery"),
        ("direct_probe_rows_consumed", json!(5u64), "direct-probe"),
    ] {
        let mut mutated = serde_json::to_value(&receipt)?;
        mutated["payload"]["work"][field] = value;
        let mut forged: UpstreamDiscoveryReceiptV1 = serde_json::from_value(mutated)?;
        forged.payload_digest = ensure_digest(discovery_payload_digest(&forged.payload))?;
        let Err(error) = validate_receipt_subject_binding(&forged) else {
            bail!("fabricated {field} must be rejected");
        };
        assert!(
            error.to_string().contains(needle),
            "rejection must name the invariant {needle}: {error}"
        );
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Falsifier 12: unknown schema/state/form/frame values are never coerced
// ---------------------------------------------------------------------------

#[test]
fn unknown_enum_values_fail_closed_on_deserialization() -> Result<()> {
    use crate::observed_discovery::model::StreamDecodeOutcome;

    let matrix = matrix()?;
    let receipt = build(&matrix, &base_input(&matrix, "component_base", b"t/base/if.t\n")?)?;

    let mutations = [
        ("/payload/state", json!("totally_bogus")),
        ("/payload/discovery_frame", json!("checkout_absolute")),
        ("/payload/invocation/runner", json!("gnu_make")),
        ("/payload/rows/0/disposition/kind", json!("silently_accepted_anyway")),
        ("/payload/terminal/completion", json!("trust_me_completed")),
        ("/payload/stdout_decode/outcome", json!("close_enough")),
    ];
    for (pointer, replacement) in mutations {
        let mut value = serde_json::to_value(&receipt)?;
        let cursor = value
            .pointer_mut(pointer)
            .ok_or_else(|| color_eyre::eyre::eyre!("missing JSON pointer {pointer}"))?;
        *cursor = replacement;
        let decoded = serde_json::from_value::<UpstreamDiscoveryReceiptV1>(value);
        assert!(decoded.is_err(), "unknown value at {pointer} must not deserialize");
    }

    let mut value = serde_json::to_value(&receipt)?;
    value["payload"]["rows"][0]["discovery_frame"] = json!("made_up_frame");
    assert!(serde_json::from_value::<UpstreamDiscoveryReceiptV1>(value).is_err());

    let mut value = serde_json::to_value(&receipt)?;
    value["unexpected_field"] = json!(1);
    assert!(serde_json::from_value::<UpstreamDiscoveryReceiptV1>(value).is_err());

    // The well-formed receipt still round-trips its decode outcome type.
    assert!(matches!(receipt.payload.stdout_decode, StreamDecodeOutcome::Complete));
    Ok(())
}

// ---------------------------------------------------------------------------
// Falsifier 13: checkout paths and iteration order never change canonical bytes
// ---------------------------------------------------------------------------

#[test]
fn absolute_checkout_paths_are_rejected_everywhere() -> Result<()> {
    let matrix = matrix()?;
    let absolute_cases: Vec<fn(ObservedDiscoveryInput) -> ObservedDiscoveryInput> = vec![
        |mut input| {
            input.argv = vec!["F:\\tree\\t\\perl.exe".to_string()];
            input
        },
        |mut input| {
            input.argv = vec!["/usr/local/bin/perl".to_string()];
            input
        },
        |mut input| {
            input.working_directory = "C:\\tree\\t".to_string();
            input
        },
        |mut input| {
            input.working_directory = "/tmp/tree/t".to_string();
            input
        },
        |mut input| {
            input.runner_artifact.canonical_path = "/abs/t/TEST".to_string();
            input
        },
        |mut input| {
            input.working_directory = "t/../..".to_string();
            input
        },
    ];
    for mutate in absolute_cases {
        let input = mutate(base_input(&matrix, "component_base", b"t/base/if.t\n")?);
        assert!(
            build_observed_discovery_receipt(&matrix, &input).is_err(),
            "absolute or escaping invocation identity must be rejected"
        );
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Falsifier 14: self-asserted digests never substitute reconstruction
// ---------------------------------------------------------------------------

#[test]
fn self_consistent_digest_cannot_substitute_for_row_reconstruction() -> Result<()> {
    let matrix = matrix()?;
    let receipt =
        build(&matrix, &base_input(&matrix, "component_base", b"t/base/if.t\nt/base/cond.t\n")?)?;

    // Replace recorded membership with a smaller projection whose work
    // counters are also re-forged, so the only remaining detection surface is
    // reconstruction from the retained raw bytes.
    let mut value = serde_json::to_value(&receipt)?;
    value["payload"]["rows"] = serde_json::to_value(&receipt.payload.rows[..1])?;
    value["payload"]["work"]["decoded_rows"] = json!(1u64);
    value["payload"]["work"]["accepted_rows"] = json!(1u64);
    let mut forged: UpstreamDiscoveryReceiptV1 = serde_json::from_value(value)?;
    forged.payload_digest = ensure_digest(discovery_payload_digest(&forged.payload))?;

    let Err(error) = validate_observed_discovery_receipt(&matrix, &forged) else {
        bail!("self-consistent forged membership must still fail reconstruction");
    };
    let message = error.to_string();
    assert!(
        message.contains("reconstruct") || message.contains("recorded decoder work"),
        "failure must come from reconstruction, not the digest: {message}"
    );

    // Same class of attack on raw bytes: keep rows, swap retained stdout.
    let mut value = serde_json::to_value(&receipt)?;
    value["payload"]["stdout"]["bytes_hex"] =
        json!(crate::observed_discovery::model::hex_encode(b"t/base/cond.t\nt/base/zombie.t\n"));
    let mut forged: UpstreamDiscoveryReceiptV1 = serde_json::from_value(value)?;
    forged.payload_digest = ensure_digest(discovery_payload_digest(&forged.payload))?;
    assert_rejected_where(
        "swapped stdout bytes under fresh digest",
        validate_observed_discovery_receipt(&matrix, &forged),
    )?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Digest binding and work-accounting coherence
// ---------------------------------------------------------------------------

#[test]
fn payload_digest_binds_content_not_serialization_spelling() -> Result<()> {
    let matrix = matrix()?;
    let receipt = build(&matrix, &base_input(&matrix, "component_base", b"t/base/if.t\n")?)?;
    let mut value = serde_json::to_value(&receipt)?;
    value["payload"]["subject"]["host_perl_identity"] = json!("host-perl-other");
    let mutated: UpstreamDiscoveryReceiptV1 = serde_json::from_value(value)?;
    assert_rejected_where(
        "subject drift under stale digest",
        validate_receipt_subject_binding(&mutated),
    )?;
    Ok(())
}

#[test]
fn oversized_capture_is_refused_before_construction() -> Result<()> {
    let matrix = matrix()?;
    let mut input = base_input(&matrix, "component_base", b"t/base/if.t\n")?;
    input.stdout_bytes = vec![b'a'; crate::observed_discovery::model::MAX_RAW_STREAM_BYTES + 1];
    assert!(build_observed_discovery_receipt(&matrix, &input).is_err());

    // At exactly the bound with truncation flagged, construction succeeds and
    // completeness fails honestly.
    input.stdout_bytes = vec![b'a'; crate::observed_discovery::model::MAX_RAW_STREAM_BYTES];
    input.stdout_truncated = true;
    let receipt = build(&matrix, &input)?;
    assert_eq!(receipt.payload.state, DiscoveryObservationState::OutputTruncated);
    Ok(())
}

// ---------------------------------------------------------------------------
// Review repair: unadmitted runner routes must fail validation (PR #12472)
// ---------------------------------------------------------------------------

#[test]
fn unadmitted_runner_route_fails_every_validation_after_deserialization() -> Result<()> {
    let matrix = matrix()?;
    let receipt = build(&matrix, &base_input(&matrix, "component_base", b"t/base/if.t\n")?)?;

    // The demonstrated probe: a receipt whose recorded invocation is the
    // unadmitted direct-fallback route with the matching artifact spelling,
    // re-digested through the public digest so every other binding holds.
    let mut value = serde_json::to_value(&receipt)?;
    value["payload"]["invocation"]["runner"] = json!("direct_fallback");
    value["payload"]["invocation"]["runner_artifact"]["canonical_path"] =
        json!(RunnerKind::DirectFallback.entrypoint());
    let mut forged: UpstreamDiscoveryReceiptV1 = serde_json::from_value(value)?;
    forged.payload_digest = ensure_digest(discovery_payload_digest(&forged.payload))?;
    assert_eq!(forged.payload.state, DiscoveryObservationState::ObservedComplete);

    let Err(error) = validate_receipt_subject_binding(&forged) else {
        bail!("unadmitted route must fail subject-binding validation");
    };
    assert!(
        error.contains("not an admitted upstream discovery route"),
        "rejection must name the admitted-route law: {error}"
    );
    assert_rejected_where(
        "unadmitted route against matrix",
        validate_observed_discovery_receipt(&matrix, &forged),
    )?;
    Ok(())
}

#[test]
fn construction_binds_matrix_fingerprint_to_matrix_authority() -> Result<()> {
    let matrix = matrix()?;
    let mut input = base_input(&matrix, "component_base", b"t/base/if.t\n")?;
    input.subject.matrix_fingerprint = "e".repeat(64);
    let Err(error) = build_observed_discovery_receipt(&matrix, &input) else {
        bail!("construction must reject a subject fingerprint foreign to the pinned matrix");
    };
    assert!(
        error.contains("matrix fingerprint"),
        "rejection must name the matrix fingerprint binding: {error}"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Review repair: registered JSON schema agrees with produced receipts
// ---------------------------------------------------------------------------

/// Minimal structural validator covering exactly the JSON Schema keywords this
/// registered schema uses. Unknown keywords fail closed instead of passing.
mod schema_check {
    use serde_json::Value;

    pub fn validate(root: &Value, instance: &Value) -> Result<(), String> {
        check(root, root, instance)
    }

    fn check(schema: &Value, root: &Value, instance: &Value) -> Result<(), String> {
        if let Some(reference) = schema.get("$ref").and_then(Value::as_str) {
            let pointer = reference.strip_prefix('#').unwrap_or(reference);
            let target = root
                .pointer(pointer)
                .ok_or_else(|| format!("schema $ref {reference} unresolved"))?;
            return check(target, root, instance);
        }
        if let Some(expected) = schema.get("type") {
            let satisfied = match expected {
                Value::String(name) => type_matches(name, instance)?,
                Value::Array(names) => {
                    let mut matched = false;
                    for name in names.iter().filter_map(Value::as_str) {
                        matched = matched || type_matches(name, instance)?;
                    }
                    matched
                }
                other => return Err(format!("unsupported schema type shape {other}")),
            };
            if !satisfied {
                return Err(format!("instance violates type constraint {expected}"));
            }
        }
        if let Some(expected) = schema.get("const")
            && instance != expected
        {
            return Err(format!("instance violates const {expected}"));
        }
        if let Some(expected) = schema.get("enum").and_then(Value::as_array)
            && !expected.contains(instance)
        {
            return Err(format!("instance is outside enum {expected:?}"));
        }
        // Pattern/numeric keywords constrain their matching instance types;
        // other types are governed solely by the checked `type` keyword.
        if let Some(pattern) = schema.get("pattern").and_then(Value::as_str)
            && let Some(text) = instance.as_str()
        {
            anchored_pattern_matches(pattern, text)?;
        }
        if let Some(minimum) = schema.get("minimum").and_then(Value::as_i64)
            && let Some(number) = instance.as_i64()
            && number < minimum
        {
            return Err(format!("instance {number} is below minimum {minimum}"));
        }
        match instance {
            Value::String(text) => {
                if let Some(min) = schema.get("minLength").and_then(Value::as_u64)
                    && (text.chars().count() as u64) < min
                {
                    return Err(format!("string shorter than minLength {min}"));
                }
                if let Some(max) = schema.get("maxLength").and_then(Value::as_u64)
                    && (text.chars().count() as u64) > max
                {
                    return Err(format!("string longer than maxLength {max}"));
                }
            }
            Value::Array(items) => {
                if let Some(min) = schema.get("minItems").and_then(Value::as_u64)
                    && (items.len() as u64) < min
                {
                    return Err(format!("array shorter than minItems {min}"));
                }
                if schema.get("uniqueItems") == Some(&Value::Bool(true)) {
                    let duplicated = items
                        .iter()
                        .enumerate()
                        .any(|(index, item)| items[index + 1..].iter().any(|later| later == item));
                    if duplicated {
                        return Err("array items are not unique".to_string());
                    }
                }
                if let Some(item_schema) = schema.get("items") {
                    for item in items {
                        check(item_schema, root, item)?;
                    }
                }
            }
            Value::Object(object) => {
                for key in schema.get("required").and_then(Value::as_array).into_iter().flatten() {
                    let key = key.as_str().ok_or("required entries must be strings")?;
                    if !object.contains_key(key) {
                        return Err(format!("object is missing required key {key}"));
                    }
                }
                let properties = schema.get("properties").and_then(Value::as_object);
                let additional = schema.get("additionalProperties");
                for (key, value) in object {
                    match properties.and_then(|properties| properties.get(key)) {
                        Some(key_schema) => check(key_schema, root, value)?,
                        None => match additional {
                            Some(&Value::Bool(false)) => {
                                return Err(format!("object carries unknown property {key}"));
                            }
                            Some(additional_schema) => {
                                check(additional_schema, root, value)?;
                            }
                            _ => {}
                        },
                    }
                }
            }
            _ => {}
        }
        if let Some(branches) = schema.get("oneOf").and_then(Value::as_array) {
            let passing =
                branches.iter().filter(|branch| check(branch, root, instance).is_ok()).count();
            if passing != 1 {
                return Err(format!("instance satisfies {passing} oneOf branches, expected 1"));
            }
        }
        Ok(())
    }

    fn type_matches(name: &str, instance: &Value) -> Result<bool, String> {
        Ok(match name {
            "object" => instance.is_object(),
            "array" => instance.is_array(),
            "string" => instance.is_string(),
            "boolean" => instance.is_boolean(),
            "null" => instance.is_null(),
            "integer" => instance.is_i64() || instance.is_u64(),
            "number" => instance.is_number(),
            other => return Err(format!("unsupported schema type name {other}")),
        })
    }

    /// Anchored pattern matcher for the single-piece character-class shapes
    /// this schema uses (`^[class]{n,m}$`, `^[class]+$`, `^([class]{w})*$`).
    /// Any other grammar fails closed instead of passing.
    fn anchored_pattern_matches(pattern: &str, text: &str) -> Result<(), String> {
        let unsupported = || format!("unsupported pattern grammar {pattern}");
        let body = pattern
            .strip_prefix('^')
            .ok_or_else(unsupported)?
            .strip_suffix('$')
            .ok_or_else(unsupported)?;
        let (unit_width, min_units, max_units, class_body) =
            if let Some(inner) = body.strip_prefix('(').and_then(|rest| rest.strip_suffix(")*")) {
                let (class_body, width) = split_bracket_and_exact_repeat(inner)?;
                (width, 0, None, class_body)
            } else {
                let (class_body, quantifier) = split_bracket_and_quantifier(body)?;
                match quantifier {
                    Quantifier::OneOrMore | Quantifier::Plain => (1, 1, None, class_body),
                    Quantifier::Exact(units) => (1, units, Some(units), class_body),
                    Quantifier::Bounded(low, high) => (1, low, Some(high), class_body),
                }
            };
        let class = parse_char_class(class_body)?;
        let bytes = text.as_bytes();
        if !bytes.iter().all(|byte| class.contains(*byte)) {
            return Err(format!("text {text:?} contains characters outside {pattern}"));
        }
        if bytes.len() % unit_width != 0 {
            return Err(format!("text {text:?} length does not fit pattern {pattern}"));
        }
        let units = (bytes.len() / unit_width) as u64;
        if units < min_units || max_units.is_some_and(|max| units > max) {
            return Err(format!("text {text:?} length does not satisfy pattern {pattern}"));
        }
        Ok(())
    }

    enum Quantifier {
        Plain,
        OneOrMore,
        Exact(u64),
        Bounded(u64, u64),
    }

    /// Splits `[class]` plus an optional `{n}`, `{n,m}`, or `+` suffix.
    fn split_bracket_and_quantifier(body: &str) -> Result<(&str, Quantifier), String> {
        if let Some(rest) = body.strip_suffix('+') {
            let class = strip_brackets(rest).ok_or_else(|| format!("bad class in {body}"))?;
            return Ok((class, Quantifier::OneOrMore));
        }
        let Some((core, suffix)) = body.split_once('{') else {
            let class = strip_brackets(body).ok_or_else(|| format!("bad class in {body}"))?;
            return Ok((class, Quantifier::Plain));
        };
        let numbers =
            suffix.strip_suffix('}').ok_or_else(|| format!("bad quantifier in {body}"))?;
        let parse_number =
            |value: &str| value.parse::<u64>().map_err(|_| format!("bad quantifier in {body}"));
        let bounds = if let Some((low, high)) = numbers.split_once(',') {
            let max = match high.is_empty() {
                true => None,
                false => Some(parse_number(high)?),
            };
            (parse_number(low)?, max)
        } else {
            let exact = parse_number(numbers)?;
            (exact, Some(exact))
        };
        let class = strip_brackets(core).ok_or_else(|| format!("bad class in {body}"))?;
        Ok((
            class,
            match bounds {
                (low, Some(high)) if low == high => Quantifier::Exact(low),
                (low, Some(high)) => Quantifier::Bounded(low, high),
                (low, None) => Quantifier::Bounded(low, u64::MAX),
            },
        ))
    }

    /// Splits `[class]{w}` where the group-star unit repeats `w` bytes each.
    fn split_bracket_and_exact_repeat(body: &str) -> Result<(&str, usize), String> {
        let (class, quantifier) = split_bracket_and_quantifier(body)?;
        let Quantifier::Exact(units) = quantifier else {
            return Err(format!("group star requires an exact byte width, got {body}"));
        };
        Ok((class, units as usize))
    }

    fn strip_brackets(body: &str) -> Option<&str> {
        body.strip_prefix('[').and_then(|rest| rest.strip_suffix(']'))
    }

    fn parse_char_class(body: &str) -> Result<CharClass, String> {
        let mut class = CharClass::default();
        let mut chars = body.chars().peekable();
        while let Some(first) = chars.next() {
            if chars.peek() == Some(&'-') {
                chars.next();
                let last = chars.next().ok_or_else(|| format!("dangling range in class {body}"))?;
                class.ranges.push((first as u8, last as u8));
            } else {
                class.ranges.push((first as u8, first as u8));
            }
        }
        Ok(class)
    }

    #[derive(Default)]
    struct CharClass {
        ranges: Vec<(u8, u8)>,
    }

    impl CharClass {
        fn contains(&self, byte: u8) -> bool {
            self.ranges.iter().any(|(low, high)| *low <= byte && byte <= *high)
        }
    }
}

#[test]
fn produced_receipt_matches_registered_json_schema() -> Result<()> {
    let matrix = matrix()?;
    // One rich constructor-produced receipt: CRLF/LF/Eof framings plus
    // accepted, duplicate, out-of-target, and unsupported-form dispositions.
    let stream = b"t/base/if.t\r\nt/base/if.t\nt/op/hook/hook.t\nt/base/readme.txt";
    let receipt = build(&matrix, &base_input(&matrix, "component_base", stream)?)?;
    let schema_path =
        repo_file("schemas/perl_core_harness_upstream_runner_discovery.v1.schema.json");
    let schema: serde_json::Value = serde_json::from_slice(&std::fs::read(schema_path)?)?;

    let serialized = serde_json::to_value(&receipt)?;
    schema_check::validate(&schema, &serialized)
        .map_err(|error| eyre!("produced receipt violates registered schema: {error}"))?;

    // The deserialized round-trip keeps conforming to the same contract.
    let round_tripped: UpstreamDiscoveryReceiptV1 = serde_json::from_value(serialized.clone())?;
    let reserialized = serde_json::to_value(&round_tripped)?;
    schema_check::validate(&schema, &reserialized)
        .map_err(|error| eyre!("round-tripped receipt violates registered schema: {error}"))?;

    // Discriminators: the pre-repair shapes and the unadmitted route must be
    // rejected by the registered schema itself, not only by Rust validators.
    let legacy_outcome_cases = [
        ("/payload/stdout_decode", json!({"complete": {}})),
        ("/payload/terminal/completion", json!({"cancelled": {}})),
        ("/payload/invocation/runner", json!("direct_fallback")),
    ];
    for (pointer, replacement) in legacy_outcome_cases {
        let mut mutated = serialized.clone();
        let cursor = mutated
            .pointer_mut(pointer)
            .ok_or_else(|| color_eyre::eyre::eyre!("missing JSON pointer {pointer}"))?;
        *cursor = replacement;
        assert!(
            schema_check::validate(&schema, &mutated).is_err(),
            "registered schema must reject drifted shape at {pointer}"
        );
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Review repair nits: path absoluteness and hex discipline
// ---------------------------------------------------------------------------

#[test]
fn relative_colon_lookalike_argv_is_not_absolute() -> Result<()> {
    let matrix = matrix()?;
    let mut input = base_input(&matrix, "component_base", b"t/base/if.t\n")?;
    input.argv = vec!["1:t/base/if.t".to_string()];
    build(&matrix, &input)?;
    Ok(())
}

#[test]
fn uppercase_repository_commit_is_rejected_at_construction() -> Result<()> {
    let matrix = matrix()?;
    let mut input = base_input(&matrix, "component_base", b"t/base/if.t\n")?;
    input.subject.repository_commit = "A".repeat(40);
    let Err(error) = build_observed_discovery_receipt(&matrix, &input) else {
        bail!("uppercase repository commits must fail construction");
    };
    assert!(
        error.contains("lower-case hexadecimal"),
        "rejection must name lower-case hexadecimal law: {error}"
    );
    Ok(())
}

impl ObservedDiscoveryInput {
    fn clone_at_frame(mut self, frame: DiscoveryFrame) -> Self {
        self.discovery_frame = frame;
        self
    }
}

#[test]
fn deserialized_receipt_intake_rejects_noncanonical_artifact_digest_spelling() -> Result<()> {
    // #7725 review falsifier: receipts arriving by deserialization bypass
    // construction entirely, so the canonical-spelling law must hold on the
    // shared receipt-validation path, not only at the constructor.
    let matrix = matrix()?;
    let receipt = build(&matrix, &base_input(&matrix, "component_base", b"t/base/if.t\n")?)?;
    let original = receipt.payload.invocation.runner_artifact.content_sha256.clone();

    let retag_artifact = |spelled: String| -> Result<UpstreamDiscoveryReceiptV1> {
        let mut value = serde_json::to_value(&receipt)?;
        value["payload"]["invocation"]["runner_artifact"]["content_sha256"] = json!(spelled);
        let mut tampered: UpstreamDiscoveryReceiptV1 = serde_json::from_value(value)?;
        tampered.payload_digest =
            discovery_payload_digest(&tampered.payload).map_err(|error| eyre!(error))?;
        Ok(tampered)
    };

    let uppercased = retag_artifact(original.to_ascii_uppercase())?;
    assert_rejected_where(
        "uppercase artifact digest under a recomputed payload digest",
        validate_receipt_subject_binding(&uppercased),
    )?;
    assert!(validate_observed_discovery_receipt(&matrix, &uppercased).is_err());

    // Flip exactly one case-bearing (letter) nibble, never a digit, so the
    // mutation cannot collapse into the canonical control when the digest
    // happens to start with a hex digit.
    let mut mixed = original.clone();
    let letter_nibble = mixed
        .bytes()
        .position(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_digit())
        .ok_or_else(|| eyre!("fixture artifact digest carries no case-bearing nibble"))?;
    let flipped = mixed[letter_nibble..=letter_nibble].to_ascii_uppercase();
    mixed.replace_range(letter_nibble..=letter_nibble, &flipped);
    assert_ne!(mixed, original, "mixed-case mutation must alter the spelling");
    let mixed_case = retag_artifact(mixed)?;
    assert_rejected_where(
        "single mixed-case nibble under a recomputed payload digest",
        validate_receipt_subject_binding(&mixed_case),
    )?;

    // Canonical control: the unchanged spelling keeps validating.
    ensure(validate_receipt_subject_binding(&receipt))?;
    ensure(validate_observed_discovery_receipt(&matrix, &receipt))?;
    Ok(())
}
