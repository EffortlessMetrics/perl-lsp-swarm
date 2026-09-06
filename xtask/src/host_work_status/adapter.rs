//! Exhaustive provider adapters over currently landed typed owners.
//!
//! Each adapter consumes a provider-owned typed result, carries the
//! provider/schema identity and observation currentness, records instrument
//! availability and limitations verbatim (never parsed), and maps every
//! variant exhaustively into the host-status model. Unknown check/variant
//! names surface as [`UnknownVariantRecord`] rows instead of disappearing.
//!
//! Providers without a landed typed owner (#11650/#11653/#11659) are declared
//! missing; their future owners feed the generic typed constructors here.

use super::dimension::{
    Attribution, CapacityFact, ClaimRelationship, ComputeWorkObservation, DurableState,
    FootprintFact, Freshness, InitiatorReturn, Instrument, LogicalWorkObservation,
    MutationOwnership, MutationWorkObservation, OrphanPremises, ProcessTreeFact, PushState,
    ReclaimClass, ReservationFact, RootClass, Settlement, StorageDisposition,
    StorageWorkObservation, UnknownVariantRecord, VolumeIdentity,
};
use super::lifecycle::CleanupReadiness;
use super::subject::{
    HostWorkSubject, ObservationScope, ProviderFamily, ProviderId, WorktreeIdentity,
};
use crate::tasks::writer_admission::{
    AdmissionReport, AdmissionVerdict, CheckResult, CheckStatus, PrStatus, WriterAdmissionSnapshot,
};
use serde::{Deserialize, Serialize};
use xtask::worktree_cleanup::{
    Observation, ObservationState, PrMatch, WorktreeClassification, WorktreeCleanupPlan,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubjectObservations {
    pub subject: HostWorkSubject,
    pub set: super::dimension::HostWorkObservationSet,
    /// Provider-owned readiness facts carried verbatim into the status.
    pub supplied_readiness: Vec<CleanupReadiness>,
}

impl SubjectObservations {
    fn new(subject: HostWorkSubject) -> Self {
        let key = subject.subject_key();
        Self {
            subject,
            set: super::dimension::HostWorkObservationSet::new(key),
            supplied_readiness: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdapterError {
    WrongScope { expected: ObservationScope },
    SubjectMismatch { expected: String, actual: String },
}

impl std::fmt::Display for AdapterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AdapterError::WrongScope { expected } => {
                write!(f, "adapter requires a {expected:?} scope subject")
            }
            AdapterError::SubjectMismatch { expected, actual } => {
                write!(f, "subject mismatch: expected {expected}, got {actual}")
            }
        }
    }
}

impl std::error::Error for AdapterError {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorktreePlanAdapterOutcome {
    pub subjects: Vec<SubjectObservations>,
}

fn worktree_plan_provider(schema_version: &str) -> ProviderId {
    ProviderId {
        family: ProviderFamily::WorktreePlan,
        schema_version: schema_version.to_string(),
        source: "worktree_cleanup".to_string(),
    }
}

/// Map one landed `WorktreeCleanupPlan` (#10256/#10263) into per-worktree
/// observation sets. Reason tokens are carried as opaque limitations — never
/// parsed. An open PR is recorded as a remote-in-flight claim relationship;
/// it never forces physical retention (the clean/pushed facts decide).
pub fn adapt_worktree_cleanup_plan(
    plan: &WorktreeCleanupPlan,
) -> Result<WorktreePlanAdapterOutcome, AdapterError> {
    let mut outcome = WorktreePlanAdapterOutcome { subjects: Vec::new() };
    let provider = worktree_plan_provider(&plan.schema_version);

    for entry in &plan.entries {
        let subject = HostWorkSubject {
            repository_root: plan.subject.repository_root.clone(),
            common_dir: plan.subject.common_dir.clone(),
            canonical_remote: None,
            host_profile: "local".to_string(),
            scope: ObservationScope::Worktree,
            worktree: Some(WorktreeIdentity {
                path: entry.path.clone(),
                branch: entry.branch.clone(),
            }),
            candidate_id: None,
            executor_operation_id: None,
            allocation_id: None,
            reservation_id: None,
            process_group_id: None,
            storage_root: None,
        };
        let mut observations = SubjectObservations::new(subject);
        let key = observations.subject.subject_key();

        let dirty_observed = fact_bool(&entry.facts.dirty);
        let untracked_observed = fact_bool(&entry.facts.untracked);
        let unpushed_observed = fact_bool(&entry.facts.unpushed_commits);

        let index_state = match (entry.facts.dirty.state, entry.facts.untracked.state) {
            (ObservationState::NotProven, _) | (_, ObservationState::NotProven) => {
                super::dimension::IndexState::NotProven
            }
            _ => match (dirty_observed, untracked_observed) {
                (Some(dirty), Some(untracked)) if !dirty && !untracked => {
                    super::dimension::IndexState::Clean
                }
                (Some(dirty), Some(untracked)) => {
                    super::dimension::IndexState::Dirty { staged: dirty, untracked }
                }
                _ => super::dimension::IndexState::NotProven,
            },
        };
        let push_state = match entry.facts.unpushed_commits.state {
            ObservationState::NotProven => PushState::NotProven,
            _ => match unpushed_observed {
                Some(true) => PushState::Unpushed {
                    ahead_count: entry.facts.unpushed_ahead_count.unwrap_or(0),
                },
                Some(false) => PushState::Pushed,
                None => PushState::NotProven,
            },
        };
        let salvage_required = entry.classification == WorktreeClassification::Salvage;

        let mutation = MutationWorkObservation {
            subject_key: key.clone(),
            provider: provider.clone(),
            observed_at: plan.observed_at.clone(),
            ownership: MutationOwnership::Unowned,
            index_state,
            push_state,
            salvage_required,
            git_mutation_in_progress: false,
            orphan_premises: None,
            limitations: entry.reason_tokens.clone(),
            instrument: Instrument::Available,
        };

        let claim_relationship = claim_from_pr_facts(&entry.facts.open_pr, &entry.facts.merged_pr);
        let residue_present = salvage_required
            || index_state_is_residue(index_state)
            || matches!(push_state, PushState::Unpushed { .. });
        let durable_state = if residue_present {
            DurableState::UniqueLocalState
        } else {
            match (&claim_relationship, entry.classification) {
                (_, WorktreeClassification::Review) | (_, WorktreeClassification::NotProven) => {
                    DurableState::NotProven
                }
                (ClaimRelationship::LinkedToOpenPr { .. }, _) => DurableState::RemoteInFlight,
                (ClaimRelationship::LinkedToMergedPr { .. }, _) => DurableState::NoLocalResidue,
                (_, WorktreeClassification::Keep) => DurableState::ReconstructibleLocalState,
                (_, WorktreeClassification::CacheOnly) => DurableState::NoLocalResidue,
                (_, WorktreeClassification::Salvage) => DurableState::UniqueLocalState,
            }
        };
        let logical = LogicalWorkObservation {
            subject_key: key.clone(),
            provider: git_github_provider(&plan.schema_version),
            observed_at: plan.observed_at.clone(),
            freshness: Freshness::Current,
            claim_relationship,
            durable_state,
            orphan_premises: None,
            limitations: Vec::new(),
            instrument: Instrument::Available,
        };

        let disposition = match entry.classification {
            WorktreeClassification::Keep => StorageDisposition::Reconstructible,
            WorktreeClassification::CacheOnly => StorageDisposition::CacheOnly,
            WorktreeClassification::Salvage => StorageDisposition::Unique,
            WorktreeClassification::Review => StorageDisposition::Ambiguous,
            WorktreeClassification::NotProven => StorageDisposition::NotProven,
        };
        let storage = StorageWorkObservation {
            subject_key: key.clone(),
            provider: filesystem_storage_provider(&plan.schema_version),
            observed_at: plan.observed_at.clone(),
            root_class: RootClass::CandidatePrivate,
            volume_identity: VolumeIdentity::Unknown,
            free_capacity: CapacityFact::Unknown,
            configured_floor_bytes: None,
            below_configured_floor: false,
            footprint: FootprintFact::Unknown,
            disposition,
            reclaim_class: ReclaimClass::NoneApproved,
            orphan_premises: None,
            limitations: Vec::new(),
            instrument: Instrument::NotApplicable,
        };

        if entry.proposed_action.as_ref().is_some_and(|action| action.targetable) {
            observations.supplied_readiness.push(CleanupReadiness::WorktreeCleanupOwnedBy {
                owner: "#10256/#10263".to_string(),
            });
        } else if entry.proposed_action.is_some() {
            observations.supplied_readiness.push(CleanupReadiness::NotTargetable);
        }

        let set = &mut observations.set;
        set.push_mutation(mutation)?;
        set.push_logical(logical)?;
        set.push_storage(storage)?;

        outcome.subjects.push(observations);
    }
    Ok(outcome)
}

fn index_state_is_residue(state: super::dimension::IndexState) -> bool {
    match state {
        super::dimension::IndexState::Clean => false,
        super::dimension::IndexState::Dirty { staged, untracked } => staged || untracked,
        super::dimension::IndexState::MergeConflict | super::dimension::IndexState::LockHeld => {
            true
        }
        super::dimension::IndexState::NotProven => false,
    }
}

fn fact_bool(observation: &Observation<bool>) -> Option<bool> {
    match observation.state {
        ObservationState::Observed => observation.value,
        ObservationState::NotApplicable | ObservationState::NotProven => None,
    }
}

fn claim_from_pr_facts(
    open_pr: &Observation<PrMatch>,
    merged_pr: &Observation<PrMatch>,
) -> ClaimRelationship {
    match (&open_pr.state, &merged_pr.state) {
        (ObservationState::Observed, ObservationState::Observed) => {}
        (ObservationState::NotProven, _) | (_, ObservationState::NotProven) => {
            return ClaimRelationship::Unknown;
        }
        _ => return ClaimRelationship::Unknown,
    }
    match (&open_pr.value, &merged_pr.value) {
        (Some(PrMatch::Match { number, .. }), _) => {
            ClaimRelationship::LinkedToOpenPr { number: *number }
        }
        (_, Some(PrMatch::Match { number, .. })) => {
            ClaimRelationship::LinkedToMergedPr { number: *number }
        }
        (Some(PrMatch::None), Some(PrMatch::None)) => ClaimRelationship::Unlinked,
        _ => ClaimRelationship::Unknown,
    }
}

fn git_github_provider(schema_version: &str) -> ProviderId {
    ProviderId {
        family: ProviderFamily::GitGithubLogical,
        schema_version: schema_version.to_string(),
        source: "worktree_cleanup".to_string(),
    }
}

fn filesystem_storage_provider(schema_version: &str) -> ProviderId {
    ProviderId {
        family: ProviderFamily::FilesystemStorage,
        schema_version: schema_version.to_string(),
        source: "worktree_cleanup".to_string(),
    }
}

impl From<super::dimension::SubjectMismatch> for AdapterError {
    fn from(mismatch: super::dimension::SubjectMismatch) -> Self {
        AdapterError::SubjectMismatch { expected: mismatch.expected, actual: mismatch.actual }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdmissionAdapterOutcome {
    pub subject_observations: SubjectObservations,
}

const ADMISSION_SCHEMA_VERSION: &str = "admission_report.v1";
const KNOWN_ADMISSION_CHECKS: [&str; 7] = [
    "canonical-base",
    "shadow-ref",
    "symbolic-head",
    "branch-worktree-mapping",
    "dirty-unpushed",
    "disk-capacity",
    "writer-collision",
];

fn admission_provider() -> ProviderId {
    ProviderId {
        family: ProviderFamily::WriterAdmission,
        schema_version: ADMISSION_SCHEMA_VERSION.to_string(),
        source: "writer_admission".to_string(),
    }
}

/// Map one landed admission report (#3957/#11617 family) onto the repository
/// subject for its target branch. Check names are stable identifiers mapped
/// exhaustively; unknown names become visible unknown-variant rows. The
/// tri-state PR ownership is preserved exactly: `unknown` is never promoted
/// to `none`.
pub fn adapt_admission_report(
    report: &AdmissionReport,
    snapshot: Option<&WriterAdmissionSnapshot>,
    repository_subject: &HostWorkSubject,
) -> Result<AdmissionAdapterOutcome, AdapterError> {
    if repository_subject.scope != ObservationScope::Repository {
        return Err(AdapterError::WrongScope { expected: ObservationScope::Repository });
    }
    let mut subject = repository_subject.clone();
    subject.worktree = Some(WorktreeIdentity {
        path: repository_subject.repository_root.clone(),
        branch: Some(report.target_branch.clone()),
    });
    let mut observations = SubjectObservations::new(subject);
    let key = observations.subject.subject_key();
    let provider = admission_provider();
    let observed_at = String::from("admission-report");

    let writer_collision_block = report
        .checks
        .iter()
        .any(|check| check.name == "writer-collision" && check.status == CheckStatus::Block);
    // Only a proven blocking capacity result may claim the below-floor fact;
    // a NotProven probe stays instrument-unavailable and never becomes a
    // concrete LOW_DISK claim.
    let disk_below_floor = report
        .checks
        .iter()
        .any(|check| check.name == "disk-capacity" && check.status == CheckStatus::Block);
    let disk_not_proven = report
        .checks
        .iter()
        .any(|check| check.name == "disk-capacity" && check.status == CheckStatus::NotProven);
    let any_check_not_proven =
        report.checks.iter().any(|check| check.status == CheckStatus::NotProven);
    let verdict_not_proven = report.verdict == AdmissionVerdict::NotProven;

    let ownership = if writer_collision_block {
        MutationOwnership::Contested
    } else if any_check_not_proven || verdict_not_proven {
        MutationOwnership::NotProven
    } else {
        MutationOwnership::Unowned
    };

    let (index_state, push_state, snapshot_dirty_error) = match snapshot {
        Some(snapshot) => (
            if snapshot.dirty.error.is_some() || snapshot.head.dangling {
                super::dimension::IndexState::NotProven
            } else if snapshot.dirty.status_count > 0 {
                super::dimension::IndexState::Dirty { staged: true, untracked: false }
            } else {
                super::dimension::IndexState::Clean
            },
            match snapshot.dirty.unpushed_commits {
                0 => PushState::Pushed,
                ahead => PushState::Unpushed { ahead_count: u64::from(ahead) },
            },
            snapshot.dirty.error.clone(),
        ),
        None => (super::dimension::IndexState::NotProven, PushState::NotProven, None),
    };

    let mutation = MutationWorkObservation {
        subject_key: key.clone(),
        provider: provider.clone(),
        observed_at: observed_at.clone(),
        ownership,
        index_state,
        push_state,
        salvage_required: false,
        git_mutation_in_progress: false,
        orphan_premises: None,
        limitations: Vec::new(),
        instrument: if snapshot_dirty_error.is_some() {
            Instrument::Unavailable { detail: snapshot_dirty_error.unwrap_or_default() }
        } else {
            Instrument::Available
        },
    };

    let (claim_relationship, pr_lookup_failed) = match snapshot.map(|s| &s.pr_ownership) {
        Some(ownership_info) => match ownership_info.status {
            PrStatus::Open => (
                ClaimRelationship::LinkedToOpenPr { number: ownership_info.pr_number.unwrap_or(0) },
                false,
            ),
            PrStatus::None => (ClaimRelationship::Unlinked, false),
            // gh absent or query failed: stays unknown, never none.
            PrStatus::Unknown => (ClaimRelationship::Unknown, true),
        },
        None => (ClaimRelationship::Unknown, true),
    };
    let remote_lookup_failed = snapshot.map(|s| s.remote_branch.error.is_some()).unwrap_or(false);
    let logical = LogicalWorkObservation {
        subject_key: key.clone(),
        provider,
        observed_at,
        freshness: Freshness::Current,
        claim_relationship,
        durable_state: if pr_lookup_failed || remote_lookup_failed {
            DurableState::NotProven
        } else {
            DurableState::NoLocalResidue
        },
        orphan_premises: None,
        limitations: Vec::new(),
        instrument: if pr_lookup_failed {
            Instrument::Unavailable {
                detail: "pr ownership lookup failed or gh absent".to_string(),
            }
        } else {
            Instrument::Available
        },
    };

    let (free_capacity, volume_identity) = match snapshot.map(|s| &s.disk) {
        Some(disk) => match disk.avail_gb {
            Some(avail_gb) if avail_gb >= 0.0 => {
                let bytes = (avail_gb * 1_000_000_000.0).round();
                if bytes <= u64::MAX as f64 {
                    (CapacityFact::Measured { free_bytes: bytes as u64 }, VolumeIdentity::Unknown)
                } else {
                    (CapacityFact::Unknown, VolumeIdentity::Unknown)
                }
            }
            Some(_) => (CapacityFact::NotProven, VolumeIdentity::Unknown),
            None => (CapacityFact::Unknown, VolumeIdentity::Unknown),
        },
        None => (CapacityFact::Unknown, VolumeIdentity::Unknown),
    };
    let storage = StorageWorkObservation {
        subject_key: key.clone(),
        provider: ProviderId {
            family: ProviderFamily::FilesystemStorage,
            schema_version: ADMISSION_SCHEMA_VERSION.to_string(),
            source: "writer_admission".to_string(),
        },
        observed_at: String::from("admission-report"),
        root_class: RootClass::SharedCache,
        volume_identity,
        free_capacity,
        configured_floor_bytes: None,
        below_configured_floor: disk_below_floor,
        footprint: FootprintFact::NotApplicable,
        disposition: StorageDisposition::Reconstructible,
        reclaim_class: ReclaimClass::NoneApproved,
        orphan_premises: None,
        limitations: Vec::new(),
        instrument: if disk_not_proven {
            Instrument::Unavailable { detail: "disk capacity check not proven".to_string() }
        } else {
            Instrument::Available
        },
    };

    for record in UnknownVariantRecordIter::new(report) {
        observations.set.record_unknown_variant(record);
    }

    let set = &mut observations.set;
    set.push_mutation(mutation).map_err(AdapterError::from)?;
    set.push_logical(logical).map_err(AdapterError::from)?;
    set.push_storage(storage).map_err(AdapterError::from)?;

    Ok(AdmissionAdapterOutcome { subject_observations: observations })
}

struct UnknownVariantRecordIter<'a> {
    checks: &'a [CheckResult],
    index: usize,
}

impl<'a> UnknownVariantRecordIter<'a> {
    fn new(report: &'a AdmissionReport) -> Self {
        Self { checks: &report.checks, index: 0 }
    }
}

impl Iterator for UnknownVariantRecordIter<'_> {
    type Item = UnknownVariantRecord;

    fn next(&mut self) -> Option<Self::Item> {
        while self.index < self.checks.len() {
            let check = &self.checks[self.index];
            self.index += 1;
            if !KNOWN_ADMISSION_CHECKS.contains(&check.name.as_str()) {
                return Some(UnknownVariantRecord {
                    family: ProviderFamily::WriterAdmission,
                    schema_version: ADMISSION_SCHEMA_VERSION.to_string(),
                    source: "writer_admission".to_string(),
                    variant: check.name.clone(),
                    detail: Some(check.reason.clone()),
                });
            }
        }
        None
    }
}

// ---- Generic constructors for future #11650/#11653/#11659 owners ----------

/// Declare a provider family whose typed owner has not landed. Its absence
/// stays visible in every derived status (`NOT_PROVEN` aggregate token).
pub fn declare_missing_provider(family: ProviderFamily) -> MissingProviderDeclaration {
    MissingProviderDeclaration { family }
}

/// #11653-shaped hook: the capacity-reservation provider has no landed owner.
pub fn missing_capacity_reservation() -> MissingProviderDeclaration {
    declare_missing_provider(ProviderFamily::CapacityReservation)
}

/// #11650-shaped hook: the executor-allocation provider has no landed owner.
pub fn missing_executor_allocation() -> MissingProviderDeclaration {
    declare_missing_provider(ProviderFamily::ExecutorStateAllocation)
}

/// #11659-shaped hook: the process-observation provider has no landed owner.
pub fn missing_process_observation() -> MissingProviderDeclaration {
    declare_missing_provider(ProviderFamily::ProcessObservation)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MissingProviderDeclaration {
    pub family: ProviderFamily,
}

/// Build a compute row from #11653-shaped reservation facts alone.
pub fn reservation_only_compute_observation(
    subject_key: impl Into<String>,
    provider: ProviderId,
    observed_at: impl Into<String>,
    reservation: ReservationFact,
) -> ComputeWorkObservation {
    ComputeWorkObservation {
        subject_key: subject_key.into(),
        provider,
        observed_at: observed_at.into(),
        freshness: Freshness::Current,
        reservation,
        process_tree: ProcessTreeFact::TerminalityUnproven {
            detail: "process provider not landed (#11659)".to_string(),
        },
        descendants_settled: Settlement::NotProven,
        output_settled: Settlement::NotProven,
        initiator_returned: InitiatorReturn::Unknown,
        queue_depth: None,
        capacity_units_in_use: None,
        capacity_units_total: None,
        orphan_premises: None::<OrphanPremises>,
        limitations: vec!["reservation provider facts only".to_string()],
        instrument: Instrument::Available,
    }
}

/// Build a compute row from #11659-shaped process-tree facts.
pub fn process_tree_compute_observation(
    subject_key: impl Into<String>,
    provider: ProviderId,
    observed_at: impl Into<String>,
    process_group_id: impl Into<String>,
    attribution: Attribution,
) -> ComputeWorkObservation {
    ComputeWorkObservation {
        subject_key: subject_key.into(),
        provider,
        observed_at: observed_at.into(),
        freshness: Freshness::Current,
        reservation: ReservationFact::Absent,
        process_tree: ProcessTreeFact::Live {
            process_group_id: process_group_id.into(),
            attribution,
        },
        descendants_settled: Settlement::NotProven,
        output_settled: Settlement::NotProven,
        initiator_returned: InitiatorReturn::Unknown,
        queue_depth: None,
        capacity_units_in_use: None,
        capacity_units_total: None,
        orphan_premises: None::<OrphanPremises>,
        limitations: vec!["process provider facts only".to_string()],
        instrument: Instrument::Available,
    }
}

/// Build a storage row from #11650-shaped executor state allocation facts.
pub fn storage_scope_observation(
    subject_key: impl Into<String>,
    provider: ProviderId,
    observed_at: impl Into<String>,
    root_class: RootClass,
    disposition: StorageDisposition,
) -> StorageWorkObservation {
    StorageWorkObservation {
        subject_key: subject_key.into(),
        provider,
        observed_at: observed_at.into(),
        root_class,
        volume_identity: VolumeIdentity::Unknown,
        free_capacity: CapacityFact::Unknown,
        configured_floor_bytes: None,
        below_configured_floor: false,
        footprint: FootprintFact::Unknown,
        disposition,
        reclaim_class: ReclaimClass::NoneApproved,
        orphan_premises: None,
        limitations: vec!["executor-state provider facts only".to_string()],
        instrument: Instrument::Available,
    }
}
