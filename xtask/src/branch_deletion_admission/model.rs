//! Typed subjects and outcomes for branch-deletion admission (#12885).
//!
//! Every type here is plain data so the decision in [`super::evaluate`] can be
//! driven from a fixture graph. Nothing in this module reads GitHub, touches
//! git, or mutates a branch.

use serde::{Deserialize, Serialize};

pub const BRANCH_DELETION_ADMISSION_SCHEMA_VERSION: &str = "branch_deletion_admission.v1";
pub const BRANCH_DELETION_ADMISSION_POLICY_VERSION: &str = "2026-08-27";

/// Repository identity, compared exactly.
///
/// A branch name is only meaningful inside one repository: the same name in a
/// fork or a sibling repository neither creates nor erases a dependency here.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct RepositoryId {
    pub owner: String,
    pub name: String,
}

impl RepositoryId {
    pub fn new(owner: impl Into<String>, name: impl Into<String>) -> Self {
        Self { owner: owner.into(), name: name.into() }
    }

    pub fn render(&self) -> String {
        format!("{}/{}", self.owner, self.name)
    }
}

/// Lifecycle state of a pull request as reported by the live graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PullRequestState {
    Open,
    Closed,
    Merged,
}

impl PullRequestState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Open => "OPEN",
            Self::Closed => "CLOSED",
            Self::Merged => "MERGED",
        }
    }
}

/// Whether the parent reached the terminal state that would admit cleanup.
///
/// Only [`Self::Merged`] is terminal *as expected* for this route. A closed
/// unmerged parent is deliberately not terminal: its branch may still carry
/// the only copy of unsalvaged work.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ParentTerminality {
    Merged,
    Open,
    ClosedUnmerged,
    NotProven,
}

/// Mergeability of a child as reported by the live graph.
///
/// [`Self::NotProven`] covers every case where the host would not say —
/// a computation still pending, a permissions failure, or a field absent
/// from the response.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Mergeability {
    Clean,
    Conflicting,
    Blocked,
    NotProven,
}

impl Mergeability {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Clean => "CLEAN",
            Self::Conflicting => "CONFLICTING",
            Self::Blocked => "BLOCKED",
            Self::NotProven => "NOT_PROVEN",
        }
    }
}

/// Completeness of the open-child query.
///
/// Anything other than [`Self::Complete`] means the answer "no open children"
/// was never actually established, so it must not be read as one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum GraphCompleteness {
    /// Every page of the open-PR query was read to exhaustion.
    Complete,
    /// Pagination stopped early — more open PRs may name this branch.
    Truncated { detail: String },
    /// The query did not run or did not return: API absence, permissions
    /// failure, rate limiting, or a transport error.
    Unavailable { detail: String },
}

/// Local worktree/writer ownership as reported by the #3957 authority.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum WorktreeOwnership {
    /// No local worktree or writer claims the branch.
    Clear,
    /// A writer still owns a worktree on this branch.
    ActiveWriter { detail: String },
    /// Ownership could not be established.
    NotProven { detail: String },
}

/// The parent whose head branch is a candidate for deletion.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParentSubject {
    pub repository: RepositoryId,
    pub number: u64,
    /// The branch proposed for deletion — children are matched against this.
    pub head_ref: String,
    /// The head SHA of the subject that was actually reviewed and merged.
    pub reviewed_head_sha: String,
    pub terminality: ParentTerminality,
    /// Whether the parent's head branch actually lives in `repository`.
    ///
    /// `false` for a cross-repository (fork) pull request, whose `head_ref`
    /// names a branch in the *fork*. Deleting that name in this repository
    /// would delete an unrelated branch that merely shares it, so a fork head
    /// retains. Required rather than defaulted: a request that does not say
    /// must not be read as saying yes.
    pub head_in_admitted_repository: bool,
}

/// The branch as it exists right now on the remote.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BranchSubject {
    /// `None` when the current tip could not be read — treated as movement,
    /// never as agreement.
    pub current_sha: Option<String>,
}

/// One open pull request observed in the live graph.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObservedPullRequest {
    pub repository: RepositoryId,
    pub number: u64,
    pub head_ref: String,
    /// The branch this PR merges *into* — the dependency edge that matters.
    pub base_ref: String,
    pub state: PullRequestState,
    pub draft: bool,
    pub mergeable: Mergeability,
    /// Whether the parent merge is known to have changed this PR's
    /// mergeability. `None` means the graph did not say — never "no".
    #[serde(default)]
    pub mergeability_changed_by_parent_merge: Option<bool>,
}

/// The live open-PR graph, plus how completely it was read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenChildGraph {
    pub completeness: GraphCompleteness,
    pub pull_requests: Vec<ObservedPullRequest>,
}

/// Everything the admission decision is allowed to consider.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdmissionRequest {
    pub parent: ParentSubject,
    pub branch: BranchSubject,
    pub graph: OpenChildGraph,
    pub worktree_ownership: WorktreeOwnership,
    /// Git remote the deletion would target. Defaults to `origin`.
    ///
    /// A remote *name* does not by itself say which repository it resolves to,
    /// so the emitted plan pairs the deletion with a verification command; see
    /// `route::remote_verification_command`.
    #[serde(default = "default_remote")]
    pub remote: String,
    /// The exact push URL this admission was verified against.
    ///
    /// A remote *name* is mutable config: verifying that `origin` resolves to
    /// the admitted endpoint and then pushing to `origin` leaves a window in
    /// which `git remote set-url --push` redirects the deletion after every
    /// check has passed. The deletion is therefore executed against this URL,
    /// not the name. `None` for snapshot requests, which are non-authorizing
    /// and emit nothing runnable.
    #[serde(default)]
    pub push_endpoint: Option<String>,
}

fn default_remote() -> String {
    "origin".to_string()
}

/// Whether `candidate` is a full 40-character lowercase hexadecimal git object
/// id.
///
/// Deliberately strict. An abbreviated, uppercase, or empty value cannot be
/// leased against, and a request carrying one is malformed rather than merely
/// unlucky — accepting it would let `SAFE_TO_DELETE` be reached with a lease
/// target git will not honour.
pub(crate) fn is_full_object_id(candidate: &str) -> bool {
    candidate.len() == 40
        && candidate.chars().all(|ch| ch.is_ascii_hexdigit() && !ch.is_ascii_uppercase())
}

impl AdmissionRequest {
    /// Structural validation of the request before any admission logic runs.
    ///
    /// The request arrives as caller-supplied JSON, so absent or malformed
    /// fields must not be able to reach `SAFE_TO_DELETE`. Returns the first
    /// problem found, phrased for the retention detail.
    pub(crate) fn structural_problem(&self) -> Option<String> {
        if self.parent.repository.owner.trim().is_empty()
            || self.parent.repository.name.trim().is_empty()
        {
            return Some("parent repository owner and name must both be non-empty".to_string());
        }
        if self.parent.number == 0 {
            return Some("parent pull request number must be non-zero".to_string());
        }
        if self.parent.head_ref.trim().is_empty() {
            return Some("parent head_ref must be non-empty".to_string());
        }
        if self.remote.trim().is_empty() {
            return Some("remote must be non-empty".to_string());
        }
        if !is_full_object_id(&self.parent.reviewed_head_sha) {
            return Some(format!(
                "parent reviewed_head_sha {:?} is not a full 40-character object id",
                self.parent.reviewed_head_sha
            ));
        }
        if let Some(current) = self.branch.current_sha.as_deref()
            && !is_full_object_id(current)
        {
            return Some(format!(
                "branch current_sha {current:?} is not a full 40-character object id"
            ));
        }
        for pull_request in &self.graph.pull_requests {
            if pull_request.number == 0 {
                return Some("an observed pull request has number 0".to_string());
            }
            if pull_request.base_ref.trim().is_empty() {
                return Some(format!(
                    "observed pull request #{} has an empty base_ref",
                    pull_request.number
                ));
            }
            if pull_request.repository.owner.trim().is_empty()
                || pull_request.repository.name.trim().is_empty()
            {
                return Some(format!(
                    "observed pull request #{} has an incomplete repository identity",
                    pull_request.number
                ));
            }
        }
        None
    }
}

/// The five admission outcomes named by #12885.
///
/// Exactly one is returned per evaluation. Only [`Self::SafeToDelete`] admits
/// deletion; every other variant retains the branch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DeletionAdmission {
    SafeToDelete,
    RetainOpenChildren,
    RetainBranchMoved,
    RetainGraphNotProven,
    RetainParentNotTerminal,
}

impl DeletionAdmission {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SafeToDelete => "SAFE_TO_DELETE",
            Self::RetainOpenChildren => "RETAIN_OPEN_CHILDREN",
            Self::RetainBranchMoved => "RETAIN_BRANCH_MOVED",
            Self::RetainGraphNotProven => "RETAIN_GRAPH_NOT_PROVEN",
            Self::RetainParentNotTerminal => "RETAIN_PARENT_NOT_TERMINAL",
        }
    }

    /// Whether this outcome permits the branch to be deleted.
    pub const fn admits_deletion(self) -> bool {
        matches!(self, Self::SafeToDelete)
    }
}

/// Proposed next owner for a retained child.
///
/// This is a *proposal for* #6188/#11773, not a decision. In particular
/// [`Self::CloseOrSupersede`] is never proposed automatically: closing or
/// superseding a child is a judgment this check has no evidence for, and
/// #12885 forbids automatic child closure outright.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum NextOwner {
    Retarget,
    Reconstruct,
    CloseOrSupersede,
    Hold,
}

impl NextOwner {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Retarget => "RETARGET",
            Self::Reconstruct => "RECONSTRUCT",
            Self::CloseOrSupersede => "CLOSE_OR_SUPERSEDE",
            Self::Hold => "HOLD",
        }
    }
}

/// A child that keeps the parent branch alive, with enough identity for the
/// reconciliation authority to act without re-querying.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetainedChild {
    pub number: u64,
    pub head_ref: String,
    pub base_ref: String,
    pub draft: bool,
    pub state: PullRequestState,
    pub mergeable: Mergeability,
    /// Whether the parent merge is known to have changed this child's
    /// mergeability. `None` means the graph did not say — not "no".
    pub mergeability_changed_by_parent_merge: Option<bool>,
    pub next_owner: NextOwner,
}

/// The typed result of one admission evaluation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdmissionOutcome {
    pub schema_version: String,
    pub policy_version: String,
    pub repository: String,
    pub parent_number: u64,
    pub branch: String,
    pub admission: DeletionAdmission,
    /// Why this outcome was reached, in terms a reconciler can act on.
    pub detail: String,
    pub retained_children: Vec<RetainedChild>,
    /// Git remote the deletion would target, echoed from the request so the
    /// emitted command and its verification name the same remote.
    pub remote: String,
    /// The exact push URL the deletion is executed against, carried from the
    /// request so the mutation never re-resolves mutable remote config.
    #[serde(default)]
    pub push_endpoint: Option<String>,
    /// The branch tip this admission was granted against, present only for
    /// `SAFE_TO_DELETE`.
    ///
    /// Deletion is executed under a lease on this exact value, so a writer
    /// advancing the branch between evaluation and execution makes the
    /// deletion fail rather than destroy the new tip. Without it there is no
    /// admitted subject to lease against and no command is produced.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub admitted_sha: Option<String>,
}
