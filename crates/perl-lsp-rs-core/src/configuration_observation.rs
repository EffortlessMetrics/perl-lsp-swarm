//! Crate-private, versioned configuration observation model (CG00A, #10813).
//!
//! One typed representation of raw configuration *input facts* — source and
//! producer identity, logical subject, per-field disposition, admission
//! against the checked [`crate::configuration_authority`] catalog, completeness,
//! and redaction — produced by fixture adapters only. Nothing here applies
//! precedence, selects winners, or mutates effective state; that remains with
//! the generation pipeline (#10386/#10387).
//!
//! Laws enforced structurally (see tests):
//! - transport position and client-declared labels cannot strengthen
//!   provenance; labels stay a bounded diagnostics projection and are
//!   rejected when oversized or excessive;
//! - absence, explicit reset, malformed, unsupported, unavailable, and
//!   instrument failure stay distinct;
//! - identical values under different provenance produce different
//!   observation identity;
//! - identity digests are algorithm-tagged SHA-256 over complete
//!   length-prefixed material, so no truncated-prefix collision is possible;
//! - mechanically decidable catalog validation rules execute during
//!   recording; interactive rules remain owned by the landed runtime slices;
//! - unmodeled external facts cannot weaken evidence to raw-value policies
//!   (`SafeValue`/`BoundedValue` are authority-reserved);
//! - digest-only evidence keeps its distinguishing digest; only redacted
//!   evidence collapses to a fixed marker;
//! - each field is recorded at most once per envelope, canonical IDs are
//!   unique in denominators, unknown canonical IDs always fail closed, and
//!   completeness counts cover only the declared denominator (surplus rows
//!   are disclosed separately);
//! - secret values never enter fingerprints or serialized receipts;
//! - environment/probe facts remain observations, never policy writers;
//! - unknown source classes fail closed (`AuthorityNotProven`);
//! - construction is builder-sealed: the finished observation derives
//!   serialization output but not deserialization input.
//!
//! Reconciliation (#10813): every landed identity is reused verbatim —
//! canonical field IDs and admission come from the checked configuration
//! authority, and [`ConfigSource`], [`ConfigValidation`],
//! [`ConfigValueKind`], [`ConfigSensitivity`]-backed [`EvidencePolicy`] are
//! the landed enums. Only the three source classes this issue itself
//! mandates beyond the landed catalog (`explicit_cli_or_operator`,
//! `feature_specific_internal_source`, `unknown_or_unsupported_source`) are
//! introduced here; the capability-role / external-effect vocabulary remains
//! owned by #10796 and is deliberately absent.

#![allow(dead_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use serde::{Deserialize, Serialize};

use crate::configuration_authority::{
    ConfigSource, ConfigValidation, ConfigValueKind, EvidencePolicy, FieldAuthority,
    authority_by_id,
};
use crate::hashing::sha256_hex;

/// Producer/schema generation of this observation contract version.
///
/// Generation 2: observation identity digests became algorithm-tagged
/// SHA-256 over full length-prefixed material (`sha256:` on the wire), and
/// the fingerprint covers validation/evidence-policy/limitations/counts.
/// Any further identity-visible change must bump this deliberately.
pub(crate) const OBSERVATION_SCHEMA_GENERATION: u32 = 2;

const MAX_IDENTITY_CHARS: usize = 128;
const MAX_TEXT_VALUE_CHARS: usize = 4_096;
const MAX_TEXT_LIST_ITEMS: usize = 256;
const MAX_FIELDS_PER_OBSERVATION: usize = 512;
const MAX_CLIENT_LABELS: usize = 16;
const MAX_LABEL_CHARS: usize = 128;

/// Failure modes for constructing or populating an observation. Adapters must
/// repair or drop the observation; construction never silently coerces.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ObservationError {
    EmptyIdentity {
        what: &'static str,
    },
    IdentityTooLong {
        what: &'static str,
        limit: usize,
    },
    UnknownCanonicalField {
        id: String,
    },
    EvidencePolicyMismatch {
        field: String,
    },
    MissingDeclaredEvidence {
        field: String,
    },
    UnsupportedValueKind {
        field: String,
    },
    ValueTooLong {
        field: String,
        limit: usize,
    },
    TextListTooLong {
        field: String,
        limit: usize,
    },
    TooManyFields {
        limit: usize,
    },
    PresentWithoutValue {
        field: String,
    },
    LabelOutOfBounds,
    TooManyClientLabels {
        limit: usize,
    },
    DuplicateCanonicalField {
        id: String,
    },
    DuplicateObservation {
        field: String,
    },
    UnsupportedEvidencePolicy {
        field: String,
    },
    MalformedValue {
        field: String,
        reason: MalformedReason,
    },
    /// An unmodeled external marker may not occupy a declared canonical
    /// denominator slot; only a genuine canonical row covers an expected ID.
    MarkerInsideDenominator {
        marker: String,
    },
}

/// Provenance class fixed by the observing adapter, never by transported
/// data.
///
/// Every class with a landed counterpart maps 1:1 onto the canonical
/// [`ConfigSource`] vocabulary — including the derived project-metadata
/// channel used by `workspace.declared_dependencies` and similar rows. The
/// three issue-mandated extensions have no landed channel and therefore
/// never admit a candidate value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub(crate) enum ConfigurationProvenanceClass {
    CompiledDefault,
    InitializationOptions,
    ProjectFile,
    TrustedUserOrMachineAdapter,
    GenericUnscopedClient,
    PerRootWorkspaceConfiguration,
    ExplicitCliOrOperator,
    ProcessEnvironment,
    SystemOrInterpreterProbe,
    ProjectMetadata,
    FeatureSpecificInternalSource,
    UnknownOrUnsupportedSource,
}

impl ConfigurationProvenanceClass {
    /// Landed effective-authority channel this class corresponds to, if any.
    pub(crate) const fn canonical_source(self) -> Option<ConfigSource> {
        match self {
            Self::CompiledDefault => Some(ConfigSource::CompiledDefault),
            Self::InitializationOptions => Some(ConfigSource::InitializationOptions),
            Self::ProjectFile => Some(ConfigSource::ProjectFile),
            Self::TrustedUserOrMachineAdapter => Some(ConfigSource::TrustedUserSettings),
            Self::GenericUnscopedClient => Some(ConfigSource::GlobalClientSettings),
            Self::PerRootWorkspaceConfiguration => Some(ConfigSource::WorkspaceConfiguration),
            Self::ProcessEnvironment => Some(ConfigSource::Environment),
            Self::SystemOrInterpreterProbe => Some(ConfigSource::SystemProbe),
            Self::ProjectMetadata => Some(ConfigSource::ProjectMetadata),
            Self::ExplicitCliOrOperator
            | Self::FeatureSpecificInternalSource
            | Self::UnknownOrUnsupportedSource => None,
        }
    }

    const fn discriminant(self) -> &'static str {
        match self {
            Self::CompiledDefault => "compiled_default",
            Self::InitializationOptions => "initialization_options",
            Self::ProjectFile => "project_file",
            Self::TrustedUserOrMachineAdapter => "trusted_user_or_machine_adapter",
            Self::GenericUnscopedClient => "generic_unscoped_client",
            Self::PerRootWorkspaceConfiguration => "per_root_workspace_configuration",
            Self::ExplicitCliOrOperator => "explicit_cli_or_operator",
            Self::ProcessEnvironment => "process_environment",
            Self::SystemOrInterpreterProbe => "system_or_interpreter_probe",
            Self::ProjectMetadata => "project_metadata",
            Self::FeatureSpecificInternalSource => "feature_specific_internal_source",
            Self::UnknownOrUnsupportedSource => "unknown_or_unsupported_source",
        }
    }

    const fn fails_closed(self) -> bool {
        matches!(self, Self::UnknownOrUnsupportedSource)
    }
}

/// Bounded transport/operation identity bound to the observation by the
/// adapter. Payloads here are correlation IDs or digests, never configuration
/// content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum ObservationTransport {
    InitializeSession { session_id: String },
    ConfigurationPullResult { request_id: String },
    ProjectFileRead { path_digest: String },
    EnvironmentVariable { name: String },
    InterpreterProbe { command_digest: String },
    CompiledDefaultsEmitted,
    OperatorInvocation,
    FeatureInternalState,
    Unidentified,
}

impl ObservationTransport {
    const fn discriminant(&self) -> &'static str {
        match self {
            Self::InitializeSession { .. } => "initialize_session",
            Self::ConfigurationPullResult { .. } => "configuration_pull_result",
            Self::ProjectFileRead { .. } => "project_file_read",
            Self::EnvironmentVariable { .. } => "environment_variable",
            Self::InterpreterProbe { .. } => "interpreter_probe",
            Self::CompiledDefaultsEmitted => "compiled_defaults_emitted",
            Self::OperatorInvocation => "operator_invocation",
            Self::FeatureInternalState => "feature_internal_state",
            Self::Unidentified => "unidentified",
        }
    }

    fn bounded_identity(&self) -> Option<&str> {
        match self {
            Self::InitializeSession { session_id } => Some(session_id),
            Self::ConfigurationPullResult { request_id } => Some(request_id),
            Self::ProjectFileRead { path_digest } => Some(path_digest),
            Self::EnvironmentVariable { name } => Some(name),
            Self::InterpreterProbe { command_digest } => Some(command_digest),
            Self::CompiledDefaultsEmitted
            | Self::OperatorInvocation
            | Self::FeatureInternalState
            | Self::Unidentified => None,
        }
    }
}

/// Where an observation was produced: producer/schema identity plus the fixed
/// provenance class and transport identity. Client-supplied labels ride along
/// for diagnostics only; they are excluded from fingerprints and never
/// consulted for provenance or admission.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ConfigurationSourceIdentity {
    producer_id: String,
    schema_generation: u32,
    provenance: ConfigurationProvenanceClass,
    transport: ObservationTransport,
    client_declared_labels: BTreeMap<String, String>,
}

impl ConfigurationSourceIdentity {
    pub(crate) fn new(
        producer_id: impl Into<String>,
        schema_generation: u32,
        provenance: ConfigurationProvenanceClass,
        transport: ObservationTransport,
    ) -> Result<Self, ObservationError> {
        let producer_id = producer_id.into();
        if producer_id.is_empty() {
            return Err(ObservationError::EmptyIdentity { what: "producer_id" });
        }
        if producer_id.len() > MAX_IDENTITY_CHARS {
            return Err(ObservationError::IdentityTooLong {
                what: "producer_id",
                limit: MAX_IDENTITY_CHARS,
            });
        }
        Ok(Self {
            producer_id,
            schema_generation,
            provenance,
            transport,
            client_declared_labels: BTreeMap::new(),
        })
    }

    /// Records a client-declared scope/trust label verbatim for diagnostics.
    ///
    /// Law: labels cannot strengthen provenance — they are excluded from
    /// fingerprints and never read by admission logic. The projection stays
    /// bounded and allowlisted to diagnostics: label count, key length, and
    /// value length are capped so transported labels cannot smuggle
    /// unbounded or credential-shaped content into serialized receipts.
    pub(crate) fn with_client_label(
        mut self,
        key: impl Into<String>,
        value: impl Into<String>,
    ) -> Result<Self, ObservationError> {
        let key = key.into();
        let value = value.into();
        if key.is_empty() || key.len() > MAX_LABEL_CHARS || value.len() > MAX_LABEL_CHARS {
            return Err(ObservationError::LabelOutOfBounds);
        }
        if self.client_declared_labels.len() >= MAX_CLIENT_LABELS {
            return Err(ObservationError::TooManyClientLabels { limit: MAX_CLIENT_LABELS });
        }
        self.client_declared_labels.insert(key, value);
        Ok(self)
    }

    pub(crate) fn provenance(&self) -> ConfigurationProvenanceClass {
        self.provenance
    }

    fn push_identity_material(&self, material: &mut String) {
        let _ = write!(
            material,
            "producer={};schema={};provenance={};transport={}",
            tag("p", self.producer_id.as_bytes()),
            self.schema_generation,
            self.provenance.discriminant(),
            self.transport.discriminant(),
        );
        if let Some(identity) = self.transport.bounded_identity() {
            let _ = write!(material, ";transport-id={}", tag("t", identity.as_bytes()));
        }
        // client_declared_labels deliberately omitted: labels cannot
        // strengthen provenance, so they cannot change identity either.
    }
}

/// Length-tagged bounded byte material for fingerprints and digests. The
/// length prefix keeps concatenated components unambiguous, and the complete
/// byte sequence is emitted so no prefix collision is possible before the
/// final collision-resistant digest compresses the material.
fn tag(prefix: &str, bytes: &[u8]) -> String {
    let mut hex = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(hex, "{byte:02x}");
    }
    format!("{prefix}{}:{hex}", bytes.len())
}

/// Logical scope of the observed input.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum ObservationScope {
    Global,
    Root { root_identity: String },
    Document { root_identity: Option<String>, document_identity: String },
    OperationDerived { request_identity: String },
}

impl ObservationScope {
    const fn discriminant(&self) -> &'static str {
        match self {
            Self::Global => "global",
            Self::Root { .. } => "root",
            Self::Document { .. } => "document",
            Self::OperationDerived { .. } => "operation_derived",
        }
    }

    fn push_scope_material(&self, material: &mut String) {
        let _ = write!(material, ";scope={}", self.discriminant());
        match self {
            Self::Global => {}
            Self::Root { root_identity } => {
                let _ = write!(material, ";root={}", tag("r", root_identity.as_bytes()));
            }
            Self::Document { root_identity, document_identity } => {
                if let Some(root_identity) = root_identity {
                    let _ = write!(material, ";root={}", tag("r", root_identity.as_bytes()));
                }
                let _ = write!(material, ";doc={}", tag("d", document_identity.as_bytes()));
            }
            Self::OperationDerived { request_identity } => {
                let _ = write!(material, ";request={}", tag("q", request_identity.as_bytes()));
            }
        }
    }
}

/// Logical subject carrying the observation ID and runtime/config/trust
/// generations so identical values under different identities or generations
/// remain distinct observations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ConfigurationObservationSubject {
    observation_id: String,
    scope: ObservationScope,
    runtime_generation: u64,
    configuration_generation: u64,
    trust_generation: u64,
}

impl ConfigurationObservationSubject {
    pub(crate) fn new(
        observation_id: impl Into<String>,
        scope: ObservationScope,
        runtime_generation: u64,
        configuration_generation: u64,
        trust_generation: u64,
    ) -> Result<Self, ObservationError> {
        let observation_id = observation_id.into();
        if observation_id.is_empty() {
            return Err(ObservationError::EmptyIdentity { what: "observation_id" });
        }
        if observation_id.len() > MAX_IDENTITY_CHARS {
            return Err(ObservationError::IdentityTooLong {
                what: "observation_id",
                limit: MAX_IDENTITY_CHARS,
            });
        }
        Ok(Self {
            observation_id,
            scope,
            runtime_generation,
            configuration_generation,
            trust_generation,
        })
    }

    fn push_subject_material(&self, material: &mut String) {
        let _ = write!(
            material,
            "id={};rt={};cfg={};trust={}",
            tag("o", self.observation_id.as_bytes()),
            self.runtime_generation,
            self.configuration_generation,
            self.trust_generation,
        );
        self.scope.push_scope_material(material);
    }
}

/// Why a value failed shape/validation checks. Bounded and structural.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub(crate) enum MalformedReason {
    WrongShape,
    OutOfRange,
    UnknownEnumMember,
    Oversized,
    Undecodable,
}

/// Per-field observation state. All non-terminal outcomes stay distinct:
/// absence is not explicit reset, and neither is malformed, unsupported,
/// unavailable, nor instrument failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub(crate) enum ConfigurationObservationDisposition {
    Present,
    Absent,
    ExplicitReset,
    Malformed { reason: MalformedReason },
    Unsupported,
    Unavailable,
    InstrumentFailure,
}

impl ConfigurationObservationDisposition {
    const fn discriminant(self) -> &'static str {
        match self {
            Self::Present => "present",
            Self::Absent => "absent",
            Self::ExplicitReset => "explicit_reset",
            Self::Malformed { .. } => "malformed",
            Self::Unsupported => "unsupported",
            Self::Unavailable => "unavailable",
            Self::InstrumentFailure => "instrument_failure",
        }
    }
}

/// Bounded normalized candidate value. Sensitive evidence collapses to
/// [`NormalizedValue::Redacted`] or a tagged digest before storage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum NormalizedValue {
    Flag(bool),
    Count(u64),
    Text(String),
    TextList(Vec<String>),
    Redacted,
    DigestOnly(String),
}

impl NormalizedValue {
    /// Deterministic identity material for fingerprints. Raw sensitive bytes
    /// cannot reach this point (enforced at admission time).
    fn evidence_material(&self) -> String {
        match self {
            Self::Flag(flag) => format!("flag={flag}"),
            Self::Count(count) => format!("count={count}"),
            Self::Text(text) => format!("text={}", tag("x", text.as_bytes())),
            Self::TextList(items) => {
                let joined = items.iter().map(|item| tag("x", item.as_bytes())).collect::<Vec<_>>();
                format!("list[{}]", joined.join(","))
            }
            Self::Redacted => "redacted".to_string(),
            Self::DigestOnly(digest) => format!("digest={digest}"),
        }
    }

    fn raw_text_parts(&self) -> Vec<&str> {
        match self {
            Self::Text(text) => vec![text.as_str()],
            Self::TextList(items) => items.iter().map(String::as_str).collect(),
            Self::Flag(_) | Self::Count(_) | Self::Redacted | Self::DigestOnly(_) => Vec::new(),
        }
    }
}

/// Admission result derived solely from the checked configuration authority:
/// whether this provenance class may supply a candidate value for the field,
/// is rejected for it, or has no provable authority relationship (unknown
/// classes, unmodeled external facts).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub(crate) enum SourceAuthorityAdmission {
    CandidateAdmitted,
    RejectedForField,
    AuthorityNotProven,
}

impl SourceAuthorityAdmission {
    const fn discriminant(self) -> &'static str {
        match self {
            Self::CandidateAdmitted => "candidate_admitted",
            Self::RejectedForField => "rejected_for_field",
            Self::AuthorityNotProven => "authority_not_proven",
        }
    }
}

/// Identity of an observed field: a canonical effective-field ID from the
/// configuration authority, or an explicitly unmodeled external marker (for
/// example `PERL5LIB`) naming a downstream fact — never a policy writer.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub(crate) enum ObservedFieldIdentity {
    Canonical { id: String },
    Unmodeled { external_marker: String },
}

impl ObservedFieldIdentity {
    pub(crate) fn canonical(id: impl Into<String>) -> Self {
        Self::Canonical { id: id.into() }
    }

    pub(crate) fn unmodeled(external_marker: impl Into<String>) -> Self {
        Self::Unmodeled { external_marker: external_marker.into() }
    }

    fn key(&self) -> &str {
        match self {
            Self::Canonical { id } => id,
            Self::Unmodeled { external_marker } => external_marker,
        }
    }

    fn push_identity_material(&self, material: &mut String) {
        match self {
            Self::Canonical { id } => {
                let _ = write!(material, "field=canon:{};", tag("f", id.as_bytes()));
            }
            Self::Unmodeled { external_marker } => {
                let _ = write!(material, "field=unmod:{};", tag("m", external_marker.as_bytes()));
            }
        }
    }
}

/// Limitations attached to observations or individual fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub(crate) enum ConfigurationObservationLimitation {
    ClientDeclaredLabelsUnverified,
    ProvenanceFromTransportPositionOnly,
    ProvenanceUnknownOrUnsupported,
    EnvironmentFactNotPolicyWriter,
    PartialFieldPopulation,
    /// The envelope recorded rows outside the declared denominator. Coverage
    /// numbers still count only expected-denominator population; this
    /// discloses the surplus rows separately.
    PopulationBeyondDenominator,
    EnvelopeShapeUntrusted,
    SensitiveValueRedacted,
}

/// One observed field with its admission, landed validation/evidence policy,
/// and limitations. Values arrive already normalized/redacted; raw input
/// never lands here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ObservedConfigurationField {
    identity: ObservedFieldIdentity,
    disposition: ConfigurationObservationDisposition,
    normalized_value: Option<NormalizedValue>,
    admission: SourceAuthorityAdmission,
    validation: Option<ConfigValidation>,
    evidence_policy: Option<EvidencePolicy>,
    limitations: BTreeSet<ConfigurationObservationLimitation>,
}

impl ObservedConfigurationField {
    pub(crate) fn admission(&self) -> SourceAuthorityAdmission {
        self.admission
    }

    pub(crate) fn disposition(&self) -> ConfigurationObservationDisposition {
        self.disposition
    }

    pub(crate) fn normalized_value(&self) -> Option<&NormalizedValue> {
        self.normalized_value.as_ref()
    }
}

/// Completeness of the observed population relative to the expected
/// denominator. Malformed, unsupported, unavailable, or instrument-failed
/// populations cannot report complete.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum ConfigurationCompleteness {
    Complete { expected: u32, observed: u32 },
    Partial { expected: u32, observed: u32 },
    EnvelopeMalformed,
    Unavailable,
    InstrumentFailure,
}

impl ConfigurationCompleteness {
    const fn discriminant(&self) -> &'static str {
        match self {
            Self::Complete { .. } => "complete",
            Self::Partial { .. } => "partial",
            Self::EnvelopeMalformed => "envelope_malformed",
            Self::Unavailable => "unavailable",
            Self::InstrumentFailure => "instrument_failure",
        }
    }
}

/// Content-independent observation identity: a tagged digest over subject,
/// provenance, transport, sorted per-field state, and completeness. Secret
/// values cannot enter because sensitive evidence contributes only fixed
/// markers or content digests.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub(crate) struct ConfigurationObservationFingerprint {
    digest: String,
}

impl ConfigurationObservationFingerprint {
    pub(crate) fn digest(&self) -> &str {
        &self.digest
    }
}

/// The immutable, versioned observation produced by
/// [`ConfigurationObservationDraft::finish`].
///
/// Serialize is derived because receipts are emitted; `Deserialize` is
/// deliberately NOT derived: JSON cannot reconstruct this authority-bearing
/// state without re-running the checked builder laws
/// ([`ConfigurationObservationDraft::finish`] computes admission,
/// redaction, limitations, and completeness from recorded facts). A decoded
/// receipt future consumers accept must go through a validated wrapper, not
/// raw derivation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct ConfigurationObservation {
    schema_generation: u32,
    subject: ConfigurationObservationSubject,
    source: ConfigurationSourceIdentity,
    expected_denominator: Vec<String>,
    fields: BTreeMap<String, ObservedConfigurationField>,
    completeness: ConfigurationCompleteness,
    limitations: BTreeSet<ConfigurationObservationLimitation>,
}

impl ConfigurationObservation {
    pub(crate) fn schema_generation(&self) -> u32 {
        self.schema_generation
    }

    pub(crate) fn provenance(&self) -> ConfigurationProvenanceClass {
        self.source.provenance
    }

    pub(crate) fn completeness(&self) -> ConfigurationCompleteness {
        self.completeness
    }

    pub(crate) fn observed_field(&self, key: &str) -> Option<&ObservedConfigurationField> {
        self.fields.get(key)
    }

    pub(crate) fn limitations(&self) -> impl Iterator<Item = &ConfigurationObservationLimitation> {
        self.limitations.iter()
    }

    /// Stable observation identity across processes and fixture generations.
    pub(crate) fn fingerprint(&self) -> ConfigurationObservationFingerprint {
        let mut material = String::new();
        let _ = write!(material, "v{}/", self.schema_generation);
        self.subject.push_subject_material(&mut material);
        material.push('/');
        self.source.push_identity_material(&mut material);
        material.push('/');
        for key in &self.expected_denominator {
            let _ = write!(material, "exp={};", tag("e", key.as_bytes()));
        }
        material.push('/');
        for (key, field) in &self.fields {
            field.identity.push_identity_material(&mut material);
            let _ = write!(
                material,
                "key={};disp={}",
                tag("k", key.as_bytes()),
                field.disposition.discriminant(),
            );
            // Malformed reasons are part of the typed fact and belong to the
            // observation identity.
            if let ConfigurationObservationDisposition::Malformed { reason } = field.disposition {
                let _ = write!(material, ";reason={}", malformed_reason_discriminant(reason));
            }
            let _ = write!(material, ";admission={};", field.admission.discriminant());
            if let Some(validation) = &field.validation {
                let _ = write!(material, "valid={};", validation_discriminant(*validation));
            }
            if let Some(evidence_policy) = &field.evidence_policy {
                let _ = write!(
                    material,
                    "evidence={};",
                    evidence_policy_discriminant(*evidence_policy)
                );
            }
            if let Some(value) = &field.normalized_value {
                let _ = write!(material, "value={};", value.evidence_material());
            }
            for limitation in &field.limitations {
                let _ = write!(material, "lim={};", limitation_discriminant(*limitation));
            }
            material.push('|');
        }
        for limitation in &self.limitations {
            let _ = write!(material, "env-lim={};", limitation_discriminant(*limitation));
        }
        match self.completeness {
            ConfigurationCompleteness::Complete { expected, observed }
            | ConfigurationCompleteness::Partial { expected, observed } => {
                let _ = write!(
                    material,
                    "/completeness={};expected={expected};observed={observed}",
                    self.completeness.discriminant(),
                );
            }
            other => {
                let _ = write!(material, "/completeness={}", other.discriminant());
            }
        }
        // Collision-resistant identity over the full canonical material; the
        // `sha256:` prefix names the algorithm explicitly on the wire.
        ConfigurationObservationFingerprint { digest: sha256_hex(material.as_bytes()) }
    }
}

/// Fixed vocabulary discriminants for identity material. Derived `Debug`
/// output is not a stable cross-compiler format, so the fingerprint never
/// relies on it; each landed enum contributes an explicit, frozen token.
fn validation_discriminant(validation: ConfigValidation) -> &'static str {
    match validation {
        ConfigValidation::Boolean => "boolean",
        ConfigValidation::NonEmptyString => "non_empty_string",
        ConfigValidation::OptionalNonEmptyString => "optional_non_empty_string",
        ConfigValidation::StringList => "string_list",
        ConfigValidation::UnsignedRange { .. } => "unsigned_range",
        ConfigValidation::PositiveFloat => "positive_float",
        ConfigValidation::KnownEnum => "known_enum",
        ConfigValidation::RelativeWorkspacePathList => "relative_workspace_path_list",
        ConfigValidation::AbsoluteExternalPathList => "absolute_external_path_list",
        ConfigValidation::ExecutableAndArgs => "executable_and_args",
        ConfigValidation::HttpHeaderName => "http_header_name",
        ConfigValidation::SafeHeaderPrefix => "safe_header_prefix",
        ConfigValidation::HttpsOrLoopbackEndpoint => "https_or_loopback_endpoint",
        ConfigValidation::Unsigned => "unsigned",
        ConfigValidation::Derived => "derived",
    }
}

fn evidence_policy_discriminant(evidence_policy: EvidencePolicy) -> &'static str {
    match evidence_policy {
        EvidencePolicy::SafeValue => "safe_value",
        EvidencePolicy::BoundedValue => "bounded_value",
        EvidencePolicy::PathIdentityOnly => "path_identity_only",
        EvidencePolicy::Redacted => "redacted",
        EvidencePolicy::DerivedDigestOnly => "derived_digest_only",
    }
}

fn limitation_discriminant(limitation: ConfigurationObservationLimitation) -> &'static str {
    match limitation {
        ConfigurationObservationLimitation::ClientDeclaredLabelsUnverified => {
            "client_declared_labels_unverified"
        }
        ConfigurationObservationLimitation::ProvenanceFromTransportPositionOnly => {
            "provenance_from_transport_position_only"
        }
        ConfigurationObservationLimitation::ProvenanceUnknownOrUnsupported => {
            "provenance_unknown_or_unsupported"
        }
        ConfigurationObservationLimitation::EnvironmentFactNotPolicyWriter => {
            "environment_fact_not_policy_writer"
        }
        ConfigurationObservationLimitation::PartialFieldPopulation => "partial_field_population",
        ConfigurationObservationLimitation::PopulationBeyondDenominator => {
            "population_beyond_denominator"
        }
        ConfigurationObservationLimitation::EnvelopeShapeUntrusted => "envelope_shape_untrusted",
        ConfigurationObservationLimitation::SensitiveValueRedacted => "sensitive_value_redacted",
    }
}

fn malformed_reason_discriminant(reason: MalformedReason) -> &'static str {
    match reason {
        MalformedReason::WrongShape => "wrong_shape",
        MalformedReason::OutOfRange => "out_of_range",
        MalformedReason::UnknownEnumMember => "unknown_enum_member",
        MalformedReason::Oversized => "oversized",
        MalformedReason::Undecodable => "undecodable",
    }
}

/// Landed authority facts a canonical field contributes to normalization.
struct CanonicalFieldPolicy {
    validation: ConfigValidation,
    value_kind: ConfigValueKind,
    evidence_policy: EvidencePolicy,
}

fn canonical_field_policy(id: &str) -> Option<CanonicalFieldPolicy> {
    authority_by_id(id).map(|row: &FieldAuthority| CanonicalFieldPolicy {
        validation: row.validation,
        value_kind: row.value_kind,
        evidence_policy: row.evidence_policy,
    })
}

/// Accumulates one observation. The builder computes admission, redaction,
/// limitations, and completeness; callers cannot hand-assert a complete
/// population and cannot inject values into fields the authority rejects.
#[derive(Debug)]
pub(crate) struct ConfigurationObservationDraft {
    subject: ConfigurationObservationSubject,
    source: ConfigurationSourceIdentity,
    expected_denominator: Vec<String>,
    fields: BTreeMap<String, ObservedConfigurationField>,
    envelope_malformed: bool,
    limitations: BTreeSet<ConfigurationObservationLimitation>,
}

impl ConfigurationObservationDraft {
    /// Assembles an empty draft. Construction is total: `subject` and
    /// `source` are validated by their own constructors, so no failure mode
    /// remains here; populating and finishing carry the typed errors.
    pub(crate) fn new(
        subject: ConfigurationObservationSubject,
        source: ConfigurationSourceIdentity,
    ) -> Self {
        Self {
            subject,
            source,
            expected_denominator: Vec::new(),
            fields: BTreeMap::new(),
            envelope_malformed: false,
            limitations: BTreeSet::new(),
        }
    }

    /// Declares the expected denominator from canonical authority IDs.
    /// Unknown IDs fail closed instead of widening the denominator, IDs must
    /// be unique (duplicates would silently skew completeness counts), and
    /// the denominator obeys the same field-count bound as observations.
    pub(crate) fn expect_canonical_fields(&mut self, ids: &[&str]) -> Result<(), ObservationError> {
        if ids.len() > MAX_FIELDS_PER_OBSERVATION {
            return Err(ObservationError::TooManyFields { limit: MAX_FIELDS_PER_OBSERVATION });
        }
        let mut seen = BTreeSet::new();
        for id in ids {
            if authority_by_id(id).is_none() {
                return Err(ObservationError::UnknownCanonicalField { id: (*id).to_string() });
            }
            if !seen.insert(*id) {
                return Err(ObservationError::DuplicateCanonicalField { id: (*id).to_string() });
            }
        }
        self.expected_denominator = seen.into_iter().map(str::to_string).collect();
        self.expected_denominator.sort();
        Ok(())
    }

    fn admission_for(&self, identity: &ObservedFieldIdentity) -> SourceAuthorityAdmission {
        let Some(source) = self.source.provenance.canonical_source() else {
            return SourceAuthorityAdmission::AuthorityNotProven;
        };
        match identity {
            ObservedFieldIdentity::Canonical { id } => match authority_by_id(id) {
                Some(row) if row.sources.contains(&source) => {
                    SourceAuthorityAdmission::CandidateAdmitted
                }
                Some(_) => SourceAuthorityAdmission::RejectedForField,
                None => SourceAuthorityAdmission::AuthorityNotProven,
            },
            ObservedFieldIdentity::Unmodeled { .. } => SourceAuthorityAdmission::AuthorityNotProven,
        }
    }

    /// Records a present value. Canonical fields take their kind, validation,
    /// and evidence policy from the landed authority row (a mismatching
    /// declared policy is rejected); unmodeled external facts must declare
    /// their own evidence policy explicitly and fail closed otherwise.
    pub(crate) fn record_present(
        &mut self,
        identity: ObservedFieldIdentity,
        value: NormalizedValue,
        declared_evidence: Option<EvidencePolicy>,
    ) -> Result<(), ObservationError> {
        if self.fields.len() >= MAX_FIELDS_PER_OBSERVATION {
            return Err(ObservationError::TooManyFields { limit: MAX_FIELDS_PER_OBSERVATION });
        }
        let key = identity.key().to_string();
        // Namespace integrity: an unmodeled external marker never occupies a
        // declared canonical denominator slot, so it cannot impersonate
        // expected-ID coverage.
        if matches!(identity, ObservedFieldIdentity::Unmodeled { .. })
            && self.expected_denominator.iter().any(|id| id == identity.key())
        {
            return Err(ObservationError::MarkerInsideDenominator { marker: key });
        }
        // A field is observed once per envelope: a later recording must never
        // silently overwrite an earlier one (permutation-sensitive receipts
        // are not deterministic observations).
        if self.fields.contains_key(&key) {
            return Err(ObservationError::DuplicateObservation { field: key });
        }
        let policy = match &identity {
            ObservedFieldIdentity::Canonical { id } => match canonical_field_policy(id) {
                Some(policy) => Some(policy),
                None => return Err(ObservationError::UnknownCanonicalField { id: id.clone() }),
            },
            ObservedFieldIdentity::Unmodeled { .. } => None,
        };

        if let Some(policy) = &policy {
            let compatible = matches!(
                (&value, policy.value_kind),
                (NormalizedValue::Flag(_), ConfigValueKind::Boolean)
                    | (NormalizedValue::Count(_), ConfigValueKind::Unsigned)
                    | (
                        NormalizedValue::Text(_),
                        ConfigValueKind::String
                            | ConfigValueKind::OptionalString
                            | ConfigValueKind::Enum,
                    )
                    | (
                        NormalizedValue::TextList(_),
                        ConfigValueKind::StringList | ConfigValueKind::DerivedList,
                    )
            );
            if !compatible {
                return Err(ObservationError::UnsupportedValueKind { field: key.clone() });
            }
            // Execute the mechanically decidable catalog rules before any
            // evidence handling, so a value the authority would reject is a
            // typed malformed outcome instead of an admitted observation:
            // NonEmptyString / OptionalNonEmptyString reject empty text
            // (absence for the optional variant travels as its own
            // disposition), StringList rejects empty list items, and
            // UnsignedRange bounds counts. Structurally interactive rules
            // (enum spellings, path/executable/header/endpoint shapes)
            // require the generation/runtime pipeline and stay owned by the
            // landed runtime slices (#7057).
            match policy.validation {
                ConfigValidation::NonEmptyString | ConfigValidation::OptionalNonEmptyString
                    if value.raw_text_parts().iter().any(|part| part.is_empty()) =>
                {
                    return Err(ObservationError::MalformedValue {
                        field: key.clone(),
                        reason: MalformedReason::WrongShape,
                    });
                }
                ConfigValidation::StringList => {
                    if let NormalizedValue::TextList(items) = &value
                        && items.iter().any(String::is_empty)
                    {
                        return Err(ObservationError::MalformedValue {
                            field: key.clone(),
                            reason: MalformedReason::WrongShape,
                        });
                    }
                }
                ConfigValidation::UnsignedRange { minimum, maximum } => {
                    if let NormalizedValue::Count(count) = &value
                        && (*count < minimum || *count > maximum)
                    {
                        return Err(ObservationError::MalformedValue {
                            field: key.clone(),
                            reason: MalformedReason::OutOfRange,
                        });
                    }
                }
                _ => {}
            }
        }
        for part in value.raw_text_parts() {
            if part.len() > MAX_TEXT_VALUE_CHARS {
                return Err(ObservationError::ValueTooLong {
                    field: key.clone(),
                    limit: MAX_TEXT_VALUE_CHARS,
                });
            }
        }
        if let NormalizedValue::TextList(items) = &value
            && items.len() > MAX_TEXT_LIST_ITEMS
        {
            return Err(ObservationError::TextListTooLong {
                field: key.clone(),
                limit: MAX_TEXT_LIST_ITEMS,
            });
        }

        let evidence_policy = match (&identity, declared_evidence, policy.as_ref()) {
            (ObservedFieldIdentity::Canonical { .. }, Some(declared), Some(policy)) => {
                if declared != policy.evidence_policy {
                    return Err(ObservationError::EvidencePolicyMismatch { field: key.clone() });
                }
                policy.evidence_policy
            }
            (ObservedFieldIdentity::Canonical { .. }, None, Some(policy)) => policy.evidence_policy,
            (ObservedFieldIdentity::Unmodeled { .. }, Some(declared), _) => declared,
            (ObservedFieldIdentity::Unmodeled { .. }, None, _) => {
                return Err(ObservationError::MissingDeclaredEvidence { field: key.clone() });
            }
            // Canonical fields always resolved a landed row above.
            (ObservedFieldIdentity::Canonical { .. }, _, None) => {
                return Err(ObservationError::UnknownCanonicalField { id: key.clone() });
            }
        };

        let mut limitations = BTreeSet::new();
        // Unmodeled external facts are sensitive-by-default: an adapter that
        // is not backed by the authority catalog cannot weaken evidence to a
        // raw-value policy. SafeValue/BoundedValue are reserved for canonical
        // rows whose sensitivity the catalog already classified.
        if matches!(identity, ObservedFieldIdentity::Unmodeled { .. })
            && matches!(evidence_policy, EvidencePolicy::SafeValue | EvidencePolicy::BoundedValue)
        {
            return Err(ObservationError::UnsupportedEvidencePolicy { field: key });
        }
        let normalized_value = match evidence_policy {
            EvidencePolicy::Redacted => {
                limitations.insert(ConfigurationObservationLimitation::SensitiveValueRedacted);
                NormalizedValue::Redacted
            }
            EvidencePolicy::PathIdentityOnly => {
                limitations.insert(ConfigurationObservationLimitation::SensitiveValueRedacted);
                NormalizedValue::DigestOnly(sha256_hex(value.evidence_material().as_bytes()))
            }
            // Digest-only evidence keeps its distinguishing collision-resistant
            // digest; only truly redacted evidence collapses to the constant
            // marker.
            EvidencePolicy::DerivedDigestOnly => {
                limitations.insert(ConfigurationObservationLimitation::SensitiveValueRedacted);
                NormalizedValue::DigestOnly(sha256_hex(value.evidence_material().as_bytes()))
            }
            EvidencePolicy::SafeValue | EvidencePolicy::BoundedValue => value,
        };

        let admission = self.admission_for(&identity);
        let validation = policy.map(|policy| policy.validation);
        self.fields.insert(
            key,
            ObservedConfigurationField {
                identity,
                disposition: ConfigurationObservationDisposition::Present,
                normalized_value: Some(normalized_value),
                admission,
                validation,
                evidence_policy: Some(evidence_policy),
                limitations,
            },
        );
        Ok(())
    }

    /// Records a non-present outcome. `Present` is rejected here — values
    /// travel through [`Self::record_present`] only.
    pub(crate) fn record_disposition(
        &mut self,
        identity: ObservedFieldIdentity,
        disposition: ConfigurationObservationDisposition,
    ) -> Result<(), ObservationError> {
        if self.fields.len() >= MAX_FIELDS_PER_OBSERVATION {
            return Err(ObservationError::TooManyFields { limit: MAX_FIELDS_PER_OBSERVATION });
        }
        let key = identity.key().to_string();
        if disposition == ConfigurationObservationDisposition::Present {
            return Err(ObservationError::PresentWithoutValue { field: key });
        }
        // Namespace integrity, mirroring record_present.
        if matches!(identity, ObservedFieldIdentity::Unmodeled { .. })
            && self.expected_denominator.iter().any(|id| id == identity.key())
        {
            return Err(ObservationError::MarkerInsideDenominator { marker: key });
        }
        // Same fail-closed law as record_present: an ID that is not in the
        // canonical namespace must surface as UnknownCanonicalField, and only
        // the explicit Unmodeled variant may carry external markers.
        if matches!(identity, ObservedFieldIdentity::Canonical { .. })
            && canonical_field_policy(identity.key()).is_none()
        {
            return Err(ObservationError::UnknownCanonicalField { id: key.clone() });
        }
        if self.fields.contains_key(&key) {
            return Err(ObservationError::DuplicateObservation { field: key });
        }
        let validation = match &identity {
            ObservedFieldIdentity::Canonical { id } => {
                canonical_field_policy(id).map(|policy| policy.validation)
            }
            ObservedFieldIdentity::Unmodeled { .. } => None,
        };
        let admission = self.admission_for(&identity);
        self.fields.insert(
            key,
            ObservedConfigurationField {
                identity,
                disposition,
                normalized_value: None,
                admission,
                validation,
                evidence_policy: None,
                limitations: BTreeSet::new(),
            },
        );
        Ok(())
    }

    /// Marks the whole transport envelope shape-untrusted (short, late,
    /// duplicate, oversized, undecodable). Completeness can then never be
    /// complete.
    pub(crate) fn mark_envelope_malformed(&mut self) {
        self.envelope_malformed = true;
    }

    pub(crate) fn add_limitation(&mut self, limitation: ConfigurationObservationLimitation) {
        self.limitations.insert(limitation);
    }

    /// Seals the draft: derives limitations and completeness from recorded
    /// facts and emits the immutable observation.
    pub(crate) fn finish(mut self) -> Result<ConfigurationObservation, ObservationError> {
        match self.source.provenance {
            ConfigurationProvenanceClass::GenericUnscopedClient => {
                self.limitations
                    .insert(ConfigurationObservationLimitation::ClientDeclaredLabelsUnverified);
                self.limitations.insert(
                    ConfigurationObservationLimitation::ProvenanceFromTransportPositionOnly,
                );
            }
            ConfigurationProvenanceClass::UnknownOrUnsupportedSource => {
                self.limitations
                    .insert(ConfigurationObservationLimitation::ProvenanceUnknownOrUnsupported);
            }
            ConfigurationProvenanceClass::ProcessEnvironment
            | ConfigurationProvenanceClass::SystemOrInterpreterProbe => {
                self.limitations
                    .insert(ConfigurationObservationLimitation::EnvironmentFactNotPolicyWriter);
            }
            _ => {}
        }
        if self.envelope_malformed {
            self.limitations.insert(ConfigurationObservationLimitation::EnvelopeShapeUntrusted);
        }

        let mut blocked = false;
        let mut instrument_failure = false;
        let mut unavailable = false;
        for field in self.fields.values() {
            match field.disposition {
                ConfigurationObservationDisposition::Malformed { .. }
                | ConfigurationObservationDisposition::Unsupported => blocked = true,
                ConfigurationObservationDisposition::InstrumentFailure => instrument_failure = true,
                ConfigurationObservationDisposition::Unavailable => unavailable = true,
                ConfigurationObservationDisposition::Present
                | ConfigurationObservationDisposition::Absent
                | ConfigurationObservationDisposition::ExplicitReset => {}
            }
        }
        // An expected ID is covered only by a genuine canonical row; markers
        // are rejected from denominator slots at recording time, so a bare
        // key match already implies canonical, but the variant check keeps
        // the coverage law explicit and future-proof.
        let missing_expected = self.expected_denominator.iter().any(|id| {
            !self.fields.get(id).is_some_and(|field| {
                matches!(field.identity, ObservedFieldIdentity::Canonical { .. })
            })
        });

        // Coverage counts only the declared denominator population; rows
        // outside it (unmodeled facts, out-of-denominator recordings) are
        // disclosed through a limitation instead of inflating completeness.
        let denominator: BTreeSet<&str> =
            self.expected_denominator.iter().map(String::as_str).collect();
        if self.fields.keys().any(|key| !denominator.contains(key.as_str())) {
            self.limitations
                .insert(ConfigurationObservationLimitation::PopulationBeyondDenominator);
        }

        let expected = self.expected_denominator.len() as u32;
        let observed =
            self.expected_denominator.iter().filter(|id| self.fields.contains_key(*id)).count()
                as u32;
        let completeness = if self.envelope_malformed {
            ConfigurationCompleteness::EnvelopeMalformed
        } else if instrument_failure {
            ConfigurationCompleteness::InstrumentFailure
        } else if unavailable {
            ConfigurationCompleteness::Unavailable
        } else if blocked || missing_expected {
            ConfigurationCompleteness::Partial { expected, observed }
        } else {
            ConfigurationCompleteness::Complete { expected, observed }
        };
        if matches!(completeness, ConfigurationCompleteness::Partial { .. }) {
            self.limitations.insert(ConfigurationObservationLimitation::PartialFieldPopulation);
        }

        Ok(ConfigurationObservation {
            schema_generation: OBSERVATION_SCHEMA_GENERATION,
            subject: self.subject,
            source: self.source,
            expected_denominator: self.expected_denominator,
            fields: self.fields,
            completeness,
            limitations: self.limitations,
        })
    }
}

#[cfg(test)]
mod tests {
    use perl_test_must::{must_some_with, must_with};

    use super::*;

    fn subject(scope: ObservationScope) -> ConfigurationObservationSubject {
        must_with(
            ConfigurationObservationSubject::new("obs-fixture", scope, 1, 1, 1),
            "valid subject",
        )
    }

    fn source(
        provenance: ConfigurationProvenanceClass,
        transport: ObservationTransport,
    ) -> ConfigurationSourceIdentity {
        must_with(
            ConfigurationSourceIdentity::new(
                "perl-lsp-rs-core/test",
                OBSERVATION_SCHEMA_GENERATION,
                provenance,
                transport,
            ),
            "valid source identity",
        )
    }

    fn try_draft(
        scope: ObservationScope,
        provenance: ConfigurationProvenanceClass,
        transport: ObservationTransport,
    ) -> anyhow::Result<ConfigurationObservationDraft> {
        let observation_subject =
            ConfigurationObservationSubject::new("obs-fixture", scope, 1, 1, 1)
                .map_err(|error| anyhow::anyhow!("constructing observation subject: {error:?}"))?;
        let source_identity = ConfigurationSourceIdentity::new(
            "perl-lsp-rs-core/test",
            OBSERVATION_SCHEMA_GENERATION,
            provenance,
            transport,
        )
        .map_err(|error| anyhow::anyhow!("constructing source identity: {error:?}"))?;
        Ok(ConfigurationObservationDraft::new(observation_subject, source_identity))
    }

    fn finish(
        provenance: ConfigurationProvenanceClass,
        transport: ObservationTransport,
        scope: ObservationScope,
        build: impl FnOnce(&mut ConfigurationObservationDraft),
    ) -> ConfigurationObservation {
        let mut draft =
            ConfigurationObservationDraft::new(subject(scope), source(provenance, transport));
        build(&mut draft);
        must_with(draft.finish(), "fixture observation finishes")
    }

    /// Falsifier #1: the same visible AI value from a trusted user adapter
    /// and from a project file must produce distinct observations with
    /// distinct authority outcomes.
    #[test]
    fn same_value_from_trusted_user_and_project_file_is_distinct_identity() {
        let trusted = finish(
            ConfigurationProvenanceClass::TrustedUserOrMachineAdapter,
            ObservationTransport::OperatorInvocation,
            ObservationScope::Global,
            |draft| {
                must_with(draft.expect_canonical_fields(&["ai.model"]), "known row");
                must_with(
                    draft.record_present(
                        ObservedFieldIdentity::canonical("ai.model"),
                        NormalizedValue::Text("gpt-perl".to_string()),
                        None,
                    ),
                    "trusted channel admits ai.model",
                );
            },
        );
        let project = finish(
            ConfigurationProvenanceClass::ProjectFile,
            ObservationTransport::ProjectFileRead {
                path_digest: "digest:.perl-lsp.toml".to_string(),
            },
            ObservationScope::Root { root_identity: "root-a".to_string() },
            |draft| {
                must_with(draft.expect_canonical_fields(&["ai.model"]), "known row");
                must_with(
                    draft.record_present(
                        ObservedFieldIdentity::canonical("ai.model"),
                        NormalizedValue::Text("gpt-perl".to_string()),
                        None,
                    ),
                    "recording is allowed; admission is computed",
                );
            },
        );

        let trusted_field = must_some_with(trusted.observed_field("ai.model"), "field recorded");
        let project_field = must_some_with(project.observed_field("ai.model"), "field recorded");
        assert_eq!(trusted_field.admission(), SourceAuthorityAdmission::CandidateAdmitted);
        assert_eq!(project_field.admission(), SourceAuthorityAdmission::RejectedForField);
        assert_ne!(trusted.fingerprint(), project.fingerprint());
        assert_eq!(
            trusted.observed_field("ai.model").map(|field| field.normalized_value()),
            project.observed_field("ai.model").map(|field| field.normalized_value())
        );
    }

    /// Falsifier #2: an unscoped client that calls itself trusted stays
    /// generic/untrusted, and its labels change neither admission nor
    /// observation identity.
    #[test]
    fn unscoped_client_cannot_self_declare_trust() {
        let identity = must_with(
            ConfigurationSourceIdentity::new(
                "perl-lsp-rs-core/test",
                OBSERVATION_SCHEMA_GENERATION,
                ConfigurationProvenanceClass::GenericUnscopedClient,
                ObservationTransport::InitializeSession { session_id: "s-1".to_string() },
            ),
            "valid identity",
        );
        let labeled = must_with(
            must_with(identity.with_client_label("scope", "global"), "bounded label")
                .with_client_label("trusted", "true"),
            "bounded label",
        );
        let mut draft =
            ConfigurationObservationDraft::new(subject(ObservationScope::Global), labeled);
        must_with(draft.expect_canonical_fields(&["ai.endpoint"]), "known row");
        must_with(
            draft.record_present(
                ObservedFieldIdentity::canonical("ai.endpoint"),
                NormalizedValue::Text("https://client.example".to_string()),
                None,
            ),
            "recording is allowed",
        );
        let labeled_observation = must_with(draft.finish(), "finishes");

        let unlabeled_observation = finish(
            ConfigurationProvenanceClass::GenericUnscopedClient,
            ObservationTransport::InitializeSession { session_id: "s-1".to_string() },
            ObservationScope::Global,
            |draft| {
                must_with(draft.expect_canonical_fields(&["ai.endpoint"]), "known row");
                must_with(
                    draft.record_present(
                        ObservedFieldIdentity::canonical("ai.endpoint"),
                        NormalizedValue::Text("https://client.example".to_string()),
                        None,
                    ),
                    "recording is allowed",
                );
            },
        );

        assert_eq!(
            labeled_observation.provenance(),
            ConfigurationProvenanceClass::GenericUnscopedClient
        );
        assert_eq!(labeled_observation.fingerprint(), unlabeled_observation.fingerprint());
        assert_eq!(
            labeled_observation.observed_field("ai.endpoint").map(|field| field.admission()),
            Some(SourceAuthorityAdmission::RejectedForField)
        );
        assert!(labeled_observation.limitations().any(|limitation| matches!(
            limitation,
            ConfigurationObservationLimitation::ClientDeclaredLabelsUnverified
                | ConfigurationObservationLimitation::ProvenanceFromTransportPositionOnly
        )));
    }

    /// Falsifier #3: root A/B responses carrying the same key/value remain
    /// distinct subjects and therefore distinct observations.
    #[test]
    fn root_a_and_root_b_identical_values_remain_distinct_subjects() {
        let build = |root: &str| {
            finish(
                ConfigurationProvenanceClass::PerRootWorkspaceConfiguration,
                ObservationTransport::ConfigurationPullResult { request_id: "req-7".to_string() },
                ObservationScope::Root { root_identity: root.to_string() },
                |draft| {
                    must_with(
                        draft.expect_canonical_fields(&["workspace.include_paths"]),
                        "known row",
                    );
                    must_with(
                        draft.record_present(
                            ObservedFieldIdentity::canonical("workspace.include_paths"),
                            NormalizedValue::TextList(vec!["lib".to_string()]),
                            None,
                        ),
                        "folder pull admits include paths",
                    );
                },
            )
        };

        let root_a = build("root-a");
        let root_b = build("root-b");
        assert_ne!(root_a.fingerprint(), root_b.fingerprint());
    }

    /// Falsifier #4: an environment PERL5LIB fact is recorded as an unmodeled,
    /// digest-only observation and can never write the `use_perl5lib` policy
    /// field; probe facts likewise cannot write `use_system_inc`.
    #[test]
    fn perl5lib_observation_stays_separate_from_use_perl5lib_policy() {
        let environment = finish(
            ConfigurationProvenanceClass::ProcessEnvironment,
            ObservationTransport::EnvironmentVariable { name: "PERL5LIB".to_string() },
            ObservationScope::Global,
            |draft| {
                must_with(
                    draft.record_present(
                        ObservedFieldIdentity::unmodeled("PERL5LIB"),
                        NormalizedValue::TextList(vec![
                            "/opt/site/lib".to_string(),
                            "/home/user/perl5".to_string(),
                        ]),
                        Some(EvidencePolicy::PathIdentityOnly),
                    ),
                    "unmodeled fact declares its evidence policy",
                );
            },
        );
        let perl5lib = must_some_with(environment.observed_field("PERL5LIB"), "fact recorded");
        assert_eq!(perl5lib.admission(), SourceAuthorityAdmission::AuthorityNotProven);
        assert!(matches!(perl5lib.normalized_value(), Some(NormalizedValue::DigestOnly(_))));
        assert!(environment.limitations().any(|limitation| matches!(
            limitation,
            ConfigurationObservationLimitation::EnvironmentFactNotPolicyWriter
        )));

        let policy_attempt = finish(
            ConfigurationProvenanceClass::ProcessEnvironment,
            ObservationTransport::EnvironmentVariable { name: "PERL5LIB".to_string() },
            ObservationScope::Global,
            |draft| {
                must_with(
                    draft.record_present(
                        ObservedFieldIdentity::canonical("workspace.use_perl5lib"),
                        NormalizedValue::Flag(true),
                        None,
                    ),
                    "recording is allowed",
                );
            },
        );
        assert_eq!(
            policy_attempt.observed_field("workspace.use_perl5lib").map(|field| field.admission()),
            Some(SourceAuthorityAdmission::RejectedForField)
        );

        let probe_attempt = finish(
            ConfigurationProvenanceClass::SystemOrInterpreterProbe,
            ObservationTransport::InterpreterProbe { command_digest: "digest:perl -V".to_string() },
            ObservationScope::Global,
            |draft| {
                must_with(
                    draft.record_present(
                        ObservedFieldIdentity::canonical("workspace.use_system_inc"),
                        NormalizedValue::Flag(true),
                        None,
                    ),
                    "recording is allowed",
                );
            },
        );
        assert_eq!(
            probe_attempt.observed_field("workspace.use_system_inc").map(|field| field.admission()),
            Some(SourceAuthorityAdmission::RejectedForField)
        );
    }

    /// Falsifier #5: a failed system probe observes `Unavailable`; it never
    /// becomes an empty-but-complete success.
    #[test]
    fn system_probe_failure_is_unavailable_not_empty_success() {
        let failed_probe = finish(
            ConfigurationProvenanceClass::SystemOrInterpreterProbe,
            ObservationTransport::InterpreterProbe { command_digest: "digest:perl -V".to_string() },
            ObservationScope::Global,
            |draft| {
                must_with(
                    draft.expect_canonical_fields(&["workspace.use_system_inc"]),
                    "known row",
                );
                must_with(
                    draft.record_disposition(
                        ObservedFieldIdentity::canonical("workspace.use_system_inc"),
                        ConfigurationObservationDisposition::Unavailable,
                    ),
                    "disposition records",
                );
            },
        );

        assert_eq!(failed_probe.completeness(), ConfigurationCompleteness::Unavailable);
        assert!(failed_probe.observed_field("workspace.use_system_inc").is_some());
    }

    /// Falsifier #6: explicit reset, absence, and malformed input stay
    /// distinct states with distinct identities.
    #[test]
    fn explicit_reset_differs_from_absent_and_malformed() {
        let build = |disposition| {
            finish(
                ConfigurationProvenanceClass::GenericUnscopedClient,
                ObservationTransport::ConfigurationPullResult { request_id: "req-9".to_string() },
                ObservationScope::Global,
                move |draft| {
                    must_with(draft.expect_canonical_fields(&["formatting.enabled"]), "known row");
                    must_with(
                        draft.record_disposition(
                            ObservedFieldIdentity::canonical("formatting.enabled"),
                            disposition,
                        ),
                        "disposition records",
                    );
                },
            )
        };

        let absent = build(ConfigurationObservationDisposition::Absent);
        let reset = build(ConfigurationObservationDisposition::ExplicitReset);
        let malformed = build(ConfigurationObservationDisposition::Malformed {
            reason: MalformedReason::WrongShape,
        });

        assert_ne!(
            absent.observed_field("formatting.enabled").map(|field| field.disposition()),
            reset.observed_field("formatting.enabled").map(|field| field.disposition())
        );
        assert_ne!(absent.fingerprint(), reset.fingerprint());
        assert_ne!(reset.fingerprint(), malformed.fingerprint());
        assert!(matches!(
            absent.completeness(),
            ConfigurationCompleteness::Complete { expected: 1, observed: 1 }
        ));
        assert!(matches!(
            reset.completeness(),
            ConfigurationCompleteness::Complete { expected: 1, observed: 1 }
        ));
        assert!(matches!(malformed.completeness(), ConfigurationCompleteness::Partial { .. }));
    }

    /// Falsifier #7: insertion order cannot change normalized identity or
    /// serialized form.
    #[test]
    fn insertion_order_does_not_change_normalized_identity() {
        let populate_first = |draft: &mut ConfigurationObservationDraft| {
            must_with(
                draft.expect_canonical_fields(&[
                    "critic.engine",
                    "formatting.tabs",
                    "telemetry.enabled",
                ]),
                "known rows",
            );
            must_with(
                draft.record_present(
                    ObservedFieldIdentity::canonical("telemetry.enabled"),
                    NormalizedValue::Flag(true),
                    None,
                ),
                "records",
            );
            must_with(
                draft.record_present(
                    ObservedFieldIdentity::canonical("formatting.tabs"),
                    NormalizedValue::Flag(false),
                    None,
                ),
                "records",
            );
            must_with(
                draft.record_present(
                    ObservedFieldIdentity::canonical("critic.engine"),
                    NormalizedValue::Text("native".to_string()),
                    None,
                ),
                "records",
            );
        };
        let populate_reversed = |draft: &mut ConfigurationObservationDraft| {
            must_with(
                draft.expect_canonical_fields(&[
                    "telemetry.enabled",
                    "formatting.tabs",
                    "critic.engine",
                ]),
                "known rows",
            );
            must_with(
                draft.record_present(
                    ObservedFieldIdentity::canonical("critic.engine"),
                    NormalizedValue::Text("native".to_string()),
                    None,
                ),
                "records",
            );
            must_with(
                draft.record_present(
                    ObservedFieldIdentity::canonical("formatting.tabs"),
                    NormalizedValue::Flag(false),
                    None,
                ),
                "records",
            );
            must_with(
                draft.record_present(
                    ObservedFieldIdentity::canonical("telemetry.enabled"),
                    NormalizedValue::Flag(true),
                    None,
                ),
                "records",
            );
        };
        let forward = {
            let mut draft = ConfigurationObservationDraft::new(
                subject(ObservationScope::Global),
                source(
                    ConfigurationProvenanceClass::CompiledDefault,
                    ObservationTransport::CompiledDefaultsEmitted,
                ),
            );
            populate_first(&mut draft);
            must_with(draft.finish(), "finishes")
        };
        let backward = {
            let mut draft = ConfigurationObservationDraft::new(
                subject(ObservationScope::Global),
                source(
                    ConfigurationProvenanceClass::CompiledDefault,
                    ObservationTransport::CompiledDefaultsEmitted,
                ),
            );
            populate_reversed(&mut draft);
            must_with(draft.finish(), "finishes")
        };

        assert_eq!(forward.fingerprint(), backward.fingerprint());
        assert_eq!(
            must_with(serde_json::to_vec(&forward), "serializes"),
            must_with(serde_json::to_vec(&backward), "serializes")
        );
    }

    /// Falsifier #8: credential values and private paths never enter
    /// serialized receipts, and redacted evidence contributes no content to
    /// fingerprints.
    #[test]
    fn secrets_and_private_paths_never_enter_receipts_or_fingerprints() {
        let build = |secret: &str| {
            finish(
                ConfigurationProvenanceClass::TrustedUserOrMachineAdapter,
                ObservationTransport::OperatorInvocation,
                ObservationScope::Global,
                move |draft| {
                    must_with(
                        draft.expect_canonical_fields(&[
                            "ai.api_key_env",
                            "workspace.external_include_paths",
                        ]),
                        "known rows",
                    );
                    must_with(
                        draft.record_present(
                            ObservedFieldIdentity::canonical("ai.api_key_env"),
                            NormalizedValue::Text(secret.to_string()),
                            None,
                        ),
                        "records",
                    );
                    must_with(
                        draft.record_present(
                            ObservedFieldIdentity::canonical("workspace.external_include_paths"),
                            NormalizedValue::TextList(vec![
                                "/Users/steven/private/toolchain".to_string(),
                            ]),
                            None,
                        ),
                        "records",
                    );
                },
            )
        };

        let with_secret_a = build("PERL_LSP_SUPER_SECRET_1");
        let with_secret_b = build("PERL_LSP_SUPER_SECRET_2");

        let receipt = must_with(serde_json::to_string(&with_secret_a), "serializes");
        assert!(!receipt.contains("PERL_LSP_SUPER_SECRET_1"));
        assert!(!receipt.contains("/Users/steven/private/toolchain"));
        let api_key =
            must_some_with(with_secret_a.observed_field("ai.api_key_env"), "field recorded");
        assert_eq!(api_key.normalized_value(), Some(&NormalizedValue::Redacted));
        let include_paths = must_some_with(
            with_secret_a.observed_field("workspace.external_include_paths"),
            "field recorded",
        );
        assert!(matches!(include_paths.normalized_value(), Some(NormalizedValue::DigestOnly(_))));
        // Different secrets collapse to the same redacted marker, so no
        // distinguishing credential content reaches the fingerprint.
        assert_eq!(with_secret_a.fingerprint(), with_secret_b.fingerprint());
    }

    /// Falsifier #9: unknown source classes fail closed — no canonical
    /// channel, admission never proven, limitation recorded.
    #[test]
    fn unknown_source_class_fails_closed() {
        assert_eq!(
            ConfigurationProvenanceClass::UnknownOrUnsupportedSource.canonical_source(),
            None
        );
        assert!(ConfigurationProvenanceClass::UnknownOrUnsupportedSource.fails_closed());

        let unknown = finish(
            ConfigurationProvenanceClass::UnknownOrUnsupportedSource,
            ObservationTransport::Unidentified,
            ObservationScope::Global,
            |draft| {
                must_with(draft.expect_canonical_fields(&["ai.model"]), "known row");
                must_with(
                    draft.record_present(
                        ObservedFieldIdentity::canonical("ai.model"),
                        NormalizedValue::Text("gpt-perl".to_string()),
                        None,
                    ),
                    "recording is allowed",
                );
            },
        );

        assert_eq!(
            unknown.observed_field("ai.model").map(|field| field.admission()),
            Some(SourceAuthorityAdmission::AuthorityNotProven)
        );
        assert!(unknown.limitations().any(|limitation| matches!(
            limitation,
            ConfigurationObservationLimitation::ProvenanceUnknownOrUnsupported
        )));

        // Denominator declaration itself fails closed for unknown IDs.
        let mut draft = ConfigurationObservationDraft::new(
            subject(ObservationScope::Global),
            source(
                ConfigurationProvenanceClass::UnknownOrUnsupportedSource,
                ObservationTransport::Unidentified,
            ),
        );
        assert!(matches!(
            draft.expect_canonical_fields(&["not.a.canonical.field"]),
            Err(ObservationError::UnknownCanonicalField { .. })
        ));
    }

    /// Falsifier #10: partial, envelope-malformed, and unsupported
    /// populations cannot report complete.
    #[test]
    fn incomplete_or_blocked_populations_cannot_report_complete() {
        let partial = finish(
            ConfigurationProvenanceClass::CompiledDefault,
            ObservationTransport::CompiledDefaultsEmitted,
            ObservationScope::Global,
            |draft| {
                must_with(
                    draft.expect_canonical_fields(&["telemetry.enabled", "formatting.tabs"]),
                    "known rows",
                );
                must_with(
                    draft.record_present(
                        ObservedFieldIdentity::canonical("telemetry.enabled"),
                        NormalizedValue::Flag(true),
                        None,
                    ),
                    "records",
                );
            },
        );
        assert!(matches!(
            partial.completeness(),
            ConfigurationCompleteness::Partial { expected: 2, observed: 1 }
        ));

        let envelope = finish(
            ConfigurationProvenanceClass::PerRootWorkspaceConfiguration,
            ObservationTransport::ConfigurationPullResult { request_id: "req-late".to_string() },
            ObservationScope::Root { root_identity: "root-a".to_string() },
            |draft| {
                must_with(draft.expect_canonical_fields(&["formatting.enabled"]), "known row");
                draft.mark_envelope_malformed();
                must_with(
                    draft.record_present(
                        ObservedFieldIdentity::canonical("formatting.enabled"),
                        NormalizedValue::Flag(true),
                        None,
                    ),
                    "records",
                );
            },
        );
        assert_eq!(envelope.completeness(), ConfigurationCompleteness::EnvelopeMalformed);
        assert!(envelope.limitations().any(|limitation| matches!(
            limitation,
            ConfigurationObservationLimitation::EnvelopeShapeUntrusted
        )));

        let unsupported = finish(
            ConfigurationProvenanceClass::CompiledDefault,
            ObservationTransport::CompiledDefaultsEmitted,
            ObservationScope::Global,
            |draft| {
                must_with(draft.expect_canonical_fields(&["formatting.enabled"]), "known row");
                must_with(
                    draft.record_disposition(
                        ObservedFieldIdentity::canonical("formatting.enabled"),
                        ConfigurationObservationDisposition::Unsupported,
                    ),
                    "records",
                );
            },
        );
        assert!(matches!(unsupported.completeness(), ConfigurationCompleteness::Partial { .. }));
    }

    /// Architecture law: this model contains no precedence/winner logic and
    /// touches no effective-state writer. Scanning non-comment lines keeps
    /// prose references out of the verdict; identifier-boundary matching keeps
    /// legitimate names like the per-root pull transport out of it.
    #[test]
    fn observation_model_has_no_ranking_or_effective_state_logic() {
        fn contains_identifier(line: &str, ident: &str) -> bool {
            const fn ident_byte(byte: u8) -> bool {
                byte.is_ascii_alphanumeric() || byte == b'_'
            }
            let bytes = line.as_bytes();
            let mut start = 0;
            while let Some(offset) = line[start..].find(ident) {
                let begin = start + offset;
                let end = begin + ident.len();
                let left_ok = begin == 0 || !ident_byte(bytes[begin - 1]);
                let right_ok = end == bytes.len() || !ident_byte(bytes[end]);
                if left_ok && right_ok {
                    return true;
                }
                start = begin + 1;
            }
            false
        }

        let source = include_str!("configuration_observation.rs");
        let banned_substrings = [
            concat!("max", "_by("),
            concat!("min", "_by("),
            concat!("win", "ner"),
            concat!("preced", "ence()"),
            concat!("sort", "_by("),
            concat!(".", "cmp("),
            concat!("update", "_from_value"),
            concat!("didChange", "Configuration"),
        ];
        let banned_identifiers = [concat!("Server", "Config"), concat!("Workspace", "Config")];

        let mut hits = Vec::new();
        for line in source.lines() {
            if line.trim_start().starts_with("//") {
                continue;
            }
            for token in banned_substrings {
                if line.contains(token) {
                    hits.push(format!("{token} <= {line}"));
                }
            }
            for ident in banned_identifiers {
                if contains_identifier(line, ident) {
                    hits.push(format!("{ident} <= {line}"));
                }
            }
        }
        assert!(hits.is_empty(), "forbidden logic surfaced: {hits:#?}");
    }

    /// Deterministic schema fixtures: two independent generations serialize
    /// byte-identically and pin the wire format. Construction is
    /// builder-sealed — the observation type deliberately derives no
    /// `Deserialize`, so serialized bytes cannot reconstruct asserted state.
    #[test]
    fn deterministic_serde_fixtures_are_byte_stable_across_generations() {
        let generate = || {
            finish(
                ConfigurationProvenanceClass::CompiledDefault,
                ObservationTransport::CompiledDefaultsEmitted,
                ObservationScope::Global,
                |draft| {
                    must_with(draft.expect_canonical_fields(&["telemetry.enabled"]), "known row");
                    must_with(
                        draft.record_present(
                            ObservedFieldIdentity::canonical("telemetry.enabled"),
                            NormalizedValue::Flag(true),
                            None,
                        ),
                        "records",
                    );
                },
            )
        };

        let first = generate();
        let second = generate();

        let first_bytes = must_with(serde_json::to_vec_pretty(&first), "serializes");
        let second_bytes = must_with(serde_json::to_vec_pretty(&second), "serializes");
        assert_eq!(first_bytes, second_bytes, "second generation must be byte-identical");

        // Byte-stability is one-directional: these bytes are an emitted
        // receipt, not a construction input. `ConfigurationObservation`
        // derives no Deserialize (see type doc).
        let golden = must_with(std::str::from_utf8(&first_bytes), "utf8 fixture");
        pin_golden_fixture(golden);
    }

    /// Falsifier #11: client labels stay bounded diagnostics — oversized or
    /// excessive label material is rejected, never serialized verbatim.
    #[test]
    fn client_labels_are_bounded_diagnostics() {
        let base = must_with(
            ConfigurationSourceIdentity::new(
                "perl-lsp-rs-core/test",
                OBSERVATION_SCHEMA_GENERATION,
                ConfigurationProvenanceClass::GenericUnscopedClient,
                ObservationTransport::InitializeSession { session_id: "s-9".to_string() },
            ),
            "valid identity",
        );

        let oversized_key = base.clone().with_client_label("k".repeat(MAX_LABEL_CHARS + 1), "v");
        let oversized_value = base.clone().with_client_label("k", "v".repeat(MAX_LABEL_CHARS + 1));
        let empty_key = base.clone().with_client_label("", "v");
        assert!(matches!(oversized_key, Err(ObservationError::LabelOutOfBounds)));
        assert!(matches!(oversized_value, Err(ObservationError::LabelOutOfBounds)));
        assert!(matches!(empty_key, Err(ObservationError::LabelOutOfBounds)));

        let saturated = (0..MAX_CLIENT_LABELS).fold(base.clone(), |identity, index| {
            must_with(identity.with_client_label(format!("key-{index}"), "v"), "bounded")
        });
        assert!(matches!(
            saturated.with_client_label("one-too-many", "v"),
            Err(ObservationError::TooManyClientLabels { .. })
        ));
    }

    /// Falsifier #12: long identities that share a prefix beyond any legacy
    /// truncation window still produce distinct observations. Fingerprint
    /// digests are algorithm-tagged SHA-256 over complete length-prefixed
    /// material (`sha256:` prefix), never truncated byte prefixes.
    #[test]
    fn same_length_same_prefix_long_identities_remain_distinct() {
        let root_for = |suffix: &str| {
            finish(
                ConfigurationProvenanceClass::PerRootWorkspaceConfiguration,
                ObservationTransport::ConfigurationPullResult { request_id: "req-l".to_string() },
                ObservationScope::Root { root_identity: format!("{}-{suffix}", "r".repeat(36)) },
                |draft| {
                    must_with(draft.expect_canonical_fields(&["workspace.include_paths"]), "row");
                    must_with(
                        draft.record_present(
                            ObservedFieldIdentity::canonical("workspace.include_paths"),
                            NormalizedValue::TextList(vec!["lib".to_string()]),
                            None,
                        ),
                        "records",
                    );
                },
            )
        };
        // Same length, same first bytes well past any truncation boundary,
        // differing only in the suffix.
        assert_ne!(root_for("aaaa").fingerprint(), root_for("bbbb").fingerprint());
    }

    /// Falsifier #13: unmodeled external facts cannot weaken evidence to a
    /// raw-value policy, digest-only policy keeps distinguishing collision-
    /// resistant digests, and redaction alone collapses to a fixed marker.
    #[test]
    fn unmodeled_evidence_floor_and_digest_only_identity() {
        let weak_source = source(
            ConfigurationProvenanceClass::ProcessEnvironment,
            ObservationTransport::EnvironmentVariable { name: "PERL5LIB".to_string() },
        );

        // Sensitivity floor: raw-value policies are authority-reserved.
        let mut safe_attempt = ConfigurationObservationDraft::new(
            subject(ObservationScope::Global),
            weak_source.clone(),
        );
        assert!(matches!(
            safe_attempt.record_present(
                ObservedFieldIdentity::unmodeled("PERL5LIB"),
                NormalizedValue::Text("/opt/site/lib".to_string()),
                Some(EvidencePolicy::SafeValue),
            ),
            Err(ObservationError::UnsupportedEvidencePolicy { .. })
        ));

        let mut bounded_attempt = ConfigurationObservationDraft::new(
            subject(ObservationScope::Global),
            weak_source.clone(),
        );
        assert!(matches!(
            bounded_attempt.record_present(
                ObservedFieldIdentity::unmodeled("PERL5LIB"),
                NormalizedValue::Count(3),
                Some(EvidencePolicy::BoundedValue),
            ),
            Err(ObservationError::UnsupportedEvidencePolicy { .. })
        ));

        // Digest-only evidence is accepted for the rejected key after the
        // failed recording and keeps its distinguishing algorithm-tagged
        // digest.
        let with_digest = finish(
            ConfigurationProvenanceClass::ProcessEnvironment,
            ObservationTransport::EnvironmentVariable { name: "PERL5LIB".to_string() },
            ObservationScope::Global,
            |draft| {
                must_with(
                    draft.record_present(
                        ObservedFieldIdentity::unmodeled("PERL5LIB"),
                        NormalizedValue::Text("/opt/site/lib".to_string()),
                        Some(EvidencePolicy::DerivedDigestOnly),
                    ),
                    "digest-only allowed",
                );
            },
        );
        let left_value =
            with_digest.observed_field("PERL5LIB").and_then(|field| field.normalized_value());
        let left_message = format!("expected digest-only evidence, got {left_value:?}");
        let left_digest = match left_value {
            Some(NormalizedValue::DigestOnly(digest)) => digest,
            _ => must_some_with(None, left_message),
        };

        let distinct = finish(
            ConfigurationProvenanceClass::ProcessEnvironment,
            ObservationTransport::EnvironmentVariable { name: "PERL5LIB".to_string() },
            ObservationScope::Global,
            |draft| {
                must_with(
                    draft.record_present(
                        ObservedFieldIdentity::unmodeled("PERL5LIB"),
                        NormalizedValue::Text("/different/private/lib".to_string()),
                        Some(EvidencePolicy::DerivedDigestOnly),
                    ),
                    "digest-only allowed",
                );
            },
        );
        let right_value =
            distinct.observed_field("PERL5LIB").and_then(|field| field.normalized_value());
        let right_message = format!("expected digest-only evidence, got {right_value:?}");
        let right_digest = match right_value {
            Some(NormalizedValue::DigestOnly(digest)) => digest,
            _ => must_some_with(None, right_message),
        };
        assert_ne!(left_digest, right_digest);
        assert!(left_digest.starts_with("sha256:"));
    }

    /// Falsifier #14: the declared denominator rejects duplicate IDs (which
    /// would silently skew completeness counts) and obeys the field-count
    /// bound.
    #[test]
    fn denominator_rejects_duplicate_ids_and_binds_size() {
        let mut draft = ConfigurationObservationDraft::new(
            subject(ObservationScope::Global),
            source(
                ConfigurationProvenanceClass::CompiledDefault,
                ObservationTransport::CompiledDefaultsEmitted,
            ),
        );
        let attempted = draft.expect_canonical_fields(&["ai.model", "ai.model"]);
        assert!(matches!(
            attempted,
            Err(ObservationError::DuplicateCanonicalField { id }) if id == "ai.model",
        ));

        let oversized: Vec<&str> =
            std::iter::repeat_n("x", MAX_FIELDS_PER_OBSERVATION + 1).collect();
        assert!(matches!(
            draft.expect_canonical_fields(&oversized),
            Err(ObservationError::TooManyFields { .. })
        ));
    }

    /// Falsifier #15: a recorded field can never be silently overwritten by a
    /// later recording, in either present/disposition permutation.
    #[test]
    fn duplicate_field_records_are_rejected_in_any_permutation() {
        let mut first = ConfigurationObservationDraft::new(
            subject(ObservationScope::Global),
            source(
                ConfigurationProvenanceClass::TrustedUserOrMachineAdapter,
                ObservationTransport::OperatorInvocation,
            ),
        );
        must_with(
            first.record_present(
                ObservedFieldIdentity::canonical("ai.model"),
                NormalizedValue::Text("native".to_string()),
                None,
            ),
            "first recording lands",
        );
        let overwritten = first.record_disposition(
            ObservedFieldIdentity::canonical("ai.model"),
            ConfigurationObservationDisposition::InstrumentFailure,
        );
        assert!(matches!(overwritten, Err(ObservationError::DuplicateObservation { .. })));
        let preserved = must_with(first.finish(), "finishes");
        assert_eq!(
            preserved.observed_field("ai.model").map(|field| field.disposition()),
            Some(ConfigurationObservationDisposition::Present)
        );

        let mut second = ConfigurationObservationDraft::new(
            subject(ObservationScope::Global),
            source(
                ConfigurationProvenanceClass::TrustedUserOrMachineAdapter,
                ObservationTransport::OperatorInvocation,
            ),
        );
        must_with(
            second.record_disposition(
                ObservedFieldIdentity::canonical("ai.model"),
                ConfigurationObservationDisposition::Unavailable,
            ),
            "disposition lands",
        );
        let overwrite_value = second.record_present(
            ObservedFieldIdentity::canonical("ai.model"),
            NormalizedValue::Text("late".to_string()),
            None,
        );
        assert!(matches!(overwrite_value, Err(ObservationError::DuplicateObservation { .. })));
    }

    /// Falsifier #16: non-present recordings reject unknown canonical IDs,
    /// exactly like value recordings; only Unmodeled may carry external
    /// markers.
    #[test]
    fn disposition_unknown_canonical_field_fails_closed() {
        let mut draft = ConfigurationObservationDraft::new(
            subject(ObservationScope::Global),
            source(
                ConfigurationProvenanceClass::GenericUnscopedClient,
                ObservationTransport::ConfigurationPullResult { request_id: "req-x".to_string() },
            ),
        );
        let attempted = draft.record_disposition(
            ObservedFieldIdentity::canonical("not.a.canonical.field"),
            ConfigurationObservationDisposition::Absent,
        );
        assert!(matches!(
            attempted,
            Err(ObservationError::UnknownCanonicalField { id }) if id == "not.a.canonical.field"
        ));
    }

    /// Falsifier #17: completeness counts describe denominator coverage, and
    /// rows beyond the denominator are disclosed as their own limitation
    /// instead of inflating coverage or forcing partial populations.
    #[test]
    fn completeness_counts_coverage_and_discloses_extras() {
        let extras = finish(
            ConfigurationProvenanceClass::PerRootWorkspaceConfiguration,
            ObservationTransport::ConfigurationPullResult { request_id: "req-e".to_string() },
            ObservationScope::Root { root_identity: "root-a".to_string() },
            |draft| {
                must_with(draft.expect_canonical_fields(&["workspace.include_paths"]), "row");
                must_with(
                    draft.record_present(
                        ObservedFieldIdentity::canonical("workspace.include_paths"),
                        NormalizedValue::TextList(vec!["lib".to_string()]),
                        None,
                    ),
                    "records",
                );
                for marker in ["PERL5LIB", "PERLLIB", "PERL5OPT"] {
                    must_with(
                        draft.record_present(
                            ObservedFieldIdentity::unmodeled(marker),
                            NormalizedValue::Text("present".to_string()),
                            Some(EvidencePolicy::DerivedDigestOnly),
                        ),
                        "floor-compliant evidence",
                    );
                }
            },
        );
        assert!(matches!(
            extras.completeness(),
            ConfigurationCompleteness::Complete { expected: 1, observed: 1 }
        ));
        assert!(extras.limitations().any(|limitation| matches!(
            limitation,
            ConfigurationObservationLimitation::PopulationBeyondDenominator
        )));

        let sparse = finish(
            ConfigurationProvenanceClass::CompiledDefault,
            ObservationTransport::CompiledDefaultsEmitted,
            ObservationScope::Global,
            |draft| {
                must_with(
                    draft.expect_canonical_fields(&["telemetry.enabled", "formatting.tabs"]),
                    "rows",
                );
                must_with(
                    draft.record_present(
                        ObservedFieldIdentity::canonical("formatting.tabs"),
                        NormalizedValue::Flag(false),
                        None,
                    ),
                    "records",
                );
            },
        );
        assert!(matches!(
            sparse.completeness(),
            ConfigurationCompleteness::Partial { expected: 2, observed: 1 }
        ));
    }

    /// Falsifier #18: mechanically decidable catalog rules execute during
    /// recording, producing typed malformed outcomes instead of admitting
    /// values the authority would reject.
    #[test]
    fn mechanical_catalog_validation_executes_on_recording() {
        let mut draft = ConfigurationObservationDraft::new(
            subject(ObservationScope::Global),
            source(
                ConfigurationProvenanceClass::TrustedUserOrMachineAdapter,
                ObservationTransport::OperatorInvocation,
            ),
        );

        // ai.api_key_env carries Validation::NonEmptyString: an empty string
        // is a typed malformed value, not an admitted secret slot.
        let empty_credential = draft.record_present(
            ObservedFieldIdentity::canonical("ai.api_key_env"),
            NormalizedValue::Text(String::new()),
            None,
        );
        assert!(matches!(
            empty_credential,
            Err(ObservationError::MalformedValue { reason: MalformedReason::WrongShape, .. })
        ));

        // ai.max_inflight carries Validation::UnsignedRange {1..=64}.
        let zero = draft.record_present(
            ObservedFieldIdentity::canonical("ai.max_inflight"),
            NormalizedValue::Count(0),
            None,
        );
        let above = draft.record_present(
            ObservedFieldIdentity::canonical("ai.max_inflight"),
            NormalizedValue::Count(65),
            None,
        );
        let ceiling = draft.record_present(
            ObservedFieldIdentity::canonical("ai.max_inflight"),
            NormalizedValue::Count(64),
            None,
        );
        assert!(matches!(
            zero,
            Err(ObservationError::MalformedValue { reason: MalformedReason::OutOfRange, .. })
        ));
        assert!(matches!(
            above,
            Err(ObservationError::MalformedValue { reason: MalformedReason::OutOfRange, .. })
        ));
        must_with(ceiling, "boundary value within range records");
    }

    /// Interactive validation remains outside this mechanically decidable
    /// layer: an authority-compatible empty KnownEnum value records here so
    /// the generation/runtime pipeline can validate its spelling (#7057).
    #[test]
    fn mechanical_validation_does_not_claim_interactive_rules() -> anyhow::Result<()> {
        let mut draft = try_draft(
            ObservationScope::Global,
            ConfigurationProvenanceClass::TrustedUserOrMachineAdapter,
            ObservationTransport::OperatorInvocation,
        )?;

        draft
            .record_present(
                ObservedFieldIdentity::canonical("ai.provider"),
                NormalizedValue::Text(String::new()),
                None,
            )
            .map_err(|error| {
                anyhow::anyhow!("KnownEnum validation must remain interactive: {error:?}")
            })?;
        Ok(())
    }

    /// Falsifier #19: unmodeled external markers cannot occupy declared
    /// canonical denominator slots, in either recording path.
    #[test]
    fn unmodeled_markers_cannot_occupy_denominator_slots() {
        let mut draft = ConfigurationObservationDraft::new(
            subject(ObservationScope::Global),
            source(
                ConfigurationProvenanceClass::PerRootWorkspaceConfiguration,
                ObservationTransport::ConfigurationPullResult { request_id: "req-n".to_string() },
            ),
        );
        must_with(draft.expect_canonical_fields(&["workspace.include_paths"]), "known row");
        let impersonating_value = draft.record_present(
            ObservedFieldIdentity::unmodeled("workspace.include_paths"),
            NormalizedValue::Text("/opt/site/lib".to_string()),
            Some(EvidencePolicy::DerivedDigestOnly),
        );
        assert!(matches!(
            impersonating_value,
            Err(ObservationError::MarkerInsideDenominator { marker }) if marker == "workspace.include_paths",
        ));
        let impersonating_disposition = draft.record_disposition(
            ObservedFieldIdentity::unmodeled("workspace.include_paths"),
            ConfigurationObservationDisposition::Absent,
        );
        assert!(matches!(
            impersonating_disposition,
            Err(ObservationError::MarkerInsideDenominator { .. })
        ));

        // The genuine canonical row still covers the slot afterwards.
        must_with(
            draft.record_present(
                ObservedFieldIdentity::canonical("workspace.include_paths"),
                NormalizedValue::TextList(vec!["lib".to_string()]),
                None,
            ),
            "canonical recording lands",
        );
        let sealed = must_with(draft.finish(), "finishes");
        assert!(matches!(
            sealed.completeness(),
            ConfigurationCompleteness::Complete { expected: 1, observed: 1 }
        ));
    }

    /// Falsifier #20: malformed reasons are part of the observation identity;
    /// WrongShape and OutOfRange never share a fingerprint.
    #[test]
    fn malformed_reason_joins_fingerprint_identity() {
        let record = |reason| {
            finish(
                ConfigurationProvenanceClass::GenericUnscopedClient,
                ObservationTransport::ConfigurationPullResult { request_id: "req-r".to_string() },
                ObservationScope::Global,
                move |draft| {
                    must_with(draft.expect_canonical_fields(&["formatting.enabled"]), "known row");
                    must_with(
                        draft.record_disposition(
                            ObservedFieldIdentity::canonical("formatting.enabled"),
                            ConfigurationObservationDisposition::Malformed { reason },
                        ),
                        "disposition records",
                    );
                },
            )
        };
        assert_ne!(
            record(MalformedReason::WrongShape).fingerprint(),
            record(MalformedReason::OutOfRange).fingerprint()
        );
    }

    /// Falsifier #21: the remaining mechanically decidable rules execute —
    /// OptionalNonEmptyString rejects empty text and StringList rejects empty
    /// list items, both as typed malformed outcomes.
    #[test]
    fn optional_and_list_validation_arms_execute() -> anyhow::Result<()> {
        let mut project = try_draft(
            ObservationScope::Root { root_identity: "root-a".to_string() },
            ConfigurationProvenanceClass::ProjectFile,
            ObservationTransport::ProjectFileRead {
                path_digest: "digest:.perl-lsp.toml".to_string(),
            },
        )?;
        let empty_optional = project.record_present(
            ObservedFieldIdentity::canonical("critic.legacy_profile"),
            NormalizedValue::Text(String::new()),
            None,
        );
        anyhow::ensure!(
            matches!(
                empty_optional,
                Err(ObservationError::MalformedValue { reason: MalformedReason::WrongShape, .. })
            ),
            "empty OptionalNonEmptyString must be rejected as WrongShape"
        );
        // Presence of an actual value is unaffected; absence travels through
        // the Absent disposition instead of an empty string.
        project
            .record_present(
                ObservedFieldIdentity::canonical("critic.legacy_profile"),
                NormalizedValue::Text("legacy".to_string()),
                None,
            )
            .map_err(|error| anyhow::anyhow!("recording non-empty optional text: {error:?}"))?;

        let mut folder = try_draft(
            ObservationScope::Global,
            ConfigurationProvenanceClass::CompiledDefault,
            ObservationTransport::CompiledDefaultsEmitted,
        )?;
        let empty_item = folder.record_present(
            ObservedFieldIdentity::canonical("critic.exclude"),
            NormalizedValue::TextList(vec!["benchi".to_string(), String::new()]),
            None,
        );
        anyhow::ensure!(
            matches!(
                empty_item,
                Err(ObservationError::MalformedValue { reason: MalformedReason::WrongShape, .. })
            ),
            "StringList with an empty item must be rejected as WrongShape"
        );
        folder
            .record_present(
                ObservedFieldIdentity::canonical("critic.exclude"),
                NormalizedValue::TextList(vec!["benchi".to_string(), "tidy".to_string()]),
                None,
            )
            .map_err(|error| anyhow::anyhow!("recording non-empty StringList: {error:?}"))?;
        Ok(())
    }

    /// Falsifier #22: the landed project-metadata channel maps to its own
    /// provenance class, admitting derived dependency rows without falsely
    /// labeling their source.
    #[test]
    fn project_metadata_channel_admits_declared_dependencies() {
        let metadata = finish(
            ConfigurationProvenanceClass::ProjectMetadata,
            ObservationTransport::FeatureInternalState,
            ObservationScope::Global,
            |draft| {
                must_with(
                    draft.expect_canonical_fields(&["workspace.declared_dependencies"]),
                    "row",
                );
                must_with(
                    draft.record_present(
                        ObservedFieldIdentity::canonical("workspace.declared_dependencies"),
                        NormalizedValue::TextList(vec!["Plack".to_string()]),
                        None,
                    ),
                    "metadata channel admits declared dependencies",
                );
            },
        );
        assert_eq!(metadata.provenance(), ConfigurationProvenanceClass::ProjectMetadata);
        let row =
            must_some_with(metadata.observed_field("workspace.declared_dependencies"), "recorded");
        assert_eq!(row.admission(), SourceAuthorityAdmission::CandidateAdmitted);
        assert!(matches!(row.normalized_value(), Some(NormalizedValue::DigestOnly(_))));
    }

    /// Pins the current fixture format; any schema-visible change must update
    /// this deliberately.
    fn pin_golden_fixture(actual: &str) {
        let expected = GOLDEN_COMPILED_DEFAULT_FIXTURE.trim();
        assert_eq!(actual.trim(), expected, "observation fixture drift");
    }

    const GOLDEN_COMPILED_DEFAULT_FIXTURE: &str = r#"{
  "schema_generation": 2,
  "subject": {
    "observation_id": "obs-fixture",
    "scope": "Global",
    "runtime_generation": 1,
    "configuration_generation": 1,
    "trust_generation": 1
  },
  "source": {
    "producer_id": "perl-lsp-rs-core/test",
    "schema_generation": 2,
    "provenance": "CompiledDefault",
    "transport": "CompiledDefaultsEmitted",
    "client_declared_labels": {}
  },
  "expected_denominator": [
    "telemetry.enabled"
  ],
  "fields": {
    "telemetry.enabled": {
      "identity": {
        "Canonical": {
          "id": "telemetry.enabled"
        }
      },
      "disposition": "Present",
      "normalized_value": {
        "Flag": true
      },
      "admission": "CandidateAdmitted",
      "validation": "Boolean",
      "evidence_policy": "SafeValue",
      "limitations": []
    }
  },
  "completeness": {
    "Complete": {
      "expected": 1,
      "observed": 1
    }
  },
  "limitations": []
}
"#;
}
