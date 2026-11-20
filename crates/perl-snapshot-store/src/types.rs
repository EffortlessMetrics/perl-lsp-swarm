use std::collections::BTreeMap;

/// Logical workspace/profile identity for a cache namespace (#4911).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WorkspaceCacheKey {
    /// Canonical workspace identity (not a raw host path).
    pub workspace_identity: String,
    /// Profile or environment fingerprint within the workspace.
    pub profile_fingerprint: String,
}

impl WorkspaceCacheKey {
    /// Construct a key from canonical identity material supplied by upstream authority.
    #[must_use]
    pub fn new(
        workspace_identity: impl Into<String>,
        profile_fingerprint: impl Into<String>,
    ) -> Self {
        Self {
            workspace_identity: workspace_identity.into(),
            profile_fingerprint: profile_fingerprint.into(),
        }
    }
}

/// Content-addressed shard identity.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StoredShardKey {
    /// Artifact or fact family label (opaque to the store).
    pub artifact_family: String,
    /// Normalized content fingerprint (lowercase hex SHA-256 of payload bytes).
    pub fingerprint: String,
}

impl StoredShardKey {
    /// Construct a shard key from upstream-normalized material.
    #[must_use]
    pub fn new(artifact_family: impl Into<String>, fingerprint: impl Into<String>) -> Self {
        Self { artifact_family: artifact_family.into(), fingerprint: fingerprint.into() }
    }
}

/// Immutable manifest identity (content hash of canonical manifest bytes).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ManifestIdentity(pub String);

impl ManifestIdentity {
    /// Borrow the lowercase hex identity.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Stored generation manifest referencing a complete shard closure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredManifest {
    /// Manifest identity derived from canonical encoded bytes.
    pub identity: ManifestIdentity,
    /// Canonical encoded manifest body (opaque to consumers except #7452).
    pub encoded: Vec<u8>,
    /// Parsed shard closure for storage integrity checks.
    pub shard_closure: Vec<StoredShardKey>,
}

/// One atomic generation commit request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotCommit {
    /// Workspace/profile namespace for the current pointer.
    pub workspace: WorkspaceCacheKey,
    /// Immutable shard payloads keyed by normalized shard keys.
    pub shards: BTreeMap<StoredShardKey, Vec<u8>>,
    /// Manifest body and closure to publish after shards are durable.
    pub manifest: StoredManifest,
    /// Expected current manifest identity (`None` when bootstrapping an empty store).
    pub expected_current: Option<ManifestIdentity>,
}

/// Receipt for a successful commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotCommitReceipt {
    /// Published manifest identity.
    pub manifest_identity: ManifestIdentity,
    /// Number of newly written object files.
    pub objects_written: u64,
    /// Number of payload bytes newly written.
    pub bytes_written: u64,
    /// Durability disposition for the reference backend.
    pub durability: DurabilityDisposition,
}

/// Durability disposition recorded by the reference backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DurabilityDisposition {
    /// Data reached the OS page cache; directory fsync attempted where supported.
    FsyncBestEffort,
}

/// Inspect query over stored generations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotInspectQuery {
    /// Optional workspace filter; when `None`, inspect reports store-level metadata only.
    pub workspace: Option<WorkspaceCacheKey>,
}

/// Deterministic inspect output (sorted keys, no directory enumeration order).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotInspectResult {
    /// Store schema generation.
    pub store_schema_version: u32,
    /// Current manifest identity per workspace key (sorted).
    pub current_manifests: BTreeMap<String, ManifestIdentity>,
    /// Count of object files present.
    pub object_count: u64,
    /// Count of manifest files present.
    pub manifest_count: u64,
}

/// Reason code for quarantine moves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotQuarantineReason {
    /// Stable reason label (no host paths).
    pub code: String,
    /// Optional opaque detail (redacted by producers).
    pub detail: String,
}

/// Prune policy for unreferenced objects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotPrunePolicy {
    /// When true, remove manifest files not referenced by any current pointer.
    pub remove_unreferenced_manifests: bool,
    /// When true, remove object files not referenced by any retained manifest.
    pub remove_unreferenced_objects: bool,
}

/// Prune receipt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotPruneReceipt {
    /// Manifest files removed.
    pub manifests_removed: u64,
    /// Object files removed.
    pub objects_removed: u64,
}
