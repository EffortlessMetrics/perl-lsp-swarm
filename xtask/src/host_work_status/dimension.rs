//! Typed observations for the four independent WIP dimensions.
//!
//! Every fact is supplied by a provider as a typed value; no field accepts
//! prose to be interpreted. `NotProven`/`Unavailable` variants are first-class
//! so instrument failure can never masquerade as a zero/empty/clean result.

use super::subject::ProviderId;
use serde::{Deserialize, Serialize};

/// Instrument availability behind an observation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Instrument {
    Available,
    Unavailable { detail: String },
    NotApplicable,
}

/// Whether the observation is current enough to classify.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Freshness {
    Current,
    Stale,
    NotProven,
}

/// Issue/PR relationship where directly known from a current typed owner.
/// `Unknown` (instrument absent or query failed) is never promoted to
/// `Unlinked`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ClaimRelationship {
    Unlinked,
    LinkedToOpenPr { number: u64 },
    LinkedToMergedPr { number: u64 },
    LinkedToOpenIssue { number: u64 },
    LinkedToClosedIssue { number: u64 },
    Unknown,
}

/// Durable local/GitHub relationship state for the logical dimension.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DurableState {
    NoLocalResidue,
    ReconstructibleLocalState,
    UniqueLocalState,
    RemoteInFlight,
    Contradictory,
    NotProven,
}

/// Positive evidence that orphan premises hold. The type carries no age,
/// silence, or return fields: those inputs cannot construct orphan status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub struct OrphanPremises {
    pub ownership_established: bool,
    pub targetability_established: bool,
    pub cleanup_premises_fully_observable: bool,
}

impl OrphanPremises {
    pub const fn all_proven(&self) -> bool {
        self.ownership_established
            && self.targetability_established
            && self.cleanup_premises_fully_observable
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogicalWorkObservation {
    pub subject_key: String,
    pub provider: ProviderId,
    pub observed_at: String,
    pub freshness: Freshness,
    pub claim_relationship: ClaimRelationship,
    pub durable_state: DurableState,
    pub orphan_premises: Option<OrphanPremises>,
    pub limitations: Vec<String>,
    pub instrument: Instrument,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum MutationOwnership {
    /// A live writer owns this candidate right now (typed writer-admission
    /// or worktree-plan fact).
    ActiveWriter {
        writer_id: String,
    },
    /// Ownership is contested between writers — a collision, not a count.
    Contested,
    /// No current owner is established.
    Unowned,
    NotProven,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum IndexState {
    Clean,
    Dirty { staged: bool, untracked: bool },
    MergeConflict,
    LockHeld,
    NotProven,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PushState {
    Pushed,
    Unpushed { ahead_count: u64 },
    NoRemoteBranch,
    NotProven,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MutationWorkObservation {
    pub subject_key: String,
    pub provider: ProviderId,
    pub observed_at: String,
    pub ownership: MutationOwnership,
    pub index_state: IndexState,
    pub push_state: PushState,
    /// Provider-supplied salvage requirement; the domain never infers it
    /// from age, silence, or issue state.
    pub salvage_required: bool,
    /// A Git mutation (merge/rebase/index lock) is in progress per provider.
    pub git_mutation_in_progress: bool,
    pub orphan_premises: Option<OrphanPremises>,
    pub limitations: Vec<String>,
    pub instrument: Instrument,
}

/// Capacity reservation lifecycle (#11653-shaped typed input). Reservation
/// release is not process terminality and parent exit is not release.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ReservationFact {
    Active { reservation_id: String, capacity_units: u64 },
    Queued { reservation_id: String },
    Released { reservation_id: String, settled: Settlement },
    Absent,
    NotProven,
}

/// Process-group/Job-Object lifecycle (#11659-shaped typed input).
/// `ExitedConfirmed` requires positive whole-tree terminality evidence from
/// the provider; parent exit or a missing API can never produce it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ProcessTreeFact {
    Live { process_group_id: String, attribution: Attribution },
    ExitedConfirmed { process_group_id: String },
    TerminalityUnproven { detail: String },
    ApiUnavailable { detail: String },
    NotApplicable,
}

/// How a compute resource binds to this exact subject. Executable name or
/// path resemblance alone can never satisfy another repository's subject.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Attribution {
    ExactSubjectBinding,
    ExecutableNameOnly,
    Unattributed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Settlement {
    Settled,
    Unsettled,
    NotProven,
}

/// Whether the initiating caller returned. Return is recorded as its own
/// fact precisely so it can never be read as descendant terminality.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum InitiatorReturn {
    Returned,
    StillWorking,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ComputeWorkObservation {
    pub subject_key: String,
    pub provider: ProviderId,
    pub observed_at: String,
    pub freshness: Freshness,
    pub reservation: ReservationFact,
    pub process_tree: ProcessTreeFact,
    pub descendants_settled: Settlement,
    pub output_settled: Settlement,
    pub initiator_returned: InitiatorReturn,
    pub queue_depth: Option<u64>,
    pub capacity_units_in_use: Option<u64>,
    pub capacity_units_total: Option<u64>,
    pub orphan_premises: Option<OrphanPremises>,
    pub limitations: Vec<String>,
    pub instrument: Instrument,
}

/// Candidate-private versus shared scope. Shared cache state is never
/// candidate/product authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RootClass {
    CandidatePrivate,
    SharedCache,
    AmbiguousScope,
    NotProven,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum VolumeIdentity {
    Identified { volume_id: String },
    Unknown,
    NotProven,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CapacityFact {
    Measured { free_bytes: u64 },
    Unknown,
    NotProven,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum FootprintFact {
    Measured { bytes: u64 },
    Unknown,
    NotApplicable,
    NotProven,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum StorageDisposition {
    Reconstructible,
    CacheOnly,
    Unique,
    Ambiguous,
    NotProven,
}

/// Approved reclaim class and owner, supplied by a current typed owner.
/// Approval describes a future plan family; it authorizes nothing here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ReclaimClass {
    Approved { class: String, owner: String },
    NoneApproved,
    NotProven,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StorageWorkObservation {
    pub subject_key: String,
    pub provider: ProviderId,
    pub observed_at: String,
    pub root_class: RootClass,
    pub volume_identity: VolumeIdentity,
    pub free_capacity: CapacityFact,
    /// Caller-supplied configured floor in bytes (e.g. the FLOOR_GB/FLOOR_PCT
    /// convention), when one applies.
    pub configured_floor_bytes: Option<u64>,
    /// Provider-owned typed fact that the measured free capacity is below the
    /// configured floor. Kept separate from the numeric comparison so a
    /// provider that knows the verdict without exporting numbers stays
    /// representable.
    pub below_configured_floor: bool,
    pub footprint: FootprintFact,
    pub disposition: StorageDisposition,
    pub reclaim_class: ReclaimClass,
    /// Positive orphan premises for unique candidate-private residue. The
    /// struct carries no age/silence/return inputs, so those cannot construct
    /// orphan status.
    pub orphan_premises: Option<OrphanPremises>,
    pub limitations: Vec<String>,
    pub instrument: Instrument,
}

/// A row bound to a different subject was offered to this set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubjectMismatch {
    pub expected: String,
    pub actual: String,
}

/// An unknown provider variant surfaced through the set's visibility rows.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct UnknownVariantRecord {
    pub family: crate::host_work_status::subject::ProviderFamily,
    pub schema_version: String,
    pub source: String,
    pub variant: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// All typed observations for exactly one subject. Rows for any other
/// subject are rejected at insertion: one subject's resource can never
/// satisfy another's classification.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostWorkObservationSet {
    subject_key: String,
    logical: Vec<LogicalWorkObservation>,
    mutation: Vec<MutationWorkObservation>,
    compute: Vec<ComputeWorkObservation>,
    storage: Vec<StorageWorkObservation>,
    unknown_variants: Vec<UnknownVariantRecord>,
    missing_providers: Vec<crate::host_work_status::subject::ProviderFamily>,
}

impl HostWorkObservationSet {
    pub fn new(subject_key: impl Into<String>) -> Self {
        Self {
            subject_key: subject_key.into(),
            logical: Vec::new(),
            mutation: Vec::new(),
            compute: Vec::new(),
            storage: Vec::new(),
            unknown_variants: Vec::new(),
            missing_providers: Vec::new(),
        }
    }

    pub fn subject_key(&self) -> &str {
        &self.subject_key
    }

    pub fn logical(&self) -> &[LogicalWorkObservation] {
        &self.logical
    }

    pub fn mutation(&self) -> &[MutationWorkObservation] {
        &self.mutation
    }

    pub fn compute(&self) -> &[ComputeWorkObservation] {
        &self.compute
    }

    pub fn storage(&self) -> &[StorageWorkObservation] {
        &self.storage
    }

    pub fn unknown_variants(&self) -> &[UnknownVariantRecord] {
        &self.unknown_variants
    }

    pub fn missing_providers(&self) -> &[crate::host_work_status::subject::ProviderFamily] {
        &self.missing_providers
    }

    fn check_key(&self, actual: &str) -> Result<(), SubjectMismatch> {
        if actual == self.subject_key {
            Ok(())
        } else {
            Err(SubjectMismatch { expected: self.subject_key.clone(), actual: actual.to_string() })
        }
    }

    pub fn push_logical(
        &mut self,
        observation: LogicalWorkObservation,
    ) -> Result<(), SubjectMismatch> {
        self.check_key(&observation.subject_key)?;
        self.logical.push(observation);
        Ok(())
    }

    pub fn push_mutation(
        &mut self,
        observation: MutationWorkObservation,
    ) -> Result<(), SubjectMismatch> {
        self.check_key(&observation.subject_key)?;
        self.mutation.push(observation);
        Ok(())
    }

    pub fn push_compute(
        &mut self,
        observation: ComputeWorkObservation,
    ) -> Result<(), SubjectMismatch> {
        self.check_key(&observation.subject_key)?;
        self.compute.push(observation);
        Ok(())
    }

    pub fn push_storage(
        &mut self,
        observation: StorageWorkObservation,
    ) -> Result<(), SubjectMismatch> {
        self.check_key(&observation.subject_key)?;
        self.storage.push(observation);
        Ok(())
    }

    pub fn record_unknown_variant(&mut self, record: UnknownVariantRecord) {
        self.unknown_variants.push(record);
    }

    pub fn declare_missing_provider(
        &mut self,
        family: crate::host_work_status::subject::ProviderFamily,
    ) {
        if !self.missing_providers.contains(&family) {
            self.missing_providers.push(family);
        }
    }
}
