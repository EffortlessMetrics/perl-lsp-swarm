//! Comparator and strict constructor for the equivalence receipt (#12286).
//!
//! The comparator never repairs either side: no local expansion, no member
//! reordering, no aggregate-count substitution, and no direct-probe repair.
//! Any changed ordinary behavior yields a measured difference or a
//! non-proven verdict rather than hidden parity.

use super::model::{
    ComparisonPlane, EquivalenceSubjects, EquivalenceVerdict, NON_TRANSFERABLE_PROPOSITIONS,
    PlaneComparison, TRACE_EQUIVALENCE_CLAIM_BOUNDARY, TRANSFERABLE_PROPOSITION,
    TraceEquivalenceInput, TraceEquivalencePayload, UPSTREAM_TRACE_EQUIVALENCE_SCHEMA_VERSION,
    UpstreamTraceEquivalenceV1,
};
use crate::artifacts::{Options, write_json};
use crate::build::sha256_bytes;
use crate::invocation_trace::instrument::InstrumentationState;
use crate::observed_discovery::model::{DiscoveryObservationState, ProcessCompletion};
use crate::observed_subject::model::OrdinaryInstrumentedEquivalenceIdentity;

/// Canonical payload digest (`SHA-256` over the serde serialization).
pub fn equivalence_payload_digest(payload: &TraceEquivalencePayload) -> Result<String, String> {
    let bytes = serde_json::to_vec(payload)
        .map_err(|error| format!("serializing trace equivalence payload: {error}"))?;
    Ok(sha256_bytes(&bytes))
}

fn digest_of(value: &impl serde::Serialize) -> String {
    match serde_json::to_vec(value) {
        Ok(bytes) => sha256_bytes(&bytes),
        Err(_) => sha256_bytes(b"unserializable-plane-identity"),
    }
}

fn accepted_members(
    receipt: &crate::observed_discovery::model::UpstreamDiscoveryReceiptV1,
) -> Vec<String> {
    receipt
        .payload
        .rows
        .iter()
        .filter(|row| row.is_accepted())
        .filter_map(|row| row.canonical_path().map(str::to_string))
        .collect()
}

fn limitations() -> Vec<String> {
    vec![
        "ordinary child-invocation fields are unobserved on the ordinary side; equivalence covers surrounding runner behavior only".to_string(),
        "equal aggregate file or assertion counts contribute zero equivalence authority".to_string(),
        "real-tree paired rows remain unproven until a prepared upstream tree is captured on both sides".to_string(),
    ]
}

fn finish(
    subjects: EquivalenceSubjects,
    target_id: String,
    perl_ref: String,
    repository_commit: String,
    verdict: EquivalenceVerdict,
    planes: Vec<PlaneComparison>,
    differences: Vec<String>,
) -> Result<UpstreamTraceEquivalenceV1, String> {
    let transferable = if verdict.grants_transfer() {
        vec![TRANSFERABLE_PROPOSITION.to_string()]
    } else {
        Vec::new()
    };
    let payload = TraceEquivalencePayload {
        subjects,
        target_id,
        perl_ref,
        repository_commit,
        verdict,
        planes,
        differences,
        transferable,
        non_transferable: NON_TRANSFERABLE_PROPOSITIONS
            .iter()
            .map(|value| value.to_string())
            .collect(),
        limitations: limitations(),
        claim_boundary: TRACE_EQUIVALENCE_CLAIM_BOUNDARY.to_string(),
    };
    let payload_digest = equivalence_payload_digest(&payload)?;
    Ok(UpstreamTraceEquivalenceV1 {
        schema_version: UPSTREAM_TRACE_EQUIVALENCE_SCHEMA_VERSION.to_string(),
        payload_digest,
        payload,
    })
}

/// Compare one exact ordinary/instrumented pair and emit the claim-ceiling
/// receipt. The same inputs always produce byte-identical receipts.
pub fn build_trace_equivalence(
    input: &TraceEquivalenceInput,
) -> Result<UpstreamTraceEquivalenceV1, String> {
    let instrumented = &input.instrumented_parent;
    let trace = &input.trace;
    let work = &input.work;
    let subjects = EquivalenceSubjects {
        ordinary_receipt_digest: input
            .ordinary
            .as_ref()
            .map(|receipt| receipt.payload_digest.clone())
            .unwrap_or_default(),
        instrumented_parent_receipt_digest: instrumented.payload_digest.clone(),
        trace_receipt_digest: trace.payload_digest.clone(),
        work_receipt_digest: work.payload_digest.clone(),
        equivalence_identity: OrdinaryInstrumentedEquivalenceIdentity {
            instrumentation_id: work.payload.instrumentation_id.clone(),
            ordinary_runner_artifact_sha256: work.payload.ordinary_artifact.content_sha256.clone(),
            instrumented_runner_artifact_sha256: work
                .payload
                .instrumented_artifact
                .content_sha256
                .clone(),
            patch_subject_digest: work.payload.patch.spec_sha256.clone(),
        },
    };
    let target_id = instrumented.payload.subject.target_id.clone();
    let perl_ref = instrumented.payload.subject.perl_ref.clone();
    let repository_commit = instrumented.payload.subject.repository_commit.clone();

    // Falsifier 1: no ordinary paired run, no relation.
    let Some(ordinary) = &input.ordinary else {
        return finish(
            subjects,
            target_id,
            perl_ref,
            repository_commit,
            EquivalenceVerdict::NotProven,
            Vec::new(),
            vec!["missing ordinary paired run".to_string()],
        );
    };

    // Instrument failure around the runner: no runner fact is claimed.
    if work.payload.state == InstrumentationState::InstrumentFailed
        || work.payload.parent_construction_error.is_some()
        || work.payload.trace_construction_error.is_some()
        || matches!(work.payload.supervisor_completion, ProcessCompletion::InstrumentFailed)
    {
        return finish(
            subjects,
            target_id,
            perl_ref,
            repository_commit,
            EquivalenceVerdict::InstrumentFailure,
            Vec::new(),
            vec!["instrument failed around the runner".to_string()],
        );
    }

    // Exact subject binding: any drift makes the pair not comparable.
    let mut reasons = Vec::new();
    let ordinary_subject = &ordinary.payload.subject;
    let instrumented_subject = &instrumented.payload.subject;
    if ordinary_subject.instrumentation_id.is_some() {
        reasons.push("ordinary side is itself instrumented".to_string());
    }
    if instrumented_subject.instrumentation_id.as_deref()
        != Some(work.payload.instrumentation_id.as_str())
    {
        reasons.push("instrumented parent does not bind the work instrumentation".to_string());
    }
    for (label, left, right) in [
        (
            "repository_commit",
            &ordinary_subject.repository_commit,
            &instrumented_subject.repository_commit,
        ),
        ("perl_ref", &ordinary_subject.perl_ref, &instrumented_subject.perl_ref),
        (
            "prepared_tree_identity",
            &ordinary_subject.prepared_tree_identity,
            &instrumented_subject.prepared_tree_identity,
        ),
        (
            "host_perl_identity",
            &ordinary_subject.host_perl_identity,
            &instrumented_subject.host_perl_identity,
        ),
        (
            "matrix_fingerprint",
            &ordinary_subject.matrix_fingerprint,
            &instrumented_subject.matrix_fingerprint,
        ),
        ("target_id", &ordinary_subject.target_id, &instrumented_subject.target_id),
        (
            "target_contract_digest",
            &ordinary_subject.target_contract_digest,
            &instrumented_subject.target_contract_digest,
        ),
    ] {
        if left != right {
            reasons.push(format!("subject {label} differs between pair sides"));
        }
    }
    if ordinary_subject.variant_target_id != instrumented_subject.variant_target_id {
        reasons.push("subject variant_target_id differs between pair sides".to_string());
    }
    if trace.payload.subject.parent_receipt_digest != instrumented.payload_digest {
        reasons.push("trace does not bind the instrumented parent receipt".to_string());
    }
    if work.payload.parent_receipt_digest.as_deref() != Some(instrumented.payload_digest.as_str()) {
        reasons.push("work receipt does not bind the instrumented parent receipt".to_string());
    }
    if work.payload.trace_receipt_digest.as_deref() != Some(trace.payload_digest.as_str()) {
        reasons.push("work receipt does not bind the trace receipt".to_string());
    }
    if ordinary.payload.invocation.runner_artifact.content_sha256
        != work.payload.ordinary_artifact.content_sha256
    {
        reasons
            .push("ordinary runner artifact differs from the patched subject origin".to_string());
    }
    if instrumented.payload.invocation.runner_artifact.content_sha256
        != work.payload.instrumented_artifact.content_sha256
    {
        reasons.push("instrumented runner artifact differs from the work receipt".to_string());
    }
    if !reasons.is_empty() {
        return finish(
            subjects,
            target_id,
            perl_ref,
            repository_commit,
            EquivalenceVerdict::NotComparable,
            Vec::new(),
            reasons,
        );
    }

    // Both sides must be complete observations before any plane is judged.
    if ordinary.payload.state != DiscoveryObservationState::ObservedComplete
        || instrumented.payload.state != DiscoveryObservationState::ObservedComplete
    {
        return finish(
            subjects,
            target_id,
            perl_ref,
            repository_commit,
            EquivalenceVerdict::NotProven,
            Vec::new(),
            vec!["paired discovery is not observed_complete on both sides".to_string()],
        );
    }

    let mut planes = Vec::new();

    // Discovery bytes: the trace channel is isolated, so stdout must match
    // byte-for-byte; only the envelope nonce identity is process-local.
    let ordinary_stdout = ordinary.payload.stdout.bytes().unwrap_or_default();
    let instrumented_stdout = instrumented.payload.stdout.bytes().unwrap_or_default();
    planes.push(PlaneComparison {
        plane: ComparisonPlane::DiscoveryBytes,
        ordinary_digest: sha256_bytes(&ordinary_stdout),
        instrumented_digest: sha256_bytes(&instrumented_stdout),
        matched: ordinary_stdout == instrumented_stdout,
        differences: if ordinary_stdout == instrumented_stdout {
            Vec::new()
        } else {
            vec!["raw discovery stdout bytes differ".to_string()]
        },
    });

    // Membership and original order, never aggregate counts.
    let ordinary_members = accepted_members(ordinary);
    let instrumented_members = accepted_members(instrumented);
    planes.push(PlaneComparison {
        plane: ComparisonPlane::MembershipOrder,
        ordinary_digest: digest_of(&ordinary_members),
        instrumented_digest: digest_of(&instrumented_members),
        matched: ordinary_members == instrumented_members,
        differences: if ordinary_members == instrumented_members {
            Vec::new()
        } else {
            vec!["accepted membership or original order differs".to_string()]
        },
    });

    // Terminal process disposition, including the trace terminal cross-check.
    let terminal_matched = ordinary.payload.terminal.completion
        == instrumented.payload.terminal.completion
        && work.payload.trace_terminal_completion == Some(work.payload.supervisor_completion);
    planes.push(PlaneComparison {
        plane: ComparisonPlane::TerminalProcess,
        ordinary_digest: digest_of(&ordinary.payload.terminal.completion),
        instrumented_digest: digest_of(&instrumented.payload.terminal.completion),
        matched: terminal_matched,
        differences: if terminal_matched {
            Vec::new()
        } else {
            vec!["terminal process disposition differs".to_string()]
        },
    });

    // Cleanup of the disposable copy, trace file, run directory, and children.
    let cleanup = &work.payload.cleanup;
    let cleanup_proven = cleanup.instrumented_tree_removed
        && cleanup.trace_file_removed
        && cleanup.run_directory_removed
        && cleanup.failures.is_empty();
    planes.push(PlaneComparison {
        plane: ComparisonPlane::Cleanup,
        ordinary_digest: digest_of(&"ordinary run needs no disposable cleanup"),
        instrumented_digest: digest_of(&cleanup),
        matched: cleanup_proven,
        differences: if cleanup_proven {
            Vec::new()
        } else {
            vec!["instrumented cleanup is not proven".to_string()]
        },
    });

    // Invocation rows: exactly the accepted instrumented members, each once.
    // A hidden or extra child shows up here even when membership matches.
    let mut row_members: Vec<String> =
        trace.payload.rows.iter().map(|row| row.subject.parent_member_path.clone()).collect();
    let row_count = row_members.len();
    row_members.sort();
    let mut expected_members = instrumented_members.clone();
    expected_members.sort();
    let rows_matched = row_members == expected_members && row_count == instrumented_members.len();
    planes.push(PlaneComparison {
        plane: ComparisonPlane::InvocationRows,
        ordinary_digest: digest_of(&instrumented_members),
        instrumented_digest: digest_of(&trace.payload.rows),
        matched: rows_matched,
        differences: if rows_matched {
            Vec::new()
        } else {
            vec!["trace rows do not cover exactly the accepted members".to_string()]
        },
    });

    let differences: Vec<String> = planes
        .iter()
        .flat_map(|plane| plane.differences.iter().map(|item| format!("{:?}: {item}", plane.plane)))
        .collect();
    let verdict = if differences.is_empty() {
        EquivalenceVerdict::EquivalentForDeclaredPlanes
    } else {
        EquivalenceVerdict::MeasuredDifference
    };
    finish(subjects, target_id, perl_ref, repository_commit, verdict, planes, differences)
}

/// CLI route (`compare-equivalence`): read the paired receipts, emit and
/// validate one equivalence receipt. `--ordinary` is optional so a missing
/// pair produces a typed `not_proven` receipt instead of a silent absence.
pub(crate) fn compare_equivalence_from_options(
    mut options: Options,
) -> color_eyre::eyre::Result<()> {
    use std::path::PathBuf;
    let ordinary = match options.optional("--ordinary")? {
        Some(path) => Some(read_receipt(&PathBuf::from(path))?),
        None => None,
    };
    let instrumented_parent =
        read_receipt(&PathBuf::from(options.required("--instrumented-parent")?))?;
    let trace = read_receipt(&PathBuf::from(options.required("--trace")?))?;
    let work = read_receipt(&PathBuf::from(options.required("--work")?))?;
    let output = PathBuf::from(options.required("--output")?);
    let input = TraceEquivalenceInput { ordinary, instrumented_parent, trace, work };
    let receipt =
        build_trace_equivalence(&input).map_err(|error| color_eyre::eyre::eyre!(error))?;
    super::validate::validate_trace_equivalence(&receipt)
        .map_err(|error| color_eyre::eyre::eyre!(error))?;
    write_json(&output, &receipt)
}

fn read_receipt<T: serde::de::DeserializeOwned>(
    path: &std::path::Path,
) -> color_eyre::eyre::Result<T> {
    let raw = std::fs::read_to_string(path)?;
    Ok(serde_json::from_str(&raw)?)
}
