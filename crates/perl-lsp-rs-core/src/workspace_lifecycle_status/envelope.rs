//! Immutable row/envelope types, checked constructor, and fail-closed validator.

use serde::{Deserialize, Deserializer, Serialize};

use super::ids::{
    DimensionProvenance, LogicalToken, ProducerIdentity, StatusFingerprint,
    WorkspaceLifecycleSubject, check_bound, envelope_row_limit, history_limit,
    operation_link_limit, reason_limit,
};
use super::vocab::{
    Completeness, ConfigurationState, HydrationState, InstrumentState, LocalDiagnosticAction,
    PrimaryActionKind, PrimaryActionPriorityPolicy, PrivacyClass, ProviderPosture, ReadinessState,
    ResponsibleOwnerRole, RootLifecycleState, StorageState, WatchReloadState,
};
use super::{
    FINGERPRINT_DOMAIN, MAX_LIMITATIONS_PER_ROW, ValidationError,
    WorkspaceLifecycleStatusSchemaVersion,
};

/// Independent dimension observation. Missing never deserializes as a healthy default.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum StatusDimension<T> {
    /// Current typed observation with retained source identity and generations.
    Observed {
        /// Exact subject/source provenance for this observation.
        provenance: DimensionProvenance,
        /// Domain value. Independent of every other dimension.
        value: T,
    },
    /// Dimension was not proven. Never coerced to ready/current/zero.
    NotProven,
    /// Instrumentation failed to observe this dimension.
    InstrumentFailed {
        /// Instrument identity that failed. Not a domain state.
        instrument_id: LogicalToken,
    },
}

impl<T> StatusDimension<T> {
    /// True when this dimension is an explicit instrument failure.
    #[must_use]
    pub const fn is_instrument_failed(&self) -> bool {
        matches!(self, Self::InstrumentFailed { .. })
    }

    /// True when this dimension is explicitly not proven.
    #[must_use]
    pub const fn is_not_proven(&self) -> bool {
        matches!(self, Self::NotProven)
    }
}

impl<T: Copy> StatusDimension<T> {
    pub(super) fn observed_value(&self) -> Option<T> {
        match self {
            Self::Observed { value, .. } => Some(*value),
            Self::NotProven | Self::InstrumentFailed { .. } => None,
        }
    }
}

/// Three independent readiness scopes. None is filled from storage or hydration.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReadinessObservation {
    active_document: StatusDimension<ReadinessState>,
    dependency_neighborhood: StatusDimension<ReadinessState>,
    whole_workspace: StatusDimension<ReadinessState>,
}

impl ReadinessObservation {
    /// Bind the three independent readiness scopes.
    pub fn new(
        active_document: StatusDimension<ReadinessState>,
        dependency_neighborhood: StatusDimension<ReadinessState>,
        whole_workspace: StatusDimension<ReadinessState>,
    ) -> Self {
        Self { active_document, dependency_neighborhood, whole_workspace }
    }

    /// Active-document readiness.
    #[must_use]
    pub const fn active_document(&self) -> &StatusDimension<ReadinessState> {
        &self.active_document
    }

    /// Dependency-neighborhood readiness.
    #[must_use]
    pub const fn dependency_neighborhood(&self) -> &StatusDimension<ReadinessState> {
        &self.dependency_neighborhood
    }

    /// Whole-workspace readiness.
    #[must_use]
    pub const fn whole_workspace(&self) -> &StatusDimension<ReadinessState> {
        &self.whole_workspace
    }
}

/// Typed repair projection. Reason and action are closed vocabularies, not prose.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RepairProjection {
    reason_code: LogicalToken,
    owner_role: ResponsibleOwnerRole,
    primary_action: PrimaryActionKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    local_diagnostic: Option<LocalDiagnosticAction>,
}

impl RepairProjection {
    /// Bind one typed repair projection.
    pub fn new(
        reason_code: LogicalToken,
        owner_role: ResponsibleOwnerRole,
        primary_action: PrimaryActionKind,
        local_diagnostic: Option<LocalDiagnosticAction>,
    ) -> Self {
        Self { reason_code, owner_role, primary_action, local_diagnostic }
    }

    /// Stable reason code.
    #[must_use]
    pub const fn reason_code(&self) -> &LogicalToken {
        &self.reason_code
    }

    /// Responsible domain/owner role.
    #[must_use]
    pub const fn owner_role(&self) -> ResponsibleOwnerRole {
        self.owner_role
    }

    /// Primary action kind.
    #[must_use]
    pub const fn primary_action(&self) -> PrimaryActionKind {
        self.primary_action
    }

    /// Optional local diagnostic/export action.
    #[must_use]
    pub const fn local_diagnostic(&self) -> Option<LocalDiagnosticAction> {
        self.local_diagnostic
    }
}

/// Completeness, instrument state, privacy class, and retained-history bound.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StatusMetadata {
    completeness: Completeness,
    instrument_state: InstrumentState,
    privacy_class: PrivacyClass,
    history_retained: u16,
}

impl StatusMetadata {
    /// Bind row metadata. History is a count, never event bodies.
    pub fn new(
        completeness: Completeness,
        instrument_state: InstrumentState,
        privacy_class: PrivacyClass,
        history_retained: u16,
    ) -> Result<Self, ValidationError> {
        check_bound(
            "history_retained",
            usize::from(history_retained),
            usize::from(history_limit()),
        )?;
        Ok(Self { completeness, instrument_state, privacy_class, history_retained })
    }

    /// Completeness classification.
    #[must_use]
    pub const fn completeness(&self) -> Completeness {
        self.completeness
    }

    /// Instrument state.
    #[must_use]
    pub const fn instrument_state(&self) -> InstrumentState {
        self.instrument_state
    }

    /// Privacy class.
    #[must_use]
    pub const fn privacy_class(&self) -> PrivacyClass {
        self.privacy_class
    }

    /// Bound on retained history entries (count only).
    #[must_use]
    pub const fn history_retained(&self) -> u16 {
        self.history_retained
    }
}

/// Checked construction parts for one root/runtime status row.
#[derive(Debug, Clone)]
pub struct WorkspaceLifecycleStatusRowParts {
    /// Exact subject identities and generations.
    pub subject: WorkspaceLifecycleSubject,
    /// Root lifecycle dimension.
    pub root_lifecycle: StatusDimension<RootLifecycleState>,
    /// Configuration dimension.
    pub configuration: StatusDimension<ConfigurationState>,
    /// Watch/reload dimension.
    pub watch_reload: StatusDimension<WatchReloadState>,
    /// Three independent readiness scopes.
    pub readiness: ReadinessObservation,
    /// Storage administration dimension.
    pub storage: StatusDimension<StorageState>,
    /// Hydration/adoption dimension.
    pub hydration: StatusDimension<HydrationState>,
    /// Provider posture dimension.
    pub provider_posture: StatusDimension<ProviderPosture>,
    /// Typed repair projection.
    pub repair: RepairProjection,
    /// Bounded stable reason codes.
    pub reasons: Vec<LogicalToken>,
    /// Bounded limitation codes.
    pub limitations: Vec<LogicalToken>,
    /// Bounded operation/evidence links.
    pub operation_links: Vec<LogicalToken>,
    /// Completeness, instrument, privacy, history bound.
    pub metadata: StatusMetadata,
}

/// One immutable status row for one exact root/runtime subject.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
pub struct WorkspaceLifecycleStatusRow {
    subject: WorkspaceLifecycleSubject,
    root_lifecycle: StatusDimension<RootLifecycleState>,
    configuration: StatusDimension<ConfigurationState>,
    watch_reload: StatusDimension<WatchReloadState>,
    readiness: ReadinessObservation,
    storage: StatusDimension<StorageState>,
    hydration: StatusDimension<HydrationState>,
    provider_posture: StatusDimension<ProviderPosture>,
    repair: RepairProjection,
    reasons: Vec<LogicalToken>,
    limitations: Vec<LogicalToken>,
    operation_links: Vec<LogicalToken>,
    metadata: StatusMetadata,
}

impl WorkspaceLifecycleStatusRow {
    /// Checked constructor for one row.
    pub fn from_parts(
        mut parts: WorkspaceLifecycleStatusRowParts,
    ) -> Result<Self, ValidationError> {
        canonicalize_tokens(&mut parts.reasons);
        canonicalize_tokens(&mut parts.limitations);
        canonicalize_tokens(&mut parts.operation_links);
        validate_row_shape(&parts)?;
        Ok(Self {
            subject: parts.subject,
            root_lifecycle: parts.root_lifecycle,
            configuration: parts.configuration,
            watch_reload: parts.watch_reload,
            readiness: parts.readiness,
            storage: parts.storage,
            hydration: parts.hydration,
            provider_posture: parts.provider_posture,
            repair: parts.repair,
            reasons: parts.reasons,
            limitations: parts.limitations,
            operation_links: parts.operation_links,
            metadata: parts.metadata,
        })
    }

    /// Exact subject.
    #[must_use]
    pub const fn subject(&self) -> &WorkspaceLifecycleSubject {
        &self.subject
    }

    /// Root lifecycle dimension.
    #[must_use]
    pub const fn root_lifecycle(&self) -> &StatusDimension<RootLifecycleState> {
        &self.root_lifecycle
    }

    /// Configuration dimension.
    #[must_use]
    pub const fn configuration(&self) -> &StatusDimension<ConfigurationState> {
        &self.configuration
    }

    /// Watch/reload dimension.
    #[must_use]
    pub const fn watch_reload(&self) -> &StatusDimension<WatchReloadState> {
        &self.watch_reload
    }

    /// Readiness scopes.
    #[must_use]
    pub const fn readiness(&self) -> &ReadinessObservation {
        &self.readiness
    }

    /// Storage dimension.
    #[must_use]
    pub const fn storage(&self) -> &StatusDimension<StorageState> {
        &self.storage
    }

    /// Hydration dimension.
    #[must_use]
    pub const fn hydration(&self) -> &StatusDimension<HydrationState> {
        &self.hydration
    }

    /// Provider posture dimension.
    #[must_use]
    pub const fn provider_posture(&self) -> &StatusDimension<ProviderPosture> {
        &self.provider_posture
    }

    /// Repair projection.
    #[must_use]
    pub const fn repair(&self) -> &RepairProjection {
        &self.repair
    }

    /// Stable reason codes.
    #[must_use]
    pub fn reasons(&self) -> &[LogicalToken] {
        &self.reasons
    }

    /// Limitation codes.
    #[must_use]
    pub fn limitations(&self) -> &[LogicalToken] {
        &self.limitations
    }

    /// Operation/evidence links.
    #[must_use]
    pub fn operation_links(&self) -> &[LogicalToken] {
        &self.operation_links
    }

    /// Status metadata.
    #[must_use]
    pub const fn metadata(&self) -> &StatusMetadata {
        &self.metadata
    }

    fn fingerprint_parts(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        push_token(&mut buf, self.subject.session_id());
        push_token(&mut buf, self.subject.workspace_id());
        push_token(&mut buf, self.subject.root_id());
        push_token(&mut buf, self.subject.runtime_generation());
        push_token(&mut buf, self.subject.configuration_generation());
        push_token(&mut buf, self.subject.trust_generation());
        push_token(&mut buf, self.subject.environment_identity());
        push_dimension(
            &mut buf,
            "root_lifecycle",
            &self.root_lifecycle,
            RootLifecycleState::as_str,
        );
        push_dimension(&mut buf, "configuration", &self.configuration, ConfigurationState::as_str);
        push_dimension(&mut buf, "watch_reload", &self.watch_reload, WatchReloadState::as_str);
        push_dimension(
            &mut buf,
            "readiness.active_document",
            &self.readiness.active_document,
            ReadinessState::as_str,
        );
        push_dimension(
            &mut buf,
            "readiness.dependency_neighborhood",
            &self.readiness.dependency_neighborhood,
            ReadinessState::as_str,
        );
        push_dimension(
            &mut buf,
            "readiness.whole_workspace",
            &self.readiness.whole_workspace,
            ReadinessState::as_str,
        );
        push_dimension(&mut buf, "storage", &self.storage, StorageState::as_str);
        push_dimension(&mut buf, "hydration", &self.hydration, HydrationState::as_str);
        push_dimension(
            &mut buf,
            "provider_posture",
            &self.provider_posture,
            ProviderPosture::as_str,
        );
        push_token(&mut buf, self.repair.reason_code());
        push_bytes(&mut buf, self.repair.owner_role().as_str().as_bytes());
        push_bytes(&mut buf, self.repair.primary_action().as_str().as_bytes());
        match self.repair.local_diagnostic() {
            Some(action) => push_bytes(&mut buf, action.as_str().as_bytes()),
            None => push_bytes(&mut buf, b"none"),
        }
        push_token_list(&mut buf, b"reasons", &self.reasons);
        push_token_list(&mut buf, b"limitations", &self.limitations);
        push_token_list(&mut buf, b"operation_links", &self.operation_links);
        push_bytes(&mut buf, self.metadata.completeness().as_str().as_bytes());
        push_bytes(&mut buf, self.metadata.instrument_state().as_str().as_bytes());
        push_bytes(&mut buf, self.metadata.privacy_class().as_str().as_bytes());
        push_bytes(&mut buf, &self.metadata.history_retained().to_be_bytes());
        buf
    }
}

/// Checked construction parts for one versioned status envelope.
#[derive(Debug, Clone)]
pub struct WorkspaceLifecycleStatusParts {
    /// Producing implementation identity.
    pub producer: ProducerIdentity,
    /// Typed primary-action priority policy input.
    pub primary_action_priority_policy: PrimaryActionPriorityPolicy,
    /// One row per exact root/runtime subject.
    pub rows: Vec<WorkspaceLifecycleStatusRow>,
}

/// Versioned, transport-neutral, immutable `workspace_lifecycle_status.v1` envelope.
///
/// Construct through [`WorkspaceLifecycleStatusV1::from_parts`] or deserialize
/// through the same validator. There is no `Default`, no mutation, and no live
/// workspace read.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
pub struct WorkspaceLifecycleStatusV1 {
    schema_version: WorkspaceLifecycleStatusSchemaVersion,
    producer: ProducerIdentity,
    primary_action_priority_policy: PrimaryActionPriorityPolicy,
    rows: Vec<WorkspaceLifecycleStatusRow>,
    fingerprint: StatusFingerprint,
}

impl WorkspaceLifecycleStatusV1 {
    /// Checked constructor. Rows are canonically ordered; identity ignores
    /// construction insertion order.
    pub fn from_parts(parts: WorkspaceLifecycleStatusParts) -> Result<Self, ValidationError> {
        let mut rows = parts.rows;
        rows.sort_by(|left, right| {
            left.subject
                .root_id()
                .cmp(right.subject.root_id())
                .then(left.subject.runtime_generation().cmp(right.subject.runtime_generation()))
        });
        validate_envelope_shape(WorkspaceLifecycleStatusSchemaVersion::V1, &rows)?;
        let fingerprint = fingerprint_over(
            WorkspaceLifecycleStatusSchemaVersion::V1,
            &parts.producer,
            parts.primary_action_priority_policy,
            &rows,
        );
        Ok(Self {
            schema_version: WorkspaceLifecycleStatusSchemaVersion::V1,
            producer: parts.producer,
            primary_action_priority_policy: parts.primary_action_priority_policy,
            rows,
            fingerprint,
        })
    }

    /// Schema version.
    #[must_use]
    pub const fn schema_version(&self) -> WorkspaceLifecycleStatusSchemaVersion {
        self.schema_version
    }

    /// Producer identity.
    #[must_use]
    pub const fn producer(&self) -> &ProducerIdentity {
        &self.producer
    }

    /// Typed priority-policy input.
    #[must_use]
    pub const fn primary_action_priority_policy(&self) -> PrimaryActionPriorityPolicy {
        self.primary_action_priority_policy
    }

    /// Canonically ordered status rows.
    #[must_use]
    pub fn rows(&self) -> &[WorkspaceLifecycleStatusRow] {
        &self.rows
    }

    /// Deterministic semantic fingerprint.
    #[must_use]
    pub const fn fingerprint(&self) -> &StatusFingerprint {
        &self.fingerprint
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct WorkspaceLifecycleStatusRowWire {
    subject: WorkspaceLifecycleSubject,
    root_lifecycle: StatusDimension<RootLifecycleState>,
    configuration: StatusDimension<ConfigurationState>,
    watch_reload: StatusDimension<WatchReloadState>,
    readiness: ReadinessObservation,
    storage: StatusDimension<StorageState>,
    hydration: StatusDimension<HydrationState>,
    provider_posture: StatusDimension<ProviderPosture>,
    repair: RepairProjection,
    reasons: Vec<LogicalToken>,
    limitations: Vec<LogicalToken>,
    operation_links: Vec<LogicalToken>,
    metadata: StatusMetadata,
}

impl<'de> Deserialize<'de> for WorkspaceLifecycleStatusRow {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = WorkspaceLifecycleStatusRowWire::deserialize(deserializer)?;
        Self::from_parts(WorkspaceLifecycleStatusRowParts {
            subject: wire.subject,
            root_lifecycle: wire.root_lifecycle,
            configuration: wire.configuration,
            watch_reload: wire.watch_reload,
            readiness: wire.readiness,
            storage: wire.storage,
            hydration: wire.hydration,
            provider_posture: wire.provider_posture,
            repair: wire.repair,
            reasons: wire.reasons,
            limitations: wire.limitations,
            operation_links: wire.operation_links,
            metadata: wire.metadata,
        })
        .map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct WorkspaceLifecycleStatusWire {
    schema_version: WorkspaceLifecycleStatusSchemaVersion,
    producer: ProducerIdentity,
    primary_action_priority_policy: PrimaryActionPriorityPolicy,
    rows: Vec<WorkspaceLifecycleStatusRow>,
    fingerprint: StatusFingerprint,
}

impl<'de> Deserialize<'de> for WorkspaceLifecycleStatusV1 {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = WorkspaceLifecycleStatusWire::deserialize(deserializer)?;
        if !wire.schema_version.is_supported() {
            return Err(serde::de::Error::custom(ValidationError::UnsupportedSchemaVersion {
                version: wire.schema_version.as_u32(),
            }));
        }
        validate_envelope_shape(wire.schema_version, &wire.rows)
            .map_err(serde::de::Error::custom)?;
        let derived = fingerprint_over(
            wire.schema_version,
            &wire.producer,
            wire.primary_action_priority_policy,
            &wire.rows,
        );
        if derived != wire.fingerprint {
            return Err(serde::de::Error::custom(ValidationError::FingerprintMismatch));
        }
        Ok(Self {
            schema_version: wire.schema_version,
            producer: wire.producer,
            primary_action_priority_policy: wire.primary_action_priority_policy,
            rows: wire.rows,
            fingerprint: wire.fingerprint,
        })
    }
}

fn canonicalize_tokens(tokens: &mut Vec<LogicalToken>) {
    tokens.sort();
    tokens.dedup();
}

fn validate_row_shape(parts: &WorkspaceLifecycleStatusRowParts) -> Result<(), ValidationError> {
    check_bound("reasons", parts.reasons.len(), reason_limit())?;
    check_bound("limitations", parts.limitations.len(), MAX_LIMITATIONS_PER_ROW)?;
    check_bound("operation_links", parts.operation_links.len(), operation_link_limit())?;
    validate_tokens_sorted(&parts.reasons, "reasons")?;
    validate_tokens_sorted(&parts.limitations, "limitations")?;
    validate_tokens_sorted(&parts.operation_links, "operation_links")?;

    check_bound(
        "history_retained",
        usize::from(parts.metadata.history_retained()),
        usize::from(history_limit()),
    )?;
    validate_dimension(&parts.root_lifecycle, &parts.subject)?;
    validate_configuration(&parts.configuration, &parts.subject)?;
    validate_dimension(&parts.watch_reload, &parts.subject)?;
    validate_dimension(&parts.readiness.active_document, &parts.subject)?;
    validate_dimension(&parts.readiness.dependency_neighborhood, &parts.subject)?;
    validate_dimension(&parts.readiness.whole_workspace, &parts.subject)?;
    validate_dimension(&parts.storage, &parts.subject)?;
    validate_dimension(&parts.hydration, &parts.subject)?;
    validate_dimension(&parts.provider_posture, &parts.subject)?;

    let instrument_failed = parts.root_lifecycle.is_instrument_failed()
        || parts.configuration.is_instrument_failed()
        || parts.watch_reload.is_instrument_failed()
        || parts.readiness.active_document.is_instrument_failed()
        || parts.readiness.dependency_neighborhood.is_instrument_failed()
        || parts.readiness.whole_workspace.is_instrument_failed()
        || parts.storage.is_instrument_failed()
        || parts.hydration.is_instrument_failed()
        || parts.provider_posture.is_instrument_failed()
        || parts.readiness.active_document.observed_value()
            == Some(ReadinessState::InstrumentFailed)
        || parts.readiness.dependency_neighborhood.observed_value()
            == Some(ReadinessState::InstrumentFailed)
        || parts.readiness.whole_workspace.observed_value()
            == Some(ReadinessState::InstrumentFailed)
        || parts.provider_posture.observed_value() == Some(ProviderPosture::InstrumentFailure);

    let not_proven = parts.root_lifecycle.is_not_proven()
        || parts.configuration.is_not_proven()
        || parts.watch_reload.is_not_proven()
        || parts.readiness.active_document.is_not_proven()
        || parts.readiness.dependency_neighborhood.is_not_proven()
        || parts.readiness.whole_workspace.is_not_proven()
        || parts.storage.is_not_proven()
        || parts.hydration.is_not_proven()
        || parts.provider_posture.is_not_proven()
        || parts.configuration.observed_value() == Some(ConfigurationState::NotProven)
        || parts.hydration.observed_value() == Some(HydrationState::NotProven)
        || parts.readiness.active_document.observed_value() == Some(ReadinessState::NotProven)
        || parts.readiness.dependency_neighborhood.observed_value()
            == Some(ReadinessState::NotProven)
        || parts.readiness.whole_workspace.observed_value() == Some(ReadinessState::NotProven);

    match parts.metadata.instrument_state() {
        InstrumentState::Ok if instrument_failed => {
            return Err(ValidationError::InstrumentStateContradiction);
        }
        InstrumentState::Failed if !instrument_failed => {
            return Err(ValidationError::InstrumentStateContradiction);
        }
        _ => {}
    }

    if parts.metadata.completeness() == Completeness::Complete && (not_proven || instrument_failed)
    {
        return Err(ValidationError::CompletenessContradiction);
    }
    if parts.metadata.completeness() == Completeness::NotProven && !not_proven && !instrument_failed
    {
        return Err(ValidationError::CompletenessContradiction);
    }

    Ok(())
}

fn validate_tokens_sorted(
    tokens: &[LogicalToken],
    what: &'static str,
) -> Result<(), ValidationError> {
    for pair in tokens.windows(2) {
        if pair[0] >= pair[1] {
            return Err(ValidationError::TokenOrder { what });
        }
    }
    Ok(())
}

fn validate_dimension<T>(
    dimension: &StatusDimension<T>,
    subject: &WorkspaceLifecycleSubject,
) -> Result<(), ValidationError> {
    match dimension {
        StatusDimension::Observed { provenance, .. } => provenance.must_join_subject(subject),
        StatusDimension::NotProven | StatusDimension::InstrumentFailed { .. } => Ok(()),
    }
}

fn validate_configuration(
    dimension: &StatusDimension<ConfigurationState>,
    subject: &WorkspaceLifecycleSubject,
) -> Result<(), ValidationError> {
    match dimension {
        StatusDimension::Observed { provenance, value } => {
            if provenance.session_id() != subject.session_id() {
                return Err(ValidationError::CrossSessionJoin);
            }
            if provenance.root_id() != subject.root_id() {
                return Err(ValidationError::CrossRootJoin);
            }
            let generation_matches = provenance.runtime_generation()
                == subject.runtime_generation()
                && provenance.domain_generation() == subject.configuration_generation();
            if *value == ConfigurationState::Stale {
                if generation_matches {
                    return Err(ValidationError::StaleRequiresGenerationMismatch);
                }
            } else if !generation_matches {
                return Err(ValidationError::CrossGenerationJoin);
            }
            Ok(())
        }
        StatusDimension::NotProven | StatusDimension::InstrumentFailed { .. } => Ok(()),
    }
}

fn validate_envelope_shape(
    schema_version: WorkspaceLifecycleStatusSchemaVersion,
    rows: &[WorkspaceLifecycleStatusRow],
) -> Result<(), ValidationError> {
    if !schema_version.is_supported() {
        return Err(ValidationError::UnsupportedSchemaVersion { version: schema_version.as_u32() });
    }
    check_bound("rows", rows.len(), envelope_row_limit())?;
    let Some(first) = rows.first() else {
        return Err(ValidationError::EmptyEnvelope);
    };
    let session = first.subject.session_id();
    for pair in rows.windows(2) {
        if pair[0].subject.root_id() > pair[1].subject.root_id()
            || (pair[0].subject.root_id() == pair[1].subject.root_id()
                && pair[0].subject.runtime_generation() >= pair[1].subject.runtime_generation())
        {
            if pair[0].subject.root_id() == pair[1].subject.root_id()
                && pair[0].subject.runtime_generation() == pair[1].subject.runtime_generation()
            {
                return Err(ValidationError::DuplicateRootGeneration);
            }
            return Err(ValidationError::RowOrder);
        }
    }
    for row in rows {
        if row.subject.session_id() != session {
            return Err(ValidationError::CrossSessionJoin);
        }
    }
    Ok(())
}

fn fingerprint_over(
    schema_version: WorkspaceLifecycleStatusSchemaVersion,
    producer: &ProducerIdentity,
    policy: PrimaryActionPriorityPolicy,
    rows: &[WorkspaceLifecycleStatusRow],
) -> StatusFingerprint {
    let version = schema_version.as_u32().to_be_bytes();
    let producer_bytes = producer.implementation().as_str().as_bytes();
    let policy_bytes = policy.as_str().as_bytes();
    let row_material: Vec<Vec<u8>> =
        rows.iter().map(WorkspaceLifecycleStatusRow::fingerprint_parts).collect();
    let mut parts: Vec<&[u8]> = Vec::with_capacity(3 + row_material.len());
    parts.push(&version);
    parts.push(producer_bytes);
    parts.push(policy_bytes);
    for row in &row_material {
        parts.push(row);
    }
    StatusFingerprint::of_parts(FINGERPRINT_DOMAIN, &parts)
}

fn push_bytes(buf: &mut Vec<u8>, bytes: &[u8]) {
    let len = u32::try_from(bytes.len()).unwrap_or(u32::MAX);
    buf.extend_from_slice(&len.to_be_bytes());
    buf.extend_from_slice(bytes);
}

fn push_token(buf: &mut Vec<u8>, token: &LogicalToken) {
    push_bytes(buf, token.as_str().as_bytes());
}

fn push_token_list(buf: &mut Vec<u8>, tag: &[u8], tokens: &[LogicalToken]) {
    push_bytes(buf, tag);
    let count = u32::try_from(tokens.len()).unwrap_or(u32::MAX);
    buf.extend_from_slice(&count.to_be_bytes());
    for token in tokens {
        push_token(buf, token);
    }
}

fn push_dimension<T: Copy>(
    buf: &mut Vec<u8>,
    name: &str,
    dimension: &StatusDimension<T>,
    as_str: fn(T) -> &'static str,
) {
    push_bytes(buf, name.as_bytes());
    match dimension {
        StatusDimension::Observed { provenance, value } => {
            push_bytes(buf, b"observed");
            push_token(buf, provenance.session_id());
            push_token(buf, provenance.root_id());
            push_token(buf, provenance.runtime_generation());
            push_token(buf, provenance.source_identity());
            push_token(buf, provenance.domain_generation());
            push_bytes(buf, as_str(*value).as_bytes());
        }
        StatusDimension::NotProven => push_bytes(buf, b"not_proven"),
        StatusDimension::InstrumentFailed { instrument_id } => {
            push_bytes(buf, b"instrument_failed");
            push_token(buf, instrument_id);
        }
    }
}
