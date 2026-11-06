use crate::{
    SnapshotCommit, SnapshotCommitReceipt, SnapshotInspectQuery, SnapshotInspectResult,
    SnapshotPrunePolicy, SnapshotPruneReceipt, SnapshotQuarantineReason, SnapshotStoreError,
    StoredManifest, StoredShardKey, WorkspaceCacheKey,
};

/// Storage-neutral snapshot persistence port (#4911).
pub trait SnapshotStore: Send + Sync {
    /// Load the current manifest for a workspace/profile key.
    fn load_current_manifest(
        &self,
        key: &WorkspaceCacheKey,
    ) -> Result<Option<StoredManifest>, SnapshotStoreError>;

    /// Load shard payload bytes for a stored key.
    fn load_shard(&self, key: &StoredShardKey) -> Result<Option<Vec<u8>>, SnapshotStoreError>;

    /// Atomically publish one complete generation.
    fn commit_generation(
        &self,
        commit: SnapshotCommit,
    ) -> Result<SnapshotCommitReceipt, SnapshotStoreError>;

    /// Deterministic, sorted inspection output.
    fn inspect(
        &self,
        query: SnapshotInspectQuery,
    ) -> Result<SnapshotInspectResult, SnapshotStoreError>;

    /// Move compromised namespace material into quarantine.
    fn quarantine(&self, reason: SnapshotQuarantineReason) -> Result<(), SnapshotStoreError>;

    /// Remove unreferenced manifests/objects according to policy.
    fn prune(
        &self,
        policy: SnapshotPrunePolicy,
    ) -> Result<SnapshotPruneReceipt, SnapshotStoreError>;
}
