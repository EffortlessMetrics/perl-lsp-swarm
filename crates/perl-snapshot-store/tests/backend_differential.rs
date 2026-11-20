#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::{collections::BTreeMap, fs};

use perl_snapshot_store::{
    DirectorySnapshotStore, ManifestIdentity, SnapshotCommit, SnapshotStore, SnapshotStoreError,
    StoredManifest, StoredShardKey, WorkspaceCacheKey,
    backend_differential::{open_directory_store, run_backend_differential},
    encode_manifest_body, sha256_hex,
};
use std::cell::RefCell;
use tempfile::TempDir;

#[test]
fn directory_backend_matches_shared_contract() {
    let keep = RefCell::new(Vec::<TempDir>::new());
    run_backend_differential(|| {
        let temp = TempDir::new().expect("tempdir");
        let path = temp.path().to_path_buf();
        keep.borrow_mut().push(temp);
        open_directory_store(&path)
    })
    .expect("directory backend differential contract");
}

#[test]
fn corrupt_manifest_is_typed_corruption() {
    let temp = TempDir::new().expect("tempdir");
    let store = DirectorySnapshotStore::open(temp.path()).expect("open store");
    let workspace = WorkspaceCacheKey::new("ws-corrupt", "p1");
    commit_simple(&store, workspace.clone(), b"x").expect("commit");
    let loaded = store.load_current_manifest(&workspace).expect("load").expect("manifest");
    let manifest_path = temp.path().join("manifests").join(loaded.identity.as_str());
    fs::write(manifest_path, b"truncated").expect("corrupt");
    let err = store.load_current_manifest(&workspace).expect_err("corrupt manifest must fail");
    assert!(matches!(err, SnapshotStoreError::Corrupt { .. }));
}

#[test]
fn manifest_missing_shard_payload_is_rejected() {
    let temp = TempDir::new().expect("tempdir");
    let store = DirectorySnapshotStore::open(temp.path()).expect("open store");
    let workspace = WorkspaceCacheKey::new("ws-missing-shard", "p1");
    let encoded =
        encode_manifest_body(&[("family".to_string(), b"present".to_vec())]).expect("encode");
    let identity = ManifestIdentity(sha256_hex(&encoded));
    let manifest = StoredManifest {
        identity,
        encoded,
        shard_closure: vec![StoredShardKey::new("family", sha256_hex(b"absent"))],
    };
    let err = store
        .commit_generation(SnapshotCommit {
            workspace,
            shards: BTreeMap::new(),
            manifest,
            expected_current: None,
        })
        .expect_err("missing shard payload must fail");
    assert!(matches!(
        err,
        SnapshotStoreError::Corrupt { .. } | SnapshotStoreError::Incompatible { .. }
    ));
}

fn commit_simple(
    store: &dyn SnapshotStore,
    workspace: WorkspaceCacheKey,
    payload: &[u8],
) -> Result<(), SnapshotStoreError> {
    let family = "project_fact_shard".to_string();
    let encoded = encode_manifest_body(&[(family.clone(), payload.to_vec())])?;
    let identity = ManifestIdentity(sha256_hex(&encoded));
    let fingerprint = sha256_hex(payload);
    let key = StoredShardKey::new(family, fingerprint);
    let mut shards = BTreeMap::new();
    shards.insert(key.clone(), payload.to_vec());
    store.commit_generation(SnapshotCommit {
        workspace,
        shards,
        manifest: StoredManifest { identity, encoded, shard_closure: vec![key] },
        expected_current: None,
    })?;
    Ok(())
}
