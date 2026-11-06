use std::path::{Component, Path};

use crate::SnapshotStoreError;

/// Reject traversal, absolute segments, and non-canonical key material.
pub(crate) fn validate_key_component(label: &str, value: &str) -> Result<(), SnapshotStoreError> {
    if value.is_empty() {
        return Err(SnapshotStoreError::corrupt(format!("empty {label}")));
    }
    if value.len() > 256 {
        return Err(SnapshotStoreError::limit(format!("{label} too long")));
    }
    if value.contains("..") || value.contains('/') || value.contains('\\') {
        return Err(SnapshotStoreError::corrupt(format!("{label} contains path separators")));
    }
    if !value.bytes().all(is_safe_key_byte) {
        return Err(SnapshotStoreError::corrupt(format!("{label} contains unsafe bytes")));
    }
    Ok(())
}

fn is_safe_key_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-' || byte == b'.'
}

/// Encode a workspace key into a single filename component.
pub(crate) fn encode_workspace_key(
    key: &crate::WorkspaceCacheKey,
) -> Result<String, SnapshotStoreError> {
    validate_key_component("workspace_identity", &key.workspace_identity)?;
    validate_key_component("profile_fingerprint", &key.profile_fingerprint)?;
    Ok(format!("{}__{}", key.workspace_identity, key.profile_fingerprint))
}

/// Encode a shard fingerprint for object filenames.
pub(crate) fn encode_object_name(fingerprint: &str) -> Result<String, SnapshotStoreError> {
    validate_key_component("fingerprint", fingerprint)?;
    if fingerprint.len() != 64 || !fingerprint.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(SnapshotStoreError::corrupt("fingerprint must be 64 lowercase hex digits"));
    }
    Ok(fingerprint.to_ascii_lowercase())
}

/// Canonicalize a cache root and ensure it does not escape via symlinks/components.
pub(crate) fn canonicalize_cache_root(
    root: &Path,
) -> Result<std::path::PathBuf, SnapshotStoreError> {
    if root.components().any(|component| matches!(component, Component::ParentDir)) {
        return Err(SnapshotStoreError::permission(
            "cache root must not contain parent-dir traversal",
        ));
    }
    std::fs::create_dir_all(root).map_err(SnapshotStoreError::io)?;
    let canonical = std::fs::canonicalize(root).map_err(SnapshotStoreError::io)?;
    Ok(canonical)
}

/// Ensure `path` remains under `root` after resolution.
pub(crate) fn ensure_within_root(root: &Path, path: &Path) -> Result<(), SnapshotStoreError> {
    let root = std::fs::canonicalize(root).map_err(SnapshotStoreError::io)?;
    let path = if path.exists() {
        std::fs::canonicalize(path).map_err(SnapshotStoreError::io)?
    } else {
        path.to_path_buf()
    };
    if !path.starts_with(&root) {
        return Err(SnapshotStoreError::permission("path escapes cache root"));
    }
    Ok(())
}
