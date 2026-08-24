//! Deterministic work dimensions, work budgets, and execution profiles.

use super::{ReachabilityContractError, ReachabilityFactFamilyId, ReachabilityOperationKind};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Stable registry of deterministic reachability work dimensions.
///
/// Every dimension is a countable work unit. Elapsed wall time, output
/// cardinality, cache hits, and host timeouts are deliberately absent: they
/// are not deterministic work units and cannot substitute for one. Consumers
/// may add a dimension only through review of this shared registry.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ReachabilityWorkDimension {
    /// Accepted workspace snapshots captured.
    WorkspaceSnapshotsCaptured,
    /// Fact families inspected.
    FactFamiliesInspected,
    /// Fact families admitted.
    FactFamiliesAdmitted,
    /// Fact families rejected.
    FactFamiliesRejected,
    /// Nodes admitted.
    NodesAdmitted,
    /// Nodes validated.
    NodesValidated,
    /// Edges admitted.
    EdgesAdmitted,
    /// Edges validated.
    EdgesValidated,
    /// Edges normalized.
    EdgesNormalized,
    /// Root facts inspected.
    RootFactsInspected,
    /// Activation facts inspected.
    ActivationFactsInspected,
    /// Exposure facts inspected.
    ExposureFactsInspected,
    /// Blocker facts inspected.
    BlockerFactsInspected,
    /// SCC discovery node visits.
    SccNodesVisited,
    /// SCC discovery edge visits.
    SccEdgesVisited,
    /// SCC stack operations.
    SccStackOperations,
    /// Components formed.
    ComponentsFormed,
    /// Condensed edges constructed.
    CondensedEdgesConstructed,
    /// Production closure nodes traversed.
    ProductionClosureNodesTraversed,
    /// Production closure edges traversed.
    ProductionClosureEdgesTraversed,
    /// Test closure nodes traversed.
    TestClosureNodesTraversed,
    /// Test closure edges traversed.
    TestClosureEdgesTraversed,
    /// Classification rows produced.
    ClassificationRows,
    /// Entity queries executed.
    EntityQueries,
    /// Component queries executed.
    ComponentQueries,
    /// Source queries executed.
    SourceQueries,
    /// Source partitions produced.
    SourcePartitions,
    /// Explanation components inspected.
    ExplanationComponents,
    /// Explanation edges inspected.
    ExplanationEdges,
    /// Explanation members inspected.
    ExplanationMembers,
    /// Explanation paths inspected.
    ExplanationPaths,
    /// Policy candidates inspected.
    PolicyCandidatesInspected,
    /// Policy candidates selected.
    PolicyCandidatesSelected,
    /// Policy candidates refused.
    PolicyCandidatesRefused,
    /// Diagnostic candidates composed.
    DiagnosticCandidates,
    /// Diagnostic items produced.
    DiagnosticItems,
    /// Transport projections produced.
    TransportProjections,
    /// Transport chunks produced.
    TransportChunks,
    /// Serialized evidence bytes.
    SerializedEvidenceBytes,
    /// Serialized output bytes.
    SerializedOutputBytes,
    /// Cache/reuse lookups performed.
    CacheLookups,
    /// Reuse hits validated against the exact subject.
    ValidatedReuseHits,
    /// Work units charged after publication eligibility was lost.
    WorkAfterEligibilityLost,
}

impl ReachabilityWorkDimension {
    /// The stable kebab-case name of this dimension.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::WorkspaceSnapshotsCaptured => "workspace-snapshots-captured",
            Self::FactFamiliesInspected => "fact-families-inspected",
            Self::FactFamiliesAdmitted => "fact-families-admitted",
            Self::FactFamiliesRejected => "fact-families-rejected",
            Self::NodesAdmitted => "nodes-admitted",
            Self::NodesValidated => "nodes-validated",
            Self::EdgesAdmitted => "edges-admitted",
            Self::EdgesValidated => "edges-validated",
            Self::EdgesNormalized => "edges-normalized",
            Self::RootFactsInspected => "root-facts-inspected",
            Self::ActivationFactsInspected => "activation-facts-inspected",
            Self::ExposureFactsInspected => "exposure-facts-inspected",
            Self::BlockerFactsInspected => "blocker-facts-inspected",
            Self::SccNodesVisited => "scc-nodes-visited",
            Self::SccEdgesVisited => "scc-edges-visited",
            Self::SccStackOperations => "scc-stack-operations",
            Self::ComponentsFormed => "components-formed",
            Self::CondensedEdgesConstructed => "condensed-edges-constructed",
            Self::ProductionClosureNodesTraversed => "production-closure-nodes-traversed",
            Self::ProductionClosureEdgesTraversed => "production-closure-edges-traversed",
            Self::TestClosureNodesTraversed => "test-closure-nodes-traversed",
            Self::TestClosureEdgesTraversed => "test-closure-edges-traversed",
            Self::ClassificationRows => "classification-rows",
            Self::EntityQueries => "entity-queries",
            Self::ComponentQueries => "component-queries",
            Self::SourceQueries => "source-queries",
            Self::SourcePartitions => "source-partitions",
            Self::ExplanationComponents => "explanation-components",
            Self::ExplanationEdges => "explanation-edges",
            Self::ExplanationMembers => "explanation-members",
            Self::ExplanationPaths => "explanation-paths",
            Self::PolicyCandidatesInspected => "policy-candidates-inspected",
            Self::PolicyCandidatesSelected => "policy-candidates-selected",
            Self::PolicyCandidatesRefused => "policy-candidates-refused",
            Self::DiagnosticCandidates => "diagnostic-candidates",
            Self::DiagnosticItems => "diagnostic-items",
            Self::TransportProjections => "transport-projections",
            Self::TransportChunks => "transport-chunks",
            Self::SerializedEvidenceBytes => "serialized-evidence-bytes",
            Self::SerializedOutputBytes => "serialized-output-bytes",
            Self::CacheLookups => "cache-lookups",
            Self::ValidatedReuseHits => "validated-reuse-hits",
            Self::WorkAfterEligibilityLost => "work-after-eligibility-lost",
        }
    }

    /// Parse one work dimension, failing closed on unknown names.
    ///
    /// # Errors
    ///
    /// Returns [`ReachabilityContractError::UnknownWorkDimension`] when the
    /// name is not one of the registry dimensions.
    pub fn parse(name: &str) -> Result<Self, ReachabilityContractError> {
        const ALL: [ReachabilityWorkDimension;
            ReachabilityWorkDimension::WorkAfterEligibilityLost as usize + 1] = [
            ReachabilityWorkDimension::WorkspaceSnapshotsCaptured,
            ReachabilityWorkDimension::FactFamiliesInspected,
            ReachabilityWorkDimension::FactFamiliesAdmitted,
            ReachabilityWorkDimension::FactFamiliesRejected,
            ReachabilityWorkDimension::NodesAdmitted,
            ReachabilityWorkDimension::NodesValidated,
            ReachabilityWorkDimension::EdgesAdmitted,
            ReachabilityWorkDimension::EdgesValidated,
            ReachabilityWorkDimension::EdgesNormalized,
            ReachabilityWorkDimension::RootFactsInspected,
            ReachabilityWorkDimension::ActivationFactsInspected,
            ReachabilityWorkDimension::ExposureFactsInspected,
            ReachabilityWorkDimension::BlockerFactsInspected,
            ReachabilityWorkDimension::SccNodesVisited,
            ReachabilityWorkDimension::SccEdgesVisited,
            ReachabilityWorkDimension::SccStackOperations,
            ReachabilityWorkDimension::ComponentsFormed,
            ReachabilityWorkDimension::CondensedEdgesConstructed,
            ReachabilityWorkDimension::ProductionClosureNodesTraversed,
            ReachabilityWorkDimension::ProductionClosureEdgesTraversed,
            ReachabilityWorkDimension::TestClosureNodesTraversed,
            ReachabilityWorkDimension::TestClosureEdgesTraversed,
            ReachabilityWorkDimension::ClassificationRows,
            ReachabilityWorkDimension::EntityQueries,
            ReachabilityWorkDimension::ComponentQueries,
            ReachabilityWorkDimension::SourceQueries,
            ReachabilityWorkDimension::SourcePartitions,
            ReachabilityWorkDimension::ExplanationComponents,
            ReachabilityWorkDimension::ExplanationEdges,
            ReachabilityWorkDimension::ExplanationMembers,
            ReachabilityWorkDimension::ExplanationPaths,
            ReachabilityWorkDimension::PolicyCandidatesInspected,
            ReachabilityWorkDimension::PolicyCandidatesSelected,
            ReachabilityWorkDimension::PolicyCandidatesRefused,
            ReachabilityWorkDimension::DiagnosticCandidates,
            ReachabilityWorkDimension::DiagnosticItems,
            ReachabilityWorkDimension::TransportProjections,
            ReachabilityWorkDimension::TransportChunks,
            ReachabilityWorkDimension::SerializedEvidenceBytes,
            ReachabilityWorkDimension::SerializedOutputBytes,
            ReachabilityWorkDimension::CacheLookups,
            ReachabilityWorkDimension::ValidatedReuseHits,
            ReachabilityWorkDimension::WorkAfterEligibilityLost,
        ];
        ALL.into_iter()
            .find(|dimension| dimension.as_str() == name)
            .ok_or_else(|| ReachabilityContractError::UnknownWorkDimension(name.to_string()))
    }
}

/// Stable identity of one execution/budget profile.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct ReachabilityProfileId(String);

impl<'de> Deserialize<'de> for ReachabilityProfileId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        ReachabilityProfileId::new(value).map_err(serde::de::Error::custom)
    }
}

impl ReachabilityProfileId {
    /// Construct a profile identifier, rejecting empty values.
    ///
    /// # Errors
    ///
    /// Returns [`ReachabilityContractError::EmptyIdentity`] when `value` is
    /// empty.
    pub fn new(value: impl Into<String>) -> Result<Self, ReachabilityContractError> {
        let value = value.into();
        if value.is_empty() {
            return Err(ReachabilityContractError::EmptyIdentity);
        }
        Ok(Self(value))
    }

    /// The opaque profile identifier value.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Reviewed justification required for one unlimited work dimension.
///
/// "Unlimited" requires one explicit reviewed reason and a higher-level
/// safety bound; an unbounded dimension without both is non-pass.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReachabilityUnlimitedJustification {
    reason: String,
    safety_bound: u64,
}

impl<'de> Deserialize<'de> for ReachabilityUnlimitedJustification {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Raw {
            reason: String,
            safety_bound: u64,
        }
        let raw = Raw::deserialize(deserializer)?;
        if raw.reason.is_empty() || raw.safety_bound == 0 {
            return Err(serde::de::Error::custom(
                "an unlimited justification requires a reviewed reason and a safety bound",
            ));
        }
        Ok(ReachabilityUnlimitedJustification {
            reason: raw.reason,
            safety_bound: raw.safety_bound,
        })
    }
}

impl ReachabilityUnlimitedJustification {
    /// Construct an unlimited justification with a non-empty reviewed reason
    /// and a safety bound above zero.
    ///
    /// # Errors
    ///
    /// Returns [`ReachabilityContractError::UnlimitedWithoutSafetyBound`]
    /// when the reason is empty or the safety bound is zero.
    pub fn new(
        dimension: ReachabilityWorkDimension,
        reason: impl Into<String>,
        safety_bound: u64,
    ) -> Result<Self, ReachabilityContractError> {
        let reason = reason.into();
        if reason.is_empty() || safety_bound == 0 {
            return Err(ReachabilityContractError::UnlimitedWithoutSafetyBound { dimension });
        }
        Ok(Self { reason, safety_bound })
    }

    /// The reviewed reason retained for audit.
    #[must_use]
    pub fn reason(&self) -> &str {
        &self.reason
    }

    /// The higher-level safety bound that still applies.
    #[must_use]
    pub const fn safety_bound(&self) -> u64 {
        self.safety_bound
    }
}

/// The limit applying to one dimension under a budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReachabilityDimensionLimit {
    /// A bounded deterministic limit in work units.
    Bounded(u64),
    /// Unlimited under a reviewed justification with a higher-level safety
    /// bound.
    Unlimited {
        /// The safety bound that still caps total work.
        safety_bound: u64,
    },
}

/// One deterministic reachability work budget.
///
/// Mechanism and values remain separate: this type enforces per-dimension
/// work-unit limits with checked arithmetic; representative product values
/// and ratchets stay with the performance/configuration authorities, and
/// historical constants do not become product truth by existing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReachabilityWorkBudget {
    profile_id: super::ReachabilityProfileId,
    selected_operation_kinds: Vec<ReachabilityOperationKind>,
    dimension_limits: BTreeMap<ReachabilityWorkDimension, u64>,
    unlimited: BTreeMap<ReachabilityWorkDimension, ReachabilityUnlimitedJustification>,
}

impl<'de> Deserialize<'de> for ReachabilityWorkBudget {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Raw {
            profile_id: ReachabilityProfileId,
            selected_operation_kinds: Vec<ReachabilityOperationKind>,
            dimension_limits: BTreeMap<ReachabilityWorkDimension, u64>,
            unlimited: BTreeMap<ReachabilityWorkDimension, ReachabilityUnlimitedJustification>,
        }
        let raw = Raw::deserialize(deserializer)?;
        ReachabilityWorkBudget::new(
            raw.profile_id,
            raw.selected_operation_kinds,
            raw.dimension_limits,
            raw.unlimited,
        )
        .map_err(serde::de::Error::custom)
    }
}

impl ReachabilityWorkBudget {
    /// Construct a budget from validated parts.
    ///
    /// # Errors
    ///
    /// Returns [`ReachabilityContractError::EmptyOperationKindSelection`]
    /// when no operation kind is selected; the profile identifier itself is
    /// already validated by [`ReachabilityProfileId::new`].
    pub fn new(
        profile_id: super::ReachabilityProfileId,
        selected_operation_kinds: Vec<ReachabilityOperationKind>,
        dimension_limits: BTreeMap<ReachabilityWorkDimension, u64>,
        unlimited: BTreeMap<ReachabilityWorkDimension, ReachabilityUnlimitedJustification>,
    ) -> Result<Self, ReachabilityContractError> {
        if selected_operation_kinds.is_empty() {
            return Err(ReachabilityContractError::EmptyOperationKindSelection);
        }
        Ok(Self { profile_id, selected_operation_kinds, dimension_limits, unlimited })
    }

    /// The profile identity of this budget.
    #[must_use]
    pub fn profile_id(&self) -> &super::ReachabilityProfileId {
        &self.profile_id
    }

    /// The operation kinds this budget governs.
    #[must_use]
    pub fn selected_operation_kinds(&self) -> &[ReachabilityOperationKind] {
        &self.selected_operation_kinds
    }

    /// The limit applying to one dimension, if the budget constrains it.
    #[must_use]
    pub fn limit_for(
        &self,
        dimension: ReachabilityWorkDimension,
    ) -> Option<ReachabilityDimensionLimit> {
        if let Some(limit) = self.dimension_limits.get(&dimension) {
            return Some(ReachabilityDimensionLimit::Bounded(*limit));
        }
        self.unlimited.get(&dimension).map(|justification| ReachabilityDimensionLimit::Unlimited {
            safety_bound: justification.safety_bound(),
        })
    }

    /// Validate that every required dimension is constrained.
    ///
    /// # Errors
    ///
    /// Returns [`ReachabilityContractError::MissingRequiredDimension`] for
    /// the first required dimension with neither a bounded limit nor a
    /// reviewed unlimited justification.
    pub fn validate_requirements(
        &self,
        required: &[ReachabilityWorkDimension],
    ) -> Result<(), ReachabilityContractError> {
        for dimension in required {
            if self.limit_for(*dimension).is_none() {
                return Err(ReachabilityContractError::MissingRequiredDimension {
                    dimension: *dimension,
                });
            }
        }
        Ok(())
    }

    /// A deterministic test profile constraining a single dimension.
    ///
    /// This helper exists so tests and deterministic fixtures can build a
    /// valid budget without implying product defaults.
    ///
    /// # Errors
    ///
    /// Returns a contract error when the profile identifier is empty.
    pub fn for_tests(
        profile_id: super::ReachabilityProfileId,
        kind: ReachabilityOperationKind,
        dimension: ReachabilityWorkDimension,
        limit: u64,
    ) -> Result<Self, ReachabilityContractError> {
        let mut dimension_limits = BTreeMap::new();
        dimension_limits.insert(dimension, limit);
        Self::new(profile_id, vec![kind], dimension_limits, BTreeMap::new())
    }
}

/// Purpose of one execution profile.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ReachabilityExecutionPurpose {
    /// Interactive latency-bound operation.
    Interactive,
    /// Batch throughput-bound operation.
    Batch,
    /// Proof-oriented deterministic operation.
    Proof,
}

/// How a stage observes the canonical cancellation/deadline control.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ReachabilityCancellationPolling {
    /// Poll at every declared checkpoint.
    AtDeclaredCheckpoints,
    /// Poll at operation start and end only; never valid for stages that
    /// declare interior checkpoints.
    AtOperationBoundariesOnly,
}

/// Output and explanation retention limits of one execution profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReachabilityRetentionLimits {
    /// Maximum explanation items retained in one bounded view.
    pub max_explanation_items: Option<u64>,
    /// Maximum serialized output bytes retained by one operation.
    pub max_output_bytes: Option<u64>,
}

/// The deterministic execution profile one reachability operation runs under.
///
/// The profile records mechanism identity — purpose, declared checkpoints,
/// polling contract, retention limits, and the source of product defaults —
/// without selecting product budget values.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReachabilityExecutionProfile {
    profile_id: super::ReachabilityProfileId,
    version: u32,
    purpose: ReachabilityExecutionPurpose,
    selected_operation_kinds: Vec<ReachabilityOperationKind>,
    selected_fact_families: Vec<ReachabilityFactFamilyId>,
    cancellation_polling: ReachabilityCancellationPolling,
    retention: ReachabilityRetentionLimits,
    defaults_source: String,
    limitations: Vec<String>,
}

impl<'de> Deserialize<'de> for ReachabilityExecutionProfile {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Raw {
            profile_id: super::ReachabilityProfileId,
            version: u32,
            purpose: ReachabilityExecutionPurpose,
            selected_operation_kinds: Vec<ReachabilityOperationKind>,
            selected_fact_families: Vec<super::ReachabilityFactFamilyId>,
            cancellation_polling: ReachabilityCancellationPolling,
            retention: ReachabilityRetentionLimits,
            defaults_source: String,
            limitations: Vec<String>,
        }
        let raw = Raw::deserialize(deserializer)?;
        ReachabilityExecutionProfile::new(
            raw.profile_id,
            raw.version,
            raw.purpose,
            raw.selected_operation_kinds,
            raw.selected_fact_families,
            raw.cancellation_polling,
            raw.retention,
            raw.defaults_source,
            raw.limitations,
        )
        .map_err(serde::de::Error::custom)
    }
}

impl ReachabilityExecutionProfile {
    /// Construct a validated execution profile.
    ///
    /// # Errors
    ///
    /// Returns
    /// [`ReachabilityContractError::EmptyOperationKindSelection`] when no
    /// operation kind is selected, or [`ReachabilityContractError::EmptyIdentity`]
    /// when the defaults source is empty or the version is zero; the profile
    /// identifier is already validated by [`ReachabilityProfileId::new`].
    #[allow(clippy::too_many_arguments)] // mirrors the profile contract fields
    pub fn new(
        profile_id: super::ReachabilityProfileId,
        version: u32,
        purpose: ReachabilityExecutionPurpose,
        selected_operation_kinds: Vec<ReachabilityOperationKind>,
        selected_fact_families: Vec<ReachabilityFactFamilyId>,
        cancellation_polling: ReachabilityCancellationPolling,
        retention: ReachabilityRetentionLimits,
        defaults_source: impl Into<String>,
        limitations: Vec<String>,
    ) -> Result<Self, ReachabilityContractError> {
        if selected_operation_kinds.is_empty() {
            return Err(ReachabilityContractError::EmptyOperationKindSelection);
        }
        let defaults_source = defaults_source.into();
        if defaults_source.is_empty() || version == 0 {
            return Err(ReachabilityContractError::EmptyIdentity);
        }
        Ok(Self {
            profile_id,
            version,
            purpose,
            selected_operation_kinds,
            selected_fact_families,
            cancellation_polling,
            retention,
            defaults_source,
            limitations,
        })
    }

    /// The stable profile identity.
    #[must_use]
    pub fn profile_id(&self) -> &super::ReachabilityProfileId {
        &self.profile_id
    }

    /// The profile schema version.
    #[must_use]
    pub const fn version(&self) -> u32 {
        self.version
    }

    /// The declared purpose.
    #[must_use]
    pub const fn purpose(&self) -> ReachabilityExecutionPurpose {
        self.purpose
    }

    /// The operation kinds this profile governs.
    #[must_use]
    pub fn selected_operation_kinds(&self) -> &[ReachabilityOperationKind] {
        &self.selected_operation_kinds
    }

    /// The fact families this profile admits.
    #[must_use]
    pub fn selected_fact_families(&self) -> &[ReachabilityFactFamilyId] {
        &self.selected_fact_families
    }

    /// The cancellation/deadline polling contract.
    #[must_use]
    pub const fn cancellation_polling(&self) -> ReachabilityCancellationPolling {
        self.cancellation_polling
    }

    /// The retention limits.
    #[must_use]
    pub const fn retention(&self) -> ReachabilityRetentionLimits {
        self.retention
    }

    /// The opaque source of product defaults for this profile.
    #[must_use]
    pub fn defaults_source(&self) -> &str {
        &self.defaults_source
    }

    /// Bounded limitation and invalidation notes.
    #[must_use]
    pub fn limitations(&self) -> &[String] {
        &self.limitations
    }
}
