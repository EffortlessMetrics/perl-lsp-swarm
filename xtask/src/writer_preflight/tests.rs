//! Falsifier-first proof for the writer-preflight domain (#11633).
//!
//! Each `shift_left_*` test pins one #11633 shift-left falsifier; the
//! remaining tests pin the decision table by requested operation, the
//! closed vocabulary, deterministic identity/order rules, and projection
//! agreement. Everything here runs against the pure core only — no Git,
//! filesystem, process, shell, or network access.

use super::*;

// ---- Fixtures -----------------------------------------------------------------

const COMMON_DIR: &str = "E:/code/perl-lsp-swarm";
const CANONICAL_REMOTE: &str = "git@github.com:EffortlessMetrics/perl-lsp-swarm.git";
const LINKED_ROOT: &str = "E:/code/perl-lsp-swarm/.claude/worktrees/agent-1";
const CANDIDATE_BRANCH: &str = "impl/11633-writer-preflight";

fn repository() -> RepositoryIdentity {
    RepositoryIdentity {
        common_dir: COMMON_DIR.to_string(),
        canonical_remote: Some(CANONICAL_REMOTE.to_string()),
    }
}

fn claim(branch: &str) -> ClaimIdentity {
    ClaimIdentity {
        issue: Some("11633".to_string()),
        branch: branch.to_string(),
        worktree_path: Some(LINKED_ROOT.to_string()),
    }
}

/// Explicit all-quiet disposition (#12059 review, FC3): the domain type has
/// no `Default`, so tests construct the safety flag explicitly too.
fn tree_disposition() -> WorkingTreeDisposition {
    WorkingTreeDisposition {
        dirty_files: 0,
        staged_files: 0,
        untracked_files: 0,
        unpushed_commits: 0,
        behind_upstream: 0,
        unique_work_at_risk: false,
    }
}

/// A fully affirmative mutating subject bound to the linked-worktree
/// fixture below.
fn create_subject() -> WriterPreflightSubject {
    WriterPreflightSubject {
        repository: repository(),
        operation: WriterPreflightOperation::Create,
        claim: claim(CANDIDATE_BRANCH),
        expected_base_sha: Some("deadbeef00".to_string()),
        candidate_head_sha: None,
        expected_writer_owner: None,
        capacity_requirement: None,
        executor_policy: None,
    }
}

fn read_only_subject() -> WriterPreflightSubject {
    WriterPreflightSubject { operation: WriterPreflightOperation::ReadOnly, ..create_subject() }
}

fn mutate_subject() -> WriterPreflightSubject {
    WriterPreflightSubject {
        operation: WriterPreflightOperation::Mutate,
        // In-place transition binds to the current checkout: no separate
        // worktree path.
        claim: ClaimIdentity { worktree_path: None, ..claim(CANDIDATE_BRANCH) },
        candidate_head_sha: Some("cafe0001".to_string()),
        ..create_subject()
    }
}

fn healthy_worktrees() -> Vec<WorktreeRecord> {
    vec![
        WorktreeRecord {
            path: COMMON_DIR.to_string(),
            branch: None,
            head_sha: Some("deadbeef00".to_string()),
            locked: false,
        },
        WorktreeRecord {
            path: LINKED_ROOT.to_string(),
            branch: Some(CANDIDATE_BRANCH.to_string()),
            head_sha: Some("cafe0001".to_string()),
            locked: false,
        },
    ]
}

/// A fully affirmative observation set for `create` at the linked worktree.
/// For `mutate`, swap `worktrees` to the single-registration form via
/// [`in_place_worktrees`] (a fresh create target must be unregistered; an
/// in-place transition owns its registration).
fn healthy_observations() -> WriterPreflightObservationSet {
    WriterPreflightObservationSet {
        repository_identity: Observation::current(repository()),
        checkout_relation: Observation::current(CheckoutRelation {
            root: LINKED_ROOT.to_string(),
            canonical_checkout: false,
        }),
        head_state: Observation::current(HeadState::OnBranch {
            name: CANDIDATE_BRANCH.to_string(),
            protected: false,
        }),
        head_sha: Observation::current("cafe0001".to_string()),
        base_sha: Observation::current("deadbeef0000ff".to_string()),
        remote_branch: Observation::current(RemoteBranchPresence::Absent),
        worktrees: Observation::current(Vec::new()),
        same_candidate_writer: Observation::current(SameCandidateWriter {
            active: false,
            owner: None,
        }),
        index_state: Observation::current(IndexState::Clean),
        working_tree: Observation::current(tree_disposition()),
        stash: Observation::current(StashState::NoSharedStash),
        reserved_local_refs: Observation::current(Vec::new()),
        ambient_cargo_overrides: Observation::current(Vec::new()),
        executor_cargo_config: Observation::current(ExecutorCargoPresence::Absent),
        capacity: Observation::current(CapacityObservation {
            free_gb: 500.0,
            meets_selected_requirement: true,
            unrelated_host_load: false,
        }),
    }
}

fn has_reason(decision: &WriterPreflightDecision, reason: WriterPreflightReason) -> bool {
    decision.reason(reason)
}

/// A fully affirmative observation set for an existing-candidate operation
/// (resume/mutate) at the linked worktree: the candidate registration is
/// bound to the invoked checkout and the remote candidate exists at the
/// expected head (#12059 review: existing-candidate polarity).
fn existing_candidate_observations() -> WriterPreflightObservationSet {
    let mut observations = healthy_observations();
    observations.worktrees = Observation::current(healthy_worktrees());
    observations.remote_branch =
        Observation::current(RemoteBranchPresence::Present { head_sha: "cafe0001".to_string() });
    observations
}

// ---- Healthy baselines ----------------------------------------------------------

#[test]
fn healthy_read_only_is_pass_with_affirmative_marker() {
    let decision = decide(&read_only_subject(), &healthy_observations());
    assert_eq!(decision.outcome, WriterPreflightOutcome::Pass);
    assert_eq!(decision.reasons, vec![WriterPreflightReason::SafeReadOnlySubject]);
    assert_eq!(decision.schema_version, WRITER_PREFLIGHT_SCHEMA_VERSION);
}

#[test]
fn healthy_create_is_pass_with_no_reasons() {
    let mut observations = healthy_observations();
    // Fresh create target: the candidate branch is registered nowhere yet.
    observations.worktrees = Observation::current(healthy_worktrees()[..1].to_vec());
    let decision = decide(&create_subject(), &observations);
    assert_eq!(decision.outcome, WriterPreflightOutcome::Pass, "{:?}", decision.reasons);
    assert!(decision.reasons.is_empty(), "{:?}", decision.reasons);
}

#[test]
fn healthy_mutate_in_place_is_pass() {
    let decision = decide(&mutate_subject(), &existing_candidate_observations());
    assert_eq!(decision.outcome, WriterPreflightOutcome::Pass, "{:?}", decision.reasons);
}

// ---- Shift-left falsifier 1: missing required evidence becomes PASS -------------

#[test]
fn shift_left_1_stale_required_fact_refuses_not_proven() {
    let mut observations = healthy_observations();
    observations.base_sha = Observation::stale();
    let decision = decide(&create_subject(), &observations);
    assert_eq!(decision.outcome, WriterPreflightOutcome::NotProven);
    assert!(has_reason(&decision, WriterPreflightReason::ProviderUnavailableOrStale));
}

#[test]
fn shift_left_1_unavailable_provider_refuses_not_proven() {
    let mut observations = healthy_observations();
    observations.worktrees = Observation::provider_unavailable();
    let decision = decide(&create_subject(), &observations);
    assert_eq!(decision.outcome, WriterPreflightOutcome::NotProven);
    assert!(has_reason(&decision, WriterPreflightReason::ProviderUnavailableOrStale));
}

#[test]
fn shift_left_1_confirmed_absent_base_is_not_proven_not_blocked() {
    let mut observations = healthy_observations();
    observations.base_sha = Observation::absent();
    let decision = decide(&create_subject(), &observations);
    assert_eq!(decision.outcome, WriterPreflightOutcome::NotProven);
    assert!(has_reason(&decision, WriterPreflightReason::BaseOrRemoteNotProven));
}

// ---- Shift-left falsifier 2: read-only requirements authorize mutation ----------

#[test]
fn shift_left_2_read_only_pass_does_not_authorize_canonical_mutation() {
    let mut observations = healthy_observations();
    observations.checkout_relation = Observation::current(CheckoutRelation {
        root: COMMON_DIR.to_string(),
        canonical_checkout: true,
    });

    // The same evidence set verifies a read-only subject...
    let read_only = decide(&read_only_subject(), &observations);
    assert_eq!(read_only.outcome, WriterPreflightOutcome::Pass);

    // ...but refuses the in-place mutation of the canonical checkout...
    let mutate = decide(&mutate_subject(), &observations);
    assert_eq!(mutate.outcome, WriterPreflightOutcome::Blocked);
    assert!(has_reason(&mutate, WriterPreflightReason::CanonicalCheckoutMutation));

    // ...and refuses creating AT the canonical root, which aliases it.
    let mut aliasing = create_subject();
    aliasing.claim.worktree_path = Some(COMMON_DIR.to_string());
    let aliased_create = decide(&aliasing, &observations);
    assert_eq!(aliased_create.outcome, WriterPreflightOutcome::Blocked);
    assert!(has_reason(&aliased_create, WriterPreflightReason::CanonicalCheckoutMutation));
}

#[test]
fn shift_left_2_read_only_wrong_repository_refuses_too() {
    let mut observations = healthy_observations();
    observations.repository_identity = Observation::current(RepositoryIdentity {
        common_dir: "E:/somewhere-else".to_string(),
        canonical_remote: None,
    });
    let read_only = decide(&read_only_subject(), &observations);
    assert_eq!(read_only.outcome, WriterPreflightOutcome::Blocked);
    assert!(has_reason(&read_only, WriterPreflightReason::WrongOrUnknownRepository));

    let create = decide(&create_subject(), &observations);
    assert_eq!(create.outcome, WriterPreflightOutcome::Blocked);
    assert!(has_reason(&create, WriterPreflightReason::WrongOrUnknownRepository));
}

// ---- Shift-left falsifier 3: one subject checked, another admitted --------------

#[test]
fn shift_left_3_checked_head_on_another_branch_refuses() {
    let mut observations = healthy_observations();
    observations.head_state = Observation::current(HeadState::OnBranch {
        name: "impl/9999-other".to_string(),
        protected: false,
    });
    let decision = decide(&mutate_subject(), &observations);
    assert_eq!(decision.outcome, WriterPreflightOutcome::Blocked);
    assert!(has_reason(&decision, WriterPreflightReason::BranchWorktreeMismatch));
}

#[test]
fn shift_left_3_named_worktree_differs_from_invoked_checkout_refuses() {
    let mut observations = healthy_observations();
    observations.checkout_relation = Observation::current(CheckoutRelation {
        root: "E:/code/perl-lsp-swarm/.claude/worktrees/agent-2".to_string(),
        canonical_checkout: false,
    });
    // The transition names another worktree than the one it runs in: the
    // exact checked-A/mutate-B refusal.
    let mut subject = mutate_subject();
    subject.claim.worktree_path = Some(LINKED_ROOT.to_string());
    let decision = decide(&subject, &observations);
    assert_eq!(decision.outcome, WriterPreflightOutcome::Blocked);
    assert!(has_reason(&decision, WriterPreflightReason::BranchWorktreeMismatch));
}

#[test]
fn shift_left_3_moved_head_against_expected_candidate_refuses() {
    let mut observations = healthy_observations();
    observations.head_sha = Observation::current("dead9999".to_string());
    let decision = decide(&mutate_subject(), &observations);
    assert_eq!(decision.outcome, WriterPreflightOutcome::Blocked);
    assert!(has_reason(&decision, WriterPreflightReason::WrongOrUnknownCandidate));
}

#[test]
fn shift_left_3_subject_digest_binds_the_decided_subject() {
    let observations = healthy_observations();
    let decided = decide(&mutate_subject(), &observations);
    assert_eq!(decided.subject_digest, digest_subject(&mutate_subject()));

    let mut different = mutate_subject();
    different.claim.branch = "impl/other".to_string();
    assert_ne!(decided.subject_digest, digest_subject(&different));
}

// ---- Shift-left falsifier 4: same-candidate collision treated as advisory -------

#[test]
fn shift_left_4_same_candidate_collision_blocks_mutations() {
    let mut observations = healthy_observations();
    observations.same_candidate_writer = Observation::current(SameCandidateWriter {
        active: true,
        owner: Some("writer-b".to_string()),
    });
    let decision = decide(&create_subject(), &observations);
    assert_eq!(decision.outcome, WriterPreflightOutcome::Blocked);
    assert!(has_reason(&decision, WriterPreflightReason::SameCandidateCollision));
}

#[test]
fn declared_owner_reentry_is_not_a_collision() {
    let mut subject = create_subject();
    subject.expected_writer_owner = Some("writer-a".to_string());
    let mut observations = healthy_observations();
    observations.same_candidate_writer = Observation::current(SameCandidateWriter {
        active: true,
        owner: Some("writer-a".to_string()),
    });
    let decision = decide(&subject, &observations);
    assert_ne!(decision.outcome, WriterPreflightOutcome::Blocked, "{:?}", decision.reasons);
    assert!(!has_reason(&decision, WriterPreflightReason::SameCandidateCollision));
}

#[test]
fn active_writer_without_declared_owner_always_collides() {
    let mut subject = create_subject();
    subject.expected_writer_owner = None;
    let mut observations = healthy_observations();
    observations.same_candidate_writer = Observation::current(SameCandidateWriter {
        active: true,
        owner: Some("writer-a".to_string()),
    });
    let decision = decide(&subject, &observations);
    assert!(has_reason(&decision, WriterPreflightReason::SameCandidateCollision));
}

// ---- Shift-left falsifier 5: dirty/unpushed unique state represented unsafe ------

#[test]
fn shift_left_5_unique_state_at_risk_blocks_while_plain_dirtiness_does_not() {
    let mut observations = existing_candidate_observations();
    observations.working_tree = Observation::current(WorkingTreeDisposition {
        dirty_files: 7,
        staged_files: 2,
        untracked_files: 3,
        unpushed_commits: 4,
        behind_upstream: 0,
        unique_work_at_risk: false,
    });
    let plain_dirty = decide(&mutate_subject(), &observations);
    assert_eq!(plain_dirty.outcome, WriterPreflightOutcome::Pass, "{:?}", plain_dirty.reasons);

    observations.working_tree = Observation::current(WorkingTreeDisposition {
        unique_work_at_risk: true,
        ..tree_disposition()
    });
    let at_risk = decide(&mutate_subject(), &observations);
    assert_eq!(at_risk.outcome, WriterPreflightOutcome::Blocked);
    assert!(has_reason(&at_risk, WriterPreflightReason::UniqueStateAtRisk));
}

// ---- Shift-left falsifiers 6 and 7: ambient versus executor-owned Cargo --------

#[test]
fn shift_left_6_ambient_override_blocks_and_is_distinct_from_executor_config() {
    let mut observations = healthy_observations();
    observations.ambient_cargo_overrides = Observation::current(vec![AmbientCargoOverride {
        variable: "CARGO_TARGET_DIR".to_string(),
        source: AmbientCargoSource::InheritedEnvironment,
    }]);
    observations.executor_cargo_config = Observation::current(ExecutorCargoPresence::Absent);
    let decision = decide(&create_subject(), &observations);
    assert_eq!(decision.outcome, WriterPreflightOutcome::Blocked);
    assert!(has_reason(&decision, WriterPreflightReason::AmbientExecutionOverride));
    assert!(!has_reason(&decision, WriterPreflightReason::ExecutorConfigurationMismatch));
}

#[test]
fn shift_left_6_persistent_config_and_unknown_provenance_are_still_ambient() {
    for source in [AmbientCargoSource::PersistentConfigFile, AmbientCargoSource::UnknownProvenance]
    {
        let mut observations = healthy_observations();
        observations.ambient_cargo_overrides = Observation::current(vec![AmbientCargoOverride {
            variable: "CARGO_TARGET_DIR".to_string(),
            source,
        }]);
        let decision = decide(&create_subject(), &observations);
        assert!(
            has_reason(&decision, WriterPreflightReason::AmbientExecutionOverride),
            "{source:?} must remain an ambient override"
        );
    }
}

#[test]
fn shift_left_7_exact_process_local_selection_matching_policy_is_not_contamination() {
    let mut subject = create_subject();
    subject.executor_policy = Some("executor-9548".to_string());
    let mut observations = healthy_observations();
    observations.executor_cargo_config = Observation::current(ExecutorCargoPresence::Present {
        policy_id: "executor-9548".to_string(),
    });
    let decision = decide(&subject, &observations);
    assert_eq!(decision.outcome, WriterPreflightOutcome::Pass, "{:?}", decision.reasons);
    assert!(!has_reason(&decision, WriterPreflightReason::AmbientExecutionOverride));
    assert!(!has_reason(&decision, WriterPreflightReason::ExecutorConfigurationMismatch));
}

#[test]
fn shift_left_7_mismatched_or_undeclared_executor_configuration_blocks() {
    let mut subject = create_subject();
    subject.executor_policy = Some("executor-9548".to_string());

    let mut observations = healthy_observations();
    observations.executor_cargo_config = Observation::current(ExecutorCargoPresence::Present {
        policy_id: "executor-other".to_string(),
    });
    let mismatched = decide(&subject, &observations);
    assert!(has_reason(&mismatched, WriterPreflightReason::ExecutorConfigurationMismatch));

    observations.executor_cargo_config = Observation::current(ExecutorCargoPresence::Absent);
    let missing = decide(&subject, &observations);
    assert!(has_reason(&missing, WriterPreflightReason::ExecutorConfigurationMismatch));

    let mut undeclared = healthy_observations();
    undeclared.executor_cargo_config = Observation::current(ExecutorCargoPresence::Present {
        policy_id: "executor-9548".to_string(),
    });
    let unexpected = decide(&create_subject(), &undeclared);
    assert!(has_reason(&unexpected, WriterPreflightReason::ExecutorConfigurationMismatch));
}

// ---- Shift-left falsifier 8: behind-only becomes a required block ----------------

#[test]
fn shift_left_8_behind_only_is_advisory_context_never_a_denial() {
    let mut observations = healthy_observations();
    observations.working_tree = Observation::current(WorkingTreeDisposition {
        behind_upstream: 5,
        unpushed_commits: 0,
        ..tree_disposition()
    });
    let decision = decide(&create_subject(), &observations);
    assert_eq!(decision.outcome, WriterPreflightOutcome::Advisory);
    assert!(has_reason(&decision, WriterPreflightReason::AdvisoryBehindOnly));
}

#[test]
fn diverged_upstream_is_not_the_behind_only_advisory() {
    let mut observations = healthy_observations();
    observations.working_tree = Observation::current(WorkingTreeDisposition {
        behind_upstream: 5,
        unpushed_commits: 2,
        ..tree_disposition()
    });
    let decision = decide(&create_subject(), &observations);
    assert!(!has_reason(&decision, WriterPreflightReason::AdvisoryBehindOnly));
}

// ---- Shift-left falsifier 9: unrelated host load becomes universal denial -------

#[test]
fn shift_left_9_unrelated_load_stays_advisory_even_for_heavy_builds() {
    let mut subject = create_subject();
    subject.capacity_requirement = Some(CapacityRequirement::HeavyBuild);
    let mut observations = healthy_observations();
    observations.capacity = Observation::current(CapacityObservation {
        free_gb: 900.0,
        meets_selected_requirement: true,
        unrelated_host_load: true,
    });
    let decision = decide(&subject, &observations);
    assert_eq!(decision.outcome, WriterPreflightOutcome::Advisory);
    assert!(has_reason(&decision, WriterPreflightReason::AdvisoryUnrelatedHostLoad));
    assert!(!has_reason(&decision, WriterPreflightReason::CriticalCapacityBlock));
}

// ---- Shift-left falsifier 10: provider success overriding typed unknowns ---------

#[test]
fn shift_left_10_failed_required_evidence_refuses_regardless_of_other_affirmatives() {
    let mut observations = healthy_observations();
    observations.head_state = Observation::provider_unavailable();
    let read_only = decide(&read_only_subject(), &observations);
    assert_eq!(read_only.outcome, WriterPreflightOutcome::NotProven);

    let unsupported_index = healthy_observations();
    let mut unsupported_index = WriterPreflightObservationSet {
        index_state: Observation::unsupported(),
        ..unsupported_index
    };
    unsupported_index.worktrees = Observation::current(healthy_worktrees());
    unsupported_index.remote_branch =
        Observation::current(RemoteBranchPresence::Present { head_sha: "cafe0001".to_string() });
    let decision = decide(&mutate_subject(), &unsupported_index);
    assert_eq!(decision.outcome, WriterPreflightOutcome::NotProven);
    assert!(has_reason(&decision, WriterPreflightReason::ProviderUnavailableOrStale));
}

// ---- Shift-left falsifier 11: machine path convention becomes repository policy -

#[test]
fn shift_left_11_identity_tokens_are_compared_not_parsed() {
    // The core never parses path shapes: only exact adapter-normalized
    // tokens decide repository identity. Two spellings of the "same" path
    // that were not normalized by the adapter are simply different
    // repositories here; normalization is adapter work (#11634), never a
    // domain-side convention.
    let mut observations = healthy_observations();
    observations.repository_identity = Observation::current(RepositoryIdentity {
        common_dir: "e:/CODE/perl-lsp-swarm".to_string(),
        canonical_remote: Some(CANONICAL_REMOTE.to_string()),
    });
    let decision = decide(&create_subject(), &observations);
    assert!(has_reason(&decision, WriterPreflightReason::WrongOrUnknownRepository));
}

// ---- Shift-left falsifier 12: input ordering changes decision identity ----------

#[test]
fn shift_left_12_input_ordering_preserves_decision_identity() {
    let subject = create_subject();

    let worktrees = healthy_worktrees();
    let reserved_refs =
        vec!["refs/heads/feature-a".to_string(), "refs/heads/origin/feature-b".to_string()];
    let ambient = vec![
        AmbientCargoOverride {
            variable: "CARGO_TARGET_DIR".to_string(),
            source: AmbientCargoSource::InheritedEnvironment,
        },
        AmbientCargoOverride {
            variable: "CARGO_BUILD_JOBS".to_string(),
            source: AmbientCargoSource::PersistentConfigFile,
        },
    ];

    let mut forward = healthy_observations();
    forward.worktrees = Observation::current(worktrees.clone());
    forward.reserved_local_refs = Observation::current(reserved_refs.clone());
    forward.ambient_cargo_overrides = Observation::current(ambient.clone());

    let mut reversed_set = worktrees;
    reversed_set.reverse();
    let mut reversed_reserved = reserved_refs;
    reversed_reserved.reverse();
    let mut reversed_ambient = ambient;
    reversed_ambient.reverse();

    let mut reversed = forward.clone();
    reversed.worktrees = Observation::current(reversed_set);
    reversed.reserved_local_refs = Observation::current(reversed_reserved);
    reversed.ambient_cargo_overrides = Observation::current(reversed_ambient);

    let forward_decision = decide(&subject, &forward);
    let reversed_decision = decide(&subject, &reversed);
    assert_eq!(forward_decision, reversed_decision);
    assert_eq!(forward_decision.digest(), reversed_decision.digest());
}

// ---- Shift-left falsifier 13: human rendering and JSON differ -------------------

#[test]
fn shift_left_13_human_json_and_explain_agree() {
    let mut observations = healthy_observations();
    observations.checkout_relation = Observation::current(CheckoutRelation {
        root: COMMON_DIR.to_string(),
        canonical_checkout: true,
    });
    let decision = decide(&mutate_subject(), &observations);

    let json = serde_json::to_string(&decision).ok().unwrap_or_default();
    assert!(!json.is_empty());
    assert!(json.contains("\"BLOCKED\""));
    assert!(json.contains("\"canonical_checkout_mutation\""));

    let human = render_human(&decision);
    assert!(human.contains("BLOCKED"), "{human}");
    assert!(human.contains("canonical_checkout_mutation"), "{human}");
    for reason in &decision.reasons {
        assert!(json.contains(reason.as_str()), "{} missing from JSON", reason.as_str());
        assert!(human.contains(reason.as_str()), "{} missing from human text", reason.as_str());
        assert!(!explain(*reason).is_empty());
    }

    let round_trip: WriterPreflightDecision = serde_json::from_str(&json).unwrap_or_else(|_| {
        serde_json::from_str(&serde_json::to_string(&decision).unwrap_or_default())
            .unwrap_or_else(|_| decision.clone())
    });
    assert_eq!(round_trip, decision);
    assert_eq!(round_trip.digest(), decision.digest());
}

#[test]
fn advisory_outcome_renders_consistently_across_projections() {
    let mut observations = healthy_observations();
    observations.working_tree = Observation::current(WorkingTreeDisposition {
        behind_upstream: 2,
        unpushed_commits: 0,
        ..tree_disposition()
    });
    let decision = decide(&create_subject(), &observations);
    assert_eq!(decision.outcome.as_str(), "ADVISORY");
    let json = serde_json::to_string(&decision).ok().unwrap_or_default();
    let human = render_human(&decision);
    assert!(json.contains("\"ADVISORY\""));
    assert!(human.contains("ADVISORY"));
    assert!(human.contains("advisory_behind_only"));
}

// ---- Shift-left falsifier 14: unknown variants silently ignored -----------------

#[test]
fn shift_left_14_unknown_fields_are_rejected_everywhere() {
    let decision = decide(&read_only_subject(), &healthy_observations());
    let json = serde_json::to_string(&decision).ok().unwrap_or_default();

    let mutated = json.replace("\"schema_version\"", "\"unexpected_field\",\"schema_version\"");
    assert!(
        serde_json::from_str::<WriterPreflightDecision>(&mutated).is_err(),
        "unknown decision fields must fail deserialization"
    );

    let bad_outcome = json.replace("\"PASS\"", "\"MAYBE\"");
    assert!(serde_json::from_str::<WriterPreflightDecision>(&bad_outcome).is_err());

    let observations = healthy_observations();
    let observations_json = serde_json::to_string(&observations).ok().unwrap_or_default();
    let bad_state = observations_json.replace("\"repository_identity\"", "\"mystery_fact\"");
    assert!(
        serde_json::from_str::<WriterPreflightObservationSet>(&bad_state).is_err(),
        "unknown observation facts must fail deserialization"
    );
    let bad_observation_state =
        observations_json.replace("\"state\":\"current\"", "\"state\":\"vibes\"");
    assert!(serde_json::from_str::<WriterPreflightObservationSet>(&bad_observation_state).is_err());
}

// ---- Decision table: operation-specific required facts ---------------------------

#[test]
fn protected_branch_refuses_in_place_transitions_but_not_create() {
    let mut observations = healthy_observations();
    observations.head_state = Observation::current(HeadState::OnBranch {
        name: CANDIDATE_BRANCH.to_string(),
        protected: true,
    });
    let decision = decide(&mutate_subject(), &observations);
    assert!(has_reason(&decision, WriterPreflightReason::ProtectedOrDetachedMutation));

    // Create does not bind to the current HEAD's branch shape.
    let create = decide(&create_subject(), &observations);
    assert!(!has_reason(&create, WriterPreflightReason::ProtectedOrDetachedMutation));
}

#[test]
fn detached_head_refuses_in_place_transitions() {
    let mut observations = healthy_observations();
    observations.head_state = Observation::current(HeadState::Detached);
    let decision = decide(&mutate_subject(), &observations);
    assert!(has_reason(&decision, WriterPreflightReason::ProtectedOrDetachedMutation));
}

#[test]
fn unknown_mutation_subject_is_load_bearing() {
    let mut subject = create_subject();
    subject.claim.branch = "   ".to_string();
    let decision = decide(&subject, &healthy_observations());
    assert_eq!(decision.outcome, WriterPreflightOutcome::Blocked);
    assert!(has_reason(&decision, WriterPreflightReason::WrongOrUnknownCandidate));
}

#[test]
fn base_expectation_mismatch_is_not_proven_even_when_live_base_exists() {
    let mut subject = create_subject();
    subject.expected_base_sha = Some("cafebabe42".to_string());
    let decision = decide(&subject, &healthy_observations());
    assert_eq!(decision.outcome, WriterPreflightOutcome::NotProven);
    assert!(has_reason(&decision, WriterPreflightReason::BaseOrRemoteNotProven));
}

#[test]
fn abbreviated_base_sha_prefix_matches_like_3957() {
    let mut subject = create_subject();
    subject.expected_base_sha = Some("DEADBEEF".to_string());
    let decision = decide(&subject, &healthy_observations());
    assert!(!has_reason(&decision, WriterPreflightReason::BaseOrRemoteNotProven));

    subject.expected_base_sha = Some("dea".to_string());
    let too_short = decide(&subject, &healthy_observations());
    assert!(has_reason(&too_short, WriterPreflightReason::BaseOrRemoteNotProven));
}

#[test]
fn malformed_observed_identity_tokens_refuse_without_panicking() {
    // Reviewer's #12059 falsifier: `full = "ab€def0"` cuts mid-character at
    // byte 4, so the pre-repair prefix slice panicked on
    // `full[..expected.len()]`. The repaired comparison must stay total:
    // no panic, and the unprovable identity routes to the typed refusal —
    // `base_or_remote_not_proven` (NOT_PROVEN outcome) for the base, never
    // a silent match.
    let mut subject = create_subject();
    subject.expected_base_sha = Some("abcd".to_string());

    let mut observations = healthy_observations();
    observations.base_sha = Observation::current("ab€def0".to_string());
    let decision = decide(&subject, &observations);
    assert_eq!(decision.outcome, WriterPreflightOutcome::NotProven);
    assert!(has_reason(&decision, WriterPreflightReason::BaseOrRemoteNotProven));

    // Overshort observed token against a well-formed expectation: same law.
    let mut short_base = healthy_observations();
    short_base.base_sha = Observation::current("abc".to_string());
    let short_decision = decide(&subject, &short_base);
    assert_eq!(short_decision.outcome, WriterPreflightOutcome::NotProven);
    assert!(has_reason(&short_decision, WriterPreflightReason::BaseOrRemoteNotProven));

    // Candidate/head comparisons refuse through the candidate reason.
    let mut head_subject = mutate_subject();
    let mut observations_head = healthy_observations();
    observations_head.worktrees = Observation::current(healthy_worktrees());
    observations_head.head_sha = Observation::current("ab€def0".to_string());
    head_subject.candidate_head_sha = Some("abcd0000".to_string());
    let head_decision = decide(&head_subject, &observations_head);
    assert_eq!(head_decision.outcome, WriterPreflightOutcome::Blocked);
    assert!(has_reason(&head_decision, WriterPreflightReason::WrongOrUnknownCandidate));
}

#[test]
fn existing_remote_branch_redirects_create_to_resume() {
    let observations = existing_candidate_observations();
    let create = decide(&create_subject(), &observations);
    assert!(has_reason(&create, WriterPreflightReason::WrongOrUnknownCandidate));

    let mut resume_subject = create_subject();
    resume_subject.operation = WriterPreflightOperation::Resume;
    resume_subject.claim.worktree_path = None;
    resume_subject.candidate_head_sha = Some("cafe0001".to_string());
    let resume = decide(&resume_subject, &observations);
    assert!(!has_reason(&resume, WriterPreflightReason::WrongOrUnknownCandidate));
}

#[test]
fn resume_requires_an_existing_remote_candidate() {
    let mut subject = create_subject();
    subject.operation = WriterPreflightOperation::Resume;
    subject.claim.worktree_path = None;

    let observations = existing_candidate_observations(); // remote candidate Present
    let mut observations = observations;
    observations.remote_branch = Observation::current(RemoteBranchPresence::Absent);
    let decision = decide(&subject, &observations);
    assert!(has_reason(&decision, WriterPreflightReason::WrongOrUnknownCandidate));
}

#[test]
fn registration_conflicts_block_create() {
    let mut observations = healthy_observations();
    // The requested branch is already checked out somewhere.
    observations.worktrees = Observation::current(healthy_worktrees());
    let branch_taken = decide(&create_subject(), &observations);
    assert!(has_reason(&branch_taken, WriterPreflightReason::BranchWorktreeMismatch));

    // The requested path is registered to a different branch.
    observations.worktrees = Observation::current(vec![WorktreeRecord {
        path: LINKED_ROOT.to_string(),
        branch: Some("impl/other".to_string()),
        head_sha: None,
        locked: false,
    }]);
    let path_taken = decide(&create_subject(), &observations);
    assert!(has_reason(&path_taken, WriterPreflightReason::BranchWorktreeMismatch));
}

#[test]
fn ambiguous_multi_registration_of_the_claimed_branch_blocks_resume() {
    let mut observations = healthy_observations();
    observations.worktrees = Observation::current(vec![
        WorktreeRecord {
            path: "E:/wt/a".to_string(),
            branch: Some(CANDIDATE_BRANCH.to_string()),
            head_sha: None,
            locked: false,
        },
        WorktreeRecord {
            path: "E:/wt/b".to_string(),
            branch: Some(CANDIDATE_BRANCH.to_string()),
            head_sha: None,
            locked: false,
        },
    ]);
    let mut subject = mutate_subject();
    subject.candidate_head_sha = None;
    let decision = decide(&subject, &observations);
    assert!(has_reason(&decision, WriterPreflightReason::BranchWorktreeMismatch));
}

#[test]
fn unresolved_merge_conflicts_block_all_mutations() {
    let mut observations = healthy_observations();
    observations.index_state = Observation::current(IndexState::UnmergedPaths);
    let decision = decide(&create_subject(), &observations);
    assert_eq!(decision.outcome, WriterPreflightOutcome::Blocked);
    assert!(has_reason(&decision, WriterPreflightReason::UnresolvedIndexOrMerge));
}

#[test]
fn reserved_local_ref_shadow_blocks_create() {
    let mut observations = healthy_observations();
    observations.reserved_local_refs =
        Observation::current(vec![format!("refs/heads/origin/{CANDIDATE_BRANCH}")]);
    let decision = decide(&create_subject(), &observations);
    assert_eq!(decision.outcome, WriterPreflightOutcome::Blocked);
    assert!(has_reason(&decision, WriterPreflightReason::ReservedLocalRefCollision));
}

#[test]
fn selected_capacity_failure_blocks_only_when_requirement_selected() {
    let mut subject = create_subject();
    subject.capacity_requirement = Some(CapacityRequirement::HeavyBuild);

    let mut failing = healthy_observations();
    failing.capacity = Observation::current(CapacityObservation {
        free_gb: 1.0,
        meets_selected_requirement: false,
        unrelated_host_load: false,
    });
    let decision = decide(&subject, &failing);
    assert_eq!(decision.outcome, WriterPreflightOutcome::Blocked);
    assert!(has_reason(&decision, WriterPreflightReason::CriticalCapacityBlock));

    let mut unavailable = healthy_observations();
    unavailable.capacity = Observation::stale();
    let stale_capacity = decide(&subject, &unavailable);
    assert_eq!(stale_capacity.outcome, WriterPreflightOutcome::NotProven);

    // No requirement selected: the same failing observation is irrelevant.
    let unselected = decide(&create_subject(), &failing);
    assert!(!has_reason(&unselected, WriterPreflightReason::CriticalCapacityBlock));
}

#[test]
fn shared_stash_is_advisory_context_only() {
    let mut observations = healthy_observations();
    observations.stash = Observation::current(StashState::SharedStashPresent);
    let decision = decide(&create_subject(), &observations);
    assert_eq!(decision.outcome, WriterPreflightOutcome::Advisory);
    assert!(has_reason(&decision, WriterPreflightReason::AdvisorySharedStashPresent));
}

#[test]
fn blocking_prerequisite_takes_precedence_over_residual_uncertainty() {
    let mut observations = healthy_observations();
    observations.index_state = Observation::current(IndexState::UnmergedPaths);
    observations.base_sha = Observation::stale();
    let decision = decide(&create_subject(), &observations);
    assert_eq!(decision.outcome, WriterPreflightOutcome::Blocked);
    assert!(has_reason(&decision, WriterPreflightReason::UnresolvedIndexOrMerge));
    assert!(has_reason(&decision, WriterPreflightReason::ProviderUnavailableOrStale));
}

// ---- Determinism and digest identity ---------------------------------------------

// ---- #12059 review repairs: false-PASS / false-BLOCK falsifiers -------------------

#[test]
fn pinned_remote_with_missing_observed_remote_is_not_proven() {
    // (Some(expected), None): a pinned canonical remote with no observed
    // remote is less evidence than the caller required; the common dir
    // alone must not mint repository identity.
    let mut observations = healthy_observations();
    observations.repository_identity = Observation::current(RepositoryIdentity {
        common_dir: COMMON_DIR.to_string(),
        canonical_remote: None,
    });
    let read_only = decide(&read_only_subject(), &observations);
    assert_eq!(read_only.outcome, WriterPreflightOutcome::NotProven);
    assert!(has_reason(&read_only, WriterPreflightReason::ProviderUnavailableOrStale));
    assert!(!has_reason(&read_only, WriterPreflightReason::WrongOrUnknownRepository));
}

#[test]
fn mutating_subject_without_expected_base_refuses() {
    let mut subject = create_subject();
    subject.expected_base_sha = None;
    let decision = decide(&subject, &existing_candidate_observations());
    assert!(has_reason(&decision, WriterPreflightReason::BaseOrRemoteNotProven));
    assert_ne!(decision.outcome, WriterPreflightOutcome::Pass);

    let mut blank = create_subject();
    blank.expected_base_sha = Some("   ".to_string());
    let blank_decision = decide(&blank, &existing_candidate_observations());
    assert!(has_reason(&blank_decision, WriterPreflightReason::BaseOrRemoteNotProven));
}

#[test]
fn existing_candidate_operations_require_expected_candidate_head() {
    let mut resume = create_subject();
    resume.operation = WriterPreflightOperation::Resume;
    resume.candidate_head_sha = None;
    let resume_decision = decide(&resume, &existing_candidate_observations());
    assert!(has_reason(&resume_decision, WriterPreflightReason::WrongOrUnknownCandidate));

    let mut mutate = mutate_subject();
    mutate.candidate_head_sha = None;
    let mutate_decision = decide(&mutate, &existing_candidate_observations());
    assert!(has_reason(&mutate_decision, WriterPreflightReason::WrongOrUnknownCandidate));

    // Create is the legitimate candidate-head exception: a fresh candidate
    // has no head yet.
    let create = decide(&create_subject(), &healthy_observations());
    assert_eq!(create.outcome, WriterPreflightOutcome::Pass, "{:?}", create.reasons);
}

#[test]
fn mutate_with_confirmed_absent_remote_candidate_refuses() {
    // Mutate is an existing-candidate operation: a confirmed-absent remote
    // candidate removes the compare-and-mutate continuity the decision
    // table requires for it.
    let mut observations = existing_candidate_observations();
    observations.remote_branch = Observation::current(RemoteBranchPresence::Absent);
    let decision = decide(&mutate_subject(), &observations);
    assert!(has_reason(&decision, WriterPreflightReason::WrongOrUnknownCandidate));
}

#[test]
fn equal_malformed_identity_tokens_refuse() {
    // Exact-equality can no longer silently pass malformed identities: both
    // operands must be hex tokens before any comparison path.
    let mut subject = create_subject();
    subject.expected_base_sha = Some("not-a-sha".to_string());

    let mut observations = healthy_observations();
    observations.base_sha = Observation::current("not-a-sha".to_string());
    let decision = decide(&subject, &observations);
    assert_eq!(decision.outcome, WriterPreflightOutcome::NotProven);
    assert!(has_reason(&decision, WriterPreflightReason::BaseOrRemoteNotProven));

    // Non-hex observed token against a hex prefix: refused, never a match.
    let mut observations_non_hex = healthy_observations();
    observations_non_hex.base_sha = Observation::current("dead\u{1f4a3}".to_string());
    let non_hex = decide(&subject_with_base("dead"), &observations_non_hex);
    assert_eq!(non_hex.outcome, WriterPreflightOutcome::NotProven);
    assert!(has_reason(&non_hex, WriterPreflightReason::BaseOrRemoteNotProven));
}

fn subject_with_base(base: &str) -> WriterPreflightSubject {
    let mut subject = create_subject();
    subject.expected_base_sha = Some(base.to_string());
    subject
}

#[test]
fn resume_or_mutate_with_unregistered_candidate_refuses() {
    // P1 (#12059 review): an all-current observation set whose worktree map
    // registers the candidate nowhere contradicts the claimed existing
    // candidate and must refuse rather than pass.
    for operation in [WriterPreflightOperation::Resume, WriterPreflightOperation::Mutate] {
        let mut observations = existing_candidate_observations();
        observations.worktrees = Observation::current(Vec::new());
        let mut subject = mutate_subject();
        subject.operation = operation;
        let decision = decide(&subject, &observations);
        assert!(
            has_reason(&decision, WriterPreflightReason::WrongOrUnknownCandidate),
            "{operation:?}"
        );
        assert_eq!(decision.outcome, WriterPreflightOutcome::Blocked);
    }
}

#[test]
fn resume_registration_at_another_path_is_not_bound_to_the_checkout() {
    // Devin BUG_0002: one matching registration at a foreign path does not
    // bind the candidate to the invoked checkout.
    let mut observations = existing_candidate_observations();
    observations.worktrees = Observation::current(vec![WorktreeRecord {
        path: "E:/code/perl-lsp-swarm/.claude/worktrees/agent-9".to_string(),
        branch: Some(CANDIDATE_BRANCH.to_string()),
        head_sha: Some("cafe0001".to_string()),
        locked: false,
    }]);
    let decision = decide(&mutate_subject(), &observations);
    assert!(has_reason(&decision, WriterPreflightReason::BranchWorktreeMismatch));
}

#[test]
fn unavailable_worktree_map_does_not_hide_known_collisions() {
    // Devin BUG_0004: the three registration observations are independent;
    // an unavailable map must not demote known blockers to NOT_PROVEN.
    let mut observations = healthy_observations();
    observations.worktrees = Observation::provider_unavailable();
    observations.same_candidate_writer = Observation::current(SameCandidateWriter {
        active: true,
        owner: Some("writer-b".to_string()),
    });
    observations.reserved_local_refs =
        Observation::current(vec![format!("refs/heads/origin/{CANDIDATE_BRANCH}")]);
    let decision = decide(&create_subject(), &observations);
    assert_eq!(decision.outcome, WriterPreflightOutcome::Blocked);
    assert!(has_reason(&decision, WriterPreflightReason::SameCandidateCollision));
    assert!(has_reason(&decision, WriterPreflightReason::ReservedLocalRefCollision));
    assert!(has_reason(&decision, WriterPreflightReason::ProviderUnavailableOrStale));
}

#[test]
fn resume_is_not_blocked_by_its_own_local_branch_ref() {
    // Devin BUG_0005: refs/heads/<branch> is the branch resume/mutate sit
    // on; only create treats it as a shadow. The remote-tracking shadow
    // collides for every operation.
    let mut observations = existing_candidate_observations();
    observations.reserved_local_refs =
        Observation::current(vec![format!("refs/heads/{CANDIDATE_BRANCH}")]);
    let resume = decide(&mutate_subject(), &observations);
    assert!(!has_reason(&resume, WriterPreflightReason::ReservedLocalRefCollision));
    assert_eq!(resume.outcome, WriterPreflightOutcome::Pass, "{:?}", resume.reasons);

    let create = decide(&create_subject(), &observations);
    assert!(has_reason(&create, WriterPreflightReason::ReservedLocalRefCollision));
}

#[test]
fn deserialized_decisions_must_satisfy_the_decision_laws() {
    // FC1 negative controls (#12059 review): wire objects that contradict
    // the decision laws, carry an unknown schema version, or supply an
    // unsorted/duplicate reason set fail deserialization instead of
    // round-tripping into a digest-attested forgery.
    let decision = decide(&create_subject(), &healthy_observations());
    assert_eq!(decision.outcome, WriterPreflightOutcome::Pass);
    let json = serde_json::to_string(&decision).ok().unwrap_or_default();
    let round_trip = serde_json::from_str::<WriterPreflightDecision>(&json);
    assert!(round_trip.is_ok(), "valid decision must round-trip: {:?}", round_trip.err());
    assert_eq!(round_trip.unwrap_or_else(|_| decision.clone()), decision);

    let forged_pass = json.replace("\"reasons\":[]", "\"reasons\":[\"same_candidate_collision\"]");
    assert!(
        serde_json::from_str::<WriterPreflightDecision>(&forged_pass).is_err(),
        "PASS with blocking reasons must be refused"
    );

    let wrong_version = json.replace(
        &format!("\"schema_version\":{}", WRITER_PREFLIGHT_SCHEMA_VERSION),
        "\"schema_version\":99",
    );
    assert!(serde_json::from_str::<WriterPreflightDecision>(&wrong_version).is_err());

    // A multi-reason decision with descending reason order is refused; a
    // duplicated reason breaks the strictly-ascending uniqueness rule.
    let mut observations = healthy_observations();
    observations.base_sha = Observation::stale();
    let mut subject = mutate_subject();
    subject.candidate_head_sha = Some("cafe0001".to_string());
    let blocked = decide(&subject, &observations);
    assert!(blocked.outcome == WriterPreflightOutcome::Blocked);
    assert!(blocked.reasons.len() >= 2, "{:?}", blocked.reasons);
    let blocked_json = serde_json::to_string(&blocked).ok().unwrap_or_default();
    let serialized_reasons = serde_json::to_string(&blocked.reasons).ok().unwrap_or_default();
    let mut reversed: Vec<WriterPreflightReason> = blocked.reasons.clone();
    reversed.reverse();
    let reversed_json = serde_json::to_string(&reversed).ok().unwrap_or_default();
    let unsorted = blocked_json.replace(&serialized_reasons, &reversed_json);
    assert!(serde_json::from_str::<WriterPreflightDecision>(&unsorted).is_err());

    let mut duplicated = blocked.reasons.clone();
    let first = blocked.reasons[0];
    duplicated.insert(1, first);
    let duplicated_json = serde_json::to_string(&duplicated).ok().unwrap_or_default();
    let duplicated_wire = blocked_json.replace(&serialized_reasons, &duplicated_json);
    assert!(serde_json::from_str::<WriterPreflightDecision>(&duplicated_wire).is_err());
}

#[test]
fn fabricated_subject_digest_refuses_deserialization() {
    let decision = decide(&read_only_subject(), &healthy_observations());
    let json = serde_json::to_string(&decision).ok().unwrap_or_default();
    let forged = json.replace(&decision.subject_digest, "0123456789abcdef");
    assert!(serde_json::from_str::<WriterPreflightDecision>(&forged).is_err());
}

// ---- Determinism and digest identity ---------------------------------------------

#[test]
fn digests_are_stable_and_field_sensitive() {
    let observations = healthy_observations();
    let first = decide(&create_subject(), &observations);
    let again = decide(&create_subject(), &observations);
    assert_eq!(first.digest(), again.digest());
    assert_eq!(first.digest().len(), 64);

    let mut changed_observation = observations.clone();
    changed_observation.index_state = Observation::current(IndexState::UnmergedPaths);
    let changed = decide(&create_subject(), &changed_observation);
    assert_ne!(first.digest(), changed.digest());

    let mut changed_subject = create_subject();
    changed_subject.expected_writer_owner = Some("writer-a".to_string());
    assert_ne!(
        first.subject_digest,
        digest_subject(&changed_subject),
        "any subject field change must move the subject digest"
    );
}
