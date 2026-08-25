//! Exact host-work subject and provider identity vocabulary.
//!
//! One repository, worktree, operation, process tree, or storage root cannot
//! satisfy another merely because paths or command names resemble each other:
//! every observation row carries the canonical [`HostWorkSubject::subject_key`]
//! of the subject it describes, and observation sets reject rows bound to any
//! other key.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub const HOST_WORK_STATUS_SCHEMA_VERSION: &str = "host_work_status.v1";

/// What kind of host-work resource this subject identifies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ObservationScope {
    Repository,
    Worktree,
    Operation,
    ProcessTree,
    StorageRoot,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorktreeIdentity {
    pub path: PathBuf,
    pub branch: Option<String>,
}

/// The exact subject a set of host-work observations describes. Identity
/// fields are load-bearing: classification refuses to aggregate rows whose
/// `subject_key` differs from the set's key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostWorkSubject {
    pub repository_root: PathBuf,
    pub common_dir: PathBuf,
    pub canonical_remote: Option<String>,
    pub host_profile: String,
    pub scope: ObservationScope,
    pub worktree: Option<WorktreeIdentity>,
    pub candidate_id: Option<String>,
    pub executor_operation_id: Option<String>,
    pub allocation_id: Option<String>,
    pub reservation_id: Option<String>,
    pub process_group_id: Option<String>,
    pub storage_root: Option<PathBuf>,
}

impl HostWorkSubject {
    /// Canonical, deterministic identity string for this subject. Two
    /// subjects with equal keys are the same resource; different keys are
    /// never substituted for one another regardless of path or name
    /// resemblance.
    ///
    /// Each field is encoded length-delimited, so the encoding is injective:
    /// field values containing the separator (or digit/colon sequences)
    /// cannot shift field boundaries between adjacent positions to make two
    /// distinct subjects collide on one key.
    pub fn subject_key(&self) -> String {
        let worktree_path =
            self.worktree.as_ref().map(|w| w.path.display().to_string()).unwrap_or_default();
        let worktree_branch =
            self.worktree.as_ref().and_then(|w| w.branch.clone()).unwrap_or_default();
        let storage_root =
            self.storage_root.as_ref().map(|p| p.display().to_string()).unwrap_or_default();
        let fields = [
            self.scope_as_key_token(),
            self.repository_root.display().to_string(),
            self.common_dir.display().to_string(),
            self.canonical_remote.clone().unwrap_or_default(),
            worktree_path,
            worktree_branch,
            self.candidate_id.clone().unwrap_or_default(),
            self.executor_operation_id.clone().unwrap_or_default(),
            self.allocation_id.clone().unwrap_or_default(),
            self.reservation_id.clone().unwrap_or_default(),
            self.process_group_id.clone().unwrap_or_default(),
            storage_root,
        ];
        let mut key = String::new();
        for field in &fields {
            key.push_str(&field.len().to_string());
            key.push(':');
            key.push_str(field);
            key.push('\u{1f}');
        }
        key.pop();
        key
    }

    fn scope_as_key_token(&self) -> String {
        match self.scope {
            ObservationScope::Repository => "REPOSITORY".to_string(),
            ObservationScope::Worktree => "WORKTREE".to_string(),
            ObservationScope::Operation => "OPERATION".to_string(),
            ObservationScope::ProcessTree => "PROCESS_TREE".to_string(),
            ObservationScope::StorageRoot => "STORAGE_ROOT".to_string(),
        }
    }
}

/// Which landed or planned provider family produced an observation. Each
/// family has exactly one current typed owner; unknown families stay visible
/// as [`crate::host_work_status::UnknownProviderRecord`]s instead of being
/// dropped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ProviderFamily {
    WorktreePlan,
    WriterAdmission,
    ExecutorStateAllocation,
    CapacityReservation,
    ProcessObservation,
    GitGithubLogical,
    FilesystemStorage,
}

impl ProviderFamily {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::WorktreePlan => "WORKTREE_PLAN",
            Self::WriterAdmission => "WRITER_ADMISSION",
            Self::ExecutorStateAllocation => "EXECUTOR_STATE_ALLOCATION",
            Self::CapacityReservation => "CAPACITY_RESERVATION",
            Self::ProcessObservation => "PROCESS_OBSERVATION",
            Self::GitGithubLogical => "GIT_GITHUB_LOGICAL",
            Self::FilesystemStorage => "FILESYSTEM_STORAGE",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ProviderId {
    pub family: ProviderFamily,
    /// The provider-owned schema/version identifier, carried verbatim.
    pub schema_version: String,
    /// The provider instance/source label (e.g. tool name), carried verbatim.
    pub source: String,
}
