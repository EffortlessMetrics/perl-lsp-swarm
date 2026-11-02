#![warn(missing_docs)]
#![deny(clippy::map_err_ignore)]

//! Storage-neutral snapshot persistence (#4911).
//!
//! This crate defines the [`SnapshotStore`] port and a deterministic directory
//! reference backend. It stores opaque versioned manifests and shard payloads;
//! it does not decide semantic compatibility, freshness, or edit authorization.

mod digest;
mod directory;
mod encoding;
mod error;
mod port;
mod types;

pub use digest::sha256_hex;
pub use directory::{DirectorySnapshotStore, encode_manifest_body};
pub use error::SnapshotStoreError;
pub use port::SnapshotStore;
pub use types::{
    DurabilityDisposition, ManifestIdentity, SnapshotCommit, SnapshotCommitReceipt,
    SnapshotInspectQuery, SnapshotInspectResult, SnapshotPrunePolicy, SnapshotPruneReceipt,
    SnapshotQuarantineReason, StoredManifest, StoredShardKey, WorkspaceCacheKey,
};

/// Store on-disk schema generation written under `metadata/store-schema`.
pub const STORE_SCHEMA_VERSION: u32 = 1;

/// Maximum shard payload bytes accepted by the reference backend.
pub const MAX_SHARD_BYTES: usize = 64 * 1024 * 1024;

/// Maximum manifest body bytes accepted by the reference backend.
pub const MAX_MANIFEST_BYTES: usize = 1024 * 1024;

/// Maximum shard references recorded in one manifest.
pub const MAX_SHARDS_PER_MANIFEST: usize = 10_000;

pub mod backend_differential;
