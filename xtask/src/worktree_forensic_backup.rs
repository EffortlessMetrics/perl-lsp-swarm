//! Explicit-destination, content-addressed forensic backup for one linked worktree.
//!
//! This module captures surviving pointer, candidate source-like files,
//! administrative files, refs, reflogs, and common config into one operator-
//! selected directory. That destination is the only permitted write location.
//! It never restores, synthesizes `.git/worktrees/**`, reconstructs an index,
//! or applies recovery.
//!
//! Path-safety, stable-read, and directory-race behavior is owned by
//! `worktree_forensic_fs` so backup cannot diverge from the evidence observer.

use crate::worktree_forensic_fs::{
    DirectoryFingerprint, FilesystemReader, StableRead, directory_fingerprint,
    has_link_or_reparse_component, is_link_or_reparse, is_source_like_path, lexical_normalize,
    os_str_order, paths_overlap, read_bounded_entries, read_stable_file, sha256,
};
use crate::worktree_forensic_recovery::{
    CandidateIdentity, OutputFormat, RepositoryIdentity, TraversalLimits, is_in_admin_namespace,
    observe_candidate_identity, observe_repository, parse_pointer,
};
use chrono::{SecondsFormat, Utc};
use color_eyre::eyre::{Context, Result, bail, eyre};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

pub const BACKUP_SCHEMA_VERSION: &str = "worktree_forensic_backup.v1";
pub const BACKUP_POLICY_VERSION: &str = "2026-09-29";
pub const RECEIPT_FILE_NAME: &str = "receipt.json";
pub const OBJECTS_DIR_NAME: &str = "objects";

const SKIP_NONE: &[&str] = &[];
const SKIP_GIT: &[&str] = &[".git"];
const MAX_FILE_BYTES: u64 = 8 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum BackupRole {
    Pointer,
    CandidateSource,
    Admin,
    Refs,
    Reflog,
    Config,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackupEntry {
    pub role: BackupRole,
    pub logical_path: String,
    pub source_path: String,
    pub bytes: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkippedPath {
    pub path: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MissingSubject {
    pub role: BackupRole,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetentionPolicy {
    pub policy: String,
    pub auto_delete: bool,
    pub instruction: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackupReceipt {
    #[serde(deserialize_with = "deserialize_backup_schema_version")]
    pub schema_version: String,
    pub policy_version: String,
    pub created_at: String,
    pub repository: RepositoryIdentity,
    pub candidate: CandidateIdentity,
    pub destination: PathBuf,
    pub entries: Vec<BackupEntry>,
    pub skipped: Vec<SkippedPath>,
    pub missing: Vec<MissingSubject>,
    pub complete: bool,
    pub retention: RetentionPolicy,
    pub restore_instructions: Vec<String>,
    pub verification_digest: String,
}

fn deserialize_backup_schema_version<'de, D>(
    deserializer: D,
) -> std::result::Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let version = String::deserialize(deserializer)
        .map_err(|error| serde::de::Error::custom(format!("schema_version: {error}")))?;
    if version != BACKUP_SCHEMA_VERSION {
        return Err(serde::de::Error::custom(format!(
            "schema_version: expected {BACKUP_SCHEMA_VERSION}, got {version:?}"
        )));
    }
    Ok(version)
}

#[derive(Debug)]
struct CaptureState {
    entries: Vec<BackupEntry>,
    skipped: Vec<SkippedPath>,
    missing: Vec<MissingSubject>,
    complete: bool,
    details: BTreeSet<String>,
    bytes: u64,
    directories_seen: usize,
}

impl CaptureState {
    fn new() -> Self {
        Self {
            entries: Vec::new(),
            skipped: Vec::new(),
            missing: Vec::new(),
            complete: true,
            details: BTreeSet::new(),
            bytes: 0,
            directories_seen: 0,
        }
    }
}

/// Create a verified content-addressed backup at `destination`.
///
/// `destination` must be explicit, must not overlap the selected repository /
/// candidate / common directory, and is the only permitted write location.
pub fn create(repository: &Path, candidate: &Path, destination: &Path) -> Result<BackupReceipt> {
    create_with_limits(repository, candidate, destination, TraversalLimits::default())
}

/// Same as [`create`], with explicit traversal bounds for proof of directory and
/// byte limits.
pub fn create_with_limits(
    repository: &Path,
    candidate: &Path,
    destination: &Path,
    limits: TraversalLimits,
) -> Result<BackupReceipt> {
    let repository_identity = observe_repository(repository)?;
    let candidate_identity = observe_candidate_identity(candidate)?;
    let destination = admit_destination(destination, &repository_identity, &candidate_identity)?;
    let objects_dir = destination.join(OBJECTS_DIR_NAME);
    fs::create_dir(&objects_dir)
        .wrap_err_with(|| format!("creating backup objects directory {}", objects_dir.display()))?;

    let mut state = CaptureState::new();
    let reader = FilesystemReader;
    let pointer_path = candidate_identity.canonical_path.join(".git");
    capture_named_file(
        &pointer_path,
        BackupRole::Pointer,
        "pointer",
        &objects_dir,
        &reader,
        limits,
        &mut state,
    );
    let administrative_path =
        match state.entries.iter().find(|entry| entry.role == BackupRole::Pointer).and_then(
            |entry| {
                let bytes = fs::read(objects_dir.join(&entry.sha256)).ok()?;
                let text = String::from_utf8(bytes).ok()?;
                parse_pointer(&candidate_identity.canonical_path, &text).ok()
            },
        ) {
            Some(path) => Some(path),
            None => {
                state.missing.push(MissingSubject {
                    role: BackupRole::Admin,
                    detail: String::from(
                        "administrative path was not parsed from the captured pointer",
                    ),
                });
                None
            }
        };
    if let Some(administrative_path) = administrative_path.as_ref() {
        if !is_in_admin_namespace(&repository_identity.common_dir, administrative_path) {
            state.missing.push(MissingSubject {
                role: BackupRole::Admin,
                detail: format!(
                    "administrative path is outside the repository common-dir worktrees namespace: {}",
                    administrative_path.display()
                ),
            });
            state.skipped.push(SkippedPath {
                path: administrative_path.display().to_string(),
                reason: String::from("administrative path outside repository worktrees namespace"),
            });
        } else {
            capture_tree(
                administrative_path,
                &TreeWalk {
                    role: BackupRole::Admin,
                    filter: FileFilter::AllRegular,
                    skip_names: SKIP_NONE,
                    logical_prefix: "",
                    limits,
                    objects_dir: &objects_dir,
                    reader: &reader,
                },
                &mut state,
            );
        }
    }
    capture_tree(
        &candidate_identity.canonical_path,
        &TreeWalk {
            role: BackupRole::CandidateSource,
            filter: FileFilter::SourceLike,
            skip_names: SKIP_GIT,
            logical_prefix: "",
            limits,
            objects_dir: &objects_dir,
            reader: &reader,
        },
        &mut state,
    );
    capture_named_file(
        &repository_identity.common_dir.join("HEAD"),
        BackupRole::Refs,
        "HEAD",
        &objects_dir,
        &reader,
        limits,
        &mut state,
    );
    capture_named_file(
        &repository_identity.common_dir.join("packed-refs"),
        BackupRole::Refs,
        "packed-refs",
        &objects_dir,
        &reader,
        limits,
        &mut state,
    );
    capture_tree(
        &repository_identity.common_dir.join("refs"),
        &TreeWalk {
            role: BackupRole::Refs,
            filter: FileFilter::AllRegular,
            skip_names: SKIP_NONE,
            logical_prefix: "refs/",
            limits,
            objects_dir: &objects_dir,
            reader: &reader,
        },
        &mut state,
    );
    capture_tree(
        &repository_identity.common_dir.join("logs"),
        &TreeWalk {
            role: BackupRole::Reflog,
            filter: FileFilter::AllRegular,
            skip_names: SKIP_NONE,
            logical_prefix: "logs/",
            limits,
            objects_dir: &objects_dir,
            reader: &reader,
        },
        &mut state,
    );
    capture_named_file(
        &repository_identity.common_dir.join("config"),
        BackupRole::Config,
        "config",
        &objects_dir,
        &reader,
        limits,
        &mut state,
    );

    if !state.complete {
        let detail = state
            .details
            .into_iter()
            .next()
            .unwrap_or_else(|| String::from("backup capture was incomplete"));
        bail!("refusing to emit a verified forensic backup: {detail}");
    }

    let mut receipt = BackupReceipt {
        schema_version: BACKUP_SCHEMA_VERSION.to_string(),
        policy_version: BACKUP_POLICY_VERSION.to_string(),
        created_at: Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
        repository: repository_identity,
        candidate: candidate_identity,
        destination: destination.clone(),
        entries: state.entries,
        skipped: state.skipped,
        missing: state.missing,
        complete: true,
        retention: retention_policy(),
        restore_instructions: restore_instructions(),
        verification_digest: String::new(),
    };
    receipt.verification_digest = digest_receipt(&receipt)?;
    let receipt_bytes =
        serde_json::to_vec_pretty(&receipt).wrap_err("serializing forensic backup receipt")?;
    fs::write(destination.join(RECEIPT_FILE_NAME), receipt_bytes)
        .wrap_err_with(|| format!("writing backup receipt {}", destination.display()))?;
    verify(&destination)
}

/// Re-read a backup destination and confirm object bytes still match the receipt.
pub fn verify(destination: &Path) -> Result<BackupReceipt> {
    if has_link_or_reparse_component(destination) {
        bail!("backup destination contains a symlink or reparse point: {}", destination.display());
    }
    let destination = fs::canonicalize(destination)
        .wrap_err_with(|| format!("resolving backup destination {}", destination.display()))?;
    let metadata = fs::symlink_metadata(&destination)?;
    if !metadata.is_dir() || is_link_or_reparse(&metadata) {
        bail!("backup destination is not a regular directory");
    }
    let receipt_path = destination.join(RECEIPT_FILE_NAME);
    let reader = FilesystemReader;
    let bytes = match read_stable_file(&receipt_path, &reader, MAX_FILE_BYTES) {
        StableRead::Stable(bytes) => bytes,
        StableRead::Unstable(detail) | StableRead::Unavailable(detail) => {
            bail!("backup receipt is not stably readable: {detail}");
        }
    };
    let receipt: BackupReceipt =
        serde_json::from_slice(&bytes).wrap_err("decoding forensic backup receipt")?;
    if !receipt.complete {
        bail!("backup receipt is marked incomplete");
    }
    for entry in &receipt.entries {
        admit_object_name(&entry.sha256)?;
    }
    let expected_digest = digest_receipt(&receipt)?;
    if expected_digest != receipt.verification_digest {
        bail!("backup receipt verification digest does not match serialized evidence");
    }
    let objects_dir = destination.join(OBJECTS_DIR_NAME);
    for entry in &receipt.entries {
        let object_path = objects_dir.join(&entry.sha256);
        match read_stable_file(&object_path, &reader, MAX_FILE_BYTES) {
            StableRead::Stable(object_bytes) => {
                let digest = sha256(&object_bytes);
                if digest != entry.sha256 || object_bytes.len() as u64 != entry.bytes {
                    bail!(
                        "backup object {} failed content-address verification for {}",
                        entry.sha256,
                        entry.logical_path
                    );
                }
            }
            StableRead::Unstable(detail) | StableRead::Unavailable(detail) => {
                bail!(
                    "backup object {} for {} is not stably readable: {detail}",
                    entry.sha256,
                    entry.logical_path
                );
            }
        }
    }
    Ok(receipt)
}

pub fn render(receipt: &BackupReceipt, format: OutputFormat) -> Result<String> {
    match format {
        OutputFormat::Json => serde_json::to_string_pretty(receipt)
            .map(|json| format!("{json}\n"))
            .wrap_err("serializing forensic backup receipt"),
        OutputFormat::Human => Ok(render_human(receipt)),
    }
}

fn render_human(receipt: &BackupReceipt) -> String {
    let mut output = String::new();
    output.push_str("Worktree forensic backup\n");
    output.push_str(&format!("schema: {}\n", receipt.schema_version));
    output.push_str(&format!("destination: {}\n", receipt.destination.display()));
    output.push_str(&format!("verification_digest: {}\n", receipt.verification_digest));
    output.push_str(&format!("complete: {}\n", receipt.complete));
    output.push_str(&format!("entries: {}\n", receipt.entries.len()));
    output.push_str(&format!("missing: {}\n", receipt.missing.len()));
    output.push_str(&format!(
        "retention: {} (auto_delete={})\n",
        receipt.retention.policy, receipt.retention.auto_delete
    ));
    output.push_str("\nRestore instructions (this command does not restore):\n");
    for instruction in &receipt.restore_instructions {
        output.push_str(&format!("- {instruction}\n"));
    }
    output
}

fn retention_policy() -> RetentionPolicy {
    RetentionPolicy {
        policy: String::from("operator-held"),
        auto_delete: false,
        instruction: String::from(
            "Retain this directory until an independently authorized restore completes or the evidence is no longer needed. This command never deletes a backup.",
        ),
    }
}

fn restore_instructions() -> Vec<String> {
    vec![
        String::from(
            "This backup is evidence only; cargo xtask worktree-recovery backup does not restore, repair, synthesize .git/worktrees, reconstruct an index, or apply recovery.",
        ),
        String::from(
            "Inspect receipt.json and objects/<sha256> files named by entries. Each object is content-addressed by SHA-256.",
        ),
        String::from(
            "A later independently authorized restore would copy pointer bytes back to the candidate .git file and surviving administrative bytes to the recorded administrative path only after revalidation.",
        ),
        String::from(
            "Do not run git worktree add --force, reset, checkout, stash, clean, or reconstruct an index from this backup.",
        ),
    ]
}

fn digest_receipt(receipt: &BackupReceipt) -> Result<String> {
    let mut copy = receipt.clone();
    copy.created_at.clear();
    copy.verification_digest.clear();
    let bytes = serde_json::to_vec(&copy).wrap_err("serializing backup receipt for digest")?;
    Ok(sha256(&bytes))
}

fn admit_destination(
    destination: &Path,
    repository: &RepositoryIdentity,
    candidate: &CandidateIdentity,
) -> Result<PathBuf> {
    if destination.as_os_str().is_empty() {
        bail!("backup destination must be an explicit path");
    }
    if has_link_or_reparse_component(destination) {
        bail!("backup destination contains a symlink or reparse point: {}", destination.display());
    }
    let parent = destination_parent(destination);
    if !parent.exists() {
        bail!(
            "backup destination parent does not exist; refusing to create paths outside the explicit destination: {}",
            parent.display()
        );
    }
    if has_link_or_reparse_component(parent) {
        bail!(
            "backup destination parent contains a symlink or reparse point: {}",
            parent.display()
        );
    }
    let parent = fs::canonicalize(parent)
        .wrap_err_with(|| format!("resolving backup destination parent {}", parent.display()))?;
    let parent_metadata = fs::symlink_metadata(&parent)?;
    if !parent_metadata.is_dir() || is_link_or_reparse(&parent_metadata) {
        bail!("backup destination parent is not a regular directory");
    }
    let file_name = destination.file_name().ok_or_else(|| {
        eyre!("backup destination is not a named directory: {}", destination.display())
    })?;
    if matches!(file_name.to_str(), Some("." | "..")) {
        bail!("backup destination must be an explicit named directory");
    }
    let resolved = parent.join(file_name);
    refuse_overlap(&resolved, repository, candidate)?;
    if resolved.exists() {
        let metadata = fs::symlink_metadata(&resolved)?;
        if is_link_or_reparse(&metadata) {
            bail!("backup destination is a symlink or reparse point: {}", resolved.display());
        }
        if !metadata.is_dir() {
            bail!("backup destination exists and is not a directory");
        }
        let mut entries = fs::read_dir(&resolved).wrap_err_with(|| {
            format!("reading existing backup destination {}", resolved.display())
        })?;
        if entries.next().is_some() {
            bail!("backup destination is not empty: {}", resolved.display());
        }
        let canonical = fs::canonicalize(&resolved).wrap_err_with(|| {
            format!("canonicalizing empty backup destination {}", resolved.display())
        })?;
        refuse_overlap(&canonical, repository, candidate)?;
        pin_named_destination(&resolved, &canonical)?;
        return Ok(canonical);
    }
    fs::create_dir(&resolved)
        .wrap_err_with(|| format!("creating backup destination {}", resolved.display()))?;
    let canonical = fs::canonicalize(&resolved).wrap_err_with(|| {
        format!("canonicalizing created backup destination {}", resolved.display())
    })?;
    refuse_overlap(&canonical, repository, candidate)?;
    pin_named_destination(&resolved, &canonical)?;
    Ok(canonical)
}

fn destination_parent(destination: &Path) -> &Path {
    match destination.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    }
}

fn pin_named_destination(named: &Path, canonical: &Path) -> Result<()> {
    let named_meta = fs::symlink_metadata(named)
        .wrap_err_with(|| format!("revalidating named backup destination {}", named.display()))?;
    if is_link_or_reparse(&named_meta) {
        bail!("backup destination was replaced by a symlink or reparse point: {}", named.display());
    }
    if !named_meta.is_dir() {
        bail!("backup destination is not a regular directory: {}", named.display());
    }
    if has_link_or_reparse_component(canonical) {
        bail!(
            "canonical backup destination contains a symlink or reparse point: {}",
            canonical.display()
        );
    }
    Ok(())
}

fn admit_object_name(name: &str) -> Result<()> {
    if name.len() == 64 && name.bytes().all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f')) {
        Ok(())
    } else {
        bail!("backup object name is not a lowercase SHA-256 hex digest: {name:?}");
    }
}

fn refuse_overlap(
    destination: &Path,
    repository: &RepositoryIdentity,
    candidate: &CandidateIdentity,
) -> Result<()> {
    for (label, path) in [
        ("repository", repository.repository_root.as_path()),
        ("common directory", repository.common_dir.as_path()),
        ("candidate", candidate.canonical_path.as_path()),
    ] {
        if paths_overlap(destination, path) {
            bail!(
                "backup destination overlaps the selected {label}: {} vs {}",
                destination.display(),
                path.display()
            );
        }
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum FileFilter {
    AllRegular,
    SourceLike,
}

fn capture_named_file(
    path: &Path,
    role: BackupRole,
    logical_path: &str,
    objects_dir: &Path,
    reader: &FilesystemReader,
    limits: TraversalLimits,
    state: &mut CaptureState,
) {
    match fs::symlink_metadata(path) {
        Ok(metadata) if is_link_or_reparse(&metadata) => {
            state.complete = false;
            state.details.insert(format!("symlink or reparse path refused: {}", path.display()));
            state.skipped.push(SkippedPath {
                path: path.display().to_string(),
                reason: String::from("symlink or reparse"),
            });
        }
        Ok(_) => capture_regular_file(path, role, logical_path, objects_dir, reader, limits, state),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            state
                .missing
                .push(MissingSubject { role, detail: format!("{logical_path} was absent") });
        }
        Err(error) => {
            state.complete = false;
            state.details.insert(format!("reading {}: {error}", path.display()));
        }
    }
}

struct TreeWalk<'a> {
    role: BackupRole,
    filter: FileFilter,
    skip_names: &'static [&'static str],
    logical_prefix: &'static str,
    limits: TraversalLimits,
    objects_dir: &'a Path,
    reader: &'a FilesystemReader,
}

fn capture_tree(root: &Path, walk: &TreeWalk<'_>, state: &mut CaptureState) {
    match fs::symlink_metadata(root) {
        Ok(metadata) if is_link_or_reparse(&metadata) => {
            state.complete = false;
            state.details.insert(format!("symlink or reparse tree refused: {}", root.display()));
            state.skipped.push(SkippedPath {
                path: root.display().to_string(),
                reason: String::from("symlink or reparse"),
            });
            return;
        }
        Ok(metadata) if metadata.is_dir() => {}
        Ok(_) => {
            state.complete = false;
            state.details.insert(format!("expected a directory: {}", root.display()));
            return;
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            state.missing.push(MissingSubject {
                role: walk.role,
                detail: format!("{} was absent", root.display()),
            });
            return;
        }
        Err(error) => {
            state.complete = false;
            state.details.insert(format!("reading {}: {error}", root.display()));
            return;
        }
    }
    if has_link_or_reparse_component(root) {
        state.complete = false;
        state
            .details
            .insert(format!("tree path contains a symlink or reparse point: {}", root.display()));
        return;
    }
    capture_tree_inner(root, root, 0, walk, state);
}

fn capture_tree_inner(
    root: &Path,
    current: &Path,
    depth: usize,
    walk: &TreeWalk<'_>,
    state: &mut CaptureState,
) {
    if !state.complete {
        return;
    }
    if depth > walk.limits.max_depth {
        state.complete = false;
        state.details.insert(format!("maximum depth {} exceeded", walk.limits.max_depth));
        return;
    }
    state.directories_seen = state.directories_seen.saturating_add(1);
    if state.directories_seen > walk.limits.max_directories {
        state.complete = false;
        state
            .details
            .insert(format!("maximum directories {} exceeded", walk.limits.max_directories));
        return;
    }
    let before = match directory_fingerprint(current) {
        Ok(value) => value,
        Err(error) => {
            state.complete = false;
            state.details.insert(format!(
                "directory identity unavailable for {}: {error}",
                current.display()
            ));
            return;
        }
    };
    let mut entries = match read_bounded_entries(current, walk.limits.max_entries_per_directory) {
        Ok((entries, truncated)) => {
            if truncated {
                state.complete = false;
                state.details.insert(format!(
                    "maximum entries per directory {} exceeded",
                    walk.limits.max_entries_per_directory
                ));
            }
            entries
        }
        Err(error) => {
            state.complete = false;
            state.details.insert(format!("reading directory {}: {error}", current.display()));
            return;
        }
    };
    if !directory_still_matches(current, before, state) {
        return;
    }
    entries.sort_by(|left, right| os_str_order(&left.file_name(), &right.file_name()));
    for entry in entries {
        let name = entry.file_name();
        if walk.skip_names.iter().any(|skip| name == *skip) {
            continue;
        }
        let path = entry.path();
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) => {
                state.complete = false;
                state.details.insert(format!("reading metadata {}: {error}", path.display()));
                continue;
            }
        };
        if is_link_or_reparse(&metadata) {
            state.complete = false;
            state.details.insert(format!("symlink or reparse entry refused: {}", path.display()));
            state.skipped.push(SkippedPath {
                path: path.display().to_string(),
                reason: String::from("symlink or reparse"),
            });
            continue;
        }
        if metadata.is_dir() {
            capture_tree_inner(root, &path, depth.saturating_add(1), walk, state);
            continue;
        }
        if !metadata.is_file() {
            state.complete = false;
            state.details.insert(format!("special entry skipped: {}", path.display()));
            continue;
        }
        let include = match walk.filter {
            FileFilter::AllRegular => true,
            FileFilter::SourceLike => is_source_like_path(&path),
        };
        if !include {
            continue;
        }
        if state.entries.len() >= walk.limits.max_files {
            state.complete = false;
            state.details.insert(format!("maximum files {} exceeded", walk.limits.max_files));
            return;
        }
        let logical_path = match logical_path_from(root, &path) {
            Ok(path) if walk.logical_prefix.is_empty() => path,
            Ok(path) => format!("{}{path}", walk.logical_prefix),
            Err(error) => {
                state.complete = false;
                state.details.insert(error);
                continue;
            }
        };
        capture_regular_file(
            &path,
            walk.role,
            &logical_path,
            walk.objects_dir,
            walk.reader,
            walk.limits,
            state,
        );
    }
    let _ = directory_still_matches(current, before, state);
}

fn directory_still_matches(
    path: &Path,
    expected: DirectoryFingerprint,
    state: &mut CaptureState,
) -> bool {
    match directory_fingerprint(path) {
        Ok(actual) if actual == expected => true,
        Ok(_) => {
            state.complete = false;
            state.details.insert(format!(
                "RACE_DETECTED directory identity changed during observation: {}",
                path.display()
            ));
            false
        }
        Err(error) => {
            state.complete = false;
            state.details.insert(format!(
                "RACE_DETECTED directory identity unavailable after observation {}: {error}",
                path.display()
            ));
            false
        }
    }
}

fn capture_regular_file(
    path: &Path,
    role: BackupRole,
    logical_path: &str,
    objects_dir: &Path,
    reader: &FilesystemReader,
    limits: TraversalLimits,
    state: &mut CaptureState,
) {
    match read_stable_file(path, reader, MAX_FILE_BYTES) {
        StableRead::Stable(bytes) => {
            if state.bytes.saturating_add(bytes.len() as u64) > limits.max_bytes {
                state.complete = false;
                state.details.insert(format!("maximum bytes {} exceeded", limits.max_bytes));
                return;
            }
            let digest = sha256(&bytes);
            let object_path = objects_dir.join(&digest);
            if object_path.exists() {
                match fs::read(&object_path) {
                    Ok(existing) if existing == bytes => {}
                    Ok(_) => {
                        state.complete = false;
                        state.details.insert(format!("content-address collision for {digest}"));
                        return;
                    }
                    Err(error) => {
                        state.complete = false;
                        state.details.insert(format!("reading object {digest}: {error}"));
                        return;
                    }
                }
            } else if let Err(error) = fs::write(&object_path, &bytes) {
                state.complete = false;
                state.details.insert(format!("writing object {digest}: {error}"));
                return;
            }
            state.bytes = state.bytes.saturating_add(bytes.len() as u64);
            state.entries.push(BackupEntry {
                role,
                logical_path: logical_path.to_string(),
                source_path: path.display().to_string(),
                bytes: bytes.len() as u64,
                sha256: digest,
            });
        }
        StableRead::Unstable(detail) => {
            state.complete = false;
            state.details.insert(detail);
        }
        StableRead::Unavailable(detail) => {
            state.complete = false;
            state.details.insert(detail);
        }
    }
}

fn logical_path_from(root: &Path, path: &Path) -> std::result::Result<String, String> {
    let relative = path
        .strip_prefix(root)
        .map_err(|_| format!("relative path unavailable: {}", path.display()))?;
    let normalized = lexical_normalize(relative)?;
    let mut parts = Vec::new();
    for component in normalized.components() {
        match component {
            Component::Normal(name) => parts.push(name.to_string_lossy().into_owned()),
            _ => return Err(format!("unsupported relative path: {}", path.display())),
        }
    }
    Ok(parts.join("/"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use color_eyre::eyre::ensure;

    #[test]
    fn receipt_digest_excludes_timestamp_and_self() -> Result<()> {
        let receipt = sample_receipt();
        let first = digest_receipt(&receipt)?;
        let mut shifted = receipt.clone();
        shifted.created_at = String::from("1999-01-01T00:00:00Z");
        shifted.verification_digest = String::from("deadbeef");
        ensure!(digest_receipt(&shifted)? == first, "timestamp or digest changed identity");
        let mut changed = receipt;
        changed.complete = false;
        ensure!(digest_receipt(&changed)? != first, "material field did not change identity");
        Ok(())
    }

    #[test]
    fn schema_decoder_requires_current_backup_version() -> Result<()> {
        let mut json = serde_json::to_value(sample_receipt())?;
        json.as_object_mut()
            .ok_or_else(|| eyre!("receipt is not an object"))?
            .insert("schema_version".to_string(), serde_json::json!("worktree_forensic_backup.v0"));
        ensure!(
            serde_json::from_value::<BackupReceipt>(json).is_err(),
            "historical backup schema was admitted"
        );
        Ok(())
    }

    #[test]
    fn restore_instructions_forbid_apply_and_force_add() {
        let instructions = restore_instructions().join("\n");
        assert!(instructions.contains("does not restore"));
        assert!(instructions.contains("worktree add --force"));
        assert!(!instructions.contains("git worktree add --force --checkout"));
    }

    #[test]
    fn destination_overlap_detects_nested_paths() {
        let parent = Path::new("/tmp/repo");
        let child = Path::new("/tmp/repo/backup");
        assert!(paths_overlap(parent, child));
        assert!(paths_overlap(child, parent));
        assert!(!paths_overlap(Path::new("/tmp/repo"), Path::new("/tmp/other")));
    }

    #[test]
    fn relative_destination_without_slash_uses_cwd_parent() {
        assert_eq!(destination_parent(Path::new("backup")), Path::new("."));
        assert_eq!(destination_parent(Path::new("nested/backup")), Path::new("nested"));
        assert_eq!(destination_parent(Path::new("/backup")), Path::new("/"));
    }

    #[test]
    fn object_name_admission_refuses_path_escape_and_non_hex() -> Result<()> {
        admit_object_name("ab".repeat(32).as_str())?;
        ensure!(admit_object_name("../escape").is_err(), "relative object path was admitted");
        ensure!(admit_object_name("..").is_err(), "`..` object name was admitted");
        ensure!(
            admit_object_name(&format!("{}{}", "..", "a".repeat(62))).is_err(),
            "dot-dot padded to digest length was admitted"
        );
        ensure!(
            admit_object_name(&"g".repeat(64)).is_err(),
            "non-hex digest-length name was admitted"
        );
        ensure!(admit_object_name(&"A".repeat(64)).is_err(), "uppercase hex digest was admitted");
        Ok(())
    }

    fn sample_receipt() -> BackupReceipt {
        BackupReceipt {
            schema_version: BACKUP_SCHEMA_VERSION.to_string(),
            policy_version: BACKUP_POLICY_VERSION.to_string(),
            created_at: String::from("2026-09-29T00:00:00Z"),
            repository: RepositoryIdentity {
                requested_path: PathBuf::from("/repo"),
                repository_root: PathBuf::from("/repo"),
                common_dir: PathBuf::from("/repo/.git"),
                path_key: String::from("repo"),
                git_version: None,
            },
            candidate: CandidateIdentity {
                requested_path: PathBuf::from("/repo/candidate"),
                canonical_path: PathBuf::from("/repo/candidate"),
                path_key: String::from("candidate"),
            },
            destination: PathBuf::from("/backup"),
            entries: Vec::new(),
            skipped: Vec::new(),
            missing: Vec::new(),
            complete: true,
            retention: retention_policy(),
            restore_instructions: restore_instructions(),
            verification_digest: String::from("pending"),
        }
    }
}
