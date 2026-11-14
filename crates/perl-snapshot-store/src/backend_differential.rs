//! Backend-neutral differential fixtures reusable by #4910 / #7889 / #7891.

use std::{collections::BTreeMap, path::Path};

use crate::{
    ManifestIdentity, SnapshotCommit, SnapshotInspectQuery, SnapshotPrunePolicy,
    SnapshotQuarantineReason, SnapshotStore, SnapshotStoreError, StoredManifest, StoredShardKey,
    WorkspaceCacheKey,
    directory::{DirectorySnapshotStore, encode_manifest_body},
    sha256_hex,
};

/// Run the shared contract against one store factory.
pub fn run_backend_differential<F>(mut open_store: F) -> Result<(), SnapshotStoreError>
where
    F: FnMut() -> Result<Box<dyn SnapshotStore>, SnapshotStoreError>,
{
    empty_store_and_miss(&mut open_store)?;
    commit_load_one_generation(&mut open_store)?;
    object_deduplication(&mut open_store)?;
    generation_publication(&mut open_store)?;
    old_generation_remains_readable(&mut open_store)?;
    namespace_isolation(&mut open_store)?;
    deterministic_inspect(&mut open_store)?;
    quarantine_and_prune(&mut open_store)?;
    Ok(())
}

fn empty_store_and_miss<F>(open_store: &mut F) -> Result<(), SnapshotStoreError>
where
    F: FnMut() -> Result<Box<dyn SnapshotStore>, SnapshotStoreError>,
{
    let store = open_store()?;
    let key = WorkspaceCacheKey::new("ws-empty", "p0");
    assert!(store.load_current_manifest(&key)?.is_none());
    let shard_key = StoredShardKey::new("family", sha256_hex(b"x"));
    assert!(store.load_shard(&shard_key)?.is_none());
    Ok(())
}

fn commit_load_one_generation<F>(open_store: &mut F) -> Result<(), SnapshotStoreError>
where
    F: FnMut() -> Result<Box<dyn SnapshotStore>, SnapshotStoreError>,
{
    let store = open_store()?;
    let workspace = WorkspaceCacheKey::new("ws-one", "p1");
    publish(store.as_ref(), workspace, b"alpha", None)?;
    Ok(())
}

fn object_deduplication<F>(open_store: &mut F) -> Result<(), SnapshotStoreError>
where
    F: FnMut() -> Result<Box<dyn SnapshotStore>, SnapshotStoreError>,
{
    let store = open_store()?;
    let workspace = WorkspaceCacheKey::new("ws-dedup", "p1");
    let receipt_a = publish(store.as_ref(), workspace.clone(), b"shared", None)?;
    let current = store.load_current_manifest(&workspace)?.ok_or(SnapshotStoreError::Miss)?;
    let receipt_b = publish(store.as_ref(), workspace, b"shared", Some(current.identity))?;
    assert_eq!(receipt_b.objects_written, 0);
    assert!(receipt_a.manifest_identity.as_str().len() == 64);
    Ok(())
}

fn generation_publication<F>(open_store: &mut F) -> Result<(), SnapshotStoreError>
where
    F: FnMut() -> Result<Box<dyn SnapshotStore>, SnapshotStoreError>,
{
    let store = open_store()?;
    let workspace = WorkspaceCacheKey::new("ws-gen", "p1");
    publish(store.as_ref(), workspace.clone(), b"A", None)?;
    let gen_a = store.load_current_manifest(&workspace)?.ok_or(SnapshotStoreError::Miss)?;
    let gen_a_id = gen_a.identity.clone();
    publish(store.as_ref(), workspace.clone(), b"B", Some(gen_a.identity))?;
    let gen_b = store.load_current_manifest(&workspace)?.ok_or(SnapshotStoreError::Miss)?;
    assert_ne!(gen_a_id, gen_b.identity);
    Ok(())
}

fn old_generation_remains_readable<F>(open_store: &mut F) -> Result<(), SnapshotStoreError>
where
    F: FnMut() -> Result<Box<dyn SnapshotStore>, SnapshotStoreError>,
{
    let store = open_store()?;
    let workspace = WorkspaceCacheKey::new("ws-old", "p1");
    publish(store.as_ref(), workspace.clone(), b"old-bytes", None)?;
    let gen_a = store.load_current_manifest(&workspace)?.ok_or(SnapshotStoreError::Miss)?;
    let shard_a = gen_a.shard_closure[0].clone();
    publish(store.as_ref(), workspace.clone(), b"new-bytes", Some(gen_a.identity))?;
    let bytes = store.load_shard(&shard_a)?.ok_or(SnapshotStoreError::Miss)?;
    assert_eq!(bytes, b"old-bytes");
    Ok(())
}

fn namespace_isolation<F>(open_store: &mut F) -> Result<(), SnapshotStoreError>
where
    F: FnMut() -> Result<Box<dyn SnapshotStore>, SnapshotStoreError>,
{
    let store = open_store()?;
    let ws_a = WorkspaceCacheKey::new("ws-a", "profile");
    let ws_b = WorkspaceCacheKey::new("ws-b", "profile");
    publish(store.as_ref(), ws_a, b"secret-a", None)?;
    assert!(store.load_current_manifest(&ws_b)?.is_none());
    Ok(())
}

fn deterministic_inspect<F>(open_store: &mut F) -> Result<(), SnapshotStoreError>
where
    F: FnMut() -> Result<Box<dyn SnapshotStore>, SnapshotStoreError>,
{
    let store = open_store()?;
    publish(store.as_ref(), WorkspaceCacheKey::new("ws-inspect", "p1"), b"x", None)?;
    let first = store.inspect(SnapshotInspectQuery { workspace: None })?;
    let second = store.inspect(SnapshotInspectQuery { workspace: None })?;
    assert_eq!(first, second);
    Ok(())
}

fn quarantine_and_prune<F>(open_store: &mut F) -> Result<(), SnapshotStoreError>
where
    F: FnMut() -> Result<Box<dyn SnapshotStore>, SnapshotStoreError>,
{
    let store = open_store()?;
    let workspace = WorkspaceCacheKey::new("ws-prune", "p1");
    publish(store.as_ref(), workspace.clone(), b"v1", None)?;
    let gen_a = store.load_current_manifest(&workspace)?.ok_or(SnapshotStoreError::Miss)?;
    publish(store.as_ref(), workspace.clone(), b"v2", Some(gen_a.identity))?;
    store.quarantine(SnapshotQuarantineReason {
        code: "test".to_string(),
        detail: "fixture".to_string(),
    })?;
    let receipt = store.prune(SnapshotPrunePolicy {
        remove_unreferenced_manifests: true,
        remove_unreferenced_objects: true,
    })?;
    assert!(receipt.manifests_removed >= 1);
    assert!(receipt.objects_removed >= 1);
    Ok(())
}

fn publish(
    store: &dyn SnapshotStore,
    workspace: WorkspaceCacheKey,
    payload: &[u8],
    expected: Option<ManifestIdentity>,
) -> Result<crate::SnapshotCommitReceipt, SnapshotStoreError> {
    let family = "project_fact_shard".to_string();
    let encoded = encode_manifest_body(&[(family.clone(), payload.to_vec())])?;
    let identity = ManifestIdentity(sha256_hex(&encoded));
    let fingerprint = sha256_hex(payload);
    let key = StoredShardKey::new(family.clone(), fingerprint.clone());
    let mut shards = BTreeMap::new();
    shards.insert(key.clone(), payload.to_vec());
    let manifest = StoredManifest { identity, encoded, shard_closure: vec![key] };
    store.commit_generation(SnapshotCommit {
        workspace,
        shards,
        manifest,
        expected_current: expected,
    })
}

/// Open a directory store rooted at `path`.
pub fn open_directory_store(path: &Path) -> Result<Box<dyn SnapshotStore>, SnapshotStoreError> {
    Ok(Box::new(DirectorySnapshotStore::open(path)?))
}
