//! Shift-left falsifier proofs for #11664 (see
//! `.spec/11664-host-work-status-domain/checklist.md` for the ID map).

use super::{
    AdapterError, AdmissionAdapterOutcome, Attribution, CapacityFact, ClaimRelationship,
    CleanupReadiness, ComputeWorkObservation, Dimension, DimensionEvidence, DurableState,
    FootprintFact, Freshness, HOST_WORK_STATUS_SCHEMA_VERSION, HostWorkClassification,
    HostWorkLifecycle, HostWorkObservationSet, HostWorkObservationToken, HostWorkReason,
    HostWorkStatus, HostWorkSubject, IndexState, InitiatorReturn, Instrument,
    LogicalWorkObservation, MissingProviderDeclaration, MutationOwnership, MutationWorkObservation,
    ObservationScope, OrphanPremises, ProcessTreeFact, ProviderFamily, ProviderId, PushState,
    ReclaimClass, ReservationFact, RootClass, Settlement, StatusError, StorageDisposition,
    StorageWorkObservation, SubjectObservations, UnknownVariantRecord, VolumeIdentity,
    WorktreeIdentity, WorktreePlanAdapterOutcome, adapt_admission_report,
    adapt_worktree_cleanup_plan, classify_compute, classify_logical, classify_mutation,
    classify_storage, declare_missing_provider, merge_classifications,
    missing_capacity_reservation, missing_executor_allocation, missing_process_observation,
    process_tree_compute_observation, reservation_only_compute_observation,
    storage_scope_observation,
};
use crate::tasks::writer_admission::{
    AdmissionGuidance, AdmissionReport, AdmissionVerdict, CheckResult, CheckStatus,
    WriterAdmissionSnapshot,
};
use xtask::worktree_cleanup::{
    Observation, ObservationState, PlanSummary, PrMatch, ProposedAction, RepositorySubject,
    WorktreeActionKind, WorktreeClassification, WorktreeCleanupPlan, WorktreeFacts,
    WorktreePlanEntry,
};

fn provider(family: ProviderFamily) -> ProviderId {
    ProviderId { family, schema_version: String::from("test.v1"), source: String::from("test") }
}

fn repository_subject(branch: Option<&str>) -> HostWorkSubject {
    let mut subject = HostWorkSubject {
        repository_root: std::path::PathBuf::from("E:/repo"),
        common_dir: std::path::PathBuf::from("E:/repo/.git"),
        canonical_remote: Some(String::from("origin")),
        host_profile: String::from("windows-local"),
        scope: ObservationScope::Repository,
        worktree: None,
        candidate_id: None,
        executor_operation_id: None,
        allocation_id: None,
        reservation_id: None,
        process_group_id: None,
        storage_root: None,
    };
    if let Some(branch) = branch {
        subject.worktree = Some(WorktreeIdentity {
            path: std::path::PathBuf::from("E:/repo"),
            branch: Some(branch.to_string()),
        });
    }
    subject
}

fn logical(key: &str, durable: DurableState) -> LogicalWorkObservation {
    LogicalWorkObservation {
        subject_key: key.to_string(),
        provider: provider(ProviderFamily::GitGithubLogical),
        observed_at: String::from("t0"),
        freshness: Freshness::Current,
        claim_relationship: ClaimRelationship::Unlinked,
        durable_state: durable,
        orphan_premises: None,
        limitations: Vec::new(),
        instrument: Instrument::Available,
    }
}

fn mutation(key: &str, ownership: MutationOwnership) -> MutationWorkObservation {
    MutationWorkObservation {
        subject_key: key.to_string(),
        provider: provider(ProviderFamily::WriterAdmission),
        observed_at: String::from("t0"),
        ownership,
        index_state: IndexState::Clean,
        push_state: PushState::Pushed,
        salvage_required: false,
        git_mutation_in_progress: false,
        orphan_premises: None,
        limitations: Vec::new(),
        instrument: Instrument::Available,
    }
}

fn compute(
    key: &str,
    process_tree: ProcessTreeFact,
    reservation: ReservationFact,
    descendants: Settlement,
    output: Settlement,
    initiator: InitiatorReturn,
) -> ComputeWorkObservation {
    ComputeWorkObservation {
        subject_key: key.to_string(),
        provider: provider(ProviderFamily::ProcessObservation),
        observed_at: String::from("t0"),
        freshness: Freshness::Current,
        reservation,
        process_tree,
        descendants_settled: descendants,
        output_settled: output,
        initiator_returned: initiator,
        queue_depth: None,
        capacity_units_in_use: None,
        capacity_units_total: None,
        orphan_premises: None,
        limitations: Vec::new(),
        instrument: Instrument::Available,
    }
}

#[allow(clippy::too_many_arguments)]
fn storage(
    key: &str,
    root_class: RootClass,
    disposition: StorageDisposition,
    free_capacity: CapacityFact,
    configured_floor_bytes: Option<u64>,
    below_configured_floor: bool,
    reclaim_class: ReclaimClass,
) -> StorageWorkObservation {
    StorageWorkObservation {
        subject_key: key.to_string(),
        provider: provider(ProviderFamily::FilesystemStorage),
        observed_at: String::from("t0"),
        root_class,
        volume_identity: VolumeIdentity::Unknown,
        free_capacity,
        configured_floor_bytes,
        below_configured_floor,
        footprint: FootprintFact::Unknown,
        disposition,
        reclaim_class,
        orphan_premises: None,
        limitations: Vec::new(),
        instrument: Instrument::Available,
    }
}

#[allow(clippy::too_many_arguments)]
fn worktree_entry(
    path: &str,
    branch: Option<&str>,
    classification: WorktreeClassification,
    dirty: bool,
    unpushed: bool,
    open_pr: Option<u64>,
) -> WorktreePlanEntry {
    let observation_bool =
        |value| Observation { state: ObservationState::Observed, value: Some(value), detail: None };
    WorktreePlanEntry {
        entry_id: format!("entry-{path}"),
        path: std::path::PathBuf::from(path),
        managed: true,
        primary: false,
        branch: branch.map(str::to_string),
        head: None,
        facts: WorktreeFacts {
            path_exists: observation_bool(true),
            administrative_path: Observation {
                state: ObservationState::NotApplicable,
                value: None,
                detail: None,
            },
            locked: false,
            lock_reason: None,
            prunable_reason: None,
            dirty: observation_bool(dirty),
            untracked: observation_bool(false),
            open_pr: match open_pr {
                Some(number) => Observation {
                    state: ObservationState::Observed,
                    value: Some(PrMatch::Match { number, head_oid: None }),
                    detail: None,
                },
                None => Observation {
                    state: ObservationState::Observed,
                    value: Some(PrMatch::None),
                    detail: None,
                },
            },
            merged_pr: Observation {
                state: ObservationState::Observed,
                value: Some(PrMatch::None),
                detail: None,
            },
            unpushed_commits: observation_bool(unpushed),
            unpushed_comparison_ref: None,
            unpushed_ahead_count: None,
        },
        classification,
        proposed_action: None,
        reason_tokens: Vec::new(),
        required_preconditions: Vec::new(),
    }
}

fn cleanup_plan(entries: Vec<WorktreePlanEntry>) -> WorktreeCleanupPlan {
    WorktreeCleanupPlan {
        schema_version: String::from("worktree_cleanup_plan.v1"),
        policy_version: String::from("2026-08-16"),
        observed_at: String::from("t0"),
        subject: RepositorySubject {
            requested_root: std::path::PathBuf::from("E:/repo"),
            repository_root: std::path::PathBuf::from("E:/repo"),
            common_dir: std::path::PathBuf::from("E:/repo/.git"),
            source_head: Observation {
                state: ObservationState::Observed,
                value: Some(String::from("0f00d")),
                detail: None,
            },
        },
        entries,
        summary: PlanSummary::default(),
        aggregate_classification: WorktreeClassification::Keep,
        plan_digest: String::from("digest"),
    }
}

fn admission_report(
    checks: Vec<(&str, CheckStatus, String)>,
    verdict: AdmissionVerdict,
) -> AdmissionReport {
    AdmissionReport {
        schema_version: String::from("1"),
        mode: String::from("report"),
        target_branch: String::from("agent/x"),
        verdict,
        checks: checks
            .into_iter()
            .map(|(name, status, reason)| CheckResult {
                name: name.to_string(),
                status,
                reason: reason.to_string(),
            })
            .collect(),
        guidance: AdmissionGuidance::default(),
    }
}

fn clean_snapshot(branch: &str, pr_open: bool) -> WriterAdmissionSnapshot {
    WriterAdmissionSnapshot {
        target_branch: branch.to_string(),
        requested_base: String::from("origin/main"),
        is_root_checkout: false,
        head: crate::tasks::writer_admission::HeadInfo {
            symbolic_ref: Some(format!("refs/heads/{branch}")),
            resolved_sha: Some(String::from("0f00d")),
            dangling: false,
            error: None,
        },
        shadow_refs: crate::tasks::writer_admission::ShadowRefInfo {
            refs: Vec::new(),
            error: None,
        },
        canonical_base: crate::tasks::writer_admission::CanonicalBaseInfo {
            remote_sha: None,
            selected_sha: None,
            error: None,
        },
        worktree_mapping: crate::tasks::writer_admission::WorktreeMappingInfo {
            entries: Vec::new(),
            error: None,
        },
        dirty: crate::tasks::writer_admission::DirtyInfo {
            status_count: 0,
            unpushed_commits: 0,
            error: None,
        },
        disk: crate::tasks::writer_admission::DiskInfo {
            avail_gb: Some(500.0),
            total_gb: Some(1000.0),
            worktree_count: Some(2),
            error: None,
        },
        pr_ownership: crate::tasks::writer_admission::PrOwnershipInfo {
            status: if pr_open {
                crate::tasks::writer_admission::PrStatus::Open
            } else {
                crate::tasks::writer_admission::PrStatus::None
            },
            pr_number: Some(4242),
            error: None,
        },
        remote_branch: crate::tasks::writer_admission::RemoteBranchInfo {
            sha: Some(String::from("0f00d")),
            error: None,
        },
    }
}

// ---- F1: agent/lane return never makes compute terminal --------------------

#[test]
fn host_work_status_f1_agent_return_not_terminal() {
    let subject = repository_subject(Some("wt"));
    let key = subject.subject_key();
    let classification = classify_compute(&compute(
        &key,
        ProcessTreeFact::Live {
            process_group_id: String::from("pg-1"),
            attribution: Attribution::ExactSubjectBinding,
        },
        ReservationFact::Released {
            reservation_id: String::from("r1"),
            settled: Settlement::Settled,
        },
        Settlement::Settled,
        Settlement::Settled,
        InitiatorReturn::Returned,
    ));
    assert_eq!(classification.lifecycle, HostWorkLifecycle::Stopping);
    assert!(
        classification.reasons.contains(&HostWorkReason::InitiatorReturnedButDescendantsUnsettled)
    );
    // The live tree is positive observed knowledge, so the evidence surface
    // is complete even though the lifecycle is STOPPING, not TERMINAL.
    assert_eq!(classification.evidence, DimensionEvidence::Complete);
}

// ---- F2: parent exit does not settle descendants/reservations --------------

#[test]
fn host_work_status_f2_parent_exit_not_terminal() {
    let subject = repository_subject(Some("wt"));
    let key = subject.subject_key();
    let classification = classify_compute(&compute(
        &key,
        ProcessTreeFact::ExitedConfirmed { process_group_id: String::from("pg-1") },
        ReservationFact::Released {
            reservation_id: String::from("r1"),
            settled: Settlement::Unsettled,
        },
        Settlement::Settled,
        Settlement::Settled,
        InitiatorReturn::Returned,
    ));
    assert_eq!(classification.lifecycle, HostWorkLifecycle::Stopping);
}

// ---- F3: process API unavailable is NOT_PROVEN, not zero processes ---------

#[test]
fn host_work_status_f3_api_unavailable_is_not_proven_not_zero() {
    let subject = repository_subject(Some("wt"));
    let key = subject.subject_key();
    let classification = classify_compute(&compute(
        &key,
        ProcessTreeFact::ApiUnavailable { detail: String::from("ps unavailable") },
        ReservationFact::Absent,
        Settlement::Settled,
        Settlement::Settled,
        InitiatorReturn::Unknown,
    ));
    assert_ne!(classification.lifecycle, HostWorkLifecycle::Terminal);
    assert_eq!(classification.lifecycle, HostWorkLifecycle::Ambiguous);
    assert!(classification.reasons.contains(&HostWorkReason::ProcessApiUnavailable));
    assert_eq!(classification.evidence, DimensionEvidence::Incomplete);

    let mut set = HostWorkObservationSet::new(key.clone());
    set.push_compute(compute(
        &key,
        ProcessTreeFact::ApiUnavailable { detail: String::from("ps unavailable") },
        ReservationFact::Absent,
        Settlement::Settled,
        Settlement::Settled,
        InitiatorReturn::Unknown,
    ))
    .expect("row bound to the set's own subject");
    let status = HostWorkStatus::build(&subject, &set, &[]).expect("single-subject set builds");
    assert!(status.aggregate.contains(&HostWorkObservationToken::NotProven));
}

// ---- F4: age alone can never create ORPHAN_CANDIDATE -----------------------

#[test]
fn host_work_status_f4_age_alone_never_orphan() {
    let subject = repository_subject(Some("wt"));
    let key = subject.subject_key();
    let aged_residue = MutationWorkObservation {
        subject_key: key.clone(),
        provider: provider(ProviderFamily::WriterAdmission),
        observed_at: String::from("1999-01-01T00:00:00Z"),
        ownership: MutationOwnership::Unowned,
        index_state: IndexState::Dirty { staged: true, untracked: true },
        push_state: PushState::Unpushed { ahead_count: 7 },
        salvage_required: true,
        git_mutation_in_progress: false,
        orphan_premises: None,
        limitations: Vec::new(),
        instrument: Instrument::Available,
    };
    let old = classify_mutation(&aged_residue);
    assert_eq!(old.lifecycle, HostWorkLifecycle::Ambiguous);

    // Partial premises stay ambiguous; only positive full evidence gates.
    let partial = MutationWorkObservation {
        orphan_premises: Some(OrphanPremises {
            ownership_established: true,
            targetability_established: false,
            cleanup_premises_fully_observable: true,
        }),
        ..clone_observation(&aged_residue)
    };
    assert_eq!(classify_mutation(&partial).lifecycle, HostWorkLifecycle::Ambiguous);

    let full = MutationWorkObservation {
        orphan_premises: Some(OrphanPremises {
            ownership_established: true,
            targetability_established: true,
            cleanup_premises_fully_observable: true,
        }),
        ..clone_observation(&aged_residue)
    };
    let gated = classify_mutation(&full);
    assert_eq!(gated.lifecycle, HostWorkLifecycle::OrphanCandidate);
    assert!(gated.reasons.contains(&HostWorkReason::OrphanPremisesProven));
}

fn clone_observation(observation: &MutationWorkObservation) -> MutationWorkObservation {
    observation.clone()
}

// ---- F5: executable name/path cannot attribute another repository ----------

#[test]
fn host_work_status_f5_basename_attribution_rejected() {
    let subject = repository_subject(Some("wt"));
    let key = subject.subject_key();
    let name_only = classify_compute(&compute(
        &key,
        ProcessTreeFact::Live {
            process_group_id: String::from("pg-9"),
            attribution: Attribution::ExecutableNameOnly,
        },
        ReservationFact::Absent,
        Settlement::Settled,
        Settlement::Settled,
        InitiatorReturn::StillWorking,
    ));
    assert_eq!(name_only.lifecycle, HostWorkLifecycle::Ambiguous);
    assert!(name_only.reasons.contains(&HostWorkReason::AttributionExecutableNameOnly));

    // The same-shaped row for a different subject is rejected outright.
    let other = repository_subject(Some("other-wt"));
    let other_key = other.subject_key();
    let mut set = HostWorkObservationSet::new(key);
    let foreign = compute(
        &other_key,
        ProcessTreeFact::Live {
            process_group_id: String::from("pg-9"),
            attribution: Attribution::ExactSubjectBinding,
        },
        ReservationFact::Active { reservation_id: String::from("r"), capacity_units: 1 },
        Settlement::Settled,
        Settlement::Settled,
        InitiatorReturn::StillWorking,
    );
    assert!(set.push_compute(foreign).is_err());
}

// ---- F6: no universal dispatch verdict exists ------------------------------

#[test]
fn host_work_status_f6_no_dispatch_verdict() {
    // The aggregate vocabulary is closed over observation tokens only; the
    // exhaustive match below compiles precisely because ADMIT/DENY-style
    // verdict variants cannot exist.
    let observed = [
        HostWorkObservationToken::Healthy,
        HostWorkObservationToken::Saturated,
        HostWorkObservationToken::Collision,
        HostWorkObservationToken::SalvageRequired,
    ];
    for token in observed {
        match token {
            HostWorkObservationToken::Healthy => {}
            HostWorkObservationToken::NotProven => {}
            HostWorkObservationToken::Ambiguous => {}
            HostWorkObservationToken::LowDisk => {}
            HostWorkObservationToken::Saturated => {}
            HostWorkObservationToken::Collision => {}
            HostWorkObservationToken::SalvageRequired => {}
        }
    }
    let subject = repository_subject(None);
    let key = subject.subject_key();
    let mut set = HostWorkObservationSet::new(key.clone());
    let mut heavy = compute(
        &key,
        ProcessTreeFact::NotApplicable,
        ReservationFact::Active { reservation_id: String::from("r"), capacity_units: 8 },
        Settlement::Settled,
        Settlement::Settled,
        InitiatorReturn::StillWorking,
    );
    heavy.capacity_units_in_use = Some(8);
    heavy.capacity_units_total = Some(8);
    set.push_compute(heavy).expect("own subject");
    let status = HostWorkStatus::build(&subject, &set, &[]).expect("builds");
    assert!(status.aggregate.contains(&HostWorkObservationToken::Saturated));
    assert!(!status.aggregate.iter().any(|token| token.as_str() == "ADMIT"));
    assert!(!status.aggregate.iter().any(|token| token.as_str() == "DENY"));
}

// ---- F7: an open PR does not force physical retention ----------------------

#[test]
fn host_work_status_f7_open_pr_allows_reconstructible_terminal() {
    let mut entry = worktree_entry(
        "E:/repo/.swarm/wt-open-pr",
        Some("agent/open-pr"),
        WorktreeClassification::Keep,
        false,
        false,
        Some(4242),
    );
    entry.proposed_action = Some(ProposedAction {
        kind: WorktreeActionKind::RemoveRegisteredWorktree,
        target: std::path::PathBuf::from("E:/repo/.swarm/wt-open-pr"),
        targetable: true,
    });
    let plan = cleanup_plan(vec![entry]);
    let outcome: WorktreePlanAdapterOutcome =
        adapt_worktree_cleanup_plan(&plan).expect("plan adapts");
    assert_eq!(outcome.subjects.len(), 1);
    let entry_subject: &SubjectObservations = &outcome.subjects[0];
    let status = HostWorkStatus::build(
        &entry_subject.subject,
        &entry_subject.set,
        &entry_subject.supplied_readiness,
    )
    .expect("builds");

    let mutation = status
        .classifications
        .iter()
        .find(|c| c.dimension == Dimension::Mutation)
        .expect("mutation classified");
    assert_eq!(mutation.lifecycle, HostWorkLifecycle::Terminal);
    assert!(mutation.reasons.contains(&HostWorkReason::CleanPushedTree));

    let logical = status
        .classifications
        .iter()
        .find(|c| c.dimension == Dimension::Logical)
        .expect("logical classified");
    assert_eq!(logical.lifecycle, HostWorkLifecycle::RemoteInFlight);

    // Readiness records the descriptive cleanup handoff; nothing forced KEEP.
    assert!(
        entry_subject
            .supplied_readiness
            .iter()
            .any(|readiness| matches!(readiness, CleanupReadiness::WorktreeCleanupOwnedBy { .. }))
    );
    // Clean pushed open-PR worktree: logical/mutation/storage are fully
    // decided and reconstructible, but this provider family supplies no
    // compute observation (#11659 has not landed), so the required compute
    // dimension stays visibly uncertain instead of collapsing to HEALTHY.
    let compute =
        status.classifications.iter().find(|c| c.dimension == Dimension::Compute).expect("compute");
    assert_eq!(compute.evidence, DimensionEvidence::Incomplete);
    assert!(status.aggregate.contains(&HostWorkObservationToken::Ambiguous));
    assert!(status.aggregate.contains(&HostWorkObservationToken::NotProven));
    assert!(!status.aggregate.contains(&HostWorkObservationToken::Healthy));
}

// ---- F8: a closed issue never makes unique dirty work removable ------------

#[test]
fn host_work_status_f8_closed_issue_keeps_salvage_required() {
    let subject = repository_subject(Some("wt"));
    let key = subject.subject_key();
    let mut set = HostWorkObservationSet::new(key.clone());
    let mut dirty_unique = mutation(&key, MutationOwnership::Unowned);
    dirty_unique.index_state = IndexState::Dirty { staged: true, untracked: true };
    dirty_unique.push_state = PushState::Unpushed { ahead_count: 3 };
    dirty_unique.salvage_required = true;
    set.push_mutation(dirty_unique).expect("own subject");
    let mut closed_issue_logical = logical(&key, DurableState::UniqueLocalState);
    closed_issue_logical.claim_relationship =
        ClaimRelationship::LinkedToClosedIssue { number: 999 };
    let closed_classification = classify_logical(&closed_issue_logical);
    assert_eq!(closed_classification.lifecycle, HostWorkLifecycle::Ambiguous);
    set.push_logical(closed_issue_logical).expect("own subject");

    let status = HostWorkStatus::build(&subject, &set, &[]).expect("builds");
    assert!(status.aggregate.contains(&HostWorkObservationToken::SalvageRequired));
    assert!(status.cleanup_readiness.contains(&CleanupReadiness::RequiresSalvage));
    let mutation_classification = status
        .classifications
        .iter()
        .find(|c| c.dimension == Dimension::Mutation)
        .expect("mutation");
    assert_ne!(mutation_classification.lifecycle, HostWorkLifecycle::Terminal);
    assert_ne!(mutation_classification.lifecycle, HostWorkLifecycle::OrphanCandidate);
}

// ---- F9: shared cache state is never candidate authority -------------------

#[test]
fn host_work_status_f9_shared_cache_not_candidate_authority() {
    let subject = repository_subject(Some("wt"));
    let key = subject.subject_key();

    let build_set = |root_class| {
        let mut set = HostWorkObservationSet::new(key.clone());
        set.push_storage(storage(
            &key,
            root_class,
            StorageDisposition::CacheOnly,
            CapacityFact::Measured { free_bytes: 900 },
            Some(200),
            false,
            ReclaimClass::Approved { class: String::from("cache"), owner: String::from("#10263") },
        ))
        .expect("own subject");
        set.push_mutation(mutation(&key, MutationOwnership::Unowned)).expect("own subject");
        set
    };

    let shared =
        HostWorkStatus::build(&subject, &build_set(RootClass::SharedCache), &[]).expect("builds");
    let private = HostWorkStatus::build(&subject, &build_set(RootClass::CandidatePrivate), &[])
        .expect("builds");

    // Whichever cache scope the storage row reports, the other three
    // dimensions are byte-identical between the two statuses.
    for dimension in [Dimension::Logical, Dimension::Mutation, Dimension::Compute] {
        let in_shared =
            shared.classifications.iter().find(|c| c.dimension == dimension).expect("present");
        let in_private =
            private.classifications.iter().find(|c| c.dimension == dimension).expect("present");
        assert_eq!(in_shared, in_private, "{dimension:?} must not move with storage scope");
    }
    assert!(shared.cleanup_readiness.contains(&CleanupReadiness::EligibleForCacheReclaimPlan));
}

// ---- F10: dimensions never collapse into one status ------------------------

#[test]
fn host_work_status_f10_dimensions_stay_independent() {
    let subject = repository_subject(Some("wt"));
    let key = subject.subject_key();
    let mut set = HostWorkObservationSet::new(key.clone());
    let mut active_writer = mutation(&key, MutationOwnership::Unowned);
    active_writer.ownership = MutationOwnership::ActiveWriter { writer_id: String::from("w1") };
    set.push_mutation(active_writer).expect("own subject");
    set.push_logical(logical(&key, DurableState::RemoteInFlight)).expect("own subject");
    set.push_compute(compute(
        &key,
        ProcessTreeFact::ExitedConfirmed { process_group_id: String::from("pg") },
        ReservationFact::Absent,
        Settlement::Settled,
        Settlement::Settled,
        InitiatorReturn::Returned,
    ))
    .expect("own subject");
    set.push_storage(storage(
        &key,
        RootClass::CandidatePrivate,
        StorageDisposition::Unique,
        CapacityFact::Measured { free_bytes: 10 },
        None,
        false,
        ReclaimClass::NoneApproved,
    ))
    .expect("own subject");

    let status = HostWorkStatus::build(&subject, &set, &[]).expect("builds");
    let lifecycles: Vec<(Dimension, HostWorkLifecycle)> =
        status.classifications.iter().map(|c| (c.dimension, c.lifecycle)).collect();
    assert!(lifecycles.contains(&(Dimension::Mutation, HostWorkLifecycle::Active)));
    assert!(lifecycles.contains(&(Dimension::Logical, HostWorkLifecycle::RemoteInFlight)));
    assert!(lifecycles.contains(&(Dimension::Compute, HostWorkLifecycle::Terminal)));
    assert!(lifecycles.contains(&(Dimension::Storage, HostWorkLifecycle::Ambiguous)));
}

// ---- F11: success/exit-zero signals cannot override typed blocked facts ----

#[test]
fn host_work_status_f11_exit_zero_cannot_override_typed_fact() {
    let report = admission_report(
        vec![
            ("writer-collision", CheckStatus::Block, String::from("open PR on agent/x")),
            ("canonical-base", CheckStatus::Pass, String::from("ok")),
            ("disk-capacity", CheckStatus::Pass, String::from("ok")),
        ],
        AdmissionVerdict::Block,
    );
    let snapshot = clean_snapshot("agent/x", true);
    let subject = repository_subject(None);

    // A worktree-scope subject is rejected: adapters bind one scope only.
    let mut wrong_scope = subject.clone();
    wrong_scope.scope = ObservationScope::Worktree;
    assert!(matches!(
        adapt_admission_report(&report, Some(&snapshot), &wrong_scope),
        Err(AdapterError::WrongScope { .. })
    ));

    let outcome: AdmissionAdapterOutcome =
        adapt_admission_report(&report, Some(&snapshot), &subject).expect("report adapts");
    let observations: &SubjectObservations = &outcome.subject_observations;
    let status = HostWorkStatus::build(
        &observations.subject,
        &observations.set,
        &observations.supplied_readiness,
    )
    .expect("builds");
    assert!(
        observations.set.mutation().iter().any(|row| row.ownership == MutationOwnership::Contested)
    );
    assert!(status.aggregate.contains(&HostWorkObservationToken::Collision));
}

// ---- F12: provider human wording is never authority ------------------------

#[test]
fn host_work_status_f12_provider_wording_is_not_authority() {
    let subject = repository_subject(None);
    let snapshot = clean_snapshot("agent/x", false);
    let calm = admission_report(
        vec![
            ("writer-collision", CheckStatus::Pass, String::from("no collision detected")),
            ("disk-capacity", CheckStatus::Pass, String::from("plenty of room")),
        ],
        AdmissionVerdict::Pass,
    );
    let alarming = admission_report(
        vec![
            ("writer-collision", CheckStatus::Pass, String::from("COLLISION IMMINENT DOOM")),
            ("disk-capacity", CheckStatus::Pass, String::from("DISK FULL CATASTROPHE")),
        ],
        AdmissionVerdict::Pass,
    );
    let calm_outcome = adapt_admission_report(&calm, Some(&snapshot), &subject).expect("adapts");
    let loud_outcome =
        adapt_admission_report(&alarming, Some(&snapshot), &subject).expect("adapts");
    let calm_status = HostWorkStatus::build(
        &calm_outcome.subject_observations.subject,
        &calm_outcome.subject_observations.set,
        &[],
    )
    .expect("builds");
    let loud_status = HostWorkStatus::build(
        &loud_outcome.subject_observations.subject,
        &loud_outcome.subject_observations.set,
        &[],
    )
    .expect("builds");
    assert_eq!(
        serde_json::to_string(&calm_status).expect("serializes"),
        serde_json::to_string(&loud_status).expect("serializes")
    );
}

// ---- F13: unknown provider variants stay visible ---------------------------

#[test]
fn host_work_status_f13_unknown_variant_visible() {
    let report = admission_report(
        vec![
            ("quantum-flux-check", CheckStatus::Pass, String::from("mystery pass")),
            ("disk-capacity", CheckStatus::Pass, String::from("ok")),
        ],
        AdmissionVerdict::Pass,
    );
    let snapshot = clean_snapshot("agent/x", false);
    let subject = repository_subject(None);
    let outcome = adapt_admission_report(&report, Some(&snapshot), &subject).expect("adapts");
    let observations = &outcome.subject_observations;
    assert_eq!(observations.set.unknown_variants().len(), 1);
    let record: UnknownVariantRecord = observations.set.unknown_variants()[0].clone();
    assert_eq!(record.variant, "quantum-flux-check");
    let status = HostWorkStatus::build(
        &observations.subject,
        &observations.set,
        &observations.supplied_readiness,
    )
    .expect("builds");
    assert!(status.aggregate.contains(&HostWorkObservationToken::Ambiguous));
    assert!(status.aggregate.contains(&HostWorkObservationToken::NotProven));
    assert!(!status.unknown_provider_variants.is_empty());
}

// ---- F14: one subject's resource can never satisfy another -----------------

#[test]
fn host_work_status_f14_subject_mismatch_unrepresentable() {
    let subject = repository_subject(Some("wt-a"));
    let other = repository_subject(Some("wt-b"));
    let mut set = HostWorkObservationSet::new(subject.subject_key());
    let foreign_logical = logical(&other.subject_key(), DurableState::NoLocalResidue);
    assert!(set.push_logical(foreign_logical).is_err());

    let built_for_other = HostWorkObservationSet::new(other.subject_key());
    match HostWorkStatus::build(&subject, &built_for_other, &[]) {
        Err(StatusError::SubjectMismatch { expected, actual }) => {
            assert_eq!(expected, subject.subject_key());
            assert_eq!(actual, other.subject_key());
        }
        other_result => panic!("expected subject mismatch, got {other_result:?}"),
    }
}

// ---- F15: aggregate HEALTHY can never hide required uncertainty ------------

#[test]
fn host_work_status_f15_healthy_never_hides_uncertainty() {
    let subject = repository_subject(Some("wt"));
    let key = subject.subject_key();
    let mut ambiguous_only = HostWorkObservationSet::new(key.clone());
    ambiguous_only.push_logical(logical(&key, DurableState::Contradictory)).expect("own subject");
    ambiguous_only.push_mutation(mutation(&key, MutationOwnership::Unowned)).expect("own subject");
    ambiguous_only
        .push_compute(compute(
            &key,
            ProcessTreeFact::ExitedConfirmed { process_group_id: String::from("pg") },
            ReservationFact::Absent,
            Settlement::Settled,
            Settlement::Settled,
            InitiatorReturn::Returned,
        ))
        .expect("own subject");
    let status = HostWorkStatus::build(&subject, &ambiguous_only, &[]).expect("builds");
    assert!(status.aggregate.contains(&HostWorkObservationToken::Ambiguous));
    assert!(!status.aggregate.contains(&HostWorkObservationToken::Healthy));

    // A declared-missing provider also blocks HEALTHY even with no rows.
    let mut missing = HostWorkObservationSet::new(key.clone());
    missing.declare_missing_provider(ProviderFamily::CapacityReservation);
    let status_missing =
        HostWorkStatus::build(&repository_subject(Some("wt")), &missing, &[]).expect("builds");
    assert!(status_missing.aggregate.contains(&HostWorkObservationToken::NotProven));
    assert!(!status_missing.aggregate.contains(&HostWorkObservationToken::Healthy));

    // Only fully decided, complete, benign evidence may be HEALTHY.
    let mut benign = HostWorkObservationSet::new(key);
    benign
        .push_logical(logical(benign.subject_key(), DurableState::NoLocalResidue))
        .expect("own subject");
    benign
        .push_mutation(mutation(benign.subject_key(), MutationOwnership::Unowned))
        .expect("own subject");
    benign
        .push_compute(compute(
            benign.subject_key(),
            ProcessTreeFact::ExitedConfirmed { process_group_id: String::from("pg") },
            ReservationFact::Absent,
            Settlement::Settled,
            Settlement::Settled,
            InitiatorReturn::Returned,
        ))
        .expect("own subject");
    benign
        .push_storage(storage(
            benign.subject_key(),
            RootClass::SharedCache,
            StorageDisposition::CacheOnly,
            CapacityFact::Measured { free_bytes: 500 },
            Some(200),
            false,
            ReclaimClass::NoneApproved,
        ))
        .expect("own subject");
    let healthy_status = HostWorkStatus::build(&subject, &benign, &[]).expect("builds");
    assert_eq!(healthy_status.aggregate, vec![HostWorkObservationToken::Healthy]);
}

// ---- F16: cleanup readiness is never cleanup authorization -----------------

#[test]
fn host_work_status_f16_readiness_is_not_authorization() {
    let subject = repository_subject(Some("wt"));
    let key = subject.subject_key();
    let mut set = HostWorkObservationSet::new(key.clone());
    set.push_compute(compute(
        &key,
        ProcessTreeFact::Live {
            process_group_id: String::from("pg-2"),
            attribution: Attribution::ExactSubjectBinding,
        },
        ReservationFact::Absent,
        Settlement::Unsettled,
        Settlement::Unsettled,
        InitiatorReturn::Returned,
    ))
    .expect("own subject");
    let status = HostWorkStatus::build(&subject, &set, &[]).expect("builds");
    // Descriptive handoff appears…
    assert!(status.cleanup_readiness.contains(&CleanupReadiness::EligibleForProcessReapPlan));
    // …but the status carries no plan, action, or verdict surface at all:
    // its aggregate remains pure observation and readiness stays a closed,
    // descriptive enum rendered without any authorization token.
    let rendered = status.render_human();
    assert!(rendered.contains("ELIGIBLE_FOR_PROCESS_REAP_PLAN"));
    assert!(!rendered.contains("AUTHORIZED"));
    assert!(!rendered.contains("APPROVED_FOR_EXECUTION"));
}

// ---- F17: input ordering cannot change semantic identity -------------------

#[test]
fn host_work_status_f17_ordering_does_not_change_identity() {
    let subject = repository_subject(Some("wt"));
    let key = subject.subject_key();

    let build = |order: [usize; 3]| {
        let rows = [
            logical(&key, DurableState::ReconstructibleLocalState),
            logical(&key, DurableState::RemoteInFlight),
            logical(&key, DurableState::NotProven),
        ];
        let mut set = HostWorkObservationSet::new(key.clone());
        for index in order {
            set.push_logical(clone_logical(&rows[index])).expect("own subject");
        }
        set.declare_missing_provider(ProviderFamily::FilesystemStorage);
        set.declare_missing_provider(ProviderFamily::CapacityReservation);
        HostWorkStatus::build(&subject, &set, &[]).expect("builds")
    };

    let one = serde_json::to_string(&build([0, 1, 2])).expect("serializes");
    let two = serde_json::to_string(&build([2, 0, 1])).expect("serializes");
    let three = serde_json::to_string(&build([1, 2, 0])).expect("serializes");
    assert_eq!(one, two);
    assert_eq!(two, three);
}

// ---- Future #11650/#11653/#11659 provider hooks stay visible and honest ----

#[test]
fn host_work_status_future_provider_hooks_classify_honestly() {
    let subject = repository_subject(Some("wt"));
    let key = subject.subject_key();

    // A reservation-only row (process owner not landed) is never terminal:
    let reservation_only = reservation_only_compute_observation(
        key.clone(),
        provider(ProviderFamily::CapacityReservation),
        "t0",
        ReservationFact::Active { reservation_id: String::from("r1"), capacity_units: 4 },
    );
    let reserved = classify_compute(&reservation_only);
    // The proven-active reservation is current work (ACTIVE), but the
    // unlanded process provider keeps the dimension's evidence incomplete.
    assert_eq!(reserved.lifecycle, HostWorkLifecycle::Active);
    assert_eq!(reserved.evidence, DimensionEvidence::Incomplete);
    assert!(reserved.reasons.contains(&HostWorkReason::ReservationActive));
    assert!(reserved.reasons.contains(&HostWorkReason::TerminalityNotProven));

    // A proven-active reservation with no process dimension at all is still
    // current work: ACTIVE comes from the reservation itself.
    let reservation_active = compute(
        &key,
        ProcessTreeFact::NotApplicable,
        ReservationFact::Active { reservation_id: String::from("r9"), capacity_units: 2 },
        Settlement::Settled,
        Settlement::Settled,
        InitiatorReturn::StillWorking,
    );
    assert_eq!(classify_compute(&reservation_active).lifecycle, HostWorkLifecycle::Active);

    // A name-attributed process tree (exact binding not yet supplied) stays
    // ambiguous even when the caller is tempted to claim it.
    let name_only = process_tree_compute_observation(
        key.clone(),
        provider(ProviderFamily::ProcessObservation),
        "t0",
        "pg-future",
        Attribution::ExecutableNameOnly,
    );
    assert_eq!(classify_compute(&name_only).lifecycle, HostWorkLifecycle::Ambiguous);

    // Executor-state storage scope without reclaim facts stays ambiguous.
    let scoped = storage_scope_observation(
        key.clone(),
        provider(ProviderFamily::ExecutorStateAllocation),
        "t0",
        RootClass::CandidatePrivate,
        StorageDisposition::Unique,
    );
    assert_eq!(classify_storage(&scoped).lifecycle, HostWorkLifecycle::Ambiguous);

    // Declared-missing providers surface as NOT_PROVEN + AMBIGUOUS aggregate.
    // Future-owner hooks all construct; their declarations stay typed data.
    let hooks: [MissingProviderDeclaration; 3] = [
        missing_capacity_reservation(),
        missing_executor_allocation(),
        missing_process_observation(),
    ];
    assert!(hooks.iter().all(|hook| hook.family != ProviderFamily::WorktreePlan));

    let mut set = HostWorkObservationSet::new(key.clone());
    set.push_compute(reservation_only).expect("own subject");
    declare_missing_provider(ProviderFamily::ProcessObservation);
    set.declare_missing_provider(ProviderFamily::CapacityReservation);
    let status = HostWorkStatus::build(&subject, &set, &[]).expect("builds");
    assert_eq!(status.schema_version, HOST_WORK_STATUS_SCHEMA_VERSION);
    assert!(status.aggregate.contains(&HostWorkObservationToken::NotProven));
    assert!(status.missing_providers.contains(&ProviderFamily::CapacityReservation));
}

// ---- Dimension merge: uncertainty dominates, reasons union -----------------

#[test]
fn host_work_status_merge_uncertainty_dominates_and_reasons_union() {
    let subject = repository_subject(Some("wt"));
    let key = subject.subject_key();
    let terminal = classify_compute(&compute(
        &key,
        ProcessTreeFact::ExitedConfirmed { process_group_id: String::from("pg") },
        ReservationFact::Absent,
        Settlement::Settled,
        Settlement::Settled,
        InitiatorReturn::Returned,
    ));
    let mut stopping = clone_compute(&terminal);
    stopping.lifecycle = HostWorkLifecycle::Stopping;
    stopping.reasons.push(HostWorkReason::DescendantsUnsettled);
    let merged = merge_classifications(&[terminal.clone(), stopping]);
    let merged = merged.expect("two rows merge");
    assert_eq!(merged.dimension, Dimension::Compute);
    assert_eq!(merged.lifecycle, HostWorkLifecycle::Stopping);
    assert!(merged.reasons.contains(&HostWorkReason::NoRelevantOwnershipRemains));
    assert!(merged.reasons.contains(&HostWorkReason::DescendantsUnsettled));
}

fn clone_compute(classification: &HostWorkClassification) -> HostWorkClassification {
    classification.clone()
}

fn clone_logical(observation: &LogicalWorkObservation) -> LogicalWorkObservation {
    observation.clone()
}

// ---- F18: the domain performs no live observation or mutation --------------

#[test]
fn host_work_status_f18_module_is_pure_no_io_or_process_surface() {
    const SOURCES: [&str; 5] =
        ["mod.rs", "subject.rs", "dimension.rs", "lifecycle.rs", "status.rs"];
    let manifest_dir = std::env!("CARGO_MANIFEST_DIR");
    for file in SOURCES {
        let path = std::path::Path::new(manifest_dir).join("src/host_work_status").join(file);
        let contents = std::fs::read_to_string(path)
            .unwrap_or_else(|error| panic!("reading domain source {file}: {error}"));
        for banned in
            ["std::process", "std::net", "Command::new", "fs::remove", "fs::write", "reqwest"]
        {
            assert!(!contents.contains(banned), "{file} must not reference {banned}");
        }
    }
}

// ---- Review response: readiness dedup is rank-independent -------------------

#[test]
fn host_work_status_readiness_dedup_collapses_nonadjacent_owned_by_duplicates() {
    let subject = repository_subject(Some("wt"));
    let key = subject.subject_key();
    let mut set = HostWorkObservationSet::new(key.clone());
    set.push_mutation(mutation(&key, MutationOwnership::Unowned)).expect("own subject");
    set.push_logical(logical(&key, DurableState::NoLocalResidue)).expect("own subject");
    set.push_compute(compute(
        &key,
        ProcessTreeFact::ExitedConfirmed { process_group_id: String::from("pg") },
        ReservationFact::Absent,
        Settlement::Settled,
        Settlement::Settled,
        InitiatorReturn::Returned,
    ))
    .expect("own subject");
    set.push_storage(storage(
        &key,
        RootClass::CandidatePrivate,
        StorageDisposition::Unique,
        CapacityFact::Measured { free_bytes: 900 },
        None,
        false,
        ReclaimClass::NoneApproved,
    ))
    .expect("own subject");

    // Two "B" rows separated by an equal-ranked "A" row: dedup must still
    // collapse them (all WorktreeCleanupOwnedBy variants share one rank).
    let supplied = vec![
        CleanupReadiness::WorktreeCleanupOwnedBy { owner: String::from("B") },
        CleanupReadiness::WorktreeCleanupOwnedBy { owner: String::from("A") },
        CleanupReadiness::WorktreeCleanupOwnedBy { owner: String::from("B") },
    ];
    let status = HostWorkStatus::build(&subject, &set, &supplied).expect("builds");

    let owned: Vec<&str> = status
        .cleanup_readiness
        .iter()
        .filter_map(|readiness| match readiness {
            CleanupReadiness::WorktreeCleanupOwnedBy { owner } => Some(owner.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(owned, vec!["A", "B"], "duplicate owners collapse; ties order deterministically");
}

// ---- Review response: the subject key is structurally collision-free -------

#[test]
fn host_work_status_subject_key_survives_delimiter_collision_attempts() {
    // Moving the U+001F separator between two adjacent string fields must
    // never merge two distinct subjects onto one key: the length-delimited
    // encoding pins every field boundary.
    let mut shifted = repository_subject(None);
    shifted.canonical_remote = Some(String::from("a\u{1f}b"));
    shifted.worktree = Some(WorktreeIdentity { path: std::path::PathBuf::from("c"), branch: None });

    let mut absorbed = repository_subject(None);
    absorbed.canonical_remote = Some(String::from("a"));
    absorbed.worktree =
        Some(WorktreeIdentity { path: std::path::PathBuf::from("b\u{1f}c"), branch: None });

    assert_ne!(shifted, absorbed, "fixture subjects must be distinct");
    assert_ne!(
        shifted.subject_key(),
        absorbed.subject_key(),
        "delimiter movement between adjacent fields cannot fuse identities"
    );

    // Equal subjects still produce equal keys, deterministically.
    let twin = shifted.clone();
    assert_eq!(shifted.subject_key(), twin.subject_key());

    // The key remains a pure function of the typed fields (no ambient state).
    assert_eq!(shifted.subject_key(), shifted.subject_key());
}

// ---- Review response: NOT_PROVEN disk evidence never claims LOW_DISK --------

#[test]
fn host_work_status_disk_not_proven_stays_unknown_not_low_disk() {
    let report = admission_report(
        vec![
            ("writer-collision", CheckStatus::Pass, String::from("ok")),
            ("disk-capacity", CheckStatus::NotProven, String::from("disk probe unavailable")),
        ],
        AdmissionVerdict::NotProven,
    );
    let snapshot = clean_snapshot("agent/x", false);
    let subject = repository_subject(None);
    let outcome = adapt_admission_report(&report, Some(&snapshot), &subject).expect("adapts");
    let observations = &outcome.subject_observations;
    let storage_row = observations.set.storage().iter().next().expect("storage row");
    assert!(
        !storage_row.below_configured_floor,
        "a failed/unavailable probe is not a below-floor capacity fact"
    );
    assert!(
        matches!(storage_row.instrument, Instrument::Unavailable { .. }),
        "the unproven probe stays visible on the instrument surface"
    );

    let status = HostWorkStatus::build(
        &observations.subject,
        &observations.set,
        &observations.supplied_readiness,
    )
    .expect("builds");
    assert!(
        !status.aggregate.contains(&HostWorkObservationToken::LowDisk),
        "NOT_PROVEN evidence must not emit the factual LOW_DISK token"
    );
    assert!(status.aggregate.contains(&HostWorkObservationToken::NotProven));
}

#[test]
fn host_work_status_disk_block_still_emits_low_disk() {
    // Positive control: a proven blocking capacity result keeps its
    // concrete below-floor claim.
    let report = admission_report(
        vec![("disk-capacity", CheckStatus::Block, String::from("free space below floor"))],
        AdmissionVerdict::Block,
    );
    let snapshot = clean_snapshot("agent/x", false);
    let subject = repository_subject(None);
    let outcome = adapt_admission_report(&report, Some(&snapshot), &subject).expect("adapts");
    let observations = &outcome.subject_observations;
    let storage_row = observations.set.storage().iter().next().expect("storage row");
    assert!(storage_row.below_configured_floor);

    let status = HostWorkStatus::build(
        &observations.subject,
        &observations.set,
        &observations.supplied_readiness,
    )
    .expect("builds");
    assert!(status.aggregate.contains(&HostWorkObservationToken::LowDisk));
}
