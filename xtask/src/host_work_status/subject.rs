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
    /// Deterministic native-value identity string for this subject. Equal
    /// keys identify equal supplied field values, not verified resources. Keys are
    /// never substituted for one another regardless of path or name
    /// resemblance.
    ///
    /// Each field is encoded length-delimited, so the encoding is injective:
    /// field values containing the separator (or digit/colon sequences)
    /// cannot shift field boundaries between adjacent positions to make two
    /// distinct subjects collide on one key. Options retain presence, and paths
    /// retain native units with an encoding discriminator. This is not filesystem
    /// canonicalization, host verification, or a portable persistent identity.
    pub fn subject_key(&self) -> String {
        let fields = [
            self.scope_as_key_token(),
            native_path_key(&self.repository_root),
            native_path_key(&self.common_dir),
            optional_string_key(self.canonical_remote.as_deref()),
            self.host_profile.clone(),
            if self.worktree.is_some() { "some" } else { "none" }.to_string(),
            optional_path_key(self.worktree.as_ref().map(|w| w.path.as_path())),
            optional_string_key(self.worktree.as_ref().and_then(|w| w.branch.as_deref())),
            optional_string_key(self.candidate_id.as_deref()),
            optional_string_key(self.executor_operation_id.as_deref()),
            optional_string_key(self.allocation_id.as_deref()),
            optional_string_key(self.reservation_id.as_deref()),
            optional_string_key(self.process_group_id.as_deref()),
            optional_path_key(self.storage_root.as_deref()),
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

fn optional_string_key(value: Option<&str>) -> String {
    match value {
        None => "none".to_string(),
        Some(value) => format!("some:{value}"),
    }
}

fn optional_path_key(value: Option<&std::path::Path>) -> String {
    match value {
        None => "none".to_string(),
        Some(value) => format!("some:{}", native_path_key(value)),
    }
}

// Native path values, not filesystem equivalence or a portable persistence format.
// Fixed-width hex preserves every native unit without lossy Unicode conversion.
fn native_path_key(path: &std::path::Path) -> String {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        let mut encoded = String::from("windows-u16:");
        for unit in path.as_os_str().encode_wide() {
            encoded.push_str(&format!("{unit:04x}"));
        }
        encoded
    }
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        let mut encoded = String::from("unix-u8:");
        for unit in path.as_os_str().as_bytes() {
            encoded.push_str(&format!("{unit:02x}"));
        }
        encoded
    }
    #[cfg(not(any(windows, unix)))]
    {
        let mut encoded = format!("{}-os-encoded:", std::env::consts::OS);
        for unit in path.as_os_str().as_encoded_bytes() {
            encoded.push_str(&format!("{unit:02x}"));
        }
        encoded
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
