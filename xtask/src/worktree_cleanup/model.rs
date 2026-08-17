use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub const WORKTREE_CLEANUP_SCHEMA_VERSION: &str = "worktree_cleanup_plan.v1";
pub const WORKTREE_CLEANUP_POLICY_VERSION: &str = "2026-08-16";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ObservationState {
    Observed,
    NotApplicable,
    NotProven,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Observation<T> {
    pub state: ObservationState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

impl<T> Observation<T> {
    pub fn observed(value: T) -> Self {
        Self { state: ObservationState::Observed, value: Some(value), detail: None }
    }

    pub fn not_applicable(detail: impl Into<String>) -> Self {
        Self { state: ObservationState::NotApplicable, value: None, detail: Some(detail.into()) }
    }

    pub fn not_proven(detail: impl Into<String>) -> Self {
        Self { state: ObservationState::NotProven, value: None, detail: Some(detail.into()) }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PrMatch {
    None,
    Match {
        number: u64,
        #[serde(skip_serializing_if = "Option::is_none")]
        head_oid: Option<String>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum WorktreeClassification {
    Keep,
    CacheOnly,
    Salvage,
    Review,
    NotProven,
}

impl WorktreeClassification {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Keep => "KEEP",
            Self::CacheOnly => "CACHE_ONLY",
            Self::Salvage => "SALVAGE",
            Self::Review => "REVIEW",
            Self::NotProven => "NOT_PROVEN",
        }
    }

    pub(crate) const fn precedence(self) -> u8 {
        match self {
            Self::Keep => 0,
            Self::CacheOnly => 1,
            Self::Review => 2,
            Self::Salvage => 3,
            Self::NotProven => 4,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum WorktreeActionKind {
    RemoveRegisteredWorktree,
    PruneAdministrativeRecord,
}

impl WorktreeActionKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::RemoveRegisteredWorktree => "REMOVE_REGISTERED_WORKTREE",
            Self::PruneAdministrativeRecord => "PRUNE_ADMINISTRATIVE_RECORD",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProposedAction {
    pub kind: WorktreeActionKind,
    pub target: PathBuf,
    pub targetable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorktreeFacts {
    pub path_exists: Observation<bool>,
    pub administrative_path: Observation<PathBuf>,
    pub locked: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lock_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prunable_reason: Option<String>,
    pub dirty: Observation<bool>,
    pub untracked: Observation<bool>,
    pub open_pr: Observation<PrMatch>,
    pub merged_pr: Observation<PrMatch>,
    pub unpushed_commits: Observation<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unpushed_comparison_ref: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unpushed_ahead_count: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorktreePlanEntry {
    pub entry_id: String,
    pub path: PathBuf,
    pub managed: bool,
    pub primary: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub head: Option<String>,
    pub facts: WorktreeFacts,
    pub classification: WorktreeClassification,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proposed_action: Option<ProposedAction>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reason_tokens: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub required_preconditions: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepositorySubject {
    pub requested_root: PathBuf,
    pub repository_root: PathBuf,
    pub common_dir: PathBuf,
    pub source_head: Observation<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct PlanSummary {
    pub keep: u64,
    pub cache_only: u64,
    pub salvage: u64,
    pub review: u64,
    pub not_proven: u64,
    pub targetable_actions: u64,
}

impl PlanSummary {
    pub(crate) fn from_entries(entries: &[WorktreePlanEntry]) -> Self {
        let mut summary = Self::default();
        for entry in entries {
            match entry.classification {
                WorktreeClassification::Keep => summary.keep += 1,
                WorktreeClassification::CacheOnly => summary.cache_only += 1,
                WorktreeClassification::Salvage => summary.salvage += 1,
                WorktreeClassification::Review => summary.review += 1,
                WorktreeClassification::NotProven => summary.not_proven += 1,
            }
            if entry.proposed_action.as_ref().is_some_and(|action| action.targetable) {
                summary.targetable_actions += 1;
            }
        }
        summary
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorktreeCleanupPlan {
    pub schema_version: String,
    pub policy_version: String,
    pub observed_at: String,
    pub subject: RepositorySubject,
    pub entries: Vec<WorktreePlanEntry>,
    pub summary: PlanSummary,
    pub aggregate_classification: WorktreeClassification,
    pub plan_digest: String,
}

impl WorktreeCleanupPlan {
    pub(crate) fn aggregate(entries: &[WorktreePlanEntry]) -> WorktreeClassification {
        entries
            .iter()
            .map(|entry| entry.classification)
            .max_by_key(|classification| classification.precedence())
            .unwrap_or(WorktreeClassification::Keep)
    }
}
