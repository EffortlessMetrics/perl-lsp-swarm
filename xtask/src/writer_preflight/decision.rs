//! The pure decision core (#11633).
//!
//! `decide` maps one [`WriterPreflightSubject`] plus one
//! [`WriterPreflightObservationSet`] to exactly one
//! [`WriterPreflightDecision`]. It performs no Git, filesystem, process,
//! shell, or network calls — the function is total over its inputs and its
//! only effects are the returned value. Totality includes malformed input:
//! identity tokens arrive unvalidated (callers, or #11636 deserializing
//! observation sets from JSON), so every string comparison guards its own
//! slicing/charset assumptions and routes malformed identities through the
//! typed refusal reasons instead of panicking or silently passing.
//!
//! # Decision laws (from #11633)
//!
//! - `PASS` requires every fact needed for the requested transition to be
//!   current and affirmative.
//! - `BLOCKED` names deterministic repairable prerequisites for the
//!   requested transition.
//! - `ADVISORY` is non-authorizing context that never weakens a required
//!   fact; behind-only state, shared stash presence, and unrelated host
//!   load are advisories, never blanket denials.
//! - `NOT_PROVEN` is contagious: an unavailable/unsupported/stale fact that
//!   is required for safety refuses the transition.
//! - Read-only and mutating subjects have mechanically distinct required
//!   fact sets (see `required_facts_for`); a read-only verification cannot
//!   authorize mutation because the operation participates in every rule.
//! - Same-candidate collision, unknown mutation subject, destructive
//!   unique-state risk, and a failed selected-capacity requirement are
//!   load-bearing blocks.
//! - Ambient persistent Cargo overrides and exact executor-owned process-
//!   local configuration are distinct facts with distinct reasons.
//! - Branch/path naming conventions are evidence only: nothing in this
//!   module treats a name shape as authority.
//!
//! # Outcome precedence
//!
//! `Blocked` > `NotProven` > `Advisory` > `Pass`. A known repairable
//! prerequisite is reported ahead of residual uncertainty so the operator
//! can fix it; uncertainty is preserved in `reasons`, and any repaired
//! transition must still re-run preflight fresh (#11635), where remaining
//! `NOT_PROVEN` facts resurface. Required uncertainty can therefore never
//! become `PASS`.
//!
//! # Determinism
//!
//! - Reasons are collected into a sorted, deduplicated set ordered by the
//!   closed vocabulary below.
//! - Vec-valued observations are scanned order-independently (counts and
//!   membership, never "first match"), so input ordering cannot change
//!   decision identity (#11633 falsifier 12).
//! - Digests hash canonical serde JSON of closed types (fixed field order,
//!   no maps, no free-form text).

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::writer_preflight::observation::{
    CapacityObservation, ExecutorCargoPresence, HeadState, IndexState, Observation,
    ObservationState, RemoteBranchPresence, StashState, WorktreeRecord,
    WriterPreflightObservationSet,
};
use crate::writer_preflight::subject::{
    WRITER_PREFLIGHT_SCHEMA_VERSION, WriterPreflightOperation, WriterPreflightSubject,
};

/// The closed outcome space (#11633). Serialized as `PASS` / `BLOCKED` /
/// `ADVISORY` / `NOT_PROVEN`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum WriterPreflightOutcome {
    Pass,
    Blocked,
    Advisory,
    NotProven,
}

impl WriterPreflightOutcome {
    /// Stable token identical to the serde form (`PASS` / `BLOCKED` /
    /// `ADVISORY` / `NOT_PROVEN`) so human and JSON renderings agree by
    /// construction (#11633 falsifier 13).
    pub fn as_str(self) -> &'static str {
        match self {
            WriterPreflightOutcome::Pass => "PASS",
            WriterPreflightOutcome::Blocked => "BLOCKED",
            WriterPreflightOutcome::Advisory => "ADVISORY",
            WriterPreflightOutcome::NotProven => "NOT_PROVEN",
        }
    }
}

/// Semantic class of one reason; drives outcome aggregation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReasonKind {
    /// Deterministic repairable prerequisite for the requested transition.
    Blocking,
    /// Required-for-safety evidence that is unavailable/contradictory.
    NotProven,
    /// Affirmative verification marker.
    Affirmative,
    /// Non-authorizing context; never weakens or replaces a required fact.
    Advisory,
}

/// Closed typed reason vocabulary (#11633 "Reason vocabulary"). Serialized
/// as the issue's snake_case tokens. Declaration order is the canonical
/// sort order (blocking hazards first, then unproven classes, then the
/// affirmative marker, then advisories). Unknown variants fail
/// deserialization — they are never silently ignored (falsifier 14).
///
/// Two advisory tokens extend the issue's minimum list (`advisory_shared_
/// stash_present`, `advisory_unrelated_host_load`) because the decision
/// laws require those contexts to be representable without becoming
/// denials; one blocking token (`reserved_local_ref_collision`) names the
/// reserved local-ref hazard the observation model carries. Human guidance
/// for each token lives in `projection::explain`; the token itself is the
/// semantic result.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WriterPreflightReason {
    CanonicalCheckoutMutation,
    ProtectedOrDetachedMutation,
    WrongOrUnknownRepository,
    WrongOrUnknownCandidate,
    BaseOrRemoteNotProven,
    BranchWorktreeMismatch,
    ReservedLocalRefCollision,
    SameCandidateCollision,
    UnresolvedIndexOrMerge,
    UniqueStateAtRisk,
    AmbientExecutionOverride,
    ExecutorConfigurationMismatch,
    CriticalCapacityBlock,
    ProviderUnavailableOrStale,
    SafeReadOnlySubject,
    AdvisoryBehindOnly,
    AdvisorySharedStashPresent,
    AdvisoryUnrelatedHostLoad,
}

impl WriterPreflightReason {
    pub fn kind(self) -> ReasonKind {
        match self {
            WriterPreflightReason::CanonicalCheckoutMutation
            | WriterPreflightReason::ProtectedOrDetachedMutation
            | WriterPreflightReason::WrongOrUnknownRepository
            | WriterPreflightReason::WrongOrUnknownCandidate
            | WriterPreflightReason::BranchWorktreeMismatch
            | WriterPreflightReason::ReservedLocalRefCollision
            | WriterPreflightReason::SameCandidateCollision
            | WriterPreflightReason::UnresolvedIndexOrMerge
            | WriterPreflightReason::UniqueStateAtRisk
            | WriterPreflightReason::AmbientExecutionOverride
            | WriterPreflightReason::ExecutorConfigurationMismatch
            | WriterPreflightReason::CriticalCapacityBlock => ReasonKind::Blocking,
            WriterPreflightReason::BaseOrRemoteNotProven
            | WriterPreflightReason::ProviderUnavailableOrStale => ReasonKind::NotProven,
            WriterPreflightReason::SafeReadOnlySubject => ReasonKind::Affirmative,
            WriterPreflightReason::AdvisoryBehindOnly
            | WriterPreflightReason::AdvisorySharedStashPresent
            | WriterPreflightReason::AdvisoryUnrelatedHostLoad => ReasonKind::Advisory,
        }
    }

    /// Stable token identical to the serde form so human and JSON renderings
    /// agree by construction (#11633 falsifier 13).
    pub fn as_str(self) -> &'static str {
        match self {
            WriterPreflightReason::CanonicalCheckoutMutation => "canonical_checkout_mutation",
            WriterPreflightReason::ProtectedOrDetachedMutation => "protected_or_detached_mutation",
            WriterPreflightReason::WrongOrUnknownRepository => "wrong_or_unknown_repository",
            WriterPreflightReason::WrongOrUnknownCandidate => "wrong_or_unknown_candidate",
            WriterPreflightReason::BaseOrRemoteNotProven => "base_or_remote_not_proven",
            WriterPreflightReason::BranchWorktreeMismatch => "branch_worktree_mismatch",
            WriterPreflightReason::ReservedLocalRefCollision => "reserved_local_ref_collision",
            WriterPreflightReason::SameCandidateCollision => "same_candidate_collision",
            WriterPreflightReason::UnresolvedIndexOrMerge => "unresolved_index_or_merge",
            WriterPreflightReason::UniqueStateAtRisk => "unique_state_at_risk",
            WriterPreflightReason::AmbientExecutionOverride => "ambient_execution_override",
            WriterPreflightReason::ExecutorConfigurationMismatch => {
                "executor_configuration_mismatch"
            }
            WriterPreflightReason::CriticalCapacityBlock => "critical_capacity_block",
            WriterPreflightReason::ProviderUnavailableOrStale => "provider_unavailable_or_stale",
            WriterPreflightReason::SafeReadOnlySubject => "safe_read_only_subject",
            WriterPreflightReason::AdvisoryBehindOnly => "advisory_behind_only",
            WriterPreflightReason::AdvisorySharedStashPresent => "advisory_shared_stash_present",
            WriterPreflightReason::AdvisoryUnrelatedHostLoad => "advisory_unrelated_host_load",
        }
    }
}

impl std::fmt::Display for WriterPreflightReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One decision over one subject (#11633). Every projection (human text,
/// JSON, explain) derives from this single object; nothing else is semantic.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WriterPreflightDecision {
    pub schema_version: u32,
    pub outcome: WriterPreflightOutcome,
    /// Sorted, deduplicated, closed-vocabulary reasons covering every
    /// blocking, not-proven, affirmative, and advisory finding.
    pub reasons: Vec<WriterPreflightReason>,
    /// Canonical digest of the decided subject. #11635 compares this
    /// immediately before mutation (compare-and-mutate continuity).
    pub subject_digest: String,
}

impl WriterPreflightDecision {
    /// Canonical digest of this entire decision (schema, outcome, reasons,
    /// and subject digest). Deterministic across processes and platforms
    /// for equal inputs.
    pub fn digest(&self) -> String {
        canonical_digest(self)
    }

    pub fn reason(&self, reason: WriterPreflightReason) -> bool {
        self.reasons.contains(&reason)
    }
}

/// Digest of a bare subject, for callers (#11635) that need to bind a
/// candidate before a decision exists.
pub fn digest_subject(subject: &WriterPreflightSubject) -> String {
    canonical_digest(subject)
}

/// The pure decision core. See module docs for the laws it implements.
pub fn decide(
    subject: &WriterPreflightSubject,
    observations: &WriterPreflightObservationSet,
) -> WriterPreflightDecision {
    let mut reasons = BTreeSet::new();
    let mutating = subject.operation.is_mutating();

    evaluate_repository_identity(subject, observations, &mut reasons);

    if mutating {
        // An unknown mutation subject is load-bearing (#11633 decision
        // laws): refuse before inventing a target, but keep evaluating the
        // rest so the refusal stays evidence-rich.
        if !subject.mutation_subject_known() {
            reasons.insert(WriterPreflightReason::WrongOrUnknownCandidate);
        }
        evaluate_checkout_binding(subject, observations, &mut reasons);
        evaluate_head_binding(subject, observations, &mut reasons);
        evaluate_base_and_candidate(subject, observations, &mut reasons);
        evaluate_registration_conflicts(subject, observations, &mut reasons);
        evaluate_index_and_unique_state(observations, &mut reasons);
        evaluate_cargo_environment(subject, observations, &mut reasons);
        evaluate_capacity(subject, observations, &mut reasons);
        evaluate_shared_stash(observations, &mut reasons);
    } else {
        evaluate_read_only_head(observations, &mut reasons);
    }

    evaluate_advisories(observations, &mut reasons);

    if !mutating
        && !reasons.iter().any(|r| matches!(r.kind(), ReasonKind::Blocking | ReasonKind::NotProven))
    {
        reasons.insert(WriterPreflightReason::SafeReadOnlySubject);
    }

    let outcome = aggregate_outcome(&reasons);
    WriterPreflightDecision {
        schema_version: WRITER_PREFLIGHT_SCHEMA_VERSION,
        outcome,
        reasons: reasons.into_iter().collect(),
        subject_digest: digest_subject(subject),
    }
}

fn aggregate_outcome(reasons: &BTreeSet<WriterPreflightReason>) -> WriterPreflightOutcome {
    if reasons.iter().any(|r| r.kind() == ReasonKind::Blocking) {
        WriterPreflightOutcome::Blocked
    } else if reasons.iter().any(|r| r.kind() == ReasonKind::NotProven) {
        WriterPreflightOutcome::NotProven
    } else if reasons.iter().any(|r| r.kind() == ReasonKind::Advisory) {
        WriterPreflightOutcome::Advisory
    } else {
        WriterPreflightOutcome::Pass
    }
}

/// Repository identity is required for every operation, read-only included:
/// verifying facts about the wrong repository mints false evidence. The
/// common dir compares exactly; the canonical remote compares only when the
/// subject supplies one.
fn evaluate_repository_identity(
    subject: &WriterPreflightSubject,
    observations: &WriterPreflightObservationSet,
    reasons: &mut BTreeSet<WriterPreflightReason>,
) {
    match observations.repository_identity.usable() {
        Some(observed) => {
            let remote_matches =
                match (&subject.repository.canonical_remote, &observed.canonical_remote) {
                    (Some(expected), Some(actual)) => expected == actual,
                    // Caller did not pin a remote: nothing extra to prove. An
                    // observed remote without a pinned expectation is fine.
                    _ => true,
                };
            if observed.common_dir != subject.repository.common_dir || !remote_matches {
                reasons.insert(WriterPreflightReason::WrongOrUnknownRepository);
            }
        }
        None => {
            reasons.insert(WriterPreflightReason::ProviderUnavailableOrStale);
        }
    }
}

/// Read-only subjects must at least know which HEAD they are reading;
/// everything beyond that is advisory context.
fn evaluate_read_only_head(
    observations: &WriterPreflightObservationSet,
    reasons: &mut BTreeSet<WriterPreflightReason>,
) {
    if observations.head_state.usable().is_none() {
        reasons.insert(WriterPreflightReason::ProviderUnavailableOrStale);
    }
}

/// Binds the requested transition to THIS checkout (#11633 falsifier 3:
/// checking one branch/worktree and mutating another must refuse).
fn evaluate_checkout_binding(
    subject: &WriterPreflightSubject,
    observations: &WriterPreflightObservationSet,
    reasons: &mut BTreeSet<WriterPreflightReason>,
) {
    let Some(relation) = require_current(&observations.checkout_relation, reasons) else {
        return;
    };

    let targets_this_checkout =
        subject.claim.worktree_path.as_deref().map(|p| p == relation.root).unwrap_or(true);

    if relation.canonical_checkout && targets_this_checkout {
        // Production writes never mutate the canonical checkout in place
        // (#3957's root-drift negative case); creating a worktree AT the
        // canonical root aliases it identically.
        reasons.insert(WriterPreflightReason::CanonicalCheckoutMutation);
    }

    // Resume/mutate act on the existing checkout named by the claim:
    // naming another path is precisely the cross-subject case.
    let names_other_checkout = subject.operation != WriterPreflightOperation::Create
        && subject
            .claim
            .worktree_path
            .as_deref()
            .is_some_and(|requested| requested != relation.root);
    if names_other_checkout {
        reasons.insert(WriterPreflightReason::BranchWorktreeMismatch);
    }
}

/// HEAD branch/protected/detached binding for in-place transitions.
fn evaluate_head_binding(
    subject: &WriterPreflightSubject,
    observations: &WriterPreflightObservationSet,
    reasons: &mut BTreeSet<WriterPreflightReason>,
) {
    // Create targets a brand-new branch elsewhere; the current checkout's
    // HEAD shape does not constrain it (base binding does).
    if subject.operation == WriterPreflightOperation::Create {
        return;
    }

    match require_current(&observations.head_state, reasons) {
        Some(HeadState::Detached) => {
            reasons.insert(WriterPreflightReason::ProtectedOrDetachedMutation);
        }
        Some(HeadState::OnBranch { name, protected }) => {
            if *protected {
                reasons.insert(WriterPreflightReason::ProtectedOrDetachedMutation);
            }
            if name != &subject.claim.branch {
                reasons.insert(WriterPreflightReason::BranchWorktreeMismatch);
            }
        }
        None => {}
    }
}

/// Base provenance plus candidate/head continuity.
fn evaluate_base_and_candidate(
    subject: &WriterPreflightSubject,
    observations: &WriterPreflightObservationSet,
    reasons: &mut BTreeSet<WriterPreflightReason>,
) {
    match observations.base_sha.usable() {
        Some(live_base) => {
            let contradicts_expected = subject
                .expected_base_sha
                .as_deref()
                .is_some_and(|expected| !sha_matches(live_base, expected));
            if contradicts_expected {
                reasons.insert(WriterPreflightReason::BaseOrRemoteNotProven);
            }
        }
        None => match observations.base_sha.state {
            // Confirmed absence of a resolvable base is itself the failure
            // to prove the base, not an instrument problem.
            ObservationState::Absent => {
                reasons.insert(WriterPreflightReason::BaseOrRemoteNotProven);
            }
            _ => {
                reasons.insert(WriterPreflightReason::ProviderUnavailableOrStale);
            }
        },
    }

    match observations.head_sha.usable() {
        Some(observed_head) => {
            if subject
                .candidate_head_sha
                .as_deref()
                .is_some_and(|expected| !sha_matches(observed_head, expected))
            {
                reasons.insert(WriterPreflightReason::WrongOrUnknownCandidate);
            }
        }
        None => {
            reasons.insert(WriterPreflightReason::ProviderUnavailableOrStale);
        }
    }

    match observations.remote_branch.usable() {
        Some(presence) => match presence {
            RemoteBranchPresence::Absent => {
                // Resume needs an existing remote candidate; create must not
                // shadow one. Either way the polarity is decisive.
                if subject.operation == WriterPreflightOperation::Resume {
                    reasons.insert(WriterPreflightReason::WrongOrUnknownCandidate);
                }
            }
            RemoteBranchPresence::Present { head_sha } => {
                // A candidate already exists remotely: create must resume
                // from its actual head instead of recreating it (#11635
                // RESUME law; recreation from local main is the cited
                // failure).
                let recreates = subject.operation == WriterPreflightOperation::Create;
                let moved = subject
                    .candidate_head_sha
                    .as_deref()
                    .is_some_and(|expected| !sha_matches(head_sha, expected));
                if recreates || moved {
                    reasons.insert(WriterPreflightReason::WrongOrUnknownCandidate);
                }
            }
        },
        None => match observations.remote_branch.state {
            // A confirmed-absent remote branch is legitimate evidence: it
            // only refuses the operation that requires existence (resume).
            ObservationState::Absent => {
                if subject.operation == WriterPreflightOperation::Resume {
                    reasons.insert(WriterPreflightReason::WrongOrUnknownCandidate);
                }
            }
            _ => {
                reasons.insert(WriterPreflightReason::ProviderUnavailableOrStale);
            }
        },
    }
}

/// Worktree registration and same-candidate writer conflicts.
fn evaluate_registration_conflicts(
    subject: &WriterPreflightSubject,
    observations: &WriterPreflightObservationSet,
    reasons: &mut BTreeSet<WriterPreflightReason>,
) {
    let worktrees = match require_current(&observations.worktrees, reasons) {
        Some(records) => records,
        None => return,
    };

    // The requested branch must not already be checked out anywhere, and a
    // pinned create location must not be registered to another branch.
    let branch_matches = count_matching_branch(worktrees, &subject.claim.branch);
    if branch_matches > 1 {
        // Ambiguous mapping: more than one registration owns the branch.
        reasons.insert(WriterPreflightReason::BranchWorktreeMismatch);
    }
    // Create additionally refuses an already-registered branch or a create
    // location registered to another branch; for resume/mutate a single
    // existing registration on the claimed branch is the normal, expected
    // state (REUSE/RESUME territory for #11635); ambiguity was handled
    // above.
    if subject.operation == WriterPreflightOperation::Create && branch_matches >= 1 {
        reasons.insert(WriterPreflightReason::BranchWorktreeMismatch);
    }
    let reuses_registered_path = subject.operation == WriterPreflightOperation::Create
        && subject
            .claim
            .worktree_path
            .as_deref()
            .is_some_and(|requested| worktrees.iter().any(|record| record.path == requested));
    if reuses_registered_path {
        reasons.insert(WriterPreflightReason::BranchWorktreeMismatch);
    }

    match observations.same_candidate_writer.usable() {
        Some(state) => {
            let declared_owner = subject.expected_writer_owner.as_deref();
            let reentry_by_declared_owner = state.active
                && declared_owner.is_some()
                && state.owner.as_deref() == declared_owner;
            if state.active && !reentry_by_declared_owner {
                reasons.insert(WriterPreflightReason::SameCandidateCollision);
            }
        }
        None => {
            reasons.insert(WriterPreflightReason::ProviderUnavailableOrStale);
        }
    }

    match observations.reserved_local_refs.usable() {
        Some(refs) => {
            let branch = &subject.claim.branch;
            let collides = refs.iter().any(|ref_name| {
                ref_name == &format!("refs/heads/{branch}")
                    || ref_name == &format!("refs/heads/origin/{branch}")
            });
            if collides {
                reasons.insert(WriterPreflightReason::ReservedLocalRefCollision);
            }
        }
        None => {
            reasons.insert(WriterPreflightReason::ProviderUnavailableOrStale);
        }
    }
}

fn evaluate_index_and_unique_state(
    observations: &WriterPreflightObservationSet,
    reasons: &mut BTreeSet<WriterPreflightReason>,
) {
    match observations.index_state.usable() {
        Some(IndexState::Clean) => {}
        Some(IndexState::UnmergedPaths) => {
            reasons.insert(WriterPreflightReason::UnresolvedIndexOrMerge);
        }
        None => {
            reasons.insert(WriterPreflightReason::ProviderUnavailableOrStale);
        }
    }

    match observations.working_tree.usable() {
        Some(disposition) => {
            if disposition.unique_work_at_risk {
                // Destructive unique-state risk is load-bearing (#11633):
                // dirty/unpushed work this transition would strand or
                // overwrite must refuse, while ordinary dirtiness stays
                // observational context for consumers.
                reasons.insert(WriterPreflightReason::UniqueStateAtRisk);
            }
        }
        None => {
            reasons.insert(WriterPreflightReason::ProviderUnavailableOrStale);
        }
    }
}

/// Ambient persistent Cargo overrides versus executor-owned process-local
/// configuration remain distinct facts (#11633 decision laws; #9548 owns
/// the executor model):
///
/// - any ambient override on a mutating transition is a deterministic
///   repairable prerequisite (`ambient_execution_override`);
/// - a declared executor policy must be present and match exactly, and an
///   undeclared policy must be absent — either violation is
///   `executor_configuration_mismatch`. Exact process-local target
///   selection matching the declared policy is therefore never rejected as
///   ambient contamination (falsifiers 6 and 7).
fn evaluate_cargo_environment(
    subject: &WriterPreflightSubject,
    observations: &WriterPreflightObservationSet,
    reasons: &mut BTreeSet<WriterPreflightReason>,
) {
    match observations.ambient_cargo_overrides.usable() {
        Some(overrides) => {
            if !overrides.is_empty() {
                reasons.insert(WriterPreflightReason::AmbientExecutionOverride);
            }
        }
        None => {
            reasons.insert(WriterPreflightReason::ProviderUnavailableOrStale);
        }
    }

    match observations.executor_cargo_config.usable() {
        Some(presence) => match (&subject.executor_policy, presence) {
            (Some(policy_id), ExecutorCargoPresence::Present { policy_id: observed })
                if policy_id == observed => {}
            (Some(_), _) | (None, ExecutorCargoPresence::Present { .. }) => {
                reasons.insert(WriterPreflightReason::ExecutorConfigurationMismatch);
            }
            (None, ExecutorCargoPresence::Absent) => {}
        },
        None => {
            reasons.insert(WriterPreflightReason::ProviderUnavailableOrStale);
        }
    }
}

/// Selected capacity requirement only: when the subject selects none,
/// capacity and host load stay out of the decision entirely except as
/// advisory context (#11633: unrelated load is not a universal denial).
fn evaluate_capacity(
    subject: &WriterPreflightSubject,
    observations: &WriterPreflightObservationSet,
    reasons: &mut BTreeSet<WriterPreflightReason>,
) {
    if subject.capacity_requirement.is_none() {
        return;
    }
    match observations.capacity.usable() {
        Some(CapacityObservation { meets_selected_requirement: false, .. }) => {
            reasons.insert(WriterPreflightReason::CriticalCapacityBlock);
        }
        Some(_) => {}
        None => {
            reasons.insert(WriterPreflightReason::ProviderUnavailableOrStale);
        }
    }
}

/// A shared stash is non-authorizing context for mutations: it warns that
/// cleanup paths must never drop it, without denying the transition.
fn evaluate_shared_stash(
    observations: &WriterPreflightObservationSet,
    reasons: &mut BTreeSet<WriterPreflightReason>,
) {
    if observations.stash.usable() == Some(&StashState::SharedStashPresent) {
        reasons.insert(WriterPreflightReason::AdvisorySharedStashPresent);
    }
}

/// Advisories attach to any operation, never weaken a required fact, and
/// never deny: behind-only upstream movement and unrelated host load are
/// context (#11633 falsifiers 8 and 9).
fn evaluate_advisories(
    observations: &WriterPreflightObservationSet,
    reasons: &mut BTreeSet<WriterPreflightReason>,
) {
    let behind_only = observations
        .working_tree
        .usable()
        .is_some_and(|d| d.behind_upstream > 0 && d.unpushed_commits == 0);
    if behind_only {
        reasons.insert(WriterPreflightReason::AdvisoryBehindOnly);
    }
    if observations.capacity.usable().map(|c| c.unrelated_host_load) == Some(true) {
        reasons.insert(WriterPreflightReason::AdvisoryUnrelatedHostLoad);
    }
}

/// Requires a fact to be current-and-valued; anything else (absent,
/// unsupported, provider-unavailable, stale, or malformed) inserts
/// `provider_unavailable_or_stale`. Used for facts whose absence has no
/// safe meaning; facts with meaningful absence are matched explicitly.
fn require_current<'a, T>(
    observation: &'a Observation<T>,
    reasons: &mut BTreeSet<WriterPreflightReason>,
) -> Option<&'a T> {
    match observation.usable() {
        Some(value) => Some(value),
        None => {
            reasons.insert(WriterPreflightReason::ProviderUnavailableOrStale);
            None
        }
    }
}

/// Counts registrations whose branch equals `branch`.
fn count_matching_branch(worktrees: &[WorktreeRecord], branch: &str) -> usize {
    worktrees.iter().filter(|record| record.branch.as_deref() == Some(branch)).count()
}

/// Mirrors #3957's `sha_matches` (xtask/src/tasks/writer_admission.rs):
/// full case-insensitive equality OR a case-insensitive hex-prefix match
/// with git's conventional minimum abbreviation, so abbreviated SHAs cited
/// in issues/plans do not false-negative. This crate cannot import the
/// bin-side tasks tree (lib/bin split), so the rule is restated here and
/// pinned by test against the same fixtures.
///
/// Totality and refusal semantics (repair after #12059 review): identity
/// tokens are unvalidated caller/observation strings, so every property
/// needed by the prefix slice is checked on BOTH operands before slicing —
/// length, a char boundary at the cut point on `full`, and ASCII-hex
/// charset on the compared bytes. Exact whole-string equality needs no
/// slicing and is accepted without charset demands (it cannot panic).
///
/// A malformed token never panics and never silently passes: it yields
/// `false`, i.e. "this identity cannot be proven to match". Callers route
/// that through the existing typed refusals — `base_or_remote_not_proven`
/// for the base comparison and `wrong_or_unknown_candidate` for candidate/
/// head comparisons — because an identity that is not well-formed hex of
/// sufficient length is precisely an identity that is not proven. This
/// keeps `decide` total and panic-free over arbitrary deserialized input
/// (#11636 feeds observation sets from JSON).
fn sha_matches(full: &str, expected: &str) -> bool {
    const MIN_PREFIX_LEN: usize = 4;
    if expected.eq_ignore_ascii_case(full) {
        return true;
    }
    let prefixable = expected.len() >= MIN_PREFIX_LEN
        && full.len() >= expected.len()
        && full.is_char_boundary(expected.len())
        && full.as_bytes()[..expected.len()].iter().all(|byte| byte.is_ascii_hexdigit())
        && expected.chars().all(|c| c.is_ascii_hexdigit());
    prefixable && full[..expected.len()].eq_ignore_ascii_case(expected)
}

/// Canonical digest: serde JSON of a closed type (fixed field declaration
/// order, no maps, no free-form text), hashed with SHA-256. Serialization
/// into a `Vec` sink cannot fail for these types, and non-finite floats
/// are outside the domain's inputs; the `unwrap_or_default` fallback keeps
/// production panic-free (AGENTS.md hygiene) on an unreachable path.
fn canonical_digest<T: Serialize>(value: &T) -> String {
    let bytes = serde_json::to_vec(value).unwrap_or_default();
    Sha256::digest(bytes).iter().map(|byte| format!("{byte:02x}")).collect()
}
