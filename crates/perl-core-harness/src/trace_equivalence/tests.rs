//! Falsifier-first unit proof for the equivalence comparator (#12286).

use super::compare::build_trace_equivalence;
use super::model::{EquivalenceVerdict, TraceEquivalenceInput};
use super::validate::validate_trace_equivalence;
use crate::invocation_trace::instrument::{
    CleanupRecord, INSTRUMENTATION_WORK_SCHEMA_VERSION, InstrumentationState, InstrumentationWork,
    InstrumentationWorkPayload, InstrumentationWorkReceiptV1, PatchRecord,
    instrumentation_payload_digest,
};
use crate::invocation_trace::test_support::{TraceFixture, sha_hex};
use crate::observed_discovery::build::discovery_payload_digest;
use crate::observed_discovery::model::{
    ProcessCompletion, RunnerArtifactIdentity, UpstreamDiscoveryReceiptV1,
};
use crate::runner_model::RunnerKind;
use color_eyre::eyre::Result;
use std::collections::BTreeMap;
use std::path::Path;

const MEMBERS: [&str; 2] = ["t/base/cond.t", "t/base/if.t"];
const STDOUT: &str = "t/base/cond.t\nt/base/if.t\n";

fn refresh_discovery(receipt: &mut UpstreamDiscoveryReceiptV1) -> Result<()> {
    receipt.payload_digest = discovery_payload_digest(&receipt.payload)
        .map_err(|error| color_eyre::eyre::eyre!(error))?;
    Ok(())
}

fn ordinary_from(fixture: &TraceFixture) -> Result<UpstreamDiscoveryReceiptV1> {
    let mut ordinary = fixture.parent.clone();
    ordinary.payload.subject.instrumentation_id = None;
    // The ordinary side is its own process: distinct nonce on every envelope.
    ordinary.payload.terminal.process_nonce = "capture-ordinary-0001".to_string();
    ordinary.payload.stdout.process_nonce = "capture-ordinary-0001".to_string();
    ordinary.payload.stderr.process_nonce = "capture-ordinary-0001".to_string();
    refresh_discovery(&mut ordinary)?;
    Ok(ordinary)
}

fn work_for(fixture: &TraceFixture, trace_digest: &str) -> Result<InstrumentationWorkReceiptV1> {
    let payload = InstrumentationWorkPayload {
        instrumentation_id: "trace-instrument-1".to_string(),
        runner: RunnerKind::Test,
        target_id: "component_base".to_string(),
        ordinary_artifact: RunnerArtifactIdentity {
            canonical_path: "t/TEST".to_string(),
            content_sha256: sha_hex(b"t/TEST"),
        },
        instrumented_artifact: RunnerArtifactIdentity {
            canonical_path: "t/TEST".to_string(),
            content_sha256: sha_hex(b"t/TEST"),
        },
        patch: PatchRecord {
            spec_sha256: sha_hex(b"patch-spec"),
            tool_identity: "exact-anchor-patch-test".to_string(),
            capture_point: "t/TEST runtests invocation decision".to_string(),
            trace_schema: "perl_core_harness.upstream_effective_invocation_trace.v1".to_string(),
            operations_applied: vec!["instrument-runtests-invocation-decision".to_string()],
            exact_anchor_matches: 1,
        },
        manifest_before: BTreeMap::from([("t/TEST".to_string(), sha_hex(b"t/TEST"))]),
        manifest_after: BTreeMap::from([("t/TEST".to_string(), sha_hex(b"t/TEST"))]),
        trace_session_id: fixture.session.clone(),
        process_nonce: fixture.parent.payload.terminal.process_nonce.clone(),
        supervisor_completion: ProcessCompletion::ExitStatus { code: 0 },
        trace_terminal_completion: Some(ProcessCompletion::ExitStatus { code: 0 }),
        parent_state: Some(
            crate::observed_discovery::model::DiscoveryObservationState::ObservedComplete,
        ),
        parent_construction_error: None,
        trace_construction_error: None,
        parent_receipt_digest: Some(fixture.parent.payload_digest.clone()),
        trace_receipt_digest: Some(trace_digest.to_string()),
        trace_decode: None,
        work: InstrumentationWork {
            instrumented_processes: 1,
            patch_operations_applied: 1,
            exact_anchor_matches: 1,
            manifest_files_before: 1,
            manifest_files_after: 1,
            manifest_files_changed: 0,
            trace_bytes: 0,
            trace_frames: 0,
            trace_rows: 2,
            complete_rows: 2,
            partial_rows: 0,
            malformed_rows: 0,
            conflicting_rows: 0,
            runner_failed_rows: 0,
            instrument_failed_rows: 0,
            subject_mismatch_rows: 0,
            not_proven_rows: 0,
            canonical_plan_projections: 2,
            canonical_plan_projections_accepted: 2,
            ordinary_output_contamination_count: 0,
            fields_synthesized: 0,
            direct_rows_consumed: 0,
            terminal_disagreements: 0,
            cleanup_failures: 0,
        },
        cleanup: CleanupRecord {
            instrumented_tree_removed: true,
            trace_file_removed: true,
            run_directory_removed: true,
            failures: Vec::new(),
        },
        state: InstrumentationState::ObservedComplete,
        limitations: Vec::new(),
        claim_boundary: "test fixture work receipt".to_string(),
    };
    let payload_digest =
        instrumentation_payload_digest(&payload).map_err(|error| color_eyre::eyre::eyre!(error))?;
    Ok(InstrumentationWorkReceiptV1 {
        schema_version: INSTRUMENTATION_WORK_SCHEMA_VERSION.to_string(),
        payload_digest,
        payload,
    })
}

fn trace_for(
    fixture: &TraceFixture,
    members: &[&str],
) -> Result<crate::invocation_trace::model::EffectiveInvocationTraceReceiptV1> {
    let bytes = fixture.emit_complete(members)?;
    crate::invocation_trace::build::build_invocation_trace_receipt(&fixture.input(bytes))
        .map_err(|error| color_eyre::eyre::eyre!(error))
}

fn pair(stdout: &str, members: &[&str]) -> Result<TraceEquivalenceInput> {
    let fixture = TraceFixture::new("component_base", stdout)?;
    let trace = trace_for(&fixture, members)?;
    let work = work_for(&fixture, &trace.payload_digest.clone())?;
    let ordinary = ordinary_from(&fixture)?;
    Ok(TraceEquivalenceInput {
        matrix: crate::invocation_trace::test_support::matrix()?,
        ordinary: Some(ordinary),
        instrumented_parent: fixture.parent.clone(),
        trace,
        work,
    })
}

#[test]
fn equivalent_pair_grants_only_bounded_transfer() -> Result<()> {
    let input = pair(STDOUT, &MEMBERS)?;
    let receipt =
        build_trace_equivalence(&input).map_err(|error| color_eyre::eyre::eyre!(error))?;
    assert_eq!(receipt.payload.verdict, EquivalenceVerdict::EquivalentForDeclaredPlanes);
    assert_eq!(receipt.payload.transferable.len(), 1);
    assert!(receipt.payload.planes.iter().all(|plane| plane.matched));
    validate_trace_equivalence(&receipt).map_err(|error| color_eyre::eyre::eyre!(error))?;
    // Determinism: same inputs, byte-identical receipt.
    let again = build_trace_equivalence(&input).map_err(|error| color_eyre::eyre::eyre!(error))?;
    assert_eq!(
        serde_json::to_vec(&receipt).map_err(|error| color_eyre::eyre::eyre!(error))?,
        serde_json::to_vec(&again).map_err(|error| color_eyre::eyre::eyre!(error))?
    );
    Ok(())
}

#[test]
fn missing_ordinary_pair_is_not_proven() -> Result<()> {
    let mut input = pair(STDOUT, &MEMBERS)?;
    input.ordinary = None;
    let receipt =
        build_trace_equivalence(&input).map_err(|error| color_eyre::eyre::eyre!(error))?;
    assert_eq!(receipt.payload.verdict, EquivalenceVerdict::NotProven);
    assert!(receipt.payload.transferable.is_empty());
    Ok(())
}

#[test]
fn member_order_perturbation_is_measured_difference() -> Result<()> {
    let swapped = "t/base/if.t\nt/base/cond.t\n";
    let input = pair(swapped, &["t/base/if.t", "t/base/cond.t"])?;
    // Ordinary side keeps the canonical order from the standard fixture.
    let canonical = TraceFixture::new("component_base", STDOUT)?;
    let mut input = input;
    input.ordinary = Some(ordinary_from(&canonical)?);
    let receipt =
        build_trace_equivalence(&input).map_err(|error| color_eyre::eyre::eyre!(error))?;
    assert_eq!(receipt.payload.verdict, EquivalenceVerdict::MeasuredDifference);
    Ok(())
}

#[test]
fn terminal_difference_is_never_equivalent() -> Result<()> {
    // A hand-mutated terminal fails strict upstream validation, so the pair
    // is not_proven here; plane-level terminal discrimination is proven with
    // real processes in tests/trace_equivalence_capture.rs.
    let mut input = pair(STDOUT, &MEMBERS)?;
    if let Some(ordinary) = &mut input.ordinary {
        ordinary.payload.terminal.completion = ProcessCompletion::ExitStatus { code: 7 };
        refresh_discovery(ordinary)?;
    }
    let receipt =
        build_trace_equivalence(&input).map_err(|error| color_eyre::eyre::eyre!(error))?;
    assert_ne!(receipt.payload.verdict, EquivalenceVerdict::EquivalentForDeclaredPlanes);
    assert!(receipt.payload.transferable.is_empty());
    Ok(())
}

#[test]
fn missing_trace_row_is_measured_difference() -> Result<()> {
    let input = pair(STDOUT, &MEMBERS[..1])?;
    let receipt =
        build_trace_equivalence(&input).map_err(|error| color_eyre::eyre::eyre!(error))?;
    assert_eq!(receipt.payload.verdict, EquivalenceVerdict::MeasuredDifference);
    Ok(())
}

#[test]
fn aggregate_count_substitution_is_measured_difference() -> Result<()> {
    // Same member count, different member: counts must not substitute.
    let input = pair("t/base/cond.t\nt/base/lex.t\n", &["t/base/cond.t", "t/base/lex.t"])?;
    let canonical = TraceFixture::new("component_base", STDOUT)?;
    let mut input = input;
    input.ordinary = Some(ordinary_from(&canonical)?);
    let receipt =
        build_trace_equivalence(&input).map_err(|error| color_eyre::eyre::eyre!(error))?;
    assert_eq!(receipt.payload.verdict, EquivalenceVerdict::MeasuredDifference);
    Ok(())
}

#[test]
fn patch_drift_is_not_comparable() -> Result<()> {
    let mut input = pair(STDOUT, &MEMBERS)?;
    input.work.payload.instrumented_artifact.content_sha256 = sha_hex(b"drifted TEST");
    input.work.payload_digest = instrumentation_payload_digest(&input.work.payload)
        .map_err(|error| color_eyre::eyre::eyre!(error))?;
    let receipt =
        build_trace_equivalence(&input).map_err(|error| color_eyre::eyre::eyre!(error))?;
    assert_eq!(receipt.payload.verdict, EquivalenceVerdict::NotComparable);
    Ok(())
}

#[test]
fn receipt_satisfies_registered_schema_and_rejects_unknown_fields() -> Result<()> {
    let input = pair(STDOUT, &MEMBERS)?;
    let receipt =
        build_trace_equivalence(&input).map_err(|error| color_eyre::eyre::eyre!(error))?;
    let schema_path = crate::invocation_trace::test_support::repo_file(
        "schemas/perl_core_harness_upstream_trace_equivalence.v1.schema.json",
    );
    let schema: serde_json::Value = serde_json::from_slice(
        &std::fs::read(schema_path).map_err(|error| color_eyre::eyre::eyre!(error))?,
    )
    .map_err(|error| color_eyre::eyre::eyre!(error))?;
    let value = serde_json::to_value(&receipt).map_err(|error| color_eyre::eyre::eyre!(error))?;
    crate::schema_check::validate(&schema, &value)
        .map_err(|error| color_eyre::eyre::eyre!(error))?;
    // An unknown field is rejected by the schema and by serde alike.
    let mut mutated = value.clone();
    mutated["payload"]["surprise"] = serde_json::json!("field");
    assert!(crate::schema_check::validate(&schema, &mutated).is_err());
    assert!(serde_json::from_value::<super::model::UpstreamTraceEquivalenceV1>(mutated).is_err());
    Ok(())
}

#[test]
fn forged_input_digest_is_not_proven() -> Result<()> {
    // Regression for review finding (1): stored digest strings alone must
    // never grant a relation; the comparator recomputes every input digest.
    let mut input = pair(STDOUT, &MEMBERS)?;
    if let Some(ordinary) = &mut input.ordinary {
        ordinary.payload_digest = "0".repeat(64);
    }
    let receipt =
        build_trace_equivalence(&input).map_err(|error| color_eyre::eyre::eyre!(error))?;
    assert_eq!(receipt.payload.verdict, EquivalenceVerdict::NotProven);
    assert!(receipt.payload.transferable.is_empty());
    Ok(())
}

#[test]
fn stderr_difference_is_never_equivalent() -> Result<()> {
    // Regression for review finding (2): stderr is ordinary behavior too.
    // A hand-mutated receipt fails strict upstream validation, so the pair
    // is not_proven here; plane-level stderr discrimination is proven with
    // real processes in tests/trace_equivalence_capture.rs.
    let mut input = pair(STDOUT, &MEMBERS)?;
    if let Some(ordinary) = &mut input.ordinary {
        ordinary.payload.stderr.bytes_hex = "7761726e0a".to_string();
        refresh_discovery(ordinary)?;
    }
    let receipt =
        build_trace_equivalence(&input).map_err(|error| color_eyre::eyre::eyre!(error))?;
    assert_ne!(receipt.payload.verdict, EquivalenceVerdict::EquivalentForDeclaredPlanes);
    assert!(receipt.payload.transferable.is_empty());
    Ok(())
}

#[test]
fn validator_rejects_invented_matched_plane() -> Result<()> {
    // Regression for review finding (1): a hand-written passing receipt with
    // one invented plane must not validate.
    let input = pair(STDOUT, &MEMBERS)?;
    let mut receipt =
        build_trace_equivalence(&input).map_err(|error| color_eyre::eyre::eyre!(error))?;
    receipt.payload.planes.truncate(1);
    receipt.payload_digest = super::compare::equivalence_payload_digest(&receipt.payload)
        .map_err(|error| color_eyre::eyre::eyre!(error))?;
    assert!(validate_trace_equivalence(&receipt).is_err());
    Ok(())
}

#[test]
fn forged_receipt_fails_provenance_check() -> Result<()> {
    // Regression for re-review finding (1b): a shape-valid forged receipt
    // with invented plane digests must fail the provenance entry point,
    // which rebuilds the receipt from the exact paired inputs.
    let input = pair(STDOUT, &MEMBERS)?;
    let mut forged =
        build_trace_equivalence(&input).map_err(|error| color_eyre::eyre::eyre!(error))?;
    for plane in &mut forged.payload.planes {
        plane.ordinary_digest = "a".repeat(64);
        plane.instrumented_digest = "a".repeat(64);
    }
    forged.payload_digest = super::compare::equivalence_payload_digest(&forged.payload)
        .map_err(|error| color_eyre::eyre::eyre!(error))?;
    assert!(super::compare::check_trace_equivalence(&input, &forged).is_err());
    let honest = build_trace_equivalence(&input).map_err(|error| color_eyre::eyre::eyre!(error))?;
    super::compare::check_trace_equivalence(&input, &honest)
        .map_err(|error| color_eyre::eyre::eyre!(error))?;
    Ok(())
}

#[test]
fn cli_route_without_ordinary_emits_not_proven() -> Result<()> {
    // Direct static call of the CLI route (the subprocess form in the
    // integration suite creates no call edge): the `None` arm for a
    // missing --ordinary must emit the typed not_proven receipt.
    let input = pair(STDOUT, &MEMBERS)?;
    let temp = tempfile::tempdir()?;
    fn write<T: serde::Serialize>(dir: &Path, name: &str, value: &T) -> Result<std::path::PathBuf> {
        let path = dir.join(name);
        std::fs::write(&path, serde_json::to_vec(value)?)?;
        Ok(path)
    }
    let dir = temp.path();
    let parent = write(dir, "parent.json", &input.instrumented_parent)?;
    let trace = write(dir, "trace.json", &input.trace)?;
    let work = write(dir, "work.json", &input.work)?;
    let output = temp.path().join("equivalence.json");
    let matrix = crate::invocation_trace::test_support::repo_file(
        ".ci/perl-core-harness/upstream-targets-5.42.2.v1",
    );
    let options = crate::artifacts::Options::parse(
        [
            "--matrix".to_string(),
            matrix.to_string_lossy().into_owned(),
            "--instrumented-parent".to_string(),
            parent.to_string_lossy().into_owned(),
            "--trace".to_string(),
            trace.to_string_lossy().into_owned(),
            "--work".to_string(),
            work.to_string_lossy().into_owned(),
            "--output".to_string(),
            output.to_string_lossy().into_owned(),
        ]
        .into_iter(),
    )?;
    super::compare::compare_equivalence_from_options(options)?;
    let receipt: super::model::UpstreamTraceEquivalenceV1 =
        serde_json::from_slice(&std::fs::read(&output)?)?;
    assert_eq!(receipt.payload.verdict, EquivalenceVerdict::NotProven);
    Ok(())
}

#[test]
fn cross_subject_reuse_is_not_comparable() -> Result<()> {
    let mut input = pair(STDOUT, &MEMBERS)?;
    if let Some(ordinary) = &mut input.ordinary {
        ordinary.payload.subject.perl_ref = "perl-5.40.0".to_string();
        refresh_discovery(ordinary)?;
    }
    let receipt =
        build_trace_equivalence(&input).map_err(|error| color_eyre::eyre::eyre!(error))?;
    assert_eq!(receipt.payload.verdict, EquivalenceVerdict::NotComparable);
    Ok(())
}
