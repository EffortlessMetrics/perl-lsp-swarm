//! Checked composition and the exact-subject binding law (#12158).
//!
//! Composition is private to the checked constructor: no public constructor
//! accepts a fingerprint, and every component passes the same relation laws
//! the observation contracts enforce. Binding re-derives the exact subject
//! from observed evidence, refuses typed when a component disagrees across a
//! binding, and refuses typed when a presented subject does not reproduce
//! from the evidence it claims. The module never executes processes, reads
//! the filesystem, or repairs missing components.

use crate::harness_subject::model::{
    CompilerHarnessSubjectV1, HARNESS_SUBJECT_CLAIM_BOUNDARY, HARNESS_SUBJECT_SCHEMA_VERSION,
    SubjectBindingRefusal, SubjectComponent, SubjectEvidence,
};
use crate::observed_discovery::build::{
    environment_identity, validate_argument, validate_artifact_path, validate_capture_identity,
    validate_reference, validate_sha256_field, validate_target_id, validate_working_directory,
};
use crate::observed_discovery::model::{
    EnvironmentIdentity, EvidenceClass, RunnerArtifactIdentity, UpstreamDiscoveryReceiptV1,
};
use crate::observed_discovery::validate::validate_receipt_subject_binding;
use crate::runner_model::RunnerKind;
use std::collections::BTreeMap;

/// Checked component input for one exact harness subject. The environment is
/// supplied as its behavior-bearing variables and the artifact as the measured
/// identity; digests are derived or validated, never independently asserted.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SubjectComposition {
    /// Measuring repository commit (lower-case hex).
    pub repository_commit: String,
    /// Resolved upstream Perl source reference.
    pub perl_ref: String,
    /// Prepared-tree identity reference.
    pub prepared_tree_identity: String,
    /// Host Perl interpreter identity reference.
    pub host_perl_identity: String,
    /// Pinned target matrix fingerprint.
    pub matrix_fingerprint: String,
    /// Target contract identity.
    pub target_id: String,
    /// SHA-256 of the pinned target selection contract.
    pub target_contract_digest: String,
    /// Environment-variant target identity when the subject ran a variant.
    pub variant_target_id: Option<String>,
    /// Instrumentation subject identity when the subject ran instrumented.
    pub instrumentation_id: Option<String>,
    /// Admitted upstream runner route.
    pub runner: RunnerKind,
    /// Measured runner artifact identity.
    pub runner_artifact: RunnerArtifactIdentity,
    /// Checkout-root-relative argv.
    pub argv: Vec<String>,
    /// Checkout-root-relative working directory.
    pub working_directory: String,
    /// Behavior-bearing environment variables.
    pub environment: BTreeMap<String, String>,
    /// Capture identity of the bound process.
    pub process_nonce: String,
}

/// Compose one exact harness subject from checked components. The
/// fingerprint is derived; there is no input that supplies it. One missing or
/// invalid component refuses typed instead of defaulting.
pub fn compose_harness_subject(
    composition: SubjectComposition,
) -> Result<CompilerHarnessSubjectV1, SubjectBindingRefusal> {
    // The composition input carries raw variables; the environment digest is
    // derived here and the relation laws bind it inside this call.
    let environment = pending_environment(&composition.environment)?;
    validate_component_relations(&SubjectRelations {
        repository_commit: &composition.repository_commit,
        perl_ref: &composition.perl_ref,
        prepared_tree_identity: &composition.prepared_tree_identity,
        host_perl_identity: &composition.host_perl_identity,
        matrix_fingerprint: &composition.matrix_fingerprint,
        target_id: &composition.target_id,
        target_contract_digest: &composition.target_contract_digest,
        variant_target_id: composition.variant_target_id.as_deref(),
        instrumentation_id: composition.instrumentation_id.as_deref(),
        runner: composition.runner,
        runner_artifact: &composition.runner_artifact,
        argv: &composition.argv,
        working_directory: &composition.working_directory,
        environment: &environment,
        process_nonce: &composition.process_nonce,
    })?;
    let mut subject = CompilerHarnessSubjectV1 {
        schema_version: HARNESS_SUBJECT_SCHEMA_VERSION.to_string(),
        repository_commit: composition.repository_commit,
        perl_ref: composition.perl_ref,
        prepared_tree_identity: composition.prepared_tree_identity,
        host_perl_identity: composition.host_perl_identity,
        matrix_fingerprint: composition.matrix_fingerprint,
        target_id: composition.target_id,
        target_contract_digest: composition.target_contract_digest,
        variant_target_id: composition.variant_target_id,
        instrumentation_id: composition.instrumentation_id,
        runner: composition.runner,
        runner_artifact: composition.runner_artifact,
        argv: composition.argv,
        working_directory: composition.working_directory,
        environment,
        process_nonce: composition.process_nonce,
        claim_boundary: HARNESS_SUBJECT_CLAIM_BOUNDARY.to_string(),
        fingerprint: String::new(),
    };
    subject.fingerprint =
        subject.derive_fingerprint().map_err(|reason| SubjectBindingRefusal::ComponentInvalid {
            component: SubjectComponent::SchemaVersion,
            reason,
        })?;
    Ok(subject)
}

/// Bind the exact subject of one observed upstream-discovery receipt. The
/// receipt must first pass its own strict subject-binding validation, and only
/// the `observed_upstream` evidence class can present a current subject.
pub fn bind_discovery_subject(
    receipt: &UpstreamDiscoveryReceiptV1,
) -> Result<CompilerHarnessSubjectV1, SubjectBindingRefusal> {
    if receipt.evidence_class != EvidenceClass::ObservedUpstream {
        return Err(SubjectBindingRefusal::EvidenceNotBindable {
            evidence_class: format!("{:?}", receipt.evidence_class),
        });
    }
    validate_receipt_subject_binding(receipt)
        .map_err(|reason| SubjectBindingRefusal::IncoherentEvidence { reason })?;
    let payload = &receipt.payload;
    compose_harness_subject(SubjectComposition {
        repository_commit: payload.subject.repository_commit.clone(),
        perl_ref: payload.subject.perl_ref.clone(),
        prepared_tree_identity: payload.subject.prepared_tree_identity.clone(),
        host_perl_identity: payload.subject.host_perl_identity.clone(),
        matrix_fingerprint: payload.subject.matrix_fingerprint.clone(),
        target_id: payload.subject.target_id.clone(),
        target_contract_digest: payload.subject.target_contract_digest.clone(),
        variant_target_id: payload.subject.variant_target_id.clone(),
        instrumentation_id: payload.subject.instrumentation_id.clone(),
        runner: payload.invocation.runner,
        runner_artifact: payload.invocation.runner_artifact.clone(),
        argv: payload.invocation.argv.clone(),
        working_directory: payload.invocation.working_directory.clone(),
        environment: payload.invocation.environment.variables.clone(),
        process_nonce: payload.terminal.process_nonce.clone(),
    })
}

/// Bind the exact subject of one instrumented effective-invocation trace and
/// its exact parent discovery receipt. The trace must pass full validation
/// against the supplied parent, every shared component must agree, the trace
/// must carry its instrumentation identity, and both artifacts must name the
/// same runner artifact bytes and process capture. The composed subject
/// carries the instrumentation identity, so it can never equal an ordinary
/// subject.
pub fn bind_invocation_trace_subject(
    parent: &UpstreamDiscoveryReceiptV1,
    trace: &crate::invocation_trace::model::EffectiveInvocationTraceReceiptV1,
) -> Result<CompilerHarnessSubjectV1, SubjectBindingRefusal> {
    if trace.evidence_class != EvidenceClass::InstrumentedUpstream {
        return Err(SubjectBindingRefusal::EvidenceNotBindable {
            evidence_class: format!("{:?}", trace.evidence_class),
        });
    }
    crate::invocation_trace::validate::validate_invocation_trace_receipt(parent, trace)
        .map_err(|reason| SubjectBindingRefusal::IncoherentEvidence { reason })?;

    let subject = &trace.payload.subject;
    let parent_subject = &parent.payload.subject;
    // Typed per-component agreement on top of the shared string validation:
    // consumers discriminate on the component, never on message text.
    for (component, trace_value, parent_value) in [
        (
            SubjectComponent::RepositoryCommit,
            subject.repository_commit.as_str(),
            parent_subject.repository_commit.as_str(),
        ),
        (SubjectComponent::PerlRef, subject.perl_ref.as_str(), parent_subject.perl_ref.as_str()),
        (
            SubjectComponent::PreparedTree,
            subject.prepared_tree_identity.as_str(),
            parent_subject.prepared_tree_identity.as_str(),
        ),
        (
            SubjectComponent::HostPerl,
            subject.host_perl_identity.as_str(),
            parent_subject.host_perl_identity.as_str(),
        ),
        (
            SubjectComponent::MatrixFingerprint,
            subject.matrix_fingerprint.as_str(),
            parent_subject.matrix_fingerprint.as_str(),
        ),
        (SubjectComponent::Target, subject.target_id.as_str(), parent_subject.target_id.as_str()),
        (
            SubjectComponent::TargetContract,
            subject.target_contract_digest.as_str(),
            parent_subject.target_contract_digest.as_str(),
        ),
        (
            SubjectComponent::VariantTarget,
            subject.variant_target_id.as_deref().unwrap_or("none"),
            parent_subject.variant_target_id.as_deref().unwrap_or("none"),
        ),
        (
            SubjectComponent::Instrumentation,
            subject.instrumentation_id.as_deref().unwrap_or("none"),
            parent_subject.instrumentation_id.as_deref().unwrap_or("none"),
        ),
        (
            SubjectComponent::ProcessNonce,
            subject.parent_process_nonce.as_str(),
            parent.payload.terminal.process_nonce.as_str(),
        ),
    ] {
        if trace_value != parent_value {
            return Err(SubjectBindingRefusal::ComponentMismatch {
                component,
                subject_value: trace_value.to_string(),
                evidence_value: parent_value.to_string(),
            });
        }
    }
    if trace.payload.runner != parent.payload.invocation.runner {
        return Err(SubjectBindingRefusal::ComponentMismatch {
            component: SubjectComponent::RunnerRoute,
            subject_value: format!("{:?}", trace.payload.runner),
            evidence_value: format!("{:?}", parent.payload.invocation.runner),
        });
    }
    if trace.payload.runner_artifact.content_sha256
        != parent.payload.invocation.runner_artifact.content_sha256
    {
        return Err(SubjectBindingRefusal::ComponentMismatch {
            component: SubjectComponent::RunnerArtifact,
            subject_value: trace.payload.runner_artifact.content_sha256.clone(),
            evidence_value: parent.payload.invocation.runner_artifact.content_sha256.clone(),
        });
    }
    let instrumentation_id = subject
        .instrumentation_id
        .clone()
        .ok_or(SubjectBindingRefusal::InstrumentationUnidentified)?;

    let payload = &parent.payload;
    compose_harness_subject(SubjectComposition {
        repository_commit: payload.subject.repository_commit.clone(),
        perl_ref: payload.subject.perl_ref.clone(),
        prepared_tree_identity: payload.subject.prepared_tree_identity.clone(),
        host_perl_identity: payload.subject.host_perl_identity.clone(),
        matrix_fingerprint: payload.subject.matrix_fingerprint.clone(),
        target_id: payload.subject.target_id.clone(),
        target_contract_digest: payload.subject.target_contract_digest.clone(),
        variant_target_id: payload.subject.variant_target_id.clone(),
        instrumentation_id: Some(instrumentation_id),
        runner: payload.invocation.runner,
        runner_artifact: payload.invocation.runner_artifact.clone(),
        argv: payload.invocation.argv.clone(),
        working_directory: payload.invocation.working_directory.clone(),
        environment: payload.invocation.environment.variables.clone(),
        process_nonce: payload.terminal.process_nonce.clone(),
    })
}

/// Verify a presented exact subject against the evidence it claims. The
/// evidence's exact subject is re-derived from scratch and compared
/// component-by-component; a rebound or substituted subject refuses typed on
/// the first disagreeing component, and an otherwise-identical subject whose
/// fingerprint does not reproduce refuses as a fingerprint mismatch.
pub fn verify_subject_binding(
    expected: &CompilerHarnessSubjectV1,
    evidence: SubjectEvidence<'_>,
) -> Result<(), SubjectBindingRefusal> {
    let bound = match evidence {
        SubjectEvidence::Discovery(receipt) => bind_discovery_subject(receipt)?,
        SubjectEvidence::InvocationTrace { parent, trace } => {
            bind_invocation_trace_subject(parent, trace)?
        }
    };
    for (component, subject_value, evidence_value) in [
        (
            SubjectComponent::SchemaVersion,
            expected.schema_version.clone(),
            bound.schema_version.clone(),
        ),
        (
            SubjectComponent::RepositoryCommit,
            expected.repository_commit.clone(),
            bound.repository_commit.clone(),
        ),
        (SubjectComponent::PerlRef, expected.perl_ref.clone(), bound.perl_ref.clone()),
        (
            SubjectComponent::PreparedTree,
            expected.prepared_tree_identity.clone(),
            bound.prepared_tree_identity.clone(),
        ),
        (
            SubjectComponent::HostPerl,
            expected.host_perl_identity.clone(),
            bound.host_perl_identity.clone(),
        ),
        (
            SubjectComponent::MatrixFingerprint,
            expected.matrix_fingerprint.clone(),
            bound.matrix_fingerprint.clone(),
        ),
        (SubjectComponent::Target, expected.target_id.clone(), bound.target_id.clone()),
        (
            SubjectComponent::TargetContract,
            expected.target_contract_digest.clone(),
            bound.target_contract_digest.clone(),
        ),
        (
            SubjectComponent::VariantTarget,
            expected.variant_target_id.clone().unwrap_or_else(|| "none".to_string()),
            bound.variant_target_id.clone().unwrap_or_else(|| "none".to_string()),
        ),
        (
            SubjectComponent::Instrumentation,
            expected.instrumentation_id.clone().unwrap_or_else(|| "none".to_string()),
            bound.instrumentation_id.clone().unwrap_or_else(|| "none".to_string()),
        ),
        (
            SubjectComponent::RunnerRoute,
            format!("{:?}", expected.runner),
            format!("{:?}", bound.runner),
        ),
        (
            SubjectComponent::RunnerArtifact,
            expected.runner_artifact.content_sha256.clone(),
            bound.runner_artifact.content_sha256.clone(),
        ),
        (SubjectComponent::InvocationArgv, expected.argv.join("\u{1f}"), bound.argv.join("\u{1f}")),
        (
            SubjectComponent::WorkingDirectory,
            expected.working_directory.clone(),
            bound.working_directory.clone(),
        ),
        (
            SubjectComponent::Environment,
            expected.environment.sha256.clone(),
            bound.environment.sha256.clone(),
        ),
        (
            SubjectComponent::ProcessNonce,
            expected.process_nonce.clone(),
            bound.process_nonce.clone(),
        ),
    ] {
        if subject_value != evidence_value {
            return Err(SubjectBindingRefusal::ComponentMismatch {
                component,
                subject_value,
                evidence_value,
            });
        }
    }
    if expected.fingerprint != bound.fingerprint {
        return Err(SubjectBindingRefusal::FingerprintMismatch {
            expected: expected.fingerprint.clone(),
            found: bound.fingerprint.clone(),
        });
    }
    Ok(())
}

/// The typed answer for evidence recorded before exact subjects existed
/// (for example the `discovery_raw.v2` envelopes and historical v1 bundles).
/// Historical evidence stays readable, but it can never present a current
/// exact subject: the answer is always the typed `historical_unbound`
/// refusal carrying the historical schema identity, never a relabel, a
/// partial merge, or a defaulted subject.
pub fn legacy_subject_refusal(schema_version: &str) -> SubjectBindingRefusal {
    SubjectBindingRefusal::HistoricalUnbound { schema_version: schema_version.to_string() }
}

/// Derive the environment identity for a composition input, refusing typed on
/// the environment relation law before the shared component validation runs.
fn pending_environment(
    variables: &BTreeMap<String, String>,
) -> Result<EnvironmentIdentity, SubjectBindingRefusal> {
    environment_identity(variables).map_err(|reason| SubjectBindingRefusal::ComponentInvalid {
        component: SubjectComponent::Environment,
        reason,
    })
}

/// Borrowed component view shared by composition and deserialized-subject
/// coherence, so both entry points enforce exactly one relation law per
/// component.
pub(crate) struct SubjectRelations<'a> {
    /// Measuring repository commit.
    pub repository_commit: &'a str,
    /// Resolved upstream Perl source reference.
    pub perl_ref: &'a str,
    /// Prepared-tree identity reference.
    pub prepared_tree_identity: &'a str,
    /// Host Perl interpreter identity reference.
    pub host_perl_identity: &'a str,
    /// Pinned target matrix fingerprint.
    pub matrix_fingerprint: &'a str,
    /// Target contract identity.
    pub target_id: &'a str,
    /// Target selection contract digest.
    pub target_contract_digest: &'a str,
    /// Environment-variant target identity when present.
    pub variant_target_id: Option<&'a str>,
    /// Instrumentation subject identity when present.
    pub instrumentation_id: Option<&'a str>,
    /// Runner route identity.
    pub runner: RunnerKind,
    /// Runner artifact identity.
    pub runner_artifact: &'a RunnerArtifactIdentity,
    /// Invocation argv identity.
    pub argv: &'a [String],
    /// Invocation working-directory identity.
    pub working_directory: &'a str,
    /// Behavior-bearing environment identity with digest.
    pub environment: &'a EnvironmentIdentity,
    /// Process capture identity.
    pub process_nonce: &'a str,
}

/// Shared component relation laws for composition and deserialized-subject
/// coherence. Every component reuses the exact law the observation contracts
/// enforce; one law, one vocabulary.
pub(crate) fn validate_component_relations(
    relations: &SubjectRelations<'_>,
) -> Result<(), SubjectBindingRefusal> {
    let SubjectRelations {
        repository_commit,
        perl_ref,
        prepared_tree_identity,
        host_perl_identity,
        matrix_fingerprint,
        target_id,
        target_contract_digest,
        variant_target_id,
        instrumentation_id,
        runner,
        runner_artifact,
        argv,
        working_directory,
        environment,
        process_nonce,
    } = relations;
    let checked = [
        (
            SubjectComponent::RepositoryCommit,
            validate_reference(repository_commit, "repository commit", 40, 64, true),
        ),
        (SubjectComponent::PerlRef, validate_reference(perl_ref, "perl ref", 1, 128, false)),
        (
            SubjectComponent::PreparedTree,
            validate_reference(prepared_tree_identity, "prepared tree identity", 1, 128, false),
        ),
        (
            SubjectComponent::HostPerl,
            validate_reference(host_perl_identity, "host perl identity", 1, 128, false),
        ),
        (
            SubjectComponent::MatrixFingerprint,
            validate_sha256_field(matrix_fingerprint, "matrix fingerprint"),
        ),
        (SubjectComponent::Target, validate_target_id(target_id)),
        (
            SubjectComponent::TargetContract,
            validate_sha256_field(target_contract_digest, "target contract digest"),
        ),
        (SubjectComponent::RunnerArtifact, validate_artifact_path(&runner_artifact.canonical_path)),
        (
            SubjectComponent::RunnerArtifact,
            validate_sha256_field(&runner_artifact.content_sha256, "runner artifact digest"),
        ),
        (SubjectComponent::ProcessNonce, validate_capture_identity(process_nonce)),
        (SubjectComponent::WorkingDirectory, validate_working_directory(working_directory)),
        (SubjectComponent::Environment, validate_environment_digest(environment)),
    ];
    // argv and optional components refuse with their own components.
    for (component, outcome) in checked {
        outcome.map_err(|reason| SubjectBindingRefusal::ComponentInvalid { component, reason })?;
    }
    for argument in argv.iter() {
        validate_argument(argument).map_err(|reason| SubjectBindingRefusal::ComponentInvalid {
            component: SubjectComponent::InvocationArgv,
            reason,
        })?;
    }
    if let Some(variant) = variant_target_id {
        validate_target_id(variant).map_err(|reason| SubjectBindingRefusal::ComponentInvalid {
            component: SubjectComponent::VariantTarget,
            reason,
        })?;
    }
    if let Some(instrument) = instrumentation_id {
        validate_reference(instrument, "instrumentation id", 1, 128, false).map_err(|reason| {
            SubjectBindingRefusal::ComponentInvalid {
                component: SubjectComponent::Instrumentation,
                reason,
            }
        })?;
    }
    if !matches!(runner, RunnerKind::Test | RunnerKind::Harness) {
        return Err(SubjectBindingRefusal::ComponentInvalid {
            component: SubjectComponent::RunnerRoute,
            reason: format!("runner {runner:?} is not an admitted upstream subject route"),
        });
    }
    if runner_artifact.canonical_path != runner.entrypoint() {
        return Err(SubjectBindingRefusal::ComponentInvalid {
            component: SubjectComponent::RunnerArtifact,
            reason: format!(
                "runner artifact {} is not the entrypoint of runner {runner:?}",
                runner_artifact.canonical_path
            ),
        });
    }
    Ok(())
}

/// The environment digest must bind the retained variables; a supplied
/// digest alone never satisfies the relation.
fn validate_environment_digest(environment: &EnvironmentIdentity) -> Result<(), String> {
    let derived = environment_identity(&environment.variables)?;
    if derived.sha256 != environment.sha256 {
        return Err("environment identity digest does not bind the retained variables".to_string());
    }
    Ok(())
}
