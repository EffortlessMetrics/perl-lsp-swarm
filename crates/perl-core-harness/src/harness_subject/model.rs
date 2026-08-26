//! The exact composed harness subject (`compiler_harness_subject.v1`, #12158).
//!
//! One immutable subject binds an observed upstream evidence artifact to the
//! exact repository/Perl/preparation/runner/target/environment/invocation/
//! process identity that produced it. The subject composes the component
//! vocabulary the observation contracts already retain — it is not a second
//! source, runner, environment, or invocation vocabulary — and answers one
//! question fail-closed: which exact subject produced this evidence. Anything
//! less than the complete exact binding refuses typed; it never defaults,
//! merges, or repairs.

use crate::observed_discovery::model::{EnvironmentIdentity, RunnerArtifactIdentity};
use crate::runner_model::RunnerKind;
use serde::{Deserialize, Serialize};

/// Versioned identity of the composed harness-subject schema.
pub const HARNESS_SUBJECT_SCHEMA_VERSION: &str = "perl_core_harness.compiler_harness_subject.v1";

/// Fixed claim boundary carried by every composed harness subject.
pub const HARNESS_SUBJECT_CLAIM_BOUNDARY: &str = "one exact composed repository/perl/preparation/\
                                                 runner/target/environment/invocation/process \
                                                 identity for observed upstream evidence; \
                                                 selection, execution, compiler results, \
                                                 reports, bundles, and publication remain \
                                                 unproven by this subject";

/// Component vocabulary of a typed subject-binding refusal. Each component
/// names the exact relation that refused; components never collapse into a
/// generic mismatch.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubjectComponent {
    /// Subject schema identity.
    SchemaVersion,
    /// Subject claim boundary.
    ClaimBoundary,
    /// Evidence class of the bound artifact.
    EvidenceClass,
    /// Measuring repository commit.
    RepositoryCommit,
    /// Resolved upstream Perl source reference.
    PerlRef,
    /// Prepared-tree identity reference.
    PreparedTree,
    /// Host Perl interpreter identity reference.
    HostPerl,
    /// Pinned target matrix fingerprint.
    MatrixFingerprint,
    /// Target contract identity.
    Target,
    /// Target selection contract digest.
    TargetContract,
    /// Environment-variant target identity.
    VariantTarget,
    /// Instrumentation subject identity.
    Instrumentation,
    /// Runner route identity.
    RunnerRoute,
    /// Runner artifact identity (path and content digest).
    RunnerArtifact,
    /// Invocation argv identity.
    InvocationArgv,
    /// Invocation working-directory identity.
    WorkingDirectory,
    /// Behavior-bearing environment identity.
    Environment,
    /// Process capture identity.
    ProcessNonce,
}

/// Typed refusal for every way an exact subject binding can fail. The refusal
/// is data: consumers (#12139 bundles, #12106 execution, #12159 reports)
/// discriminate on the component, never on message text.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "refusal", content = "detail")]
pub enum SubjectBindingRefusal {
    /// The artifact names a schema this subject contract does not own.
    UnsupportedSchema {
        /// Schema identity the artifact carried.
        found: String,
        /// Schema identity this contract binds.
        expected: String,
    },
    /// The evidence class can never present a current exact subject (declared
    /// input, reconstructed, direct diagnostic, or historical rows).
    EvidenceNotBindable {
        /// Evidence class that refused the binding.
        evidence_class: String,
    },
    /// Historical evidence recorded before exact subjects existed. It stays
    /// readable, but no constructor can relabel it as a current subject.
    HistoricalUnbound {
        /// Schema identity of the historical evidence.
        schema_version: String,
    },
    /// A required component failed its own relation law during composition.
    /// Nothing is defaulted: the missing or invalid component is named.
    ComponentInvalid {
        /// Component that failed its relation law.
        component: SubjectComponent,
        /// Why the component failed.
        reason: String,
    },
    /// Two bound artifacts disagree on one exact component. Equal spelling
    /// elsewhere never repairs the disagreement.
    ComponentMismatch {
        /// Component that disagrees across the binding.
        component: SubjectComponent,
        /// Value the expected subject carries.
        subject_value: String,
        /// Value the evidence carries.
        evidence_value: String,
    },
    /// Instrumented evidence failed to carry its instrumentation identity and
    /// therefore cannot distinguish itself from ordinary execution.
    InstrumentationUnidentified,
    /// A presented subject's derived fingerprint does not reproduce from the
    /// bound evidence: the subject was rebound or substituted.
    FingerprintMismatch {
        /// Fingerprint of the expected exact subject.
        expected: String,
        /// Fingerprint the bound evidence produces.
        found: String,
    },
    /// The bound artifact failed its own strict receipt validation, so no
    /// subject relation can be established through it.
    IncoherentEvidence {
        /// First strict-validation failure of the bound artifact.
        reason: String,
    },
}

impl std::fmt::Display for SubjectBindingRefusal {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsupportedSchema { found, expected } => {
                write!(
                    formatter,
                    "unsupported subject schema {found}; this contract binds {expected}"
                )
            }
            Self::EvidenceNotBindable { evidence_class } => {
                write!(
                    formatter,
                    "evidence class {evidence_class} can never present a current exact subject"
                )
            }
            Self::HistoricalUnbound { schema_version } => {
                write!(
                    formatter,
                    "historical evidence {schema_version} predates exact subjects and stays \
                     non-current"
                )
            }
            Self::ComponentInvalid { component, reason } => {
                write!(formatter, "subject component {component:?} is invalid: {reason}")
            }
            Self::ComponentMismatch { component, subject_value, evidence_value } => {
                write!(
                    formatter,
                    "subject component {component:?} disagrees: subject carries {subject_value} \
                     but evidence carries {evidence_value}"
                )
            }
            Self::InstrumentationUnidentified => {
                write!(
                    formatter,
                    "instrumented evidence carries no instrumentation identity and cannot \
                     masquerade as ordinary execution"
                )
            }
            Self::FingerprintMismatch { expected, found } => {
                write!(
                    formatter,
                    "bound evidence produces subject fingerprint {found}, not the presented \
                     {expected}: the subject was rebound or substituted"
                )
            }
            Self::IncoherentEvidence { reason } => {
                write!(formatter, "bound evidence failed its own strict validation: {reason}")
            }
        }
    }
}

/// One bound observed-evidence artifact a subject can be re-derived from.
#[derive(Clone, Copy)]
pub enum SubjectEvidence<'a> {
    /// An observed upstream-discovery receipt (#12103 producer row).
    Discovery(&'a crate::observed_discovery::model::UpstreamDiscoveryReceiptV1),
    /// An effective-invocation trace bound to its exact parent discovery
    /// receipt (#12104 producer row).
    InvocationTrace {
        /// The exact parent discovery receipt the trace binds.
        parent: &'a crate::observed_discovery::model::UpstreamDiscoveryReceiptV1,
        /// The instrumented trace receipt.
        trace: &'a crate::invocation_trace::model::EffectiveInvocationTraceReceiptV1,
    },
}

/// The exact composed harness subject. Every field comes from the component
/// vocabulary the observation contracts retain; the fingerprint is derived
/// over the complete identity and is never accepted as an input. Host paths
/// cannot enter the identity, so the fingerprint stays checkout-root
/// independent.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompilerHarnessSubjectV1 {
    /// Schema identity; always [`HARNESS_SUBJECT_SCHEMA_VERSION`].
    pub schema_version: String,
    /// Commit of the measuring repository.
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
    /// Instrumentation subject identity; load-bearing and impossible to drop
    /// from an instrumented subject.
    pub instrumentation_id: Option<String>,
    /// Runner route identity.
    pub runner: RunnerKind,
    /// Runner artifact identity with content digest.
    pub runner_artifact: RunnerArtifactIdentity,
    /// Invocation argv identity; checkout-root relative.
    pub argv: Vec<String>,
    /// Invocation working-directory identity; checkout-root relative.
    pub working_directory: String,
    /// Behavior-bearing environment identity with digest.
    pub environment: EnvironmentIdentity,
    /// Process capture identity shared by the bound evidence.
    pub process_nonce: String,
    /// Fixed claim boundary retained verbatim.
    pub claim_boundary: String,
    /// Deterministic SHA-256 over the canonical identity serialization
    /// (every field above except this one). Derived, never supplied.
    pub fingerprint: String,
}

/// Canonical identity view bound by the fingerprint: every subject field in
/// declaration order except the fingerprint itself.
#[derive(Serialize)]
struct SubjectIdentityView<'a> {
    schema_version: &'a str,
    repository_commit: &'a str,
    perl_ref: &'a str,
    prepared_tree_identity: &'a str,
    host_perl_identity: &'a str,
    matrix_fingerprint: &'a str,
    target_id: &'a str,
    target_contract_digest: &'a str,
    variant_target_id: &'a Option<String>,
    instrumentation_id: &'a Option<String>,
    runner: &'a RunnerKind,
    runner_artifact: &'a RunnerArtifactIdentity,
    argv: &'a [String],
    working_directory: &'a str,
    environment: &'a EnvironmentIdentity,
    process_nonce: &'a str,
    claim_boundary: &'a str,
}

impl CompilerHarnessSubjectV1 {
    /// Deterministic fingerprint over the exact composed identity. The same
    /// semantic subject always yields the same fingerprint; any component
    /// change — including the process capture identity — changes it.
    pub fn derive_fingerprint(&self) -> Result<String, String> {
        let view = SubjectIdentityView {
            schema_version: &self.schema_version,
            repository_commit: &self.repository_commit,
            perl_ref: &self.perl_ref,
            prepared_tree_identity: &self.prepared_tree_identity,
            host_perl_identity: &self.host_perl_identity,
            matrix_fingerprint: &self.matrix_fingerprint,
            target_id: &self.target_id,
            target_contract_digest: &self.target_contract_digest,
            variant_target_id: &self.variant_target_id,
            instrumentation_id: &self.instrumentation_id,
            runner: &self.runner,
            runner_artifact: &self.runner_artifact,
            argv: &self.argv,
            working_directory: &self.working_directory,
            environment: &self.environment,
            process_nonce: &self.process_nonce,
            claim_boundary: &self.claim_boundary,
        };
        let bytes = serde_json::to_vec(&view)
            .map_err(|error| format!("serializing the composed subject identity: {error}"))?;
        Ok(crate::build::sha256_bytes(&bytes))
    }

    /// Coherence law for a deserialized subject: the schema and claim boundary
    /// are exactly this contract's, every component still passes its relation
    /// law, and the retained fingerprint reproduces from the identity. A
    /// rebound, edited, or drifted subject refuses typed.
    pub fn refusal_if_incoherent(&self) -> Result<(), SubjectBindingRefusal> {
        if self.schema_version != HARNESS_SUBJECT_SCHEMA_VERSION {
            return Err(SubjectBindingRefusal::UnsupportedSchema {
                found: self.schema_version.clone(),
                expected: HARNESS_SUBJECT_SCHEMA_VERSION.to_string(),
            });
        }
        if self.claim_boundary != HARNESS_SUBJECT_CLAIM_BOUNDARY {
            return Err(SubjectBindingRefusal::ComponentInvalid {
                component: SubjectComponent::ClaimBoundary,
                reason: "claim boundary does not match the fixed subject claim boundary"
                    .to_string(),
            });
        }
        crate::harness_subject::build::validate_component_relations(
            &crate::harness_subject::build::SubjectRelations {
                repository_commit: &self.repository_commit,
                perl_ref: &self.perl_ref,
                prepared_tree_identity: &self.prepared_tree_identity,
                host_perl_identity: &self.host_perl_identity,
                matrix_fingerprint: &self.matrix_fingerprint,
                target_id: &self.target_id,
                target_contract_digest: &self.target_contract_digest,
                variant_target_id: self.variant_target_id.as_deref(),
                instrumentation_id: self.instrumentation_id.as_deref(),
                runner: self.runner,
                runner_artifact: &self.runner_artifact,
                argv: &self.argv,
                working_directory: &self.working_directory,
                environment: &self.environment,
                process_nonce: &self.process_nonce,
            },
        )?;
        let derived = self.derive_fingerprint().map_err(|reason| {
            SubjectBindingRefusal::ComponentInvalid {
                component: SubjectComponent::SchemaVersion,
                reason,
            }
        })?;
        if derived != self.fingerprint {
            return Err(SubjectBindingRefusal::FingerprintMismatch {
                expected: derived,
                found: self.fingerprint.clone(),
            });
        }
        Ok(())
    }
}
