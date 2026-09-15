//! Closed lifecycle vocabulary, mechanical classification laws, aggregate
//! observations, and cleanup-readiness projections.
//!
//! Classification is total over the typed observations: every branch below is
//! exhaustive, required uncertainty is contagious into the aggregate, and the
//! aggregate never returns a dispatch verdict.

use super::dimension::{
    Attribution, CapacityFact, ClaimRelationship, ComputeWorkObservation, DurableState, Freshness,
    IndexState, InitiatorReturn, Instrument, LogicalWorkObservation, MutationOwnership,
    MutationWorkObservation, OrphanPremises, ProcessTreeFact, PushState, ReclaimClass,
    ReservationFact, RootClass, Settlement, StorageDisposition, StorageWorkObservation,
};
use serde::{Deserialize, Serialize};

/// Closed host-work lifecycle. The variants are mechanically distinct; see
/// the truth table in `.spec/11664-host-work-status-domain/acceptance.md`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum HostWorkLifecycle {
    Terminal,
    Queued,
    RemoteInFlight,
    OrphanCandidate,
    Active,
    Stopping,
    Ambiguous,
}

impl HostWorkLifecycle {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Terminal => "TERMINAL",
            Self::Queued => "QUEUED",
            Self::RemoteInFlight => "REMOTE_IN_FLIGHT",
            Self::OrphanCandidate => "ORPHAN_CANDIDATE",
            Self::Active => "ACTIVE",
            Self::Stopping => "STOPPING",
            Self::Ambiguous => "AMBIGUOUS",
        }
    }

    /// Merge rank: the most demanding lifecycle wins when several rows feed
    /// one dimension. Uncertainty (`AMBIGUOUS`) always dominates.
    const fn merge_rank(self) -> u8 {
        match self {
            Self::Terminal => 0,
            Self::Queued => 1,
            Self::RemoteInFlight => 2,
            Self::OrphanCandidate => 3,
            Self::Active => 4,
            Self::Stopping => 5,
            Self::Ambiguous => 6,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Dimension {
    Logical,
    Mutation,
    Compute,
    Storage,
}

impl Dimension {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Logical => "LOGICAL",
            Self::Mutation => "MUTATION",
            Self::Compute => "COMPUTE",
            Self::Storage => "STORAGE",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DimensionEvidence {
    Complete,
    Incomplete,
}

/// Closed cross-cutting reason vocabulary. One closed enum per repository;
/// two vocabularies cannot describe the same dimension because adapters have
/// no other constructor surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum HostWorkReason {
    // logical
    ClaimLinked,
    RemoteIntegrationWait,
    ReconstructibleLocal,
    UniqueLocalState,
    RelationshipUnknown,
    CurrentnessNotProven,
    // mutation
    WriterActive,
    MergeConflictPresent,
    GitLockHeld,
    UnpushedCommits,
    SalvageRequiredByProvider,
    CleanPushedTree,
    // compute
    ReservationActive,
    ReservationQueued,
    ReservationSettlementPending,
    ProcessTreeLive,
    DescendantsUnsettled,
    OutputDrainPending,
    InitiatorReturnedButDescendantsUnsettled,
    TerminalityNotProven,
    ProcessApiUnavailable,
    AttributionExact,
    AttributionExecutableNameOnly,
    AttributionUnattributed,
    QueueWaitingOnCapacity,
    // storage
    CandidatePrivateRoot,
    SharedCacheRoot,
    FreeCapacityBelowFloor,
    FreeCapacityUnknown,
    DispositionReconstructible,
    DispositionCacheOnly,
    DispositionUnique,
    DispositionAmbiguous,
    ReclaimClassApproved,
    // cross-cutting
    EvidenceContradictory,
    InstrumentUnavailable,
    UnknownProviderVariant,
    OrphanPremisesProven,
    NoRelevantOwnershipRemains,
}

impl HostWorkReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ClaimLinked => "CLAIM_LINKED",
            Self::RemoteIntegrationWait => "REMOTE_INTEGRATION_WAIT",
            Self::ReconstructibleLocal => "RECONSTRUCTIBLE_LOCAL",
            Self::UniqueLocalState => "UNIQUE_LOCAL_STATE",
            Self::RelationshipUnknown => "RELATIONSHIP_UNKNOWN",
            Self::CurrentnessNotProven => "CURRENTNESS_NOT_PROVEN",
            Self::WriterActive => "WRITER_ACTIVE",
            Self::MergeConflictPresent => "MERGE_CONFLICT_PRESENT",
            Self::GitLockHeld => "GIT_LOCK_HELD",
            Self::UnpushedCommits => "UNPUSHED_COMMITS",
            Self::SalvageRequiredByProvider => "SALVAGE_REQUIRED_BY_PROVIDER",
            Self::CleanPushedTree => "CLEAN_PUSHED_TREE",
            Self::ReservationActive => "RESERVATION_ACTIVE",
            Self::ReservationQueued => "RESERVATION_QUEUED",
            Self::ReservationSettlementPending => "RESERVATION_SETTLEMENT_PENDING",
            Self::ProcessTreeLive => "PROCESS_TREE_LIVE",
            Self::DescendantsUnsettled => "DESCENDANTS_UNSETTLED",
            Self::OutputDrainPending => "OUTPUT_DRAIN_PENDING",
            Self::InitiatorReturnedButDescendantsUnsettled => {
                "INITIATOR_RETURNED_BUT_DESCENDANTS_UNSETTLED"
            }
            Self::TerminalityNotProven => "TERMINALITY_NOT_PROVEN",
            Self::ProcessApiUnavailable => "PROCESS_API_UNAVAILABLE",
            Self::AttributionExact => "ATTRIBUTION_EXACT",
            Self::AttributionExecutableNameOnly => "ATTRIBUTION_EXECUTABLE_NAME_ONLY",
            Self::AttributionUnattributed => "ATTRIBUTION_UNATTRIBUTED",
            Self::QueueWaitingOnCapacity => "QUEUE_WAITING_ON_CAPACITY",
            Self::CandidatePrivateRoot => "CANDIDATE_PRIVATE_ROOT",
            Self::SharedCacheRoot => "SHARED_CACHE_ROOT",
            Self::FreeCapacityBelowFloor => "FREE_CAPACITY_BELOW_FLOOR",
            Self::FreeCapacityUnknown => "FREE_CAPACITY_UNKNOWN",
            Self::DispositionReconstructible => "DISPOSITION_RECONSTRUCTIBLE",
            Self::DispositionCacheOnly => "DISPOSITION_CACHE_ONLY",
            Self::DispositionUnique => "DISPOSITION_UNIQUE",
            Self::DispositionAmbiguous => "DISPOSITION_AMBIGUOUS",
            Self::ReclaimClassApproved => "RECLAIM_CLASS_APPROVED",
            Self::EvidenceContradictory => "EVIDENCE_CONTRADICTORY",
            Self::InstrumentUnavailable => "INSTRUMENT_UNAVAILABLE",
            Self::UnknownProviderVariant => "UNKNOWN_PROVIDER_VARIANT",
            Self::OrphanPremisesProven => "ORPHAN_PREMISES_PROVEN",
            Self::NoRelevantOwnershipRemains => "NO_RELEVANT_OWNERSHIP_REMAINS",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostWorkClassification {
    pub dimension: Dimension,
    pub lifecycle: HostWorkLifecycle,
    /// Sorted, deduplicated reasons.
    pub reasons: Vec<HostWorkReason>,
    pub evidence: DimensionEvidence,
}

/// A provider family with no landed typed owner. Its absence stays visible
/// until its owner lands an adapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum HostWorkObservationToken {
    Healthy,
    NotProven,
    Ambiguous,
    LowDisk,
    Saturated,
    Collision,
    SalvageRequired,
}

impl HostWorkObservationToken {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Healthy => "HEALTHY",
            Self::NotProven => "NOT_PROVEN",
            Self::Ambiguous => "AMBIGUOUS",
            Self::LowDisk => "LOW_DISK",
            Self::Saturated => "SATURATED",
            Self::Collision => "COLLISION",
            Self::SalvageRequired => "SALVAGE_REQUIRED",
        }
    }

    /// Severity for canonical aggregate ordering (ascending; worst last).
    /// Uncertainty ranks below concrete findings so both stay visible.
    pub const fn severity(self) -> u8 {
        match self {
            Self::Healthy => 0,
            Self::NotProven => 1,
            Self::Ambiguous => 2,
            Self::LowDisk => 3,
            Self::Saturated => 4,
            Self::Collision => 5,
            Self::SalvageRequired => 6,
        }
    }
}

/// Descriptive cleanup handoffs. These fields state what a *future* plan
/// family could consider; they are not authorization and this domain creates
/// no plan.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CleanupReadiness {
    NotTargetable,
    RequiresSalvage,
    ReadOnlyObservationComplete,
    EligibleForProcessReapPlan,
    EligibleForCacheReclaimPlan,
    WorktreeCleanupOwnedBy { owner: String },
    NotProven,
}

fn classify_orphan_gate(
    residue_reasons: &[HostWorkReason],
    orphan_premises: Option<&OrphanPremises>,
) -> Option<(HostWorkLifecycle, Vec<HostWorkReason>)> {
    let premises = orphan_premises?;
    if !premises.all_proven() {
        return None;
    }
    let mut reasons = vec![HostWorkReason::OrphanPremisesProven];
    reasons.extend_from_slice(residue_reasons);
    Some((HostWorkLifecycle::OrphanCandidate, reasons))
}

fn instrument_incomplete(instrument: &Instrument) -> (bool, Vec<HostWorkReason>) {
    match instrument {
        Instrument::Unavailable { .. } => (true, vec![HostWorkReason::InstrumentUnavailable]),
        _ => (false, Vec::new()),
    }
}

fn freshness_incomplete(freshness: Freshness) -> (bool, Vec<HostWorkReason>) {
    match freshness {
        Freshness::Current => (false, Vec::new()),
        Freshness::Stale => (false, vec![HostWorkReason::CurrentnessNotProven]),
        Freshness::NotProven => (
            true,
            vec![HostWorkReason::CurrentnessNotProven, HostWorkReason::InstrumentUnavailable],
        ),
    }
}

pub fn classify_logical(observation: &LogicalWorkObservation) -> HostWorkClassification {
    let mut reasons: Vec<HostWorkReason> = Vec::new();
    let mut incomplete = false;

    let (bad_instrument, mut add) = instrument_incomplete(&observation.instrument);
    incomplete |= bad_instrument;
    reasons.append(&mut add);
    let (stale, mut add) = freshness_incomplete(observation.freshness);
    incomplete |= stale;
    reasons.append(&mut add);

    match observation.claim_relationship {
        ClaimRelationship::Unknown => {
            reasons.push(HostWorkReason::RelationshipUnknown);
            incomplete = true;
        }
        _ => reasons.push(HostWorkReason::ClaimLinked),
    }

    let (lifecycle, residue_reasons): (HostWorkLifecycle, Vec<HostWorkReason>) = match observation
        .durable_state
    {
        DurableState::NoLocalResidue => {
            (HostWorkLifecycle::Terminal, vec![HostWorkReason::NoRelevantOwnershipRemains])
        }
        DurableState::ReconstructibleLocalState => {
            (HostWorkLifecycle::Terminal, vec![HostWorkReason::ReconstructibleLocal])
        }
        DurableState::RemoteInFlight => {
            (HostWorkLifecycle::RemoteInFlight, vec![HostWorkReason::RemoteIntegrationWait])
        }
        DurableState::UniqueLocalState => {
            (HostWorkLifecycle::Ambiguous, vec![HostWorkReason::UniqueLocalState])
        }
        DurableState::Contradictory => {
            (HostWorkLifecycle::Ambiguous, vec![HostWorkReason::EvidenceContradictory])
        }
        DurableState::NotProven => {
            incomplete = true;
            (
                HostWorkLifecycle::Ambiguous,
                vec![HostWorkReason::RelationshipUnknown, HostWorkReason::InstrumentUnavailable],
            )
        }
    };

    let (lifecycle, mut more) = if lifecycle == HostWorkLifecycle::Ambiguous
        && residue_reasons.contains(&HostWorkReason::UniqueLocalState)
    {
        match classify_orphan_gate(&residue_reasons, observation.orphan_premises.as_ref()) {
            Some(gated) => gated,
            None => (lifecycle, residue_reasons),
        }
    } else {
        (lifecycle, residue_reasons)
    };
    reasons.append(&mut more);

    finish(Dimension::Logical, lifecycle, reasons, incomplete)
}

pub fn classify_mutation(observation: &MutationWorkObservation) -> HostWorkClassification {
    let mut reasons: Vec<HostWorkReason> = Vec::new();
    let mut incomplete = false;

    let (bad_instrument, mut add) = instrument_incomplete(&observation.instrument);
    incomplete |= bad_instrument;
    reasons.append(&mut add);

    match observation.index_state {
        IndexState::NotProven => {
            incomplete = true;
            reasons.push(HostWorkReason::InstrumentUnavailable);
        }
        IndexState::Dirty { .. } => reasons.push(HostWorkReason::SalvageRequiredByProvider),
        IndexState::MergeConflict => reasons.push(HostWorkReason::MergeConflictPresent),
        IndexState::LockHeld => reasons.push(HostWorkReason::GitLockHeld),
        IndexState::Clean => {}
    }
    match observation.push_state {
        PushState::NotProven => {
            incomplete = true;
            reasons.push(HostWorkReason::InstrumentUnavailable);
        }
        PushState::Unpushed { .. } | PushState::NoRemoteBranch => {
            reasons.push(HostWorkReason::UnpushedCommits)
        }
        PushState::Pushed => {}
    }
    if observation.salvage_required {
        reasons.push(HostWorkReason::SalvageRequiredByProvider);
    }

    let has_residue = !matches!(observation.index_state, IndexState::Clean)
        || !matches!(observation.push_state, PushState::Pushed)
        || observation.salvage_required;

    let (lifecycle, mut more) = if observation.git_mutation_in_progress
        || observation.index_state == IndexState::LockHeld
    {
        (HostWorkLifecycle::Active, vec![HostWorkReason::GitLockHeld])
    } else {
        match observation.ownership {
            MutationOwnership::ActiveWriter { .. } => {
                (HostWorkLifecycle::Active, vec![HostWorkReason::WriterActive])
            }
            MutationOwnership::Contested => {
                (HostWorkLifecycle::Ambiguous, vec![HostWorkReason::EvidenceContradictory])
            }
            MutationOwnership::NotProven => {
                incomplete = true;
                (HostWorkLifecycle::Ambiguous, vec![HostWorkReason::InstrumentUnavailable])
            }
            MutationOwnership::Unowned => {
                if observation.index_state == IndexState::MergeConflict {
                    (HostWorkLifecycle::Stopping, vec![HostWorkReason::MergeConflictPresent])
                } else if has_residue {
                    match classify_orphan_gate(
                        &[HostWorkReason::UniqueLocalState],
                        observation.orphan_premises.as_ref(),
                    ) {
                        Some(gated) => gated,
                        None => {
                            (HostWorkLifecycle::Ambiguous, vec![HostWorkReason::UniqueLocalState])
                        }
                    }
                } else {
                    (
                        HostWorkLifecycle::Terminal,
                        vec![
                            HostWorkReason::CleanPushedTree,
                            HostWorkReason::NoRelevantOwnershipRemains,
                        ],
                    )
                }
            }
        }
    };
    reasons.append(&mut more);

    finish(Dimension::Mutation, lifecycle, reasons, incomplete)
}

pub fn classify_compute(observation: &ComputeWorkObservation) -> HostWorkClassification {
    let mut reasons: Vec<HostWorkReason> = Vec::new();
    let mut incomplete = false;

    let (bad_instrument, mut add) = instrument_incomplete(&observation.instrument);
    incomplete |= bad_instrument;
    reasons.append(&mut add);

    let mut terminality_proven = false;
    let mut live_exact = false;
    let mut attribution_problem = false;
    match &observation.process_tree {
        ProcessTreeFact::Live { attribution, .. } => match attribution {
            Attribution::ExactSubjectBinding => {
                live_exact = true;
                reasons.push(HostWorkReason::AttributionExact);
                reasons.push(HostWorkReason::ProcessTreeLive);
            }
            Attribution::ExecutableNameOnly => {
                attribution_problem = true;
                incomplete = true;
                reasons.push(HostWorkReason::AttributionExecutableNameOnly);
                reasons.push(HostWorkReason::ProcessTreeLive);
            }
            Attribution::Unattributed => {
                attribution_problem = true;
                incomplete = true;
                reasons.push(HostWorkReason::AttributionUnattributed);
                reasons.push(HostWorkReason::ProcessTreeLive);
            }
        },
        ProcessTreeFact::ExitedConfirmed { .. } => terminality_proven = true,
        ProcessTreeFact::TerminalityUnproven { .. } => {
            incomplete = true;
            reasons.push(HostWorkReason::TerminalityNotProven);
        }
        ProcessTreeFact::ApiUnavailable { .. } => {
            incomplete = true;
            reasons.push(HostWorkReason::ProcessApiUnavailable);
            reasons.push(HostWorkReason::TerminalityNotProven);
        }
        ProcessTreeFact::NotApplicable => terminality_proven = true,
    }

    let mut reservation_unsettled = false;
    match &observation.reservation {
        ReservationFact::Active { .. } => reasons.push(HostWorkReason::ReservationActive),
        ReservationFact::Queued { .. } => reasons.push(HostWorkReason::QueueWaitingOnCapacity),
        ReservationFact::Released { settled, .. } => match settled {
            Settlement::Settled => {}
            Settlement::Unsettled => {
                reservation_unsettled = true;
                reasons.push(HostWorkReason::ReservationSettlementPending);
            }
            Settlement::NotProven => {
                reservation_unsettled = true;
                incomplete = true;
                reasons.push(HostWorkReason::ReservationSettlementPending);
                reasons.push(HostWorkReason::InstrumentUnavailable);
            }
        },
        ReservationFact::Absent => {}
        ReservationFact::NotProven => {
            reservation_unsettled = true;
            incomplete = true;
            reasons.push(HostWorkReason::InstrumentUnavailable);
        }
    }

    let mut descendants_unsettled = false;
    match observation.descendants_settled {
        Settlement::Settled => {}
        Settlement::Unsettled => {
            descendants_unsettled = true;
            reasons.push(HostWorkReason::DescendantsUnsettled);
        }
        Settlement::NotProven => {
            descendants_unsettled = true;
            incomplete = true;
            reasons.push(HostWorkReason::DescendantsUnsettled);
            reasons.push(HostWorkReason::InstrumentUnavailable);
        }
    }

    let mut output_unsettled = false;
    match observation.output_settled {
        Settlement::Settled => {}
        Settlement::Unsettled => {
            output_unsettled = true;
            reasons.push(HostWorkReason::OutputDrainPending);
        }
        Settlement::NotProven => {
            output_unsettled = true;
            incomplete = true;
            reasons.push(HostWorkReason::OutputDrainPending);
            reasons.push(HostWorkReason::InstrumentUnavailable);
        }
    }

    let anything_unsettled =
        reservation_unsettled || descendants_unsettled || output_unsettled || !terminality_proven;
    let initiator_returned = observation.initiator_returned == InitiatorReturn::Returned;

    let (lifecycle, mut more) = if attribution_problem {
        (HostWorkLifecycle::Ambiguous, Vec::new())
    } else if initiator_returned && anything_unsettled {
        let mut stop = vec![HostWorkReason::InitiatorReturnedButDescendantsUnsettled];
        if reservation_unsettled || descendants_unsettled || output_unsettled {
            stop.push(HostWorkReason::DescendantsUnsettled);
        }
        (HostWorkLifecycle::Stopping, stop)
    } else if live_exact {
        (HostWorkLifecycle::Active, Vec::new())
    } else if matches!(observation.reservation, ReservationFact::Queued { .. }) {
        (HostWorkLifecycle::Queued, vec![HostWorkReason::ReservationQueued])
    } else if matches!(observation.reservation, ReservationFact::Active { .. }) {
        // A proven-active capacity reservation is itself current work
        // (ACTIVE definition), independent of process supervision.
        (HostWorkLifecycle::Active, Vec::new())
    } else if terminality_proven
        && !reservation_unsettled
        && !descendants_unsettled
        && !output_unsettled
        && observation.freshness == Freshness::Current
    {
        (HostWorkLifecycle::Terminal, vec![HostWorkReason::NoRelevantOwnershipRemains])
    } else if observation.freshness != Freshness::Current {
        incomplete = true;
        (HostWorkLifecycle::Ambiguous, vec![HostWorkReason::CurrentnessNotProven])
    } else {
        incomplete = true;
        (HostWorkLifecycle::Ambiguous, vec![HostWorkReason::TerminalityNotProven])
    };
    reasons.append(&mut more);

    finish(Dimension::Compute, lifecycle, reasons, incomplete)
}

pub fn classify_storage(observation: &StorageWorkObservation) -> HostWorkClassification {
    let mut reasons: Vec<HostWorkReason> = Vec::new();
    let mut incomplete = false;

    let (bad_instrument, mut add) = instrument_incomplete(&observation.instrument);
    incomplete |= bad_instrument;
    reasons.append(&mut add);

    match observation.free_capacity {
        CapacityFact::Measured { free_bytes } => {
            if observation.configured_floor_bytes.is_some_and(|floor| free_bytes < floor) {
                reasons.push(HostWorkReason::FreeCapacityBelowFloor);
            }
        }
        CapacityFact::Unknown => reasons.push(HostWorkReason::FreeCapacityUnknown),
        CapacityFact::NotProven => {
            incomplete = true;
            reasons.push(HostWorkReason::FreeCapacityUnknown);
            reasons.push(HostWorkReason::InstrumentUnavailable);
        }
    }
    if observation.below_configured_floor {
        reasons.push(HostWorkReason::FreeCapacityBelowFloor);
    }
    if matches!(observation.volume_identity, super::dimension::VolumeIdentity::NotProven) {
        incomplete = true;
        reasons.push(HostWorkReason::InstrumentUnavailable);
    }
    match observation.reclaim_class {
        ReclaimClass::Approved { .. } => reasons.push(HostWorkReason::ReclaimClassApproved),
        ReclaimClass::NoneApproved => {}
        ReclaimClass::NotProven => {
            incomplete = true;
            reasons.push(HostWorkReason::InstrumentUnavailable);
        }
    }

    match observation.root_class {
        RootClass::SharedCache => reasons.push(HostWorkReason::SharedCacheRoot),
        RootClass::CandidatePrivate => reasons.push(HostWorkReason::CandidatePrivateRoot),
        RootClass::NotProven => {
            incomplete = true;
            reasons.push(HostWorkReason::InstrumentUnavailable);
        }
        RootClass::AmbiguousScope => reasons.push(HostWorkReason::DispositionAmbiguous),
    }

    match observation.disposition {
        StorageDisposition::CacheOnly => reasons.push(HostWorkReason::DispositionCacheOnly),
        StorageDisposition::Reconstructible => {
            reasons.push(HostWorkReason::DispositionReconstructible)
        }
        StorageDisposition::Unique => reasons.push(HostWorkReason::DispositionUnique),
        StorageDisposition::Ambiguous => reasons.push(HostWorkReason::DispositionAmbiguous),
        StorageDisposition::NotProven => {
            incomplete = true;
            reasons.push(HostWorkReason::InstrumentUnavailable);
        }
    }

    let (lifecycle, mut more) = match observation.disposition {
        StorageDisposition::Unique => {
            match classify_orphan_gate(&[], observation.orphan_premises.as_ref()) {
                Some(gated) => gated,
                None => (HostWorkLifecycle::Ambiguous, Vec::new()),
            }
        }
        StorageDisposition::Ambiguous | StorageDisposition::NotProven => {
            (HostWorkLifecycle::Ambiguous, Vec::new())
        }
        StorageDisposition::CacheOnly | StorageDisposition::Reconstructible => {
            (HostWorkLifecycle::Terminal, vec![HostWorkReason::NoRelevantOwnershipRemains])
        }
    };
    reasons.append(&mut more);

    finish(Dimension::Storage, lifecycle, reasons, incomplete)
}

fn finish(
    dimension: Dimension,
    lifecycle: HostWorkLifecycle,
    mut reasons: Vec<HostWorkReason>,
    incomplete: bool,
) -> HostWorkClassification {
    reasons.sort();
    reasons.dedup();
    HostWorkClassification {
        dimension,
        lifecycle,
        reasons,
        evidence: if incomplete {
            DimensionEvidence::Incomplete
        } else {
            DimensionEvidence::Complete
        },
    }
}

/// Merge several classifications of one dimension: most demanding lifecycle
/// wins, uncertainty outranks everything, reasons union. `None` only when no
/// row exists for the dimension (callers treat that as missing evidence).
pub fn merge_classifications(
    classifications: &[HostWorkClassification],
) -> Option<HostWorkClassification> {
    let first = classifications.first()?;
    let dimension = first.dimension;
    let mut lifecycle = HostWorkLifecycle::Terminal;
    let mut evidence = DimensionEvidence::Complete;
    let mut reasons: Vec<HostWorkReason> = Vec::new();
    for classification in classifications {
        if classification.lifecycle.merge_rank() > lifecycle.merge_rank() {
            lifecycle = classification.lifecycle;
        }
        if classification.evidence == DimensionEvidence::Incomplete {
            evidence = DimensionEvidence::Incomplete;
        }
        reasons.extend_from_slice(&classification.reasons);
    }
    reasons.sort();
    reasons.dedup();
    Some(HostWorkClassification { dimension, lifecycle, reasons, evidence })
}
