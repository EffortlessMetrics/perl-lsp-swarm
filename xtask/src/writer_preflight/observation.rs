//! The typed observation model (#11633).
//!
//! One provider-neutral `WriterPreflightObservationSet` carries exactly the
//! facts #11633 names. Every fact is wrapped in an [`Observation`] whose
//! state keeps absence, unsupported evidence, provider failure, stale
//! evidence, and a negative (but present) observation distinct:
//!
//! - `Current` — observed now against a current provider schema; the value
//!   is trustworthy and may be affirmative or negative.
//! - `Absent` — the provider confirmed the fact does not exist (legitimate
//!   absence, e.g. no remote branch yet). This is *evidence*, not failure.
//! - `Unsupported` — this provider/platform does not own the fact.
//! - `ProviderUnavailable` — the instrument failed. Never folded into
//!   "absent" or "safe" (#3957's invariant: instrument failure must never
//!   become a clean result).
//! - `Stale` — evidence exists but its currentness is insufficient for a
//!   safety decision.
//!
//! Values carry polarity themselves (`IndexState::UnmergedPaths` is a
//! negative-but-present observation), so the wrapper only has to answer
//! "may this value be used at all".
//!
//! There is deliberately **no free-form text anywhere in this module**:
//! provider prose must never enter the semantic result (#11633: human
//! guidance is rendered from typed reasons). Diagnostic detail stays with
//! the adapter that gathered it.

use serde::{Deserialize, Serialize};

use crate::writer_preflight::subject::RepositoryIdentity;

/// Availability/currentness of one observed fact.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObservationState {
    Current,
    Absent,
    Unsupported,
    ProviderUnavailable,
    Stale,
}

/// One observed fact plus its availability state. `value` is always
/// serialized (as JSON `null` when absent) so the schema stays closed and
/// deterministic without requiring `Default` on fact payloads.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Observation<T> {
    pub state: ObservationState,
    pub value: Option<T>,
}

impl<T> Observation<T> {
    pub fn current(value: T) -> Self {
        Self { state: ObservationState::Current, value: Some(value) }
    }

    pub fn absent() -> Self {
        Self { state: ObservationState::Absent, value: None }
    }

    pub fn unsupported() -> Self {
        Self { state: ObservationState::Unsupported, value: None }
    }

    pub fn provider_unavailable() -> Self {
        Self { state: ObservationState::ProviderUnavailable, value: None }
    }

    pub fn stale() -> Self {
        Self { state: ObservationState::Stale, value: None }
    }

    /// The value when the fact is usable; `None` for every non-current
    /// state *and* defensively for a malformed current-without-value
    /// construction (constructors make that unreachable).
    pub fn usable(&self) -> Option<&T> {
        if self.state == ObservationState::Current { self.value.as_ref() } else { None }
    }
}

/// Where the invoking checkout sits relative to the canonical checkout.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheckoutRelation {
    /// Adapter-normalized root of the checkout the observations were taken
    /// in. Opaque to the decision core (compared, never parsed).
    pub root: String,
    /// True when this checkout IS the canonical/root checkout.
    pub canonical_checkout: bool,
}

/// Branch/detached/protected state of HEAD.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HeadState {
    OnBranch { name: String, protected: bool },
    Detached,
}

/// One registered worktree from the worktree mapping.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorktreeRecord {
    pub path: String,
    pub branch: Option<String>,
    pub head_sha: Option<String>,
    pub locked: bool,
}

/// Same-candidate writer/collision state. `owner` is the opaque writer/
/// owner identity token; comparison against
/// [`crate::writer_preflight::WriterPreflightSubject::expected_writer_owner`]
/// decides re-entry versus collision.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SameCandidateWriter {
    pub active: bool,
    pub owner: Option<String>,
}

/// Index/merge-conflict state.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IndexState {
    Clean,
    UnmergedPaths,
}

/// Dirty/staged/untracked/unpushed/behind disposition of the working tree.
/// `unique_work_at_risk` is the adapter's typed judgment that this tree
/// holds unique unpushed/uncommitted work the requested transition would
/// strand or overwrite — the core never recomputes it from counts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkingTreeDisposition {
    pub dirty_files: u32,
    pub staged_files: u32,
    pub untracked_files: u32,
    pub unpushed_commits: u32,
    pub behind_upstream: u32,
    pub unique_work_at_risk: bool,
}

/// Shared stash state.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StashState {
    NoSharedStash,
    SharedStashPresent,
}

/// Ambient Cargo environment overrides with provenance class. Raw values
/// are excluded on purpose: they vary per machine and must not enter
/// decision identity or parity packets.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AmbientCargoOverride {
    pub variable: String,
    pub source: AmbientCargoSource,
}

/// Provenance of an ambient override. Presence alone is never provenance
/// (#11634 falsifier 7): the adapter must classify, or report
/// `UnknownProvenance`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AmbientCargoSource {
    PersistentConfigFile,
    InheritedEnvironment,
    UnknownProvenance,
}

/// Executor-owned process-local Cargo configuration (#9548), distinct from
/// ambient overrides by construction: this is a separate fact with its own
/// presence type.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutorCargoPresence {
    Absent,
    Present { policy_id: String },
}

/// Free-disk/capacity/process observation. `meets_selected_requirement` is
/// evaluated by the adapter against the subject's selected requirement (the
/// floor policy belongs to the caller that selected it); the core treats a
/// confirmed `false` as `CriticalCapacityBlock`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapacityObservation {
    pub free_gb: f64,
    pub meets_selected_requirement: bool,
    /// Unrelated host/agent/worktree load — advisory context, never a
    /// denial (#11633 decision law).
    pub unrelated_host_load: bool,
}

/// Remote existence of the candidate branch. `Absent` is legitimate
/// evidence (a genuinely new branch); availability failures keep their own
/// states on the wrapping `Observation`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RemoteBranchPresence {
    Absent,
    Present { head_sha: String },
}

/// The full provider-neutral observation set for one subject (#11633
/// "Observation model"). Field set is closed (`deny_unknown_fields`): an
/// unknown observation variant cannot be silently ignored (#11633
/// falsifier 14) — deserialization fails instead.
///
/// # Consumer seams
///
/// - #11634 adapters populate this struct from native platform providers;
///   POSIX and Windows may observe differently but project onto exactly
///   these fields — there is no platform-specific decision path.
/// - #11635 persists the set alongside the decision digest so mutation can
///   revalidate continuity.
/// - #11636 compares normalized sets across platforms cell-by-cell.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WriterPreflightObservationSet {
    pub repository_identity: Observation<RepositoryIdentity>,
    pub checkout_relation: Observation<CheckoutRelation>,
    pub head_state: Observation<HeadState>,
    pub head_sha: Observation<String>,
    pub base_sha: Observation<String>,
    pub remote_branch: Observation<RemoteBranchPresence>,
    /// Sorted by path before any count/scan in `decide`; input order never
    /// changes decision identity (#11633 falsifier 12).
    pub worktrees: Observation<Vec<WorktreeRecord>>,
    pub same_candidate_writer: Observation<SameCandidateWriter>,
    pub index_state: Observation<IndexState>,
    pub working_tree: Observation<WorkingTreeDisposition>,
    pub stash: Observation<StashState>,
    pub reserved_local_refs: Observation<Vec<String>>,
    pub ambient_cargo_overrides: Observation<Vec<AmbientCargoOverride>>,
    pub executor_cargo_config: Observation<ExecutorCargoPresence>,
    pub capacity: Observation<CapacityObservation>,
}
