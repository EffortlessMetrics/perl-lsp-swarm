//! Shift-left falsifier proofs for #11664 (see
//! `.spec/11664-host-work-status-domain/checklist.md` for the ID map).

use anyhow::{Context, Result, bail, ensure};

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
    TargetBranchState, WriterAdmissionSnapshot,
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
        freshness: Freshness::Current,
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
        freshness: Freshness::Current,
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
        target_branch_state: TargetBranchState::Named,
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
        target_branch_state: TargetBranchState::Named,
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
            observed: true,
            sha: Some(String::from("0f00d")),
            error: None,
        },
    }
}

// ---- F1: agent/lane return never makes compute terminal --------------------

#[test]
fn host_work_status_f1_agent_return_not_terminal() -> Result<()> {
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
    ensure!(classification.lifecycle == HostWorkLifecycle::Stopping);
    ensure!(
        classification.reasons.contains(&HostWorkReason::InitiatorReturnedButDescendantsUnsettled)
    );
    // The live tree is positive observed knowledge, so the evidence surface
    // is complete even though the lifecycle is STOPPING, not TERMINAL.
    ensure!(classification.evidence == DimensionEvidence::Complete);
    Ok(())
}

// ---- F2: parent exit does not settle descendants/reservations --------------

#[test]
fn host_work_status_f2_parent_exit_not_terminal() -> Result<()> {
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
    ensure!(classification.lifecycle == HostWorkLifecycle::Stopping);
    Ok(())
}

// ---- F3: process API unavailable is NOT_PROVEN, not zero processes ---------

#[test]
fn host_work_status_f3_api_unavailable_is_not_proven_not_zero() -> Result<()> {
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
    ensure!(classification.lifecycle != HostWorkLifecycle::Terminal);
    ensure!(classification.lifecycle == HostWorkLifecycle::Ambiguous);
    ensure!(classification.reasons.contains(&HostWorkReason::ProcessApiUnavailable));
    ensure!(classification.evidence == DimensionEvidence::Incomplete);

    let mut set = HostWorkObservationSet::new(key.clone());
    set.push_compute(compute(
        &key,
        ProcessTreeFact::ApiUnavailable { detail: String::from("ps unavailable") },
        ReservationFact::Absent,
        Settlement::Settled,
        Settlement::Settled,
        InitiatorReturn::Unknown,
    ))
    .context("row bound to the set's own subject")?;
    let status = HostWorkStatus::build(&subject, &set, &[]).context("single-subject set builds")?;
    ensure!(status.aggregate.contains(&HostWorkObservationToken::NotProven));
    Ok(())
}

// ---- F4: age alone can never create ORPHAN_CANDIDATE -----------------------

#[test]
fn host_work_status_f4_age_alone_never_orphan() -> Result<()> {
    let subject = repository_subject(Some("wt"));
    let key = subject.subject_key();
    let aged_residue = MutationWorkObservation {
        subject_key: key.clone(),
        provider: provider(ProviderFamily::WriterAdmission),
        observed_at: String::from("1999-01-01T00:00:00Z"),
        freshness: Freshness::Current,
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
    ensure!(old.lifecycle == HostWorkLifecycle::Ambiguous);

    // Partial premises stay ambiguous; only positive full evidence gates.
    let partial = MutationWorkObservation {
        orphan_premises: Some(OrphanPremises {
            ownership_established: true,
            targetability_established: false,
            cleanup_premises_fully_observable: true,
        }),
        ..clone_observation(&aged_residue)
    };
    ensure!(classify_mutation(&partial).lifecycle == HostWorkLifecycle::Ambiguous);

    let full = MutationWorkObservation {
        orphan_premises: Some(OrphanPremises {
            ownership_established: true,
            targetability_established: true,
            cleanup_premises_fully_observable: true,
        }),
        ..clone_observation(&aged_residue)
    };
    let gated = classify_mutation(&full);
    ensure!(gated.lifecycle == HostWorkLifecycle::OrphanCandidate);
    ensure!(gated.reasons.contains(&HostWorkReason::OrphanPremisesProven));
    Ok(())
}

fn clone_observation(observation: &MutationWorkObservation) -> MutationWorkObservation {
    observation.clone()
}

// ---- F5: executable name/path cannot attribute another repository ----------

#[test]
fn host_work_status_f5_basename_attribution_rejected() -> Result<()> {
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
    ensure!(name_only.lifecycle == HostWorkLifecycle::Ambiguous);
    ensure!(name_only.reasons.contains(&HostWorkReason::AttributionExecutableNameOnly));

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
    ensure!(set.push_compute(foreign).is_err());
    Ok(())
}

// ---- F6: no universal dispatch verdict exists ------------------------------

#[test]
fn host_work_status_f6_no_dispatch_verdict() -> Result<()> {
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
    set.push_compute(heavy).context("own subject")?;
    let status = HostWorkStatus::build(&subject, &set, &[]).context("builds")?;
    ensure!(status.aggregate.contains(&HostWorkObservationToken::Saturated));
    ensure!(!status.aggregate.iter().any(|token| token.as_str() == "ADMIT"));
    ensure!(!status.aggregate.iter().any(|token| token.as_str() == "DENY"));
    Ok(())
}

// ---- F7: an open PR does not force physical retention ----------------------

#[test]
fn host_work_status_f7_open_pr_allows_reconstructible_terminal() -> Result<()> {
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
        adapt_worktree_cleanup_plan(&plan).context("plan adapts")?;
    ensure!(outcome.subjects.len() == 1);
    let entry_subject: &SubjectObservations =
        outcome.subjects.first().context("expected adapted subject")?;
    let status = HostWorkStatus::build(
        &entry_subject.subject,
        &entry_subject.set,
        &entry_subject.supplied_readiness,
    )
    .context("builds")?;

    let mutation = status
        .classifications
        .iter()
        .find(|c| c.dimension == Dimension::Mutation)
        .context("mutation classified")?;
    ensure!(mutation.lifecycle == HostWorkLifecycle::Terminal);
    ensure!(mutation.reasons.contains(&HostWorkReason::CleanPushedTree));

    let logical = status
        .classifications
        .iter()
        .find(|c| c.dimension == Dimension::Logical)
        .context("logical classified")?;
    ensure!(logical.lifecycle == HostWorkLifecycle::RemoteInFlight);

    // Readiness records the descriptive cleanup handoff; nothing forced KEEP.
    ensure!(
        entry_subject
            .supplied_readiness
            .iter()
            .any(|readiness| matches!(readiness, CleanupReadiness::WorktreeCleanupOwnedBy { .. }))
    );
    // Clean pushed open-PR worktree: logical/mutation/storage are fully
    // decided and reconstructible, but this provider family supplies no
    // compute observation (#11659 has not landed), so the required compute
    // dimension stays visibly uncertain instead of collapsing to HEALTHY.
    let compute = status
        .classifications
        .iter()
        .find(|c| c.dimension == Dimension::Compute)
        .context("compute")?;
    ensure!(compute.evidence == DimensionEvidence::Incomplete);
    ensure!(status.aggregate.contains(&HostWorkObservationToken::Ambiguous));
    ensure!(status.aggregate.contains(&HostWorkObservationToken::NotProven));
    ensure!(!status.aggregate.contains(&HostWorkObservationToken::Healthy));
    Ok(())
}

// ---- F8: a closed issue never makes unique dirty work removable ------------

#[test]
fn host_work_status_f8_closed_issue_keeps_salvage_required() -> Result<()> {
    let subject = repository_subject(Some("wt"));
    let key = subject.subject_key();
    let mut set = HostWorkObservationSet::new(key.clone());
    let mut dirty_unique = mutation(&key, MutationOwnership::Unowned);
    dirty_unique.index_state = IndexState::Dirty { staged: true, untracked: true };
    dirty_unique.push_state = PushState::Unpushed { ahead_count: 3 };
    dirty_unique.salvage_required = true;
    set.push_mutation(dirty_unique).context("own subject")?;
    let mut closed_issue_logical = logical(&key, DurableState::UniqueLocalState);
    closed_issue_logical.claim_relationship =
        ClaimRelationship::LinkedToClosedIssue { number: 999 };
    let closed_classification = classify_logical(&closed_issue_logical);
    ensure!(closed_classification.lifecycle == HostWorkLifecycle::Ambiguous);
    set.push_logical(closed_issue_logical).context("own subject")?;

    let status = HostWorkStatus::build(&subject, &set, &[]).context("builds")?;
    ensure!(status.aggregate.contains(&HostWorkObservationToken::SalvageRequired));
    ensure!(status.cleanup_readiness.contains(&CleanupReadiness::RequiresSalvage));
    let mutation_classification = status
        .classifications
        .iter()
        .find(|c| c.dimension == Dimension::Mutation)
        .context("mutation")?;
    ensure!(mutation_classification.lifecycle != HostWorkLifecycle::Terminal);
    ensure!(mutation_classification.lifecycle != HostWorkLifecycle::OrphanCandidate);
    Ok(())
}

// ---- F9: shared cache state is never candidate authority -------------------

#[test]
fn host_work_status_f9_shared_cache_not_candidate_authority() -> Result<()> {
    let subject = repository_subject(Some("wt"));
    let key = subject.subject_key();

    let build_set = |root_class| -> Result<HostWorkObservationSet> {
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
        .context("own subject")?;
        set.push_mutation(mutation(&key, MutationOwnership::Unowned)).context("own subject")?;
        Ok(set)
    };

    let shared = HostWorkStatus::build(&subject, &build_set(RootClass::SharedCache)?, &[])
        .context("builds")?;
    let private = HostWorkStatus::build(&subject, &build_set(RootClass::CandidatePrivate)?, &[])
        .context("builds")?;

    // Whichever cache scope the storage row reports, the other three
    // dimensions are byte-identical between the two statuses.
    for dimension in [Dimension::Logical, Dimension::Mutation, Dimension::Compute] {
        let in_shared =
            shared.classifications.iter().find(|c| c.dimension == dimension).context("present")?;
        let in_private =
            private.classifications.iter().find(|c| c.dimension == dimension).context("present")?;
        ensure!(in_shared == in_private, "{dimension:?} must not move with storage scope");
    }
    ensure!(shared.cleanup_readiness.contains(&CleanupReadiness::EligibleForCacheReclaimPlan));
    Ok(())
}

// ---- F10: dimensions never collapse into one status ------------------------

#[test]
fn host_work_status_f10_dimensions_stay_independent() -> Result<()> {
    let subject = repository_subject(Some("wt"));
    let key = subject.subject_key();
    let mut set = HostWorkObservationSet::new(key.clone());
    let mut active_writer = mutation(&key, MutationOwnership::Unowned);
    active_writer.ownership = MutationOwnership::ActiveWriter { writer_id: String::from("w1") };
    set.push_mutation(active_writer).context("own subject")?;
    set.push_logical(logical(&key, DurableState::RemoteInFlight)).context("own subject")?;
    set.push_compute(compute(
        &key,
        ProcessTreeFact::ExitedConfirmed { process_group_id: String::from("pg") },
        ReservationFact::Absent,
        Settlement::Settled,
        Settlement::Settled,
        InitiatorReturn::Returned,
    ))
    .context("own subject")?;
    set.push_storage(storage(
        &key,
        RootClass::CandidatePrivate,
        StorageDisposition::Unique,
        CapacityFact::Measured { free_bytes: 10 },
        None,
        false,
        ReclaimClass::NoneApproved,
    ))
    .context("own subject")?;

    let status = HostWorkStatus::build(&subject, &set, &[]).context("builds")?;
    let lifecycles: Vec<(Dimension, HostWorkLifecycle)> =
        status.classifications.iter().map(|c| (c.dimension, c.lifecycle)).collect();
    ensure!(lifecycles.contains(&(Dimension::Mutation, HostWorkLifecycle::Active)));
    ensure!(lifecycles.contains(&(Dimension::Logical, HostWorkLifecycle::RemoteInFlight)));
    ensure!(lifecycles.contains(&(Dimension::Compute, HostWorkLifecycle::Terminal)));
    ensure!(lifecycles.contains(&(Dimension::Storage, HostWorkLifecycle::Ambiguous)));
    Ok(())
}

// ---- F11: success/exit-zero signals cannot override typed blocked facts ----

#[test]
fn host_work_status_f11_exit_zero_cannot_override_typed_fact() -> Result<()> {
    let report = admission_report(
        vec![
            // Not part of the current provider vocabulary: its meaning is
            // undefined, so it can never drive a typed collision verdict.
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
    ensure!(matches!(
        adapt_admission_report(&report, Some(&snapshot), &wrong_scope),
        Err(AdapterError::WrongScope { .. })
    ));

    let outcome: AdmissionAdapterOutcome =
        adapt_admission_report(&report, Some(&snapshot), &subject).context("report adapts")?;
    let observations: &SubjectObservations = &outcome.subject_observations;
    let status = HostWorkStatus::build(
        &observations.subject,
        &observations.set,
        &observations.supplied_readiness,
    )
    .context("builds")?;
    // A check outside the provider's current vocabulary stays a visible
    // unknown variant and never fabricates a contested-ownership fact.
    ensure!(
        observations.set.mutation().iter().all(|row| row.ownership != MutationOwnership::Contested),
        "an unknown-named check must not be parsed into a collision verdict"
    );
    ensure!(!status.aggregate.contains(&HostWorkObservationToken::Collision));
    ensure!(status.aggregate.contains(&HostWorkObservationToken::Ambiguous));
    ensure!(status.aggregate.contains(&HostWorkObservationToken::NotProven));
    ensure!(status.unknown_provider_variants.len() == 1);
    Ok(())
}

// ---- F12: provider human wording is never authority ------------------------

#[test]
fn host_work_status_f12_provider_wording_is_not_authority() -> Result<()> {
    let subject = repository_subject(None);
    let snapshot = clean_snapshot("agent/x", false);
    let calm = admission_report(
        vec![
            ("remote-branch-identity", CheckStatus::Pass, String::from("no collision detected")),
            ("disk-capacity", CheckStatus::Pass, String::from("plenty of room")),
        ],
        AdmissionVerdict::Pass,
    );
    let alarming = admission_report(
        vec![
            ("remote-branch-identity", CheckStatus::Pass, String::from("COLLISION IMMINENT DOOM")),
            ("disk-capacity", CheckStatus::Pass, String::from("DISK FULL CATASTROPHE")),
        ],
        AdmissionVerdict::Pass,
    );
    let calm_outcome =
        adapt_admission_report(&calm, Some(&snapshot), &subject).context("adapts")?;
    let loud_outcome =
        adapt_admission_report(&alarming, Some(&snapshot), &subject).context("adapts")?;
    let calm_status = HostWorkStatus::build(
        &calm_outcome.subject_observations.subject,
        &calm_outcome.subject_observations.set,
        &[],
    )
    .context("builds")?;
    let loud_status = HostWorkStatus::build(
        &loud_outcome.subject_observations.subject,
        &loud_outcome.subject_observations.set,
        &[],
    )
    .context("builds")?;
    ensure!(
        serde_json::to_string(&calm_status).context("serializes")?
            == serde_json::to_string(&loud_status).context("serializes")?
    );
    Ok(())
}

// ---- F13: unknown provider variants stay visible ---------------------------

#[test]
fn host_work_status_f13_unknown_variant_visible() -> Result<()> {
    let report = admission_report(
        vec![
            ("quantum-flux-check", CheckStatus::Pass, String::from("mystery pass")),
            ("disk-capacity", CheckStatus::Pass, String::from("ok")),
        ],
        AdmissionVerdict::Pass,
    );
    let snapshot = clean_snapshot("agent/x", false);
    let subject = repository_subject(None);
    let outcome = adapt_admission_report(&report, Some(&snapshot), &subject).context("adapts")?;
    let observations = &outcome.subject_observations;
    ensure!(observations.set.unknown_variants().len() == 1);
    let record: UnknownVariantRecord =
        observations.set.unknown_variants().first().context("expected unknown variant")?.clone();
    ensure!(record.variant == "quantum-flux-check");
    let status = HostWorkStatus::build(
        &observations.subject,
        &observations.set,
        &observations.supplied_readiness,
    )
    .context("builds")?;
    ensure!(status.aggregate.contains(&HostWorkObservationToken::Ambiguous));
    ensure!(status.aggregate.contains(&HostWorkObservationToken::NotProven));
    ensure!(!status.unknown_provider_variants.is_empty());
    Ok(())
}

// ---- F14: one subject's resource can never satisfy another -----------------

#[test]
fn host_work_status_f14_subject_mismatch_unrepresentable() -> Result<()> {
    let subject = repository_subject(Some("wt-a"));
    let other = repository_subject(Some("wt-b"));
    let mut set = HostWorkObservationSet::new(subject.subject_key());
    let foreign_logical = logical(&other.subject_key(), DurableState::NoLocalResidue);
    ensure!(set.push_logical(foreign_logical).is_err());

    let built_for_other = HostWorkObservationSet::new(other.subject_key());
    match HostWorkStatus::build(&subject, &built_for_other, &[]) {
        Err(StatusError::SubjectMismatch { expected, actual }) => {
            ensure!(expected == subject.subject_key());
            ensure!(actual == other.subject_key());
        }
        other_result => bail!("expected subject mismatch, got {other_result:?}"),
    }
    Ok(())
}

// ---- F15: aggregate HEALTHY can never hide required uncertainty ------------

#[test]
fn host_work_status_missing_provider_blocks_complete_cleanup_observation() -> Result<()> {
    let subject = repository_subject(Some("wt"));
    let key = subject.subject_key();
    let mut set = HostWorkObservationSet::new(key.clone());
    set.push_logical(logical(&key, DurableState::NoLocalResidue))?;
    set.push_mutation(mutation(&key, MutationOwnership::Unowned))?;
    set.push_compute(compute(
        &key,
        ProcessTreeFact::ExitedConfirmed { process_group_id: String::from("pg") },
        ReservationFact::Absent,
        Settlement::Settled,
        Settlement::Settled,
        InitiatorReturn::Returned,
    ))?;
    set.push_storage(storage(
        &key,
        RootClass::CandidatePrivate,
        StorageDisposition::CacheOnly,
        CapacityFact::Measured { free_bytes: 900 },
        None,
        false,
        ReclaimClass::NoneApproved,
    ))?;

    let complete = HostWorkStatus::build(&subject, &set, &[])?;
    if complete.classifications.len() != 4
        || complete.classifications.iter().any(|row| row.evidence != DimensionEvidence::Complete)
    {
        bail!("control requires all four dimensions to have complete evidence");
    }
    if complete.cleanup_readiness != [CleanupReadiness::ReadOnlyObservationComplete]
        || complete.aggregate != [HostWorkObservationToken::Healthy]
    {
        bail!("complete observations without a missing provider must remain complete");
    }

    set.declare_missing_provider(ProviderFamily::CapacityReservation);
    let missing = HostWorkStatus::build(&subject, &set, &[])?;
    if missing.classifications != complete.classifications {
        bail!("provider uncertainty must not change the four observed dimensions");
    }
    if missing.cleanup_readiness != [CleanupReadiness::NotProven]
        || !missing.aggregate.contains(&HostWorkObservationToken::NotProven)
        || !missing.aggregate.contains(&HostWorkObservationToken::Ambiguous)
        || missing.aggregate.contains(&HostWorkObservationToken::Healthy)
    {
        bail!("missing provider must prevent a complete cleanup observation");
    }
    Ok(())
}

#[test]
fn host_work_status_f15_healthy_never_hides_uncertainty() -> Result<()> {
    let subject = repository_subject(Some("wt"));
    let key = subject.subject_key();
    let mut ambiguous_only = HostWorkObservationSet::new(key.clone());
    ambiguous_only
        .push_logical(logical(&key, DurableState::Contradictory))
        .context("own subject")?;
    ambiguous_only
        .push_mutation(mutation(&key, MutationOwnership::Unowned))
        .context("own subject")?;
    ambiguous_only
        .push_compute(compute(
            &key,
            ProcessTreeFact::ExitedConfirmed { process_group_id: String::from("pg") },
            ReservationFact::Absent,
            Settlement::Settled,
            Settlement::Settled,
            InitiatorReturn::Returned,
        ))
        .context("own subject")?;
    let status = HostWorkStatus::build(&subject, &ambiguous_only, &[]).context("builds")?;
    ensure!(status.aggregate.contains(&HostWorkObservationToken::Ambiguous));
    ensure!(!status.aggregate.contains(&HostWorkObservationToken::Healthy));

    // A declared-missing provider also blocks HEALTHY even with no rows.
    let mut missing = HostWorkObservationSet::new(key.clone());
    missing.declare_missing_provider(ProviderFamily::CapacityReservation);
    let status_missing =
        HostWorkStatus::build(&repository_subject(Some("wt")), &missing, &[]).context("builds")?;
    ensure!(status_missing.aggregate.contains(&HostWorkObservationToken::NotProven));
    ensure!(!status_missing.aggregate.contains(&HostWorkObservationToken::Healthy));

    // Only fully decided, complete, benign evidence may be HEALTHY.
    let mut benign = HostWorkObservationSet::new(key);
    benign
        .push_logical(logical(benign.subject_key(), DurableState::NoLocalResidue))
        .context("own subject")?;
    benign
        .push_mutation(mutation(benign.subject_key(), MutationOwnership::Unowned))
        .context("own subject")?;
    benign
        .push_compute(compute(
            benign.subject_key(),
            ProcessTreeFact::ExitedConfirmed { process_group_id: String::from("pg") },
            ReservationFact::Absent,
            Settlement::Settled,
            Settlement::Settled,
            InitiatorReturn::Returned,
        ))
        .context("own subject")?;
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
        .context("own subject")?;
    let healthy_status = HostWorkStatus::build(&subject, &benign, &[]).context("builds")?;
    ensure!(healthy_status.aggregate == vec![HostWorkObservationToken::Healthy]);
    Ok(())
}

// ---- F16: cleanup readiness is never cleanup authorization -----------------

#[test]
fn host_work_status_f16_readiness_is_not_authorization() -> Result<()> {
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
    .context("own subject")?;
    let status = HostWorkStatus::build(&subject, &set, &[]).context("builds")?;
    // Descriptive handoff appears…
    ensure!(status.cleanup_readiness.contains(&CleanupReadiness::EligibleForProcessReapPlan));
    // …but the status carries no plan, action, or verdict surface at all:
    // its aggregate remains pure observation and readiness stays a closed,
    // descriptive enum rendered without any authorization token.
    let rendered = status.render_human();
    ensure!(rendered.contains("ELIGIBLE_FOR_PROCESS_REAP_PLAN"));
    ensure!(!rendered.contains("AUTHORIZED"));
    ensure!(!rendered.contains("APPROVED_FOR_EXECUTION"));
    Ok(())
}

// ---- F17: input ordering cannot change semantic identity -------------------

#[test]
fn host_work_status_f17_ordering_does_not_change_identity() -> Result<()> {
    let subject = repository_subject(Some("wt"));
    let key = subject.subject_key();

    let build = |order: [usize; 3]| -> Result<HostWorkStatus> {
        let rows = [
            logical(&key, DurableState::ReconstructibleLocalState),
            logical(&key, DurableState::RemoteInFlight),
            logical(&key, DurableState::NotProven),
        ];
        let mut set = HostWorkObservationSet::new(key.clone());
        for index in order {
            set.push_logical(clone_logical(
                rows.get(index).context("fixture ordering index must identify a row")?,
            ))
            .context("own subject")?;
        }
        set.declare_missing_provider(ProviderFamily::FilesystemStorage);
        set.declare_missing_provider(ProviderFamily::CapacityReservation);
        HostWorkStatus::build(&subject, &set, &[]).context("builds")
    };

    let one = serde_json::to_string(&build([0, 1, 2])?).context("serializes")?;
    let two = serde_json::to_string(&build([2, 0, 1])?).context("serializes")?;
    let three = serde_json::to_string(&build([1, 2, 0])?).context("serializes")?;
    ensure!(one == two);
    ensure!(two == three);
    Ok(())
}

// ---- Future #11650/#11653/#11659 provider hooks stay visible and honest ----

#[test]
fn host_work_status_future_provider_hooks_classify_honestly() -> Result<()> {
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
    ensure!(reserved.lifecycle == HostWorkLifecycle::Active);
    ensure!(reserved.evidence == DimensionEvidence::Incomplete);
    ensure!(reserved.reasons.contains(&HostWorkReason::ReservationActive));
    ensure!(reserved.reasons.contains(&HostWorkReason::TerminalityNotProven));

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
    ensure!(classify_compute(&reservation_active).lifecycle == HostWorkLifecycle::Active);

    // A name-attributed process tree (exact binding not yet supplied) stays
    // ambiguous even when the caller is tempted to claim it.
    let name_only = process_tree_compute_observation(
        key.clone(),
        provider(ProviderFamily::ProcessObservation),
        "t0",
        "pg-future",
        Attribution::ExecutableNameOnly,
    );
    ensure!(classify_compute(&name_only).lifecycle == HostWorkLifecycle::Ambiguous);

    // Executor-state storage scope without reclaim facts stays ambiguous.
    let scoped = storage_scope_observation(
        key.clone(),
        provider(ProviderFamily::ExecutorStateAllocation),
        "t0",
        RootClass::CandidatePrivate,
        StorageDisposition::Unique,
    );
    ensure!(classify_storage(&scoped).lifecycle == HostWorkLifecycle::Ambiguous);

    // Declared-missing providers surface as NOT_PROVEN + AMBIGUOUS aggregate.
    // Future-owner hooks all construct; their declarations stay typed data.
    let hooks: [MissingProviderDeclaration; 3] = [
        missing_capacity_reservation(),
        missing_executor_allocation(),
        missing_process_observation(),
    ];
    ensure!(hooks.iter().all(|hook| hook.family != ProviderFamily::WorktreePlan));

    let mut set = HostWorkObservationSet::new(key.clone());
    set.push_compute(reservation_only).context("own subject")?;
    declare_missing_provider(ProviderFamily::ProcessObservation);
    set.declare_missing_provider(ProviderFamily::CapacityReservation);
    let status = HostWorkStatus::build(&subject, &set, &[]).context("builds")?;
    ensure!(status.schema_version == HOST_WORK_STATUS_SCHEMA_VERSION);
    ensure!(status.aggregate.contains(&HostWorkObservationToken::NotProven));
    ensure!(status.missing_providers.contains(&ProviderFamily::CapacityReservation));
    Ok(())
}

// ---- Dimension merge: uncertainty dominates, reasons union -----------------

#[test]
fn host_work_status_merge_uncertainty_dominates_and_reasons_union() -> Result<()> {
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
    let merged = merged.context("two rows merge")?;
    ensure!(merged.dimension == Dimension::Compute);
    ensure!(merged.lifecycle == HostWorkLifecycle::Stopping);
    ensure!(merged.reasons.contains(&HostWorkReason::NoRelevantOwnershipRemains));
    ensure!(merged.reasons.contains(&HostWorkReason::DescendantsUnsettled));
    Ok(())
}

fn clone_compute(classification: &HostWorkClassification) -> HostWorkClassification {
    classification.clone()
}

fn clone_logical(observation: &LogicalWorkObservation) -> LogicalWorkObservation {
    observation.clone()
}

// ---- F18: the domain performs no live observation or mutation --------------

#[test]
fn host_work_status_f18_module_is_pure_no_io_or_process_surface() -> Result<()> {
    const SOURCES: [&str; 6] =
        ["mod.rs", "subject.rs", "dimension.rs", "lifecycle.rs", "status.rs", "adapter.rs"];
    let manifest_dir = std::env!("CARGO_MANIFEST_DIR");
    for file in SOURCES {
        let path = std::path::Path::new(manifest_dir).join("src/host_work_status").join(file);
        let contents = std::fs::read_to_string(path)
            .with_context(|| format!("reading domain source {file}"))?;
        for banned in
            ["std::process", "std::net", "Command::new", "fs::remove", "fs::write", "reqwest"]
        {
            ensure!(!contents.contains(banned), "{file} must not reference {banned}");
        }
    }
    Ok(())
}

// ---- Review response: readiness dedup is rank-independent -------------------

#[test]
fn host_work_status_readiness_dedup_collapses_nonadjacent_owned_by_duplicates() -> Result<()> {
    let subject = repository_subject(Some("wt"));
    let key = subject.subject_key();
    let mut set = HostWorkObservationSet::new(key.clone());
    set.push_mutation(mutation(&key, MutationOwnership::Unowned)).context("own subject")?;
    set.push_logical(logical(&key, DurableState::NoLocalResidue)).context("own subject")?;
    set.push_compute(compute(
        &key,
        ProcessTreeFact::ExitedConfirmed { process_group_id: String::from("pg") },
        ReservationFact::Absent,
        Settlement::Settled,
        Settlement::Settled,
        InitiatorReturn::Returned,
    ))
    .context("own subject")?;
    set.push_storage(storage(
        &key,
        RootClass::CandidatePrivate,
        StorageDisposition::Unique,
        CapacityFact::Measured { free_bytes: 900 },
        None,
        false,
        ReclaimClass::NoneApproved,
    ))
    .context("own subject")?;

    // Two "B" rows separated by an equal-ranked "A" row: dedup must still
    // collapse them (all WorktreeCleanupOwnedBy variants share one rank).
    let supplied = vec![
        CleanupReadiness::WorktreeCleanupOwnedBy { owner: String::from("B") },
        CleanupReadiness::WorktreeCleanupOwnedBy { owner: String::from("A") },
        CleanupReadiness::WorktreeCleanupOwnedBy { owner: String::from("B") },
    ];
    let status = HostWorkStatus::build(&subject, &set, &supplied).context("builds")?;

    let owned: Vec<&str> = status
        .cleanup_readiness
        .iter()
        .filter_map(|readiness| match readiness {
            CleanupReadiness::WorktreeCleanupOwnedBy { owner } => Some(owner.as_str()),
            _ => None,
        })
        .collect();
    ensure!(owned == vec!["A", "B"], "duplicate owners collapse; ties order deterministically");
    Ok(())
}

// ---- Review response: the subject key is structurally collision-free -------

#[test]
fn host_work_status_subject_key_survives_delimiter_collision_attempts() -> Result<()> {
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

    ensure!(shifted != absorbed, "fixture subjects must be distinct");
    ensure!(
        shifted.subject_key() != absorbed.subject_key(),
        "delimiter movement between adjacent fields cannot fuse identities"
    );

    // Equal subjects still produce equal keys, deterministically.
    let twin = shifted.clone();
    ensure!(shifted.subject_key() == twin.subject_key());

    // The key remains a pure function of the typed fields (no ambient state).
    ensure!(shifted.subject_key() == shifted.subject_key());
    Ok(())
}

// ---- Review response: NOT_PROVEN disk evidence never claims LOW_DISK --------

#[test]
fn host_work_status_disk_not_proven_stays_unknown_not_low_disk() -> Result<()> {
    let report = admission_report(
        vec![
            ("remote-branch-identity", CheckStatus::Pass, String::from("ok")),
            ("disk-capacity", CheckStatus::NotProven, String::from("disk probe unavailable")),
        ],
        AdmissionVerdict::NotProven,
    );
    let snapshot = clean_snapshot("agent/x", false);
    let subject = repository_subject(None);
    let outcome = adapt_admission_report(&report, Some(&snapshot), &subject).context("adapts")?;
    let observations = &outcome.subject_observations;
    let storage_row = observations.set.storage().iter().next().context("storage row")?;
    ensure!(
        !storage_row.below_configured_floor,
        "a failed/unavailable probe is not a below-floor capacity fact"
    );
    ensure!(
        matches!(storage_row.instrument, Instrument::Unavailable { .. }),
        "the unproven probe stays visible on the instrument surface"
    );

    let status = HostWorkStatus::build(
        &observations.subject,
        &observations.set,
        &observations.supplied_readiness,
    )
    .context("builds")?;
    ensure!(
        !status.aggregate.contains(&HostWorkObservationToken::LowDisk),
        "NOT_PROVEN evidence must not emit the factual LOW_DISK token"
    );
    ensure!(status.aggregate.contains(&HostWorkObservationToken::NotProven));
    Ok(())
}

#[test]
fn host_work_status_disk_block_still_emits_low_disk() -> Result<()> {
    // Positive control: a proven blocking capacity result keeps its
    // concrete below-floor claim.
    let report = admission_report(
        vec![("disk-capacity", CheckStatus::Block, String::from("free space below floor"))],
        AdmissionVerdict::Block,
    );
    let snapshot = clean_snapshot("agent/x", false);
    let subject = repository_subject(None);
    let outcome = adapt_admission_report(&report, Some(&snapshot), &subject).context("adapts")?;
    let observations = &outcome.subject_observations;
    let storage_row = observations.set.storage().iter().next().context("storage row")?;
    ensure!(storage_row.below_configured_floor);

    let status = HostWorkStatus::build(
        &observations.subject,
        &observations.set,
        &observations.supplied_readiness,
    )
    .context("builds")?;
    ensure!(status.aggregate.contains(&HostWorkObservationToken::LowDisk));
    Ok(())
}

// ---- F15b: an unlinked claim never emits CLAIM_LINKED -----------------------

#[test]
fn host_work_status_f15b_unlinked_claim_does_not_emit_claim_linked() -> Result<()> {
    let subject = repository_subject(Some("wt"));
    let key = subject.subject_key();
    let row = LogicalWorkObservation {
        claim_relationship: ClaimRelationship::Unlinked,
        ..clone_logical(&logical(&key, DurableState::NoLocalResidue))
    };
    let classification = classify_logical(&row);
    ensure!(!classification.reasons.contains(&HostWorkReason::ClaimLinked));
    ensure!(classification.reasons.contains(&HostWorkReason::ClaimUnlinked));
    ensure!(classification.evidence == DimensionEvidence::Complete);
    Ok(())
}

// ---- F1b: reservation-only unsettled never emits DescendantsUnsettled -------

#[test]
fn host_work_status_f1b_only_reservation_unsettled_does_not_emit_descendants_unsettled()
-> Result<()> {
    let subject = repository_subject(Some("wt"));
    let key = subject.subject_key();
    let classification = classify_compute(&compute(
        &key,
        ProcessTreeFact::ExitedConfirmed { process_group_id: String::from("pg") },
        ReservationFact::Released {
            reservation_id: String::from("r"),
            settled: Settlement::Unsettled,
        },
        Settlement::Settled,
        Settlement::Settled,
        InitiatorReturn::Returned,
    ));
    ensure!(classification.lifecycle == HostWorkLifecycle::Stopping);
    ensure!(!classification.reasons.contains(&HostWorkReason::DescendantsUnsettled));
    ensure!(classification.reasons.contains(&HostWorkReason::ReservationSettlementPending));
    Ok(())
}

// ---- F15c: stale evidence can never yield a HEALTHY aggregate ---------------

#[test]
fn host_work_status_f15c_stale_logical_never_healthy() -> Result<()> {
    let subject = repository_subject(Some("wt"));
    let key = subject.subject_key();
    let stale = LogicalWorkObservation {
        freshness: Freshness::Stale,
        ..clone_logical(&logical(&key, DurableState::NoLocalResidue))
    };
    let classification = classify_logical(&stale);
    ensure!(classification.evidence == DimensionEvidence::Incomplete);
    ensure!(classification.reasons.contains(&HostWorkReason::CurrentnessNotProven));

    let mut set = HostWorkObservationSet::new(key.clone());
    set.push_logical(stale).context("own subject")?;
    set.push_mutation(mutation(&key, MutationOwnership::Unowned)).context("own subject")?;
    set.push_compute(compute(
        &key,
        ProcessTreeFact::ExitedConfirmed { process_group_id: String::from("pg") },
        ReservationFact::Absent,
        Settlement::Settled,
        Settlement::Settled,
        InitiatorReturn::Returned,
    ))
    .context("own subject")?;
    set.push_storage(storage(
        &key,
        RootClass::CandidatePrivate,
        StorageDisposition::CacheOnly,
        CapacityFact::Measured { free_bytes: 900 },
        None,
        false,
        ReclaimClass::NoneApproved,
    ))
    .context("own subject")?;
    let status = HostWorkStatus::build(&subject, &set, &[]).context("builds")?;
    ensure!(
        !status.aggregate.contains(&HostWorkObservationToken::Healthy),
        "a stale observation must keep the aggregate away from HEALTHY"
    );
    ensure!(status.aggregate.contains(&HostWorkObservationToken::NotProven));
    Ok(())
}

// ---- F15d: stale facts on every dimension are contagious --------------------

#[test]
fn host_work_status_f15d_stale_fact_on_each_dimension_is_contagious() -> Result<()> {
    let subject = repository_subject(Some("wt"));
    let key = subject.subject_key();
    let mut set = HostWorkObservationSet::new(key.clone());
    let mut stale_logical = clone_logical(&logical(&key, DurableState::NoLocalResidue));
    stale_logical.freshness = Freshness::Stale;
    set.push_logical(stale_logical).context("own subject")?;
    let mut stale_mutation = mutation(&key, MutationOwnership::Unowned);
    stale_mutation.freshness = Freshness::Stale;
    set.push_mutation(stale_mutation).context("own subject")?;
    let mut stale_compute = compute(
        &key,
        ProcessTreeFact::ExitedConfirmed { process_group_id: String::from("pg") },
        ReservationFact::Absent,
        Settlement::Settled,
        Settlement::Settled,
        InitiatorReturn::Returned,
    );
    stale_compute.freshness = Freshness::Stale;
    set.push_compute(stale_compute).context("own subject")?;
    let mut stale_storage = storage(
        &key,
        RootClass::CandidatePrivate,
        StorageDisposition::CacheOnly,
        CapacityFact::Measured { free_bytes: 900 },
        None,
        false,
        ReclaimClass::NoneApproved,
    );
    stale_storage.freshness = Freshness::Stale;
    set.push_storage(stale_storage).context("own subject")?;

    let status = HostWorkStatus::build(&subject, &set, &[]).context("builds")?;
    ensure!(!status.aggregate.contains(&HostWorkObservationToken::Healthy));
    for classification in &status.classifications {
        ensure!(classification.evidence == DimensionEvidence::Incomplete);
        ensure!(classification.reasons.contains(&HostWorkReason::CurrentnessNotProven));
    }
    ensure!(status.cleanup_readiness.contains(&CleanupReadiness::NotProven));
    Ok(())
}

// ---- F14b: host profile is part of the load-bearing subject key -------------

#[test]
fn host_work_status_f14b_host_profile_participates_in_subject_key() -> Result<()> {
    let mut other = repository_subject(Some("wt"));
    other.host_profile = String::from("linux-ci");
    ensure!(
        repository_subject(Some("wt")).subject_key() != other.subject_key(),
        "evidence from one host profile can never satisfy another"
    );
    Ok(())
}

// ---- F14c: incomplete PR ownership never fabricates an identity -------------

#[test]
fn host_work_status_f14c_open_without_number_stays_unknown() -> Result<()> {
    let report = admission_report(
        vec![
            ("candidate-presence", CheckStatus::Pass, String::from("ok")),
            ("disk-capacity", CheckStatus::Pass, String::from("ok")),
        ],
        AdmissionVerdict::Pass,
    );
    let mut snapshot = clean_snapshot("agent/x", true);
    snapshot.pr_ownership.pr_number = None;
    let subject = repository_subject(None);
    let outcome = adapt_admission_report(&report, Some(&snapshot), &subject).context("adapts")?;
    let observations = &outcome.subject_observations;
    let logical_row = observations.set.logical().iter().next().context("logical row")?;
    ensure!(logical_row.claim_relationship == ClaimRelationship::Unknown);
    ensure!(
        matches!(logical_row.durable_state, DurableState::NotProven),
        "incomplete ownership evidence must never read as an established PR link"
    );
    ensure!(matches!(logical_row.instrument, Instrument::Unavailable { .. }));
    Ok(())
}

// ---- F14d: a snapshot for another target is rejected, not merged ------------

#[test]
fn host_work_status_f14d_mismatched_snapshot_target_rejected() -> Result<()> {
    let report = admission_report(
        vec![("disk-capacity", CheckStatus::Pass, String::from("ok"))],
        AdmissionVerdict::Pass,
    );
    let snapshot = clean_snapshot("agent/other", false);
    let subject = repository_subject(None);
    match adapt_admission_report(&report, Some(&snapshot), &subject) {
        Err(AdapterError::SnapshotTargetMismatch { report_target, snapshot_target }) => {
            ensure!(report_target == "agent/x");
            ensure!(snapshot_target == "agent/other");
        }
        other => bail!("expected snapshot target mismatch, got {other:?}"),
    }
    Ok(())
}

// ---- F14e: a detached target never shares identity with a named branch ------

#[test]
fn host_work_status_f14e_detached_target_distinct_from_named_branch() -> Result<()> {
    let named_report = admission_report(
        vec![("disk-capacity", CheckStatus::Pass, String::from("ok"))],
        AdmissionVerdict::Pass,
    );
    let mut detached_report = named_report.clone();
    detached_report.target_branch_state = TargetBranchState::Detached;
    let subject = repository_subject(None);
    let named =
        adapt_admission_report(&named_report, None, &subject).context("named report adapts")?;
    let detached = adapt_admission_report(&detached_report, None, &subject)
        .context("detached report adapts")?;
    ensure!(
        named.subject_observations.subject.subject_key()
            != detached.subject_observations.subject.subject_key(),
        "a named branch literally spelled '(detached)' can never impersonate a detached checkout"
    );
    ensure!(
        detached
            .subject_observations
            .subject
            .worktree
            .as_ref()
            .context("detached worktree fixture missing")?
            .branch
            .is_none()
    );
    Ok(())
}

// ---- F13b: the known-check list is exhaustive over the live provider --------

#[test]
fn host_work_status_f13b_current_check_names_have_no_unknown_variants() -> Result<()> {
    let report = admission_report(
        vec![
            ("canonical-base", CheckStatus::Pass, String::from("ok")),
            ("shadow-ref", CheckStatus::Pass, String::from("ok")),
            ("symbolic-head", CheckStatus::Pass, String::from("ok")),
            ("branch-worktree-mapping", CheckStatus::Pass, String::from("ok")),
            ("dirty-unpushed", CheckStatus::Pass, String::from("ok")),
            ("disk-capacity", CheckStatus::Pass, String::from("ok")),
            ("remote-branch-identity", CheckStatus::Pass, String::from("ok")),
            ("candidate-presence", CheckStatus::Pass, String::from("ok")),
        ],
        AdmissionVerdict::Pass,
    );
    let snapshot = clean_snapshot("agent/x", false);
    let subject = repository_subject(None);
    let outcome = adapt_admission_report(&report, Some(&snapshot), &subject).context("adapts")?;
    ensure!(
        outcome.subject_observations.set.unknown_variants().is_empty(),
        "every check emitted by run_checks must map exhaustively"
    );
    Ok(())
}

// ---- F3b: an unobserved remote branch is a missing observation --------------

#[test]
fn host_work_status_f3b_unobserved_remote_branch_is_not_no_residue() -> Result<()> {
    let report = admission_report(
        vec![("remote-branch-identity", CheckStatus::NotProven, String::from("no lookup"))],
        AdmissionVerdict::NotProven,
    );
    let mut snapshot = clean_snapshot("agent/x", false);
    snapshot.remote_branch = crate::tasks::writer_admission::RemoteBranchInfo::default();
    let subject = repository_subject(None);
    let outcome = adapt_admission_report(&report, Some(&snapshot), &subject).context("adapts")?;
    let logical_row = outcome.subject_observations.set.logical().iter().next().context("row")?;
    ensure!(
        matches!(logical_row.durable_state, DurableState::NotProven),
        "an absent observation can never read as confirmed remote absence"
    );
    Ok(())
}

#[test]
fn host_work_status_subject_key_preserves_optional_presence() -> Result<()> {
    let mut absent = repository_subject(None);
    absent.canonical_remote = None;
    absent.worktree = None;
    absent.candidate_id = None;
    absent.executor_operation_id = None;
    absent.allocation_id = None;
    absent.reservation_id = None;
    absent.process_group_id = None;
    absent.storage_root = None;
    let absent_key = absent.subject_key();
    let mut alternatives = Vec::new();
    for position in 0..6 {
        let mut present = absent.clone();
        let field = match position {
            0 => &mut present.canonical_remote,
            1 => &mut present.candidate_id,
            2 => &mut present.executor_operation_id,
            3 => &mut present.allocation_id,
            4 => &mut present.reservation_id,
            _ => &mut present.process_group_id,
        };
        *field = Some(String::new());
        alternatives.push(present);
    }
    let mut storage = absent.clone();
    storage.storage_root = Some(std::path::PathBuf::new());
    alternatives.push(storage);
    let mut worktree = absent.clone();
    worktree.worktree = Some(WorktreeIdentity { path: std::path::PathBuf::new(), branch: None });
    alternatives.push(worktree.clone());
    let branch_absent_key = worktree.subject_key();
    worktree.worktree.as_mut().context("worktree fixture missing")?.branch = Some(String::new());
    if branch_absent_key == worktree.subject_key() {
        bail!("absent and present-empty worktree branch must differ");
    }
    alternatives.push(worktree);
    let mut keys = std::collections::BTreeSet::new();
    keys.insert(absent_key);
    for alternative in alternatives {
        let key = alternative.subject_key();
        if key != alternative.clone().subject_key() || !keys.insert(key) {
            bail!("optional presence must be distinct and deterministic");
        }
    }
    Ok(())
}

#[cfg(any(unix, windows))]
#[test]
fn host_work_status_subject_key_preserves_native_non_unicode_paths() -> Result<()> {
    #[cfg(unix)]
    let paths = {
        use std::os::unix::ffi::OsStringExt;
        [std::ffi::OsString::from_vec(vec![0x80]), std::ffi::OsString::from_vec(vec![0x81])]
    };
    #[cfg(windows)]
    let paths = {
        use std::os::windows::ffi::OsStringExt;
        [std::ffi::OsString::from_wide(&[0xd800]), std::ffi::OsString::from_wide(&[0xd801])]
    };
    let [first, second] = paths.map(std::path::PathBuf::from);
    if first.to_string_lossy() != second.to_string_lossy() {
        bail!("control must distinguish paths the old display encoding collapsed");
    }
    for position in 0..4 {
        let mut left = repository_subject(None);
        let mut right = left.clone();
        for (subject, path) in [(&mut left, first.clone()), (&mut right, second.clone())] {
            match position {
                0 => subject.repository_root = path,
                1 => subject.common_dir = path,
                2 => subject.worktree = Some(WorktreeIdentity { path, branch: None }),
                _ => subject.storage_root = Some(path),
            }
        }
        if left.subject_key() == right.subject_key()
            || left.subject_key() != left.clone().subject_key()
        {
            bail!("native path identity must be lossless and deterministic in every field");
        }
        let mut set = HostWorkObservationSet::new(left.subject_key());
        if set.push_logical(logical(&right.subject_key(), DurableState::NoLocalResidue)).is_ok() {
            bail!("distinct native paths must not satisfy another subject");
        }
    }
    Ok(())
}
