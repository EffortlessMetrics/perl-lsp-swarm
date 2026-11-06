use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use crate::{
    DurabilityDisposition, MAX_MANIFEST_BYTES, MAX_SHARD_BYTES, MAX_SHARDS_PER_MANIFEST,
    ManifestIdentity, STORE_SCHEMA_VERSION, SnapshotCommit, SnapshotCommitReceipt,
    SnapshotInspectQuery, SnapshotInspectResult, SnapshotPrunePolicy, SnapshotPruneReceipt,
    SnapshotQuarantineReason, SnapshotStore, SnapshotStoreError, StoredManifest, StoredShardKey,
    WorkspaceCacheKey, digest::sha256_hex, encoding,
};

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Deterministic directory-backed reference implementation (#4911).
#[derive(Debug, Clone)]
pub struct DirectorySnapshotStore {
    root: PathBuf,
}

impl DirectorySnapshotStore {
    /// Open or create a store at `root`. The path is canonicalized and confined.
    pub fn open(root: impl AsRef<Path>) -> Result<Self, SnapshotStoreError> {
        let root = encoding::canonicalize_cache_root(root.as_ref())?;
        let store = Self { root };
        store.ensure_layout()?;
        Ok(store)
    }

    fn ensure_layout(&self) -> Result<(), SnapshotStoreError> {
        for dir in ["objects", "manifests", "current", "quarantine", "metadata"] {
            let path = self.root.join(dir);
            fs::create_dir_all(&path).map_err(SnapshotStoreError::io)?;
        }
        let schema_path = self.root.join("metadata/store-schema");
        if !schema_path.exists() {
            atomic_write(&schema_path, STORE_SCHEMA_VERSION.to_string().as_bytes())?;
        }
        Ok(())
    }

    fn objects_dir(&self) -> PathBuf {
        self.root.join("objects")
    }

    fn manifests_dir(&self) -> PathBuf {
        self.root.join("manifests")
    }

    fn current_dir(&self) -> PathBuf {
        self.root.join("current")
    }

    fn object_path(&self, fingerprint: &str) -> Result<PathBuf, SnapshotStoreError> {
        let name = encoding::encode_object_name(fingerprint)?;
        let path = self.objects_dir().join(name);
        encoding::ensure_within_root(&self.root, &path)?;
        Ok(path)
    }

    fn manifest_path(&self, identity: &ManifestIdentity) -> Result<PathBuf, SnapshotStoreError> {
        let name = encoding::encode_object_name(identity.as_str())?;
        let path = self.manifests_dir().join(name);
        encoding::ensure_within_root(&self.root, &path)?;
        Ok(path)
    }

    fn current_path(&self, workspace: &WorkspaceCacheKey) -> Result<PathBuf, SnapshotStoreError> {
        let name = encoding::encode_workspace_key(workspace)?;
        let path = self.current_dir().join(name);
        encoding::ensure_within_root(&self.root, &path)?;
        Ok(path)
    }

    fn read_current_identity(
        &self,
        workspace: &WorkspaceCacheKey,
    ) -> Result<Option<ManifestIdentity>, SnapshotStoreError> {
        let path = self.current_path(workspace)?;
        if !path.is_file() {
            return Ok(None);
        }
        let mut bytes = Vec::new();
        File::open(&path)
            .map_err(SnapshotStoreError::io)?
            .read_to_end(&mut bytes)
            .map_err(SnapshotStoreError::io)?;
        let text = String::from_utf8(bytes)
            .map_err(|_error| SnapshotStoreError::corrupt("current pointer not utf8"))?;
        let trimmed = text.trim();
        if trimmed.len() != 64 {
            return Err(SnapshotStoreError::corrupt("current pointer length"));
        }
        Ok(Some(ManifestIdentity(trimmed.to_ascii_lowercase())))
    }

    fn load_manifest_by_identity(
        &self,
        identity: &ManifestIdentity,
    ) -> Result<StoredManifest, SnapshotStoreError> {
        let path = self.manifest_path(identity)?;
        let encoded = read_file_bounded(&path, MAX_MANIFEST_BYTES)?;
        let computed = ManifestIdentity(sha256_hex(&encoded));
        if computed != *identity {
            return Err(SnapshotStoreError::corrupt("manifest identity mismatch"));
        }
        let shard_closure = parse_manifest_closure(&encoded)?;
        Ok(StoredManifest { identity: identity.clone(), encoded, shard_closure })
    }

    fn validate_commit(&self, commit: &SnapshotCommit) -> Result<(), SnapshotStoreError> {
        if commit.shards.len() > MAX_SHARDS_PER_MANIFEST {
            return Err(SnapshotStoreError::limit("too many shards in commit"));
        }
        if commit.manifest.encoded.len() > MAX_MANIFEST_BYTES {
            return Err(SnapshotStoreError::limit("manifest too large"));
        }
        let identity = ManifestIdentity(sha256_hex(&commit.manifest.encoded));
        if identity != commit.manifest.identity {
            return Err(SnapshotStoreError::corrupt(
                "manifest identity does not match encoded body",
            ));
        }
        let parsed_closure = parse_manifest_closure(&commit.manifest.encoded)?;
        if parsed_closure != commit.manifest.shard_closure {
            return Err(SnapshotStoreError::corrupt("manifest closure mismatch"));
        }
        for key in &commit.manifest.shard_closure {
            encoding::validate_key_component("artifact_family", &key.artifact_family)?;
            encoding::encode_object_name(&key.fingerprint)?;
        }
        for (key, bytes) in &commit.shards {
            if bytes.len() > MAX_SHARD_BYTES {
                return Err(SnapshotStoreError::limit("shard too large"));
            }
            let digest = sha256_hex(bytes);
            if digest != key.fingerprint {
                return Err(SnapshotStoreError::corrupt("shard fingerprint mismatch"));
            }
            if !commit.manifest.shard_closure.contains(key) {
                return Err(SnapshotStoreError::corrupt("commit shard not listed in manifest"));
            }
        }
        for key in &commit.manifest.shard_closure {
            if !commit.shards.contains_key(key) {
                return Err(SnapshotStoreError::corrupt(
                    "manifest references missing shard payload",
                ));
            }
        }
        Ok(())
    }
}

impl SnapshotStore for DirectorySnapshotStore {
    fn load_current_manifest(
        &self,
        key: &WorkspaceCacheKey,
    ) -> Result<Option<StoredManifest>, SnapshotStoreError> {
        let Some(identity) = self.read_current_identity(key)? else {
            return Ok(None);
        };
        self.load_manifest_by_identity(&identity).map(Some)
    }

    fn load_shard(&self, key: &StoredShardKey) -> Result<Option<Vec<u8>>, SnapshotStoreError> {
        let path = self.object_path(&key.fingerprint)?;
        if !path.is_file() {
            return Ok(None);
        }
        let bytes = read_file_bounded(&path, MAX_SHARD_BYTES)?;
        let digest = sha256_hex(&bytes);
        if digest != key.fingerprint {
            return Err(SnapshotStoreError::corrupt("shard digest mismatch"));
        }
        Ok(Some(bytes))
    }

    fn commit_generation(
        &self,
        commit: SnapshotCommit,
    ) -> Result<SnapshotCommitReceipt, SnapshotStoreError> {
        self.validate_commit(&commit)?;
        let lock_path = self.root.join(".commit.lock");
        let lock =
            OpenOptions::new().create_new(true).write(true).open(&lock_path).map_err(|error| {
                if error.kind() == std::io::ErrorKind::AlreadyExists {
                    SnapshotStoreError::Busy
                } else {
                    SnapshotStoreError::io(error)
                }
            })?;

        let result = self.commit_generation_locked(commit, lock);
        let _ = fs::remove_file(lock_path);
        result
    }

    fn inspect(
        &self,
        query: SnapshotInspectQuery,
    ) -> Result<SnapshotInspectResult, SnapshotStoreError> {
        let mut current_manifests = BTreeMap::new();
        if let Some(workspace) = query.workspace {
            let key = encoding::encode_workspace_key(&workspace)?;
            if let Some(identity) = self.read_current_identity(&workspace)? {
                current_manifests.insert(key, identity);
            }
        } else {
            for entry in fs::read_dir(self.current_dir()).map_err(SnapshotStoreError::io)? {
                let entry = entry.map_err(SnapshotStoreError::io)?;
                let file_name = entry.file_name();
                let Some(name) = file_name.to_str() else {
                    continue;
                };
                let bytes = fs::read(entry.path()).map_err(SnapshotStoreError::io)?;
                let text = String::from_utf8(bytes)
                    .map_err(|_error| SnapshotStoreError::corrupt("current pointer not utf8"))?;
                current_manifests
                    .insert(name.to_string(), ManifestIdentity(text.trim().to_string()));
            }
        }

        let object_count = fs::read_dir(self.objects_dir())
            .map_err(SnapshotStoreError::io)?
            .filter_map(Result::ok)
            .filter(|entry| entry.file_type().map(|t| t.is_file()).unwrap_or(false))
            .count() as u64;
        let manifest_count = fs::read_dir(self.manifests_dir())
            .map_err(SnapshotStoreError::io)?
            .filter_map(Result::ok)
            .filter(|entry| entry.file_type().map(|t| t.is_file()).unwrap_or(false))
            .count() as u64;

        Ok(SnapshotInspectResult {
            store_schema_version: STORE_SCHEMA_VERSION,
            current_manifests,
            object_count,
            manifest_count,
        })
    }

    fn quarantine(&self, reason: SnapshotQuarantineReason) -> Result<(), SnapshotStoreError> {
        encoding::validate_key_component("quarantine code", &reason.code)?;
        let id = format!("{}_{}", reason.code, TEMP_COUNTER.fetch_add(1, Ordering::Relaxed));
        let dest = self.root.join("quarantine").join(id);
        fs::create_dir_all(&dest).map_err(SnapshotStoreError::io)?;
        atomic_write(&dest.join("reason.txt"), reason.detail.as_bytes())?;
        Ok(())
    }

    fn prune(
        &self,
        policy: SnapshotPrunePolicy,
    ) -> Result<SnapshotPruneReceipt, SnapshotStoreError> {
        let referenced_manifests = current_manifest_identities(self)?;
        let mut referenced_objects = BTreeSet::new();
        for identity in &referenced_manifests {
            let manifest = self.load_manifest_by_identity(identity)?;
            for key in manifest.shard_closure {
                referenced_objects.insert(key.fingerprint);
            }
        }

        let mut manifests_removed = 0u64;
        let mut objects_removed = 0u64;

        if policy.remove_unreferenced_manifests {
            for entry in fs::read_dir(self.manifests_dir()).map_err(SnapshotStoreError::io)? {
                let entry = entry.map_err(SnapshotStoreError::io)?;
                if !entry.file_type().map_err(SnapshotStoreError::io)?.is_file() {
                    continue;
                }
                let name = entry.file_name();
                let Some(name) = name.to_str() else {
                    continue;
                };
                if !referenced_manifests.iter().any(|id| id.as_str() == name) {
                    fs::remove_file(entry.path()).map_err(SnapshotStoreError::io)?;
                    manifests_removed += 1;
                }
            }
        }

        if policy.remove_unreferenced_objects {
            for entry in fs::read_dir(self.objects_dir()).map_err(SnapshotStoreError::io)? {
                let entry = entry.map_err(SnapshotStoreError::io)?;
                if !entry.file_type().map_err(SnapshotStoreError::io)?.is_file() {
                    continue;
                }
                let name = entry.file_name();
                let Some(name) = name.to_str() else {
                    continue;
                };
                if !referenced_objects.contains(name) {
                    fs::remove_file(entry.path()).map_err(SnapshotStoreError::io)?;
                    objects_removed += 1;
                }
            }
        }

        Ok(SnapshotPruneReceipt { manifests_removed, objects_removed })
    }
}

impl DirectorySnapshotStore {
    fn commit_generation_locked(
        &self,
        commit: SnapshotCommit,
        _lock: File,
    ) -> Result<SnapshotCommitReceipt, SnapshotStoreError> {
        let current = self.read_current_identity(&commit.workspace)?;
        match (&commit.expected_current, &current) {
            (None, None) => {}
            (Some(expected), Some(actual)) if expected == actual => {}
            (Some(_), None) => {
                return Err(SnapshotStoreError::incompatible(
                    "expected existing current generation",
                ));
            }
            (None, Some(_)) => {
                return Err(SnapshotStoreError::incompatible("expected empty current generation"));
            }
            (Some(expected), Some(actual)) if expected != actual => {
                return Err(SnapshotStoreError::incompatible("current generation changed"));
            }
            _ => {}
        }

        let mut objects_written = 0u64;
        let mut bytes_written = 0u64;

        for (key, bytes) in &commit.shards {
            let path = self.object_path(&key.fingerprint)?;
            if path.is_file() {
                let existing = read_file_bounded(&path, MAX_SHARD_BYTES)?;
                if existing != *bytes {
                    return Err(SnapshotStoreError::incompatible("object fingerprint collision"));
                }
                continue;
            }
            atomic_write(&path, bytes)?;
            objects_written += 1;
            bytes_written += bytes.len() as u64;
        }

        let manifest_path = self.manifest_path(&commit.manifest.identity)?;
        if !manifest_path.is_file() {
            atomic_write(&manifest_path, &commit.manifest.encoded)?;
            objects_written += 1;
            bytes_written += commit.manifest.encoded.len() as u64;
        }

        let current_path = self.current_path(&commit.workspace)?;
        atomic_write(&current_path, commit.manifest.identity.as_str().as_bytes())?;

        Ok(SnapshotCommitReceipt {
            manifest_identity: commit.manifest.identity,
            objects_written,
            bytes_written,
            durability: DurabilityDisposition::FsyncBestEffort,
        })
    }
}

fn current_manifest_identities(
    store: &DirectorySnapshotStore,
) -> Result<BTreeSet<ManifestIdentity>, SnapshotStoreError> {
    let mut out = BTreeSet::new();
    for entry in fs::read_dir(store.current_dir()).map_err(SnapshotStoreError::io)? {
        let entry = entry.map_err(SnapshotStoreError::io)?;
        if !entry.file_type().map_err(SnapshotStoreError::io)?.is_file() {
            continue;
        }
        let bytes = fs::read(entry.path()).map_err(SnapshotStoreError::io)?;
        let text = String::from_utf8(bytes)
            .map_err(|_error| SnapshotStoreError::corrupt("current pointer not utf8"))?;
        out.insert(ManifestIdentity(text.trim().to_string()));
    }
    Ok(out)
}

fn read_file_bounded(path: &Path, max: usize) -> Result<Vec<u8>, SnapshotStoreError> {
    let metadata = fs::metadata(path).map_err(SnapshotStoreError::io)?;
    let len = metadata.len() as usize;
    if len > max {
        return Err(SnapshotStoreError::limit("file exceeds bound"));
    }
    let mut bytes = Vec::with_capacity(len);
    File::open(path)
        .map_err(SnapshotStoreError::io)?
        .read_to_end(&mut bytes)
        .map_err(SnapshotStoreError::io)?;
    Ok(bytes)
}

fn atomic_write(path: &Path, contents: &[u8]) -> Result<(), SnapshotStoreError> {
    let parent =
        path.parent().ok_or_else(|| SnapshotStoreError::corrupt("missing parent directory"))?;
    fs::create_dir_all(parent).map_err(SnapshotStoreError::io)?;
    let id = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let temp = parent.join(format!(".tmp-{id}"));
    {
        let mut file = File::create(&temp).map_err(SnapshotStoreError::io)?;
        file.write_all(contents).map_err(SnapshotStoreError::io)?;
        file.sync_all().map_err(SnapshotStoreError::io)?;
    }
    fs::rename(&temp, path).map_err(SnapshotStoreError::io)?;
    if let Ok(dir) = File::open(parent) {
        let _ = dir.sync_all();
    }
    Ok(())
}

/// Parse deterministic manifest closure lines: `family fingerprint length`.
fn parse_manifest_closure(encoded: &[u8]) -> Result<Vec<StoredShardKey>, SnapshotStoreError> {
    let text = std::str::from_utf8(encoded)
        .map_err(|_error| SnapshotStoreError::corrupt("manifest not utf8"))?;
    let mut out = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let mut parts = trimmed.split_whitespace();
        let Some(family) = parts.next() else {
            continue;
        };
        let Some(fingerprint) = parts.next() else {
            return Err(SnapshotStoreError::corrupt("manifest line missing fingerprint"));
        };
        let Some(length) = parts.next() else {
            return Err(SnapshotStoreError::corrupt("manifest line missing length"));
        };
        let length: usize = length
            .parse()
            .map_err(|_error| SnapshotStoreError::corrupt("manifest length not integer"))?;
        if length > MAX_SHARD_BYTES {
            return Err(SnapshotStoreError::limit("manifest shard length too large"));
        }
        let key = StoredShardKey::new(family, fingerprint);
        encoding::encode_object_name(&key.fingerprint)?;
        out.push(key);
    }
    if out.len() > MAX_SHARDS_PER_MANIFEST {
        return Err(SnapshotStoreError::limit("too many manifest shards"));
    }
    Ok(out)
}

/// Build canonical manifest bytes from shard closure entries `(family, bytes)`.
pub fn encode_manifest_body(shards: &[(String, Vec<u8>)]) -> Result<Vec<u8>, SnapshotStoreError> {
    let mut lines = Vec::new();
    for (family, bytes) in shards {
        encoding::validate_key_component("artifact_family", family)?;
        if bytes.len() > MAX_SHARD_BYTES {
            return Err(SnapshotStoreError::limit("shard too large"));
        }
        let fingerprint = sha256_hex(bytes);
        lines.push(format!("{family} {fingerprint} {}", bytes.len()));
    }
    lines.sort();
    Ok(lines.join("\n").into_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{SnapshotCommit, StoredManifest, WorkspaceCacheKey};
    use std::collections::BTreeMap;

    #[test]
    fn commit_and_load_generation() -> Result<(), SnapshotStoreError> {
        let temp = tempfile::tempdir().map_err(SnapshotStoreError::io)?;
        let store = DirectorySnapshotStore::open(temp.path())?;
        let shard_bytes = b"payload".to_vec();
        let family = "project_fact_shard".to_string();
        let encoded = encode_manifest_body(&[(family.clone(), shard_bytes.clone())])?;
        let identity = ManifestIdentity(sha256_hex(&encoded));
        let fingerprint = sha256_hex(&shard_bytes);
        let key = StoredShardKey::new(family, fingerprint);
        let mut shards = BTreeMap::new();
        shards.insert(key.clone(), shard_bytes);
        let manifest = StoredManifest {
            identity: identity.clone(),
            encoded,
            shard_closure: vec![key.clone()],
        };
        let workspace = WorkspaceCacheKey::new("ws-alpha", "profile-1");
        store.commit_generation(SnapshotCommit {
            workspace: workspace.clone(),
            shards,
            manifest,
            expected_current: None,
        })?;
        let loaded = store.load_current_manifest(&workspace)?.ok_or(SnapshotStoreError::Miss)?;
        assert_eq!(loaded.identity, identity);
        let bytes = store.load_shard(&key)?.ok_or(SnapshotStoreError::Miss)?;
        assert_eq!(bytes, b"payload");
        Ok(())
    }
}
