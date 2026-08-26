//! Fixtures and falsifiers for the exact composed harness subject (#12158).
//!
//! Positive fixtures prove a subject-bound observation round-trips with its
//! identity intact. Each numbered falsifier below is the discriminating test
//! for one law of the issue: a mismatched, rebound, or substituted subject
//! refuses typed, instrumented runs cannot masquerade as ordinary ones,
//! legacy rows stay historical, and canonical fingerprints stay deterministic
//! and checkout-root independent.

use crate::harness_subject::build::{
    SubjectComposition, bind_discovery_subject, bind_invocation_trace_subject,
    compose_harness_subject, legacy_subject_refusal, verify_subject_binding,
};
use crate::harness_subject::model::{
    CompilerHarnessSubjectV1, HARNESS_SUBJECT_SCHEMA_VERSION, SubjectBindingRefusal,
    SubjectComponent, SubjectEvidence,
};
use crate::invocation_trace::build::build_invocation_trace_receipt;
use crate::invocation_trace::test_support::{TraceFixture, matrix, sha_hex};
use crate::observed_discovery::build::build_observed_discovery_receipt;
use crate::observed_discovery::model::{
    DiscoverySubjectIdentity, EnvironmentIdentity, EvidenceClass, ObservedDiscoveryInput,
    ProcessCompletion, RunnerArtifactIdentity, UPSTREAM_DISCOVERY_SCHEMA_VERSION,
    UpstreamDiscoveryReceiptV1,
};
use crate::runner_model::{DiscoveryFrame, RunnerKind};
use color_eyre::eyre::{Result, bail, eyre};
use serde_json::json;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn repo_file(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").join(relative)
}

fn ensure_subject<T>(outcome: std::result::Result<T, SubjectBindingRefusal>) -> Result<T> {
    outcome.map_err(|refusal| eyre!("{refusal}"))
}

fn ordinary_input(target_id: &str, stdout: &[u8], nonce: &str) -> Result<ObservedDiscoveryInput> {
    let matrix = matrix()?;
    let entry = matrix
        .targets
        .iter()
        .find(|entry| entry.contract.target_id == target_id)
        .ok_or_else(|| eyre!("matrix has no target {target_id}"))?;
    let contract_digest =
        sha_hex(&serde_json::to_vec(&entry.contract).map_err(|error| eyre!(error))?);
    Ok(ObservedDiscoveryInput {
        subject: DiscoverySubjectIdentity {
            repository_commit: "a".repeat(40),
            perl_ref: "perl-5.42.2".to_string(),
            prepared_tree_identity: "prepared-tree-generation-1".to_string(),
            host_perl_identity: "host-perl-5.42.2".to_string(),
            matrix_fingerprint: matrix.fingerprint().map_err(|error| eyre!(error))?,
            target_id: target_id.to_string(),
            target_contract_digest: contract_digest,
            variant_target_id: None,
            instrumentation_id: None,
        },
        runner: RunnerKind::Test,
        runner_artifact: RunnerArtifactIdentity {
            canonical_path: "t/TEST".to_string(),
            content_sha256: sha_hex(b"t/TEST"),
        },
        argv: vec!["./perl".to_string(), "../t/TEST".to_string(), "--dumptests".to_string()],
        working_directory: "t".to_string(),
        environment: BTreeMap::from([("LC_ALL".to_string(), "C".to_string())]),
        discovery_frame: DiscoveryFrame::CanonicalRepositoryPath,
        completion: ProcessCompletion::ExitStatus { code: 0 },
        process_nonce: nonce.to_string(),
        stdout_bytes: stdout.to_vec(),
        stdout_truncated: false,
        stderr_bytes: Vec::new(),
        stderr_truncated: false,
    })
}

fn ordinary_receipt(target_id: &str, nonce: &str) -> Result<UpstreamDiscoveryReceiptV1> {
    let input = ordinary_input(target_id, b"t/base/if.t\n", nonce)?;
    let matrix = matrix()?;
    build_observed_discovery_receipt(&matrix, &input).map_err(|error| eyre!(error))
}

fn trace_pair() -> Result<(
    UpstreamDiscoveryReceiptV1,
    crate::invocation_trace::model::EffectiveInvocationTraceReceiptV1,
)> {
    let fixture = TraceFixture::new("component_base", "t/base/if.t\n")?;
    let bytes = fixture.emit_complete(&["t/base/if.t"])?;
    let receipt =
        build_invocation_trace_receipt(&fixture.input(bytes)).map_err(|error| eyre!(error))?;
    Ok((fixture.parent.clone(), receipt))
}

// ---------------------------------------------------------------------------
// Positive: subject-bound observations round-trip with identity intact
// ---------------------------------------------------------------------------

#[test]
fn discovery_receipt_binds_and_round_trips_its_exact_subject() -> Result<()> {
    let receipt = ordinary_receipt("component_base", "capture-0001")?;
    let subject = ensure_subject(bind_discovery_subject(&receipt))?;
    assert_eq!(subject.schema_version, HARNESS_SUBJECT_SCHEMA_VERSION);
    assert_eq!(subject.process_nonce, "capture-0001");
    assert_eq!(subject.instrumentation_id, None);

    // Serialization keeps the identity intact and the subject stays coherent.
    let encoded = serde_json::to_string(&subject)?;
    let decoded: CompilerHarnessSubjectV1 = serde_json::from_str(&encoded)?;
    if decoded != subject {
        bail!("exact subject did not survive its serialization round-trip");
    }
    decoded
        .refusal_if_incoherent()
        .map_err(|refusal| eyre!("round-tripped subject refused: {refusal}"))?;

    // Binding the same receipt again reproduces the identical subject.
    let rebound = ensure_subject(bind_discovery_subject(&receipt))?;
    if rebound != subject || rebound.fingerprint != subject.fingerprint {
        bail!("binding the same receipt twice produced different exact subjects");
    }

    // The derived fingerprint is deterministic over the same identity.
    let recomputed = subject.derive_fingerprint().map_err(|error| eyre!(error))?;
    if recomputed != subject.fingerprint {
        bail!("fingerprint is not deterministic over the same identity");
    }

    // Verification against its own evidence accepts.
    ensure_subject(verify_subject_binding(&subject, SubjectEvidence::Discovery(&receipt)))
        .map_err(|error| eyre!("exact subject refused its own evidence: {error}"))?;
    Ok(())
}

#[test]
fn trace_receipt_binds_its_parent_and_keeps_instrumentation_load_bearing() -> Result<()> {
    let (parent, trace) = trace_pair()?;
    let subject = ensure_subject(bind_invocation_trace_subject(&parent, &trace))?;
    assert_eq!(subject.instrumentation_id, Some("trace-instrument-1".to_string()));
    assert_eq!(subject.process_nonce, parent.payload.terminal.process_nonce);
    assert_eq!(
        subject.runner_artifact.content_sha256,
        parent.payload.invocation.runner_artifact.content_sha256
    );

    let encoded = serde_json::to_string(&subject)?;
    let decoded: CompilerHarnessSubjectV1 = serde_json::from_str(&encoded)?;
    if decoded != subject {
        bail!("instrumented exact subject did not survive round-trip");
    }
    decoded
        .refusal_if_incoherent()
        .map_err(|refusal| eyre!("round-tripped subject refused: {refusal}"))?;

    ensure_subject(verify_subject_binding(
        &subject,
        SubjectEvidence::InvocationTrace { parent: &parent, trace: &trace },
    ))
    .map_err(|error| eyre!("instrumented subject refused its own evidence: {error}"))?;

    // Masquerade law: the instrumentation identity is load-bearing inside the
    // fingerprint — an instrumented subject can never equal the ordinary
    // subject of the same underlying identity.
    let ordinary = ordinary_receipt("component_base", "capture-0001")?;
    let ordinary_subject = ensure_subject(bind_discovery_subject(&ordinary))?;
    let mut retooled = ordinary_subject.clone();
    retooled.instrumentation_id = Some("trace-instrument-1".to_string());
    retooled.fingerprint = retooled.derive_fingerprint().map_err(|error| eyre!(error))?;
    if retooled == ordinary_subject {
        bail!("instrumentation identity is not load-bearing in subject identity");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Falsifiers: rebound, mismatched, and substituted subjects refuse typed
// ---------------------------------------------------------------------------

#[test]
fn substituted_process_result_refuses_typed_on_process_nonce() -> Result<()> {
    let receipt = ordinary_receipt("component_base", "capture-0001")?;
    let subject = ensure_subject(bind_discovery_subject(&receipt))?;
    let substituted = ordinary_receipt("component_base", "capture-0002")?;
    let Err(refusal) = verify_subject_binding(&subject, SubjectEvidence::Discovery(&substituted))
    else {
        bail!("a receipt from another capture identity must refuse")
    };
    match refusal {
        SubjectBindingRefusal::ComponentMismatch {
            component: SubjectComponent::ProcessNonce,
            ..
        } => {}
        other => bail!("expected typed ProcessNonce mismatch, got {other:?}"),
    }
    Ok(())
}

#[test]
fn stale_or_mutated_runner_binary_refuses_typed_on_artifact_digest() -> Result<()> {
    let receipt = ordinary_receipt("component_base", "capture-0001")?;
    let subject = ensure_subject(bind_discovery_subject(&receipt))?;
    // Same path, changed bytes: the runner binary was replaced or mutated
    // behind the expected path spelling.
    let mut stale = subject.clone();
    stale.runner_artifact.content_sha256 = sha_hex(b"t/TEST-with-drifted-bytes");
    stale.fingerprint = stale.derive_fingerprint().map_err(|error| eyre!(error))?;
    let Err(refusal) = verify_subject_binding(&stale, SubjectEvidence::Discovery(&receipt)) else {
        bail!("changed runner bytes must refuse");
    };
    match refusal {
        SubjectBindingRefusal::ComponentMismatch {
            component: SubjectComponent::RunnerArtifact,
            ..
        } => {}
        other => bail!("expected typed RunnerArtifact mismatch, got {other:?}"),
    }
    Ok(())
}

#[test]
fn cross_preparation_and_environment_substitution_refuse_typed() -> Result<()> {
    let receipt = ordinary_receipt("component_base", "capture-0001")?;
    let subject = ensure_subject(bind_discovery_subject(&receipt))?;

    // Another preparation behind the same repository commit.
    let mut other_preparation = subject.clone();
    other_preparation.prepared_tree_identity = "prepared-tree-generation-2".to_string();
    other_preparation.fingerprint =
        other_preparation.derive_fingerprint().map_err(|error| eyre!(error))?;
    let Err(refusal) =
        verify_subject_binding(&other_preparation, SubjectEvidence::Discovery(&receipt))
    else {
        bail!("another preparation must refuse")
    };
    match refusal {
        SubjectBindingRefusal::ComponentMismatch {
            component: SubjectComponent::PreparedTree,
            ..
        } => {}
        other => bail!("expected typed PreparedTree mismatch, got {other:?}"),
    }

    // Same argv under another environment/capability subject.
    let mut other_environment = subject.clone();
    other_environment.environment = EnvironmentIdentity {
        variables: BTreeMap::from([
            ("LC_ALL".to_string(), "C".to_string()),
            ("PERL_TEST_MEMORY".to_string(), "1073741824".to_string()),
        ]),
        sha256: sha_hex(b"LC_ALL=C\nPERL_TEST_MEMORY=1073741824\n"),
    };
    other_environment.fingerprint =
        other_environment.derive_fingerprint().map_err(|error| eyre!(error))?;
    let Err(refusal) =
        verify_subject_binding(&other_environment, SubjectEvidence::Discovery(&receipt))
    else {
        bail!("another environment behind the same argv must refuse")
    };
    match refusal {
        SubjectBindingRefusal::ComponentMismatch {
            component: SubjectComponent::Environment,
            ..
        } => {}
        other => bail!("expected typed Environment mismatch, got {other:?}"),
    }
    Ok(())
}

#[test]
fn rebound_subject_fingerprint_refuses_typed() -> Result<()> {
    let receipt = ordinary_receipt("component_base", "capture-0001")?;
    let mut rebound = ensure_subject(bind_discovery_subject(&receipt))?;
    // A forged fingerprint on otherwise-identical identity: the coherence law
    // itself must refuse before any verification runs.
    rebound.fingerprint = sha_hex(b"a-forged-fingerprint");
    let Err(refusal) = rebound.refusal_if_incoherent() else {
        bail!("forged fingerprint must refuse");
    };
    match refusal {
        SubjectBindingRefusal::FingerprintMismatch { .. } => {}
        other => bail!("expected FingerprintMismatch, got {other:?}"),
    }
    Ok(())
}

#[test]
fn cross_subject_trace_binding_refuses_typed_per_component() -> Result<()> {
    let (parent, trace) = trace_pair()?;
    let subject = ensure_subject(bind_invocation_trace_subject(&parent, &trace))?;

    // The trace presented against a discovery receipt from another subject
    // (another repository, another capture) refuses: the shared validation
    // cannot establish the parent relation.
    let mut foreign_input = ordinary_input("component_base", b"t/base/if.t\n", "capture-0002")?;
    foreign_input.subject.repository_commit = "b".repeat(40);
    let foreign_matrix = matrix()?;
    let foreign_receipt = build_observed_discovery_receipt(&foreign_matrix, &foreign_input)
        .map_err(|error| eyre!(error))?;
    let Err(refusal) = verify_subject_binding(
        &subject,
        SubjectEvidence::InvocationTrace { parent: &foreign_receipt, trace: &trace },
    ) else {
        bail!("a trace bound to another parent must refuse");
    };
    match refusal {
        SubjectBindingRefusal::IncoherentEvidence { reason }
            if reason.contains("parent receipt") || reason.contains("process") => {}
        other => bail!("expected incoherent cross-subject parent, got {other:?}"),
    }

    // Component-typed disagreement: a subject whose repository component
    // differs refuses on exactly that component.
    let mut other_repo = subject.clone();
    other_repo.repository_commit = "b".repeat(40);
    other_repo.fingerprint = other_repo.derive_fingerprint().map_err(|error| eyre!(error))?;
    let Err(refusal) = verify_subject_binding(
        &other_repo,
        SubjectEvidence::InvocationTrace { parent: &parent, trace: &trace },
    ) else {
        bail!("a subject from another repository must refuse");
    };
    match refusal {
        SubjectBindingRefusal::ComponentMismatch {
            component: SubjectComponent::RepositoryCommit,
            ..
        } => {}
        other => bail!("expected typed RepositoryCommit mismatch, got {other:?}"),
    }
    Ok(())
}

#[test]
fn discovery_and_invocation_from_different_runner_artifacts_refuse() -> Result<()> {
    let fixture = TraceFixture::new("component_base", "t/base/if.t\n")?;
    let bytes = fixture.emit_complete(&["t/base/if.t"])?;
    let mut input = fixture.input(bytes);
    // Same path and route, different measured bytes: a stale binary behind
    // the expected path cannot lend the parent's identity.
    input.runner_artifact.content_sha256 = sha_hex(b"t/TEST-stale-bytes");
    let Err(error) = build_invocation_trace_receipt(&input) else {
        bail!("trace construction must refuse a different runner artifact digest");
    };
    if !error.contains("does not match the parent discovery artifact digest") {
        bail!("unexpected artifact refusal: {error}");
    }
    Ok(())
}

#[test]
fn instrumented_trace_cannot_present_an_ordinary_subject() -> Result<()> {
    // Falsifier: an instrumented run relabelled ordinary. Both the missing
    // instrument identity and a disagreeing parent instrument refuse.
    let fixture = TraceFixture::new("component_base", "t/base/if.t\n")?;
    let bytes = fixture.emit_complete(&["t/base/if.t"])?;
    let mut input = fixture.input(bytes.clone());
    input.subject.instrumentation_id = None;
    let Err(missing) = build_invocation_trace_receipt(&input) else {
        bail!("instrumented trace without instrument identity must refuse");
    };
    if !missing.contains("no instrumentation id") {
        bail!("unexpected unidentified-instrument refusal: {missing}");
    }

    let mut input = fixture.input(bytes);
    input.subject.instrumentation_id = Some("another-instrument".to_string());
    let Err(disagreeing) = build_invocation_trace_receipt(&input) else {
        bail!("trace with a foreign instrument identity must refuse");
    };
    if !disagreeing.contains("does not match the parent discovery instrumentation") {
        bail!("unexpected instrument disagreement refusal: {disagreeing}");
    }

    // The typed binding law answers the same relabelling refusal as data: a
    // receipt whose payload no longer binds its digest refuses typed as
    // incoherent before composition can run.
    let (parent, trace) = trace_pair()?;
    let mut uninstrumented = trace.clone();
    uninstrumented.payload.subject.instrumentation_id = None;
    let Err(refusal) = bind_invocation_trace_subject(&parent, &uninstrumented) else {
        bail!("uninstrumented trace subject must refuse");
    };
    match refusal {
        SubjectBindingRefusal::IncoherentEvidence { .. } => {}
        other => bail!("expected incoherent evidence refusal, got {other:?}"),
    }
    Ok(())
}

#[test]
fn non_observed_evidence_class_never_presents_a_current_subject() -> Result<()> {
    let mut receipt = ordinary_receipt("component_base", "capture-0001")?;
    receipt.evidence_class = EvidenceClass::HistoricalUnbound;
    let Err(refusal) = bind_discovery_subject(&receipt) else {
        bail!("historical unbound rows can never present a current subject");
    };
    match refusal {
        SubjectBindingRefusal::EvidenceNotBindable { evidence_class }
            if evidence_class == "HistoricalUnbound" => {}
        other => bail!("expected typed EvidenceNotBindable, got {other:?}"),
    }
    Ok(())
}

#[test]
fn legacy_evidence_stays_historical_unbound() -> Result<()> {
    // The real historical envelope schema can never present a current
    // subject: the answer is the typed refusal carrying its schema identity.
    let refusal = legacy_subject_refusal(crate::artifacts::DISCOVERY_RAW_SCHEMA_VERSION);
    match refusal {
        SubjectBindingRefusal::HistoricalUnbound { ref schema_version }
            if schema_version == crate::artifacts::DISCOVERY_RAW_SCHEMA_VERSION => {}
        other => bail!("expected typed HistoricalUnbound, got {other:?}"),
    }

    // A subject composed from historical spelling can never be relabelled
    // current: coherence refuses any edited claim boundary.
    let receipt = ordinary_receipt("component_base", "capture-0001")?;
    let mut relabelled = ensure_subject(bind_discovery_subject(&receipt))?;
    relabelled.claim_boundary = "historical".to_string();
    let Err(refusal) = relabelled.refusal_if_incoherent() else {
        bail!("a relabelled claim boundary must refuse coherence");
    };
    match refusal {
        SubjectBindingRefusal::ComponentInvalid {
            component: SubjectComponent::ClaimBoundary,
            ..
        } => {}
        other => bail!("expected typed ClaimBoundary refusal, got {other:?}"),
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Falsifiers: composition refuses missing, invalid, and host-path components
// ---------------------------------------------------------------------------

fn base_composition() -> Result<SubjectComposition> {
    let matrix = matrix()?;
    Ok(SubjectComposition {
        repository_commit: "a".repeat(40),
        perl_ref: "perl-5.42.2".to_string(),
        prepared_tree_identity: "prepared-tree-generation-1".to_string(),
        host_perl_identity: "host-perl-5.42.2".to_string(),
        matrix_fingerprint: matrix.fingerprint().map_err(|error| eyre!(error))?,
        target_id: "component_base".to_string(),
        target_contract_digest: sha_hex(b"contract"),
        variant_target_id: None,
        instrumentation_id: None,
        runner: RunnerKind::Test,
        runner_artifact: RunnerArtifactIdentity {
            canonical_path: "t/TEST".to_string(),
            content_sha256: sha_hex(b"t/TEST"),
        },
        argv: vec!["./perl".to_string(), "../t/TEST".to_string(), "--dumptests".to_string()],
        working_directory: "t".to_string(),
        environment: BTreeMap::from([("LC_ALL".to_string(), "C".to_string())]),
        process_nonce: "capture-0001".to_string(),
    })
}

#[test]
fn missing_or_defaulted_component_refuses_typed() -> Result<()> {
    // One required component missing entirely: composition must refuse, not
    // default. An empty commit stands in for every missing component.
    let mut composition = base_composition()?;
    composition.repository_commit = String::new();
    let Err(refusal) = compose_harness_subject(composition) else {
        bail!("a missing repository component must refuse composition");
    };
    match refusal {
        SubjectBindingRefusal::ComponentInvalid {
            component: SubjectComponent::RepositoryCommit,
            ..
        } => {}
        other => bail!("expected typed RepositoryCommit refusal, got {other:?}"),
    }

    // A malformed digest shape on the runner artifact refuses on that
    // component: independently supplied digest strings never pass.
    let mut composition = base_composition()?;
    composition.runner_artifact.content_sha256 = "not-a-digest".to_string();
    let Err(refusal) = compose_harness_subject(composition) else {
        bail!("a malformed artifact digest must refuse composition");
    };
    match refusal {
        SubjectBindingRefusal::ComponentInvalid {
            component: SubjectComponent::RunnerArtifact,
            ..
        } => {}
        other => bail!("expected typed RunnerArtifact refusal, got {other:?}"),
    }

    // An unadmitted runner route refuses on the route component.
    let mut composition = base_composition()?;
    composition.runner = RunnerKind::DirectFallback;
    let Err(refusal) = compose_harness_subject(composition) else {
        bail!("an unadmitted runner route must refuse composition");
    };
    match refusal {
        SubjectBindingRefusal::ComponentInvalid {
            component: SubjectComponent::RunnerRoute,
            ..
        } => {}
        other => bail!("expected typed RunnerRoute refusal, got {other:?}"),
    }
    Ok(())
}

#[test]
fn host_paths_never_enter_subject_identity() -> Result<()> {
    // Host-path equality is not identity: absolute spellings refuse at the
    // component laws, so a checkout-root move cannot change identity and a
    // host path cannot impersonate one.
    let mut composition = base_composition()?;
    composition.working_directory = "F:\\prepared\\perl\\t".to_string();
    let Err(refusal) = compose_harness_subject(composition) else {
        bail!("an absolute working directory must refuse composition");
    };
    match refusal {
        SubjectBindingRefusal::ComponentInvalid {
            component: SubjectComponent::WorkingDirectory,
            ..
        } => {}
        other => bail!("expected typed WorkingDirectory refusal, got {other:?}"),
    }

    let mut composition = base_composition()?;
    composition.argv.push("C:\\host\\perl".to_string());
    let Err(refusal) = compose_harness_subject(composition) else {
        bail!("an absolute argv entry must refuse composition");
    };
    match refusal {
        SubjectBindingRefusal::ComponentInvalid {
            component: SubjectComponent::InvocationArgv,
            ..
        } => {}
        other => bail!("expected typed InvocationArgv refusal, got {other:?}"),
    }

    let mut composition = base_composition()?;
    composition.runner_artifact.canonical_path = "/prepared/perl/t/TEST".to_string();
    let Err(refusal) = compose_harness_subject(composition) else {
        bail!("an absolute artifact path must refuse composition");
    };
    match refusal {
        SubjectBindingRefusal::ComponentInvalid {
            component: SubjectComponent::RunnerArtifact,
            ..
        } => {}
        other => bail!("expected typed RunnerArtifact refusal, got {other:?}"),
    }

    // Checkout-root independence: identical relative identity composes to the
    // identical fingerprint no matter which host produced it.
    let first = ensure_subject(compose_harness_subject(base_composition()?))?;
    let second = ensure_subject(compose_harness_subject(base_composition()?))?;
    if first.fingerprint != second.fingerprint || first != second {
        bail!("identical components must compose the identical subject");
    }
    Ok(())
}

#[test]
fn unknown_schema_version_refuses_typed() -> Result<()> {
    let receipt = ordinary_receipt("component_base", "capture-0001")?;
    let mut subject = ensure_subject(bind_discovery_subject(&receipt))?;
    subject.schema_version = "perl_core_harness.compiler_harness_subject.v2".to_string();
    subject.fingerprint = subject.derive_fingerprint().map_err(|error| eyre!(error))?;
    let Err(refusal) = subject.refusal_if_incoherent() else {
        bail!("unknown schema must refuse");
    };
    match refusal {
        SubjectBindingRefusal::UnsupportedSchema { found, .. }
            if found == "perl_core_harness.compiler_harness_subject.v2" => {}
        other => bail!("expected typed UnsupportedSchema, got {other:?}"),
    }

    // A discovery receipt carrying a foreign schema refuses before binding.
    let mut drifted = ordinary_receipt("component_base", "capture-0001")?;
    drifted.schema_version = format!("{UPSTREAM_DISCOVERY_SCHEMA_VERSION}.drifted");
    let Err(refusal) = bind_discovery_subject(&drifted) else {
        bail!("a drifted receipt schema must refuse binding");
    };
    match refusal {
        SubjectBindingRefusal::IncoherentEvidence { .. } => {}
        other => bail!("expected incoherent evidence refusal, got {other:?}"),
    }
    Ok(())
}

#[test]
fn produced_subject_matches_registered_json_schema() -> Result<()> {
    let receipt = ordinary_receipt("component_base", "capture-0001")?;
    let subject = ensure_subject(bind_discovery_subject(&receipt))?;
    let schema_path =
        repo_file("schemas/perl_core_harness_compiler_harness_subject.v1.schema.json");
    let schema: serde_json::Value = serde_json::from_slice(&std::fs::read(schema_path)?)?;

    let serialized = serde_json::to_value(&subject)?;
    schema_check::validate(&schema, &serialized)
        .map_err(|error| eyre!("produced subject violates registered schema: {error}"))?;

    let round_tripped: CompilerHarnessSubjectV1 = serde_json::from_value(serialized.clone())?;
    let reserialized = serde_json::to_value(&round_tripped)?;
    schema_check::validate(&schema, &reserialized)
        .map_err(|error| eyre!("round-tripped subject violates registered schema: {error}"))?;

    // Discriminators: the drifted schema identity, a non-hex fingerprint, and
    // a short commit must be rejected by the registered schema itself.
    for (pointer, replacement) in [
        ("/schema_version", json!("perl_core_harness.compiler_harness_subject.v2")),
        ("/fingerprint", json!("not-a-digest")),
        ("/repository_commit", json!("short")),
    ] {
        let mut mutated = serialized.clone();
        let cursor =
            mutated.pointer_mut(pointer).ok_or_else(|| eyre!("missing JSON pointer {pointer}"))?;
        *cursor = replacement;
        assert!(
            schema_check::validate(&schema, &mutated).is_err(),
            "registered schema must reject drifted shape at {pointer}"
        );
    }
    Ok(())
}

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
        if !bytes.len().is_multiple_of(unit_width) {
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
