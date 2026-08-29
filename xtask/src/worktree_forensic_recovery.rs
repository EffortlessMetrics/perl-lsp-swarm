//! Read-only evidence collection for an explicitly selected damaged linked worktree.
//!
//! This module is intentionally narrower than a recovery executor.  It never
//! discovers candidates, writes backups, repairs Git administration, or changes
//! a repository.  A candidate path is evidence only, and uncertainty remains
//! visible in the typed result and its fail-closed classification.
//!
//! Platform boundary: Unix uses device/inode metadata plus byte comparison for
//! the sampled read interval. Windows uses the repository's stable WinAPI
//! `FileIdInfo` adapter when available; adapter failure remains `Unavailable`
//! and cannot support a clean or race-detection claim.

use chrono::{SecondsFormat, Utc};
use color_eyre::eyre::{Context, Result, bail, eyre};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::cmp::Ordering;
use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::fs;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Component, Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering as AtomicOrdering},
};
use std::time::{Duration, Instant};

pub const FORENSIC_SCHEMA_VERSION: &str = "worktree_forensic_evidence.v1";
pub const FORENSIC_POLICY_VERSION: &str = "2026-08-27";

const MAX_MANIFEST_FILES: usize = 256;
const MAX_MANIFEST_BYTES: u64 = 4 * 1024 * 1024;
const MAX_MANIFEST_DIRECTORIES: usize = 512;
const MAX_MANIFEST_DEPTH: usize = 32;
const MAX_DIRECTORY_ENTRIES: usize = 256;
const MAX_ADMIN_FILE_BYTES: u64 = 8 * 1024 * 1024;
const MAX_GIT_OUTPUT_BYTES: u64 = MAX_MANIFEST_BYTES;
const GIT_OUTPUT_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RecoveryClassification {
    CleanReconstructable,
    SalvageRequired,
    DirtyOrIndexUnknown,
    DetachedOrHeadUnknown,
    IdentityConflict,
    ActiveOrLocked,
    ForensicInstrumentUnavailable,
    NotProven,
}

impl RecoveryClassification {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::CleanReconstructable => "CLEAN_RECONSTRUCTABLE",
            Self::SalvageRequired => "SALVAGE_REQUIRED",
            Self::DirtyOrIndexUnknown => "DIRTY_OR_INDEX_UNKNOWN",
            Self::DetachedOrHeadUnknown => "DETACHED_OR_HEAD_UNKNOWN",
            Self::IdentityConflict => "IDENTITY_CONFLICT",
            Self::ActiveOrLocked => "ACTIVE_OR_LOCKED",
            Self::ForensicInstrumentUnavailable => "FORENSIC_INSTRUMENT_UNAVAILABLE",
            Self::NotProven => "NOT_PROVEN",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Observation<T> {
    pub value: Option<T>,
    pub detail: String,
}

impl<T> Observation<T> {
    pub fn observed(value: T) -> Self {
        Self { value: Some(value), detail: String::from("observed") }
    }

    pub fn unavailable(detail: impl Into<String>) -> Self {
        Self { value: None, detail: detail.into() }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepositoryIdentity {
    pub requested_path: PathBuf,
    pub repository_root: PathBuf,
    pub common_dir: PathBuf,
    pub path_key: String,
    pub git_version: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CandidateIdentity {
    pub requested_path: PathBuf,
    pub canonical_path: PathBuf,
    pub path_key: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PointerEvidence {
    pub sha256: String,
    pub bytes: u64,
    pub administrative_path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AdministrationState {
    Present,
    Missing,
    Unknown(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum HeadEvidence {
    Attached { reference: String, oid: String },
    Detached { value: String, reachable: bool },
    Unknown(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReferenceEvidence {
    Resolved { reference: String, oid: String },
    Unknown(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum IndexEvidence {
    Present { bytes: u64, sha256: String },
    Missing,
    Unknown(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConfigEvidence {
    Present { path: String, bytes: u64, sha256: String },
    OptionalAbsent { detail: String },
    Missing,
    Unknown(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum UniqueWorkEvidence {
    None,
    Present { status_lines: usize },
    IgnoredSource { paths: Vec<String> },
    IgnoredContent { paths: Vec<String> },
    Unknown(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ActiveUseEvidence {
    Inactive,
    Locked { paths: Vec<String> },
    Active { detail: String },
    Unknown(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestPathComponent {
    pub display: String,
    pub identity: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestRelativePath {
    pub components: Vec<ManifestPathComponent>,
    pub platform_key: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestEntry {
    pub relative_path: ManifestRelativePath,
    pub bytes: u64,
    pub sha256: String,
    pub source_like: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestEvidence {
    pub files: Vec<ManifestEntry>,
    pub complete: bool,
    pub directories_seen: usize,
    pub detail: String,
}

impl ManifestEvidence {
    fn unavailable(detail: impl Into<String>) -> Self {
        Self { files: Vec::new(), complete: false, directories_seen: 0, detail: detail.into() }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecoveryEvidence {
    pub repository_identity: Observation<RepositoryIdentity>,
    pub candidate_identity: Observation<CandidateIdentity>,
    pub pointer: Observation<PointerEvidence>,
    pub administrative_gitdir: Observation<PathBuf>,
    pub administrative_commondir: Observation<PathBuf>,
    pub administration: AdministrationState,
    pub head: HeadEvidence,
    pub reference: ReferenceEvidence,
    pub index: IndexEvidence,
    pub config: ConfigEvidence,
    pub unique_work: UniqueWorkEvidence,
    pub active_use: ActiveUseEvidence,
    pub source_manifest: ManifestEvidence,
    pub unknowns: Vec<String>,
    pub contradictions: Vec<String>,
    pub instrument_failures: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecoveryPlan {
    pub schema_version: String,
    pub policy_version: String,
    pub observed_at: String,
    pub repository: Observation<RepositoryIdentity>,
    pub candidate: Observation<CandidateIdentity>,
    pub evidence: RecoveryEvidence,
    pub classification: RecoveryClassification,
    pub reasons: Vec<String>,
    pub proposed_actions: Vec<String>,
    pub plan_digest: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    Human,
    Json,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TraversalLimits {
    pub max_files: usize,
    pub max_bytes: u64,
    pub max_directories: usize,
    pub max_depth: usize,
    pub max_entries_per_directory: usize,
}

impl Default for TraversalLimits {
    fn default() -> Self {
        Self {
            max_files: MAX_MANIFEST_FILES,
            max_bytes: MAX_MANIFEST_BYTES,
            max_directories: MAX_MANIFEST_DIRECTORIES,
            max_depth: MAX_MANIFEST_DEPTH,
            max_entries_per_directory: MAX_DIRECTORY_ENTRIES,
        }
    }
}

pub trait StableFileReader {
    fn read_twice(&self, path: &Path, max_bytes: u64) -> io::Result<(Vec<u8>, Vec<u8>)>;
}

pub trait ActiveUseProbe {
    fn observe(&self, candidate: &Path, administrative_path: &Path) -> ActiveUseEvidence;
}

pub struct ProcessHandleUnavailable;

impl ActiveUseProbe for ProcessHandleUnavailable {
    fn observe(&self, _candidate: &Path, _administrative_path: &Path) -> ActiveUseEvidence {
        ActiveUseEvidence::Unknown(String::from(
            "process-handle inspection is unavailable; absence of locks is not proof of inactivity",
        ))
    }
}

struct FilesystemReader;

impl StableFileReader for FilesystemReader {
    fn read_twice(&self, path: &Path, max_bytes: u64) -> io::Result<(Vec<u8>, Vec<u8>)> {
        let mut file = fs::File::open(path)?;
        let first = read_at_most(&mut file, max_bytes)?;
        file.seek(SeekFrom::Start(0))?;
        let second = read_at_most(&mut file, max_bytes)?;
        Ok((first, second))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum StableRead {
    Stable(Vec<u8>),
    Unstable(String),
    Unavailable(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct MetadataFingerprint {
    length: u64,
    modified_nanos: Option<u128>,
    #[cfg(windows)]
    file_identity: Option<crate::file_identity::WindowsFileIdentity>,
    #[cfg(not(windows))]
    file_identity: Option<(u64, u64)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DirectoryFingerprint {
    modified_nanos: Option<u128>,
    #[cfg(windows)]
    file_identity: crate::file_identity::WindowsFileIdentity,
    #[cfg(not(windows))]
    file_identity: (u64, u64),
}

pub fn inspect(repository: &Path, candidate: &Path) -> Result<RecoveryPlan> {
    inspect_with_limits_and_probe(
        repository,
        candidate,
        TraversalLimits::default(),
        &ProcessHandleUnavailable,
    )
}

pub fn inspect_with_limits(
    repository: &Path,
    candidate: &Path,
    limits: TraversalLimits,
) -> Result<RecoveryPlan> {
    inspect_with_limits_and_probe(repository, candidate, limits, &ProcessHandleUnavailable)
}

pub fn inspect_with_limits_and_probe(
    repository: &Path,
    candidate: &Path,
    limits: TraversalLimits,
    active_use_probe: &dyn ActiveUseProbe,
) -> Result<RecoveryPlan> {
    let repository_identity = match observe_repository(repository) {
        Ok(identity) => identity,
        Err(error) => {
            let detail = error.to_string();
            let evidence = initial_evidence(Observation::unavailable(detail.clone()));
            let mut evidence = evidence;
            evidence.instrument_failures.push(detail);
            return finish_plan(evidence);
        }
    };
    let mut evidence = initial_evidence(Observation::observed(repository_identity.clone()));

    let candidate_identity = match observe_candidate_identity(candidate) {
        Ok(identity) => {
            evidence.candidate_identity = Observation::observed(identity);
            true
        }
        Err(error) => {
            let detail = error.to_string();
            evidence.candidate_identity = Observation::unavailable(detail.clone());
            evidence.instrument_failures.push(detail);
            false
        }
    };

    if !candidate_identity {
        evidence.source_manifest = ManifestEvidence::unavailable(
            "candidate identity unavailable; no candidate bytes were followed",
        );
        return finish_plan(evidence);
    }

    let candidate_path = match evidence.candidate_identity.value.as_ref() {
        Some(identity) => identity.canonical_path.clone(),
        None => return finish_plan(evidence),
    };
    if !candidate_path.is_dir() {
        evidence
            .instrument_failures
            .push(format!("candidate is not an existing directory: {}", candidate_path.display()));
        evidence.source_manifest = ManifestEvidence::unavailable("candidate is not a directory");
        return finish_plan(evidence);
    }
    let candidate_directory_fingerprint = match directory_fingerprint(&candidate_path) {
        Ok(value) => value,
        Err(error) => {
            evidence.instrument_failures.push(format!(
                "candidate directory identity unavailable before observation: {error}"
            ));
            evidence.source_manifest = ManifestEvidence::unavailable(
                "candidate directory identity unavailable; no candidate bytes were followed",
            );
            return finish_plan(evidence);
        }
    };

    let reader = FilesystemReader;
    evidence.source_manifest = collect_manifest(&candidate_path, limits, &reader);

    let pointer_path = candidate_path.join(".git");
    let pointer_read = read_stable_file(&pointer_path, &reader, MAX_ADMIN_FILE_BYTES);
    let pointer_bytes = match pointer_read {
        StableRead::Stable(bytes) => bytes,
        StableRead::Unstable(detail) => {
            evidence.instrument_failures.push(detail.clone());
            evidence.pointer = Observation::unavailable(detail);
            return finish_plan(evidence);
        }
        StableRead::Unavailable(detail) => {
            if is_missing_path(&pointer_path) {
                evidence.contradictions.push(String::from("MISSING_GIT_POINTER"));
                evidence.pointer = Observation::unavailable("candidate has no .git pointer");
            } else {
                evidence.instrument_failures.push(detail.clone());
                evidence.pointer = Observation::unavailable(detail);
            }
            return finish_plan(evidence);
        }
    };

    let pointer_text = match String::from_utf8(pointer_bytes.clone()) {
        Ok(text) => text,
        Err(error) => {
            let detail = format!("candidate .git pointer is not UTF-8: {error}");
            evidence.contradictions.push(String::from("INVALID_GIT_POINTER"));
            evidence.pointer = Observation::unavailable(detail);
            return finish_plan(evidence);
        }
    };
    let administrative_path = match parse_pointer(&candidate_path, &pointer_text) {
        Ok(path) => path,
        Err(detail) => {
            evidence.contradictions.push(detail.clone());
            evidence.pointer = Observation::unavailable(detail);
            return finish_plan(evidence);
        }
    };
    evidence.pointer = Observation::observed(PointerEvidence {
        sha256: sha256(&pointer_bytes),
        bytes: pointer_bytes.len() as u64,
        administrative_path: administrative_path.clone(),
    });

    if !is_in_admin_namespace(&repository_identity.common_dir, &administrative_path) {
        evidence
            .contradictions
            .push(String::from("ADMIN_PATH_OUTSIDE_REPOSITORY_COMMON_DIR_WORKTREES"));
        return finish_plan(evidence);
    }
    if has_link_or_reparse_component(&administrative_path) {
        let detail = format!(
            "administrative path contains a symlink or reparse point: {}",
            administrative_path.display()
        );
        evidence.instrument_failures.push(detail);
        evidence.administration =
            AdministrationState::Unknown(String::from("administrative path was not followed"));
        return finish_plan(evidence);
    }

    if !administrative_path.is_dir() {
        evidence.administration = AdministrationState::Missing;
        evidence.head = HeadEvidence::Unknown(String::from("administrative HEAD is missing"));
        evidence.reference = ReferenceEvidence::Unknown(String::from("branch ref is unknown"));
        evidence.index = IndexEvidence::Missing;
        evidence.config = ConfigEvidence::Missing;
        evidence.unique_work = UniqueWorkEvidence::Unknown(String::from(
            "candidate status cannot be observed without linked-worktree administration",
        ));
        evidence.unknowns.extend([
            String::from("ADMIN_RECORD_MISSING"),
            String::from("HEAD_UNKNOWN"),
            String::from("INDEX_UNKNOWN"),
            String::from("CONFIG_UNKNOWN"),
        ]);
        return finish_plan(evidence);
    }
    evidence.administration = AdministrationState::Present;

    observe_administration(
        &repository_identity,
        &candidate_path,
        &administrative_path,
        active_use_probe,
        &mut evidence,
    );
    match directory_fingerprint(&candidate_path) {
        Ok(actual) if actual == candidate_directory_fingerprint => {}
        Ok(_) => evidence.instrument_failures.push(format!(
            "RACE_DETECTED candidate directory identity changed during observation: {}",
            candidate_path.display()
        )),
        Err(error) => evidence.instrument_failures.push(format!(
            "RACE_DETECTED candidate directory identity unavailable after observation {}: {error}",
            candidate_path.display()
        )),
    }
    finish_plan(evidence)
}

fn initial_evidence(repository_identity: Observation<RepositoryIdentity>) -> RecoveryEvidence {
    RecoveryEvidence {
        repository_identity,
        candidate_identity: Observation::unavailable("candidate identity not observed"),
        pointer: Observation::unavailable("candidate pointer not observed"),
        administrative_gitdir: Observation::unavailable("administrative gitdir not observed"),
        administrative_commondir: Observation::unavailable("administrative commondir not observed"),
        administration: AdministrationState::Unknown(String::from("not observed")),
        head: HeadEvidence::Unknown(String::from("not observed")),
        reference: ReferenceEvidence::Unknown(String::from("not observed")),
        index: IndexEvidence::Unknown(String::from("not observed")),
        config: ConfigEvidence::Unknown(String::from("not observed")),
        unique_work: UniqueWorkEvidence::Unknown(String::from("not observed")),
        active_use: ActiveUseEvidence::Unknown(String::from("not observed")),
        source_manifest: ManifestEvidence::unavailable("not observed"),
        unknowns: Vec::new(),
        contradictions: Vec::new(),
        instrument_failures: Vec::new(),
    }
}

pub fn classify(evidence: &RecoveryEvidence) -> RecoveryClassification {
    if !evidence.contradictions.is_empty() {
        return RecoveryClassification::IdentityConflict;
    }
    if evidence.repository_identity.value.is_none()
        || evidence.candidate_identity.value.is_none()
        || evidence.pointer.value.is_none()
    {
        return RecoveryClassification::ForensicInstrumentUnavailable;
    }
    if !evidence.instrument_failures.is_empty() {
        return RecoveryClassification::ForensicInstrumentUnavailable;
    }
    if matches!(
        &evidence.active_use,
        ActiveUseEvidence::Locked { .. } | ActiveUseEvidence::Active { .. }
    ) {
        return RecoveryClassification::ActiveOrLocked;
    }
    if matches!(&evidence.administration, AdministrationState::Missing) {
        return RecoveryClassification::DirtyOrIndexUnknown;
    }
    if matches!(&evidence.head, HeadEvidence::Detached { .. } | HeadEvidence::Unknown(_)) {
        return RecoveryClassification::DetachedOrHeadUnknown;
    }
    if matches!(&evidence.index, IndexEvidence::Missing | IndexEvidence::Unknown(_))
        || matches!(&evidence.config, ConfigEvidence::Missing | ConfigEvidence::Unknown(_))
    {
        return RecoveryClassification::DirtyOrIndexUnknown;
    }
    if matches!(
        &evidence.unique_work,
        UniqueWorkEvidence::Present { .. } | UniqueWorkEvidence::IgnoredSource { .. }
    ) {
        return RecoveryClassification::SalvageRequired;
    }
    if matches!(&evidence.unique_work, UniqueWorkEvidence::IgnoredContent { .. }) {
        return RecoveryClassification::NotProven;
    }
    if matches!(&evidence.unique_work, UniqueWorkEvidence::Unknown(_))
        || matches!(&evidence.active_use, ActiveUseEvidence::Unknown(_))
        || !evidence.source_manifest.complete
    {
        return RecoveryClassification::NotProven;
    }
    if evidence.administrative_gitdir.value.is_none()
        || evidence.administrative_commondir.value.is_none()
    {
        return RecoveryClassification::DirtyOrIndexUnknown;
    }
    if matches!(&evidence.head, HeadEvidence::Attached { .. })
        && matches!(&evidence.reference, ReferenceEvidence::Resolved { .. })
        && matches!(&evidence.index, IndexEvidence::Present { .. })
        && matches!(
            &evidence.config,
            ConfigEvidence::Present { .. } | ConfigEvidence::OptionalAbsent { .. }
        )
        && matches!(&evidence.unique_work, UniqueWorkEvidence::None)
        && matches!(&evidence.active_use, ActiveUseEvidence::Inactive)
    {
        return RecoveryClassification::CleanReconstructable;
    }
    RecoveryClassification::NotProven
}

pub fn exit_code(plan: &RecoveryPlan) -> i32 {
    if plan.classification == RecoveryClassification::CleanReconstructable { 0 } else { 2 }
}

pub fn render(plan: &RecoveryPlan, format: OutputFormat) -> Result<String> {
    match format {
        OutputFormat::Json => serde_json::to_string_pretty(plan)
            .map(|text| format!("{text}\n"))
            .wrap_err("serializing forensic evidence"),
        OutputFormat::Human => {
            let mut output = String::new();
            output.push_str("worktree forensic evidence (read-only)\n");
            output.push_str(&format!("  classification: {}\n", plan.classification.as_str()));
            output.push_str(&format!("  repository: {}\n", display_repository(&plan.repository)));
            output.push_str(&format!("  candidate: {}\n", display_candidate(&plan.candidate)));
            output.push_str(&format!("  plan digest: {}\n", plan.plan_digest));
            output.push_str(&format!("  reasons: {}\n", plan.reasons.join(", ")));
            output.push_str("  actions: none (preserve evidence and escalate)\n");
            output.push_str(
                "No backup, recovery, repair, prune, remove, reset, checkout, stash, clean, or filesystem mutation was performed.\n",
            );
            Ok(output)
        }
    }
}

fn finish_plan(evidence: RecoveryEvidence) -> Result<RecoveryPlan> {
    let classification = classify(&evidence);
    let mut reasons = Vec::new();
    reasons.extend(evidence.contradictions.iter().cloned());
    reasons.extend(evidence.instrument_failures.iter().cloned());
    reasons.extend(evidence.unknowns.iter().cloned());
    if reasons.is_empty() {
        reasons.push(String::from("evidence does not authorize recovery mutation"));
    }
    reasons.push(String::from("NO_DESTRUCTIVE_ACTION"));
    let repository_observation = evidence.repository_identity.clone();
    let candidate_observation = evidence.candidate_identity.clone();
    let mut plan = RecoveryPlan {
        schema_version: FORENSIC_SCHEMA_VERSION.to_string(),
        policy_version: FORENSIC_POLICY_VERSION.to_string(),
        observed_at: Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
        repository: repository_observation,
        candidate: candidate_observation,
        evidence,
        classification,
        reasons,
        proposed_actions: Vec::new(),
        plan_digest: String::new(),
    };
    plan.plan_digest = digest_plan(&plan)?;
    Ok(plan)
}

fn observe_repository(path: &Path) -> Result<RepositoryIdentity> {
    let canonical = observe_existing_directory(path)?;
    let reported_root = resolve_git_path(
        &canonical,
        &read_git_line_required(&canonical, &["rev-parse", "--show-toplevel"])?,
    )?;
    let common_dir = resolve_git_path(
        &canonical,
        &read_git_line_required(&canonical, &["rev-parse", "--git-common-dir"])?,
    )?;
    let root_key = platform_path_key(&reported_root);
    let requested_key = platform_path_key(&canonical);
    if root_key != requested_key {
        bail!(
            "repository identity conflict: requested {} but Git reports {}",
            canonical.display(),
            reported_root.display()
        );
    }
    let git_version = read_git_line_required(&canonical, &["--version"]).ok();
    Ok(RepositoryIdentity {
        requested_path: path.to_path_buf(),
        repository_root: reported_root,
        common_dir,
        path_key: requested_key,
        git_version,
    })
}

fn observe_candidate_identity(path: &Path) -> Result<CandidateIdentity> {
    if has_link_or_reparse_component(path) {
        bail!("candidate path contains a symlink or reparse point; refusing traversal");
    }
    let canonical = fs::canonicalize(path)
        .wrap_err_with(|| format!("resolving candidate identity {}", path.display()))?;
    let metadata = fs::symlink_metadata(&canonical)
        .wrap_err_with(|| format!("reading candidate metadata {}", canonical.display()))?;
    if !metadata.is_dir() || is_link_or_reparse(&metadata) {
        bail!("candidate is not a regular directory");
    }
    Ok(CandidateIdentity {
        requested_path: path.to_path_buf(),
        path_key: platform_path_key(&canonical),
        canonical_path: canonical,
    })
}

fn observe_administration(
    repository: &RepositoryIdentity,
    candidate: &Path,
    administrative_path: &Path,
    active_use_probe: &dyn ActiveUseProbe,
    evidence: &mut RecoveryEvidence,
) {
    let reader = FilesystemReader;
    observe_administrative_identity(repository, candidate, administrative_path, &reader, evidence);
    let head_path = administrative_path.join("HEAD");
    match read_stable_file(&head_path, &reader, MAX_ADMIN_FILE_BYTES) {
        StableRead::Stable(bytes) => {
            let text = String::from_utf8_lossy(&bytes).trim().to_string();
            if let Some(reference) = text.strip_prefix("ref: refs/heads/") {
                if reference.is_empty() || reference.chars().any(char::is_whitespace) {
                    evidence.head = HeadEvidence::Unknown(String::from("invalid branch ref"));
                    evidence.reference =
                        ReferenceEvidence::Unknown(String::from("invalid branch ref"));
                } else {
                    let full_reference = format!("refs/heads/{reference}");
                    match read_git_line_required(
                        &repository.repository_root,
                        &["rev-parse", "--verify", &full_reference],
                    ) {
                        Ok(oid) => {
                            evidence.head = HeadEvidence::Attached {
                                reference: full_reference.clone(),
                                oid: oid.clone(),
                            };
                            evidence.reference =
                                ReferenceEvidence::Resolved { reference: full_reference, oid };
                        }
                        Err(error) => {
                            evidence.head = HeadEvidence::Unknown(error.to_string());
                            evidence.reference = ReferenceEvidence::Unknown(error.to_string());
                        }
                    }
                }
            } else if text.is_empty() {
                evidence.head = HeadEvidence::Unknown(String::from("empty HEAD"));
                evidence.reference = ReferenceEvidence::Unknown(String::from("empty HEAD"));
            } else {
                let reachable = read_git_line_required(
                    &repository.repository_root,
                    &["cat-file", "-e", &format!("{text}^{{commit}}")],
                )
                .is_ok();
                evidence.head = HeadEvidence::Detached { value: text, reachable };
                evidence.reference = ReferenceEvidence::Unknown(String::from("detached HEAD"));
            }
        }
        StableRead::Unstable(detail) | StableRead::Unavailable(detail) => {
            evidence.head = HeadEvidence::Unknown(detail.clone());
            evidence.unknowns.push(String::from("HEAD_UNKNOWN"));
            if !is_missing_path(&head_path) {
                evidence.instrument_failures.push(detail);
            }
        }
    }

    observe_candidate_git_identity(repository, candidate, administrative_path, evidence);

    let index_path = administrative_path.join("index");
    evidence.index = observe_digest_file(&index_path, &reader, "index");
    let config_worktree = administrative_path.join("config.worktree");
    let config = observe_digest_file(&config_worktree, &reader, "config.worktree");
    evidence.config = match config {
        IndexEvidence::Present { bytes, sha256 } => {
            ConfigEvidence::Present { path: String::from("config.worktree"), bytes, sha256 }
        }
        IndexEvidence::Missing => {
            match extensions_worktree_config_state(&repository.common_dir, &reader) {
                WorktreeConfigExtension::Enabled => ConfigEvidence::Missing,
                WorktreeConfigExtension::Disabled => ConfigEvidence::OptionalAbsent {
                    detail: String::from(
                        "config.worktree is legitimately absent: extensions.worktreeConfig is not enabled",
                    ),
                },
                WorktreeConfigExtension::Unknown(detail) => ConfigEvidence::Unknown(detail),
            }
        }
        IndexEvidence::Unknown(detail) => ConfigEvidence::Unknown(detail),
    };

    let mut lock_paths = BTreeSet::new();
    for name in ["HEAD.lock", "index.lock", "config.lock"] {
        let lock_path = administrative_path.join(name);
        match fs::symlink_metadata(&lock_path) {
            Ok(metadata) if is_link_or_reparse(&metadata) => {
                evidence.instrument_failures.push(format!(
                    "lock path is a symlink or reparse point: {}",
                    lock_path.display()
                ));
            }
            Ok(_) => {
                lock_paths.insert(name.to_string());
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => evidence
                .instrument_failures
                .push(format!("reading lock path {}: {error}", lock_path.display())),
        }
    }
    evidence.active_use = if lock_paths.is_empty() {
        active_use_probe.observe(candidate, administrative_path)
    } else {
        ActiveUseEvidence::Locked { paths: lock_paths.into_iter().collect() }
    };

    let status = run_git_output(candidate, &unique_work_status_args());
    evidence.unique_work = match status {
        Ok(output) if output.status.success() => match String::from_utf8(output.stdout) {
            Err(error) => UniqueWorkEvidence::Unknown(format!(
                "git status emitted a non-UTF-8 path; ignored content is not classifiable: {error}"
            )),
            Ok(status_lines) => {
                let ignored_paths =
                    status_lines.lines().filter_map(ignored_status_path).collect::<Vec<_>>();
                let quoted_path = ignored_paths.iter().any(|path| path.starts_with('"'));
                if quoted_path {
                    UniqueWorkEvidence::Unknown(String::from(
                        "git status emitted a quoted path; ignored content is not classifiable",
                    ))
                } else {
                    let ignored_sources = ignored_paths
                        .iter()
                        .filter(|path| {
                            is_source_like_path(Path::new(path))
                                || manifest_has_source_under(&evidence.source_manifest, path)
                        })
                        .map(|path| (*path).to_string())
                        .collect::<Vec<_>>();
                    let lines = status_lines
                        .lines()
                        .filter(|line| !line.starts_with("!! ") && !line.trim().is_empty())
                        .count();
                    decide_unique_work_evidence(
                        ignored_sources,
                        ignored_paths.into_iter().map(String::from).collect(),
                        lines,
                    )
                }
            }
        },
        Ok(output) => UniqueWorkEvidence::Unknown(format!(
            "git status failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )),
        Err(error) => UniqueWorkEvidence::Unknown(error.to_string()),
    };
}

fn unique_work_status_args() -> Vec<&'static str> {
    vec![
        "status",
        "--porcelain=v1",
        "--ignored=matching",
        "--untracked-files=all",
        // Ambient repository configuration such as diff.ignoreSubmodules=all or
        // status.ignoreSubmodules=all would otherwise hide modified or untracked
        // submodule content from the forensic status evidence.
        "--ignore-submodules=none",
    ]
}

fn decide_unique_work_evidence(
    ignored_sources: Vec<String>,
    ignored_paths: Vec<String>,
    changed_lines: usize,
) -> UniqueWorkEvidence {
    if !ignored_sources.is_empty() {
        UniqueWorkEvidence::IgnoredSource { paths: ignored_sources }
    } else if changed_lines > 0 {
        UniqueWorkEvidence::Present { status_lines: changed_lines }
    } else if !ignored_paths.is_empty() {
        UniqueWorkEvidence::IgnoredContent { paths: ignored_paths }
    } else {
        UniqueWorkEvidence::None
    }
}

enum WorktreeConfigExtension {
    Enabled,
    Disabled,
    Unknown(String),
}

fn extensions_worktree_config_state(
    common_dir: &Path,
    reader: &dyn StableFileReader,
) -> WorktreeConfigExtension {
    let common_config = common_dir.join("config");
    match read_stable_file(&common_config, reader, MAX_ADMIN_FILE_BYTES) {
        StableRead::Stable(bytes) if extensions_worktree_config_enabled(&bytes) => {
            WorktreeConfigExtension::Enabled
        }
        StableRead::Stable(_) => WorktreeConfigExtension::Disabled,
        StableRead::Unstable(detail) | StableRead::Unavailable(detail) => {
            WorktreeConfigExtension::Unknown(format!(
                "extensions.worktreeConfig state unavailable: {detail}"
            ))
        }
    }
}

fn extensions_worktree_config_enabled(config_bytes: &[u8]) -> bool {
    let mut in_extensions_section = false;
    for line in String::from_utf8_lossy(config_bytes).lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if line.starts_with('[') {
            let header = line.trim_start_matches('[').trim_end_matches(']').trim();
            let base = header
                .split(['"', ' ', '\t'])
                .next()
                .unwrap_or_default()
                .trim()
                .to_ascii_lowercase();
            // `[extensions "subsection"]` rows belong to extensions.<subsection>,
            // not to the extensions table that owns worktreeConfig.
            let has_subsection = header.contains('"') || header.contains(' ');
            in_extensions_section = base == "extensions" && !has_subsection;
            continue;
        }
        if !in_extensions_section {
            continue;
        }
        let (key, value) = match line.split_once('=') {
            Some((key, value)) => (key.trim(), value.trim()),
            None => (line, ""),
        };
        if key.eq_ignore_ascii_case("worktreeconfig") {
            let value = value.to_ascii_lowercase();
            return value.is_empty()
                || matches!(value.as_str(), "true" | "yes" | "on")
                || value.parse::<i64>().map(|parsed| parsed != 0).unwrap_or(false);
        }
    }
    false
}

fn observe_administrative_identity(
    repository: &RepositoryIdentity,
    candidate: &Path,
    administrative_path: &Path,
    reader: &dyn StableFileReader,
    evidence: &mut RecoveryEvidence,
) {
    let gitdir_path = administrative_path.join("gitdir");
    match read_stable_file(&gitdir_path, reader, MAX_ADMIN_FILE_BYTES) {
        StableRead::Stable(bytes) => match String::from_utf8(bytes) {
            Ok(text) if !text.trim().is_empty() => {
                match lexical_normalize(&resolve_relative(
                    administrative_path,
                    Path::new(text.trim()),
                )) {
                    Ok(path) => {
                        evidence.administrative_gitdir = Observation::observed(path.clone());
                        if platform_path_key(&path) != platform_path_key(&candidate.join(".git")) {
                            evidence
                                .contradictions
                                .push(String::from("ADMIN_GITDIR_CANDIDATE_IDENTITY_CONFLICT"));
                        }
                    }
                    Err(error) => evidence.contradictions.push(error),
                }
            }
            Ok(_) => evidence.unknowns.push(String::from("ADMIN_GITDIR_UNKNOWN")),
            Err(error) => evidence
                .instrument_failures
                .push(format!("administrative gitdir is not UTF-8: {error}")),
        },
        StableRead::Unstable(detail) => evidence.instrument_failures.push(detail),
        StableRead::Unavailable(_detail) if is_missing_path(&gitdir_path) => {
            evidence.unknowns.push(String::from("ADMIN_GITDIR_UNKNOWN"))
        }
        StableRead::Unavailable(detail) => evidence.instrument_failures.push(detail),
    }

    let commondir_path = administrative_path.join("commondir");
    match read_stable_file(&commondir_path, reader, MAX_ADMIN_FILE_BYTES) {
        StableRead::Stable(bytes) => match String::from_utf8(bytes) {
            Ok(text) if !text.trim().is_empty() => {
                match lexical_normalize(&resolve_relative(
                    administrative_path,
                    Path::new(text.trim()),
                )) {
                    Ok(path) => match fs::canonicalize(&path) {
                        Ok(canonical) => {
                            evidence.administrative_commondir =
                                Observation::observed(canonical.clone());
                            if platform_path_key(&canonical)
                                != platform_path_key(&repository.common_dir)
                            {
                                evidence
                                    .contradictions
                                    .push(String::from("ADMIN_COMMONDIR_IDENTITY_CONFLICT"));
                            }
                        }
                        Err(error) => evidence.instrument_failures.push(format!(
                            "canonicalizing administrative commondir {}: {error}",
                            path.display()
                        )),
                    },
                    Err(error) => evidence.contradictions.push(error),
                }
            }
            Ok(_) => evidence.unknowns.push(String::from("ADMIN_COMMONDIR_UNKNOWN")),
            Err(error) => evidence
                .instrument_failures
                .push(format!("administrative commondir is not UTF-8: {error}")),
        },
        StableRead::Unstable(detail) => evidence.instrument_failures.push(detail),
        StableRead::Unavailable(_detail) if is_missing_path(&commondir_path) => {
            evidence.unknowns.push(String::from("ADMIN_COMMONDIR_UNKNOWN"))
        }
        StableRead::Unavailable(detail) => evidence.instrument_failures.push(detail),
    }
}

fn ignored_status_path(line: &str) -> Option<&str> {
    line.strip_prefix("!! ").map(str::trim)
}

fn is_source_like_path(path: &Path) -> bool {
    let Some(extension) = path.extension().and_then(OsStr::to_str) else {
        return false;
    };
    matches!(extension.to_ascii_lowercase().as_str(), "pl" | "pm" | "pod" | "t" | "plx" | "xs")
}

fn manifest_has_source_under(manifest: &ManifestEvidence, ignored_path: &str) -> bool {
    let trimmed = ignored_path.trim_end_matches(['/', '\\']);
    let ignored_key = platform_path_key(Path::new(trimmed));
    let prefix = format!("{ignored_key}/");
    manifest.files.iter().any(|entry| {
        entry.source_like
            && (entry.relative_path.platform_key == ignored_key
                || entry.relative_path.platform_key.starts_with(&prefix))
    })
}

fn observe_candidate_git_identity(
    repository: &RepositoryIdentity,
    candidate: &Path,
    administrative_path: &Path,
    evidence: &mut RecoveryEvidence,
) {
    let checks = [
        ("git-dir", "--git-dir", administrative_path.to_path_buf()),
        ("git-common-dir", "--git-common-dir", repository.common_dir.clone()),
        ("top-level", "--show-toplevel", candidate.to_path_buf()),
    ];
    for (label, argument, expected) in checks {
        match read_git_line_required(candidate, &["rev-parse", argument]) {
            Ok(observed) => match resolve_git_path(candidate, &observed) {
                Ok(path) if platform_path_key(&path) == platform_path_key(&expected) => {}
                Ok(path) => evidence.contradictions.push(format!(
                    "CANDIDATE_{label}_IDENTITY_CONFLICT: observed {}, expected {}",
                    path.display(),
                    expected.display()
                )),
                Err(error) => evidence
                    .instrument_failures
                    .push(format!("resolving candidate {label} identity: {error}")),
            },
            Err(error) => evidence
                .contradictions
                .push(format!("CANDIDATE_{label}_IDENTITY_UNAVAILABLE: {error}")),
        }
    }
    match read_git_line_required(candidate, &["rev-parse", "HEAD"]) {
        Ok(oid) => {
            if let HeadEvidence::Attached { oid: expected, .. } = &evidence.head
                && &oid != expected
            {
                evidence.contradictions.push(String::from("CANDIDATE_HEAD_IDENTITY_CONFLICT"));
            }
        }
        Err(error) => {
            evidence.contradictions.push(format!("CANDIDATE_HEAD_IDENTITY_UNAVAILABLE: {error}"))
        }
    }
}

fn observe_digest_file(path: &Path, reader: &dyn StableFileReader, label: &str) -> IndexEvidence {
    match fs::symlink_metadata(path) {
        Ok(metadata) if is_link_or_reparse(&metadata) => {
            IndexEvidence::Unknown(format!("{label} is a symlink or reparse point"))
        }
        Ok(metadata) if !metadata.is_file() => {
            IndexEvidence::Unknown(format!("{label} is not a regular file"))
        }
        Ok(metadata) if metadata.len() > MAX_ADMIN_FILE_BYTES => {
            IndexEvidence::Unknown(format!("{label} exceeds the bounded observation size"))
        }
        Ok(_) => match read_stable_file(path, reader, MAX_ADMIN_FILE_BYTES) {
            StableRead::Stable(bytes) => {
                IndexEvidence::Present { bytes: bytes.len() as u64, sha256: sha256(&bytes) }
            }
            StableRead::Unstable(detail) | StableRead::Unavailable(detail) => {
                IndexEvidence::Unknown(detail)
            }
        },
        Err(error) if error.kind() == io::ErrorKind::NotFound => IndexEvidence::Missing,
        Err(error) => IndexEvidence::Unknown(format!("reading {label}: {error}")),
    }
}

fn collect_manifest(
    root: &Path,
    limits: TraversalLimits,
    reader: &dyn StableFileReader,
) -> ManifestEvidence {
    let mut state = ManifestState {
        files: Vec::new(),
        bytes: 0,
        directories_seen: 0,
        complete: true,
        details: BTreeSet::new(),
    };
    collect_manifest_inner(root, root, 0, limits, reader, &mut state);
    state.files.sort_by(|left, right| {
        left.relative_path.platform_key.cmp(&right.relative_path.platform_key)
    });
    let detail = if state.complete {
        String::from("bounded source-like manifest complete")
    } else {
        let mut details = state.details.into_iter().collect::<Vec<_>>();
        details.sort();
        format!("bounded source-like manifest incomplete: {}", details.join("; "))
    };
    ManifestEvidence {
        files: state.files,
        complete: state.complete,
        directories_seen: state.directories_seen,
        detail,
    }
}

struct ManifestState {
    files: Vec<ManifestEntry>,
    bytes: u64,
    directories_seen: usize,
    complete: bool,
    details: BTreeSet<String>,
}

fn collect_manifest_inner(
    root: &Path,
    current: &Path,
    depth: usize,
    limits: TraversalLimits,
    reader: &dyn StableFileReader,
    state: &mut ManifestState,
) {
    if depth > limits.max_depth {
        state.complete = false;
        state.details.insert(format!("maximum depth {} exceeded", limits.max_depth));
        return;
    }
    state.directories_seen = state.directories_seen.saturating_add(1);
    if state.directories_seen > limits.max_directories {
        state.complete = false;
        state.details.insert(format!("maximum directories {} exceeded", limits.max_directories));
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
    let mut entries = match read_bounded_entries(current, limits.max_entries_per_directory) {
        Ok((entries, truncated)) => {
            if truncated {
                state.complete = false;
                state.details.insert(format!(
                    "maximum entries per directory {} exceeded",
                    limits.max_entries_per_directory
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
        if state.files.len() >= limits.max_files {
            state.complete = false;
            state.details.insert(format!("maximum files {} exceeded", limits.max_files));
            return;
        }
        if state.bytes >= limits.max_bytes {
            state.complete = false;
            state.details.insert(format!("maximum bytes {} exceeded", limits.max_bytes));
            return;
        }
        let name = entry.file_name();
        if name == OsStr::new(".git") {
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
            state.details.insert(format!("symlink or reparse entry skipped: {}", path.display()));
            continue;
        }
        if metadata.is_dir() {
            collect_manifest_inner(root, &path, depth.saturating_add(1), limits, reader, state);
            continue;
        }
        if !metadata.is_file() {
            state.complete = false;
            state.details.insert(format!("special entry skipped: {}", path.display()));
            continue;
        }
        let length = metadata.len();
        if state.bytes.saturating_add(length) > limits.max_bytes {
            state.complete = false;
            state.details.insert(format!("maximum bytes {} exceeded", limits.max_bytes));
            return;
        }
        match read_stable_file(&path, reader, limits.max_bytes.saturating_sub(state.bytes)) {
            StableRead::Stable(bytes) => {
                let relative_path = match path.strip_prefix(root) {
                    Ok(relative) => manifest_relative_path(relative),
                    Err(_) => {
                        state.complete = false;
                        state
                            .details
                            .insert(format!("relative path unavailable: {}", path.display()));
                        continue;
                    }
                };
                state.files.push(ManifestEntry {
                    relative_path,
                    bytes: bytes.len() as u64,
                    sha256: sha256(&bytes),
                    source_like: is_source_like_path(&path),
                });
                state.bytes = state.bytes.saturating_add(bytes.len() as u64);
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
    let _ = directory_still_matches(current, before, state);
}

fn directory_still_matches(
    path: &Path,
    expected: DirectoryFingerprint,
    state: &mut ManifestState,
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

fn read_bounded_entries(current: &Path, maximum: usize) -> io::Result<(Vec<fs::DirEntry>, bool)> {
    let mut entries = Vec::new();
    let mut truncated = false;
    for result in fs::read_dir(current)? {
        let entry = result?;
        if entries.len() >= maximum {
            truncated = true;
            break;
        }
        entries.push(entry);
    }
    Ok((entries, truncated))
}

fn manifest_relative_path(path: &Path) -> ManifestRelativePath {
    let components = path
        .components()
        .map(|component| ManifestPathComponent {
            display: component.as_os_str().to_string_lossy().into_owned(),
            identity: path_component_key(component),
        })
        .collect();
    ManifestRelativePath { components, platform_key: platform_path_key(path) }
}

fn read_stable_file(path: &Path, reader: &dyn StableFileReader, max_bytes: u64) -> StableRead {
    let before = match metadata_fingerprint(path) {
        Ok(value) => value,
        Err(error) => {
            return StableRead::Unavailable(format!("reading {}: {error}", path.display()));
        }
    };
    let (first, second) = match reader.read_twice(path, max_bytes) {
        Ok(bytes) => bytes,
        Err(error) => {
            return StableRead::Unavailable(format!(
                "reading {} through an open handle: {error}",
                path.display()
            ));
        }
    };
    let after = match metadata_fingerprint(path) {
        Ok(value) => value,
        Err(error) => {
            return StableRead::Unavailable(format!("finalizing {}: {error}", path.display()));
        }
    };
    if before != after || first != second {
        return StableRead::Unstable(format!(
            "RACE_DETECTED while observing {}; path identity, metadata, or bytes changed during the open-handle read interval",
            path.display(),
        ));
    }
    // This is an observation-time guarantee. A replacement after the final
    // metadata check cannot be ruled out by a portable read-only observer;
    // callers must revalidate immediately before any separately authorized
    // action and must treat a changed digest as stale evidence.
    StableRead::Stable(first)
}

fn read_at_most(file: &mut fs::File, max_bytes: u64) -> io::Result<Vec<u8>> {
    let limit = max_bytes.saturating_add(1);
    let mut bytes = Vec::new();
    file.take(limit).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > max_bytes {
        return Err(io::Error::other(format!(
            "file exceeds bounded observation size of {max_bytes} bytes"
        )));
    }
    Ok(bytes)
}

fn metadata_fingerprint(path: &Path) -> io::Result<MetadataFingerprint> {
    let metadata = fs::symlink_metadata(path)?;
    if is_link_or_reparse(&metadata) || !metadata.is_file() {
        return Err(io::Error::other("path is not a regular non-reparse file"));
    }
    let file_identity = file_identity(path, &metadata)?;
    let modified_nanos = metadata
        .modified()
        .ok()
        .and_then(|modified| modified.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| duration.as_nanos());
    Ok(MetadataFingerprint { length: metadata.len(), modified_nanos, file_identity })
}

fn directory_fingerprint(path: &Path) -> io::Result<DirectoryFingerprint> {
    let metadata = fs::symlink_metadata(path)?;
    if is_link_or_reparse(&metadata) || !metadata.is_dir() {
        return Err(io::Error::other("path is not a regular non-reparse directory"));
    }
    let file_identity = file_identity(path, &metadata)?.ok_or_else(|| {
        io::Error::other("stable directory identity is unavailable on this platform")
    })?;
    let modified_nanos = metadata
        .modified()
        .ok()
        .and_then(|modified| modified.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| duration.as_nanos());
    Ok(DirectoryFingerprint { modified_nanos, file_identity })
}

#[cfg(unix)]
fn file_identity(_path: &Path, metadata: &fs::Metadata) -> io::Result<Option<(u64, u64)>> {
    use std::os::unix::fs::MetadataExt;
    Ok(Some((metadata.dev(), metadata.ino())))
}

#[cfg(windows)]
fn file_identity(
    path: &Path,
    _metadata: &fs::Metadata,
) -> io::Result<Option<crate::file_identity::WindowsFileIdentity>> {
    match crate::file_identity::windows_file_identity(path) {
        Ok(Some(identity)) => Ok(Some(identity)),
        Ok(None) => Err(io::Error::other(
            "stable Windows file identity became unavailable during observation",
        )),
        Err(error) => {
            Err(io::Error::other(format!("stable Windows file identity is unavailable: {error:#}")))
        }
    }
}

#[cfg(not(any(unix, windows)))]
fn file_identity(_path: &Path, _metadata: &fs::Metadata) -> io::Result<Option<(u64, u64)>> {
    Ok(None)
}

fn parse_pointer(candidate: &Path, text: &str) -> std::result::Result<PathBuf, String> {
    let mut records = Vec::new();
    for line in text.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let Some(value) = line.strip_prefix("gitdir:") else {
            return Err(String::from("UNKNOWN_GIT_POINTER_RECORD"));
        };
        if !value.chars().next().is_some_and(char::is_whitespace) || value.trim().is_empty() {
            return Err(String::from("INVALID_GIT_POINTER"));
        }
        records.push(value.trim());
    }
    match records.as_slice() {
        [value] => lexical_normalize(&resolve_relative(candidate, Path::new(value))),
        [] => Err(String::from("MISSING_GITDIR_RECORD")),
        _ => Err(String::from("MULTIPLE_OR_CONFLICTING_GITDIR_LINES")),
    }
}

fn resolve_relative(base: &Path, value: &Path) -> PathBuf {
    if value.is_absolute() { value.to_path_buf() } else { base.join(value) }
}

fn is_in_admin_namespace(common_dir: &Path, administrative_path: &Path) -> bool {
    [common_dir.join("worktrees"), common_dir.join(".git").join("worktrees")]
        .iter()
        .any(|root| lexical_is_within(root, administrative_path))
}

fn lexical_is_within(parent: &Path, child: &Path) -> bool {
    let parent = match lexical_normalize(&normalize_extended_prefix(parent)) {
        Ok(path) => path_components_key(&path),
        Err(_) => return false,
    };
    let child = match lexical_normalize(&normalize_extended_prefix(child)) {
        Ok(path) => path_components_key(&path),
        Err(_) => return false,
    };
    child.len() >= parent.len()
        && child.iter().zip(parent.iter()).all(|(left, right)| left == right)
}

fn lexical_normalize(path: &Path) -> std::result::Result<PathBuf, String> {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !normalized.pop() {
                    return Err(String::from("path traversal escapes filesystem root"));
                }
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    Ok(normalized)
}

fn observe_existing_directory(path: &Path) -> Result<PathBuf> {
    if has_link_or_reparse_component(path) {
        bail!("repository path contains a symlink or reparse point");
    }
    let canonical = fs::canonicalize(path)
        .wrap_err_with(|| format!("resolving repository identity {}", path.display()))?;
    let metadata = fs::symlink_metadata(&canonical)?;
    if !metadata.is_dir() || is_link_or_reparse(&metadata) {
        bail!("repository path is not a regular directory");
    }
    Ok(canonical)
}

fn has_link_or_reparse_component(path: &Path) -> bool {
    let mut current = if path.is_absolute() {
        PathBuf::new()
    } else {
        match std::env::current_dir() {
            Ok(directory) => directory,
            Err(_) => return true,
        }
    };
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => current.push(prefix.as_os_str()),
            Component::RootDir => current.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => current.push(component.as_os_str()),
            Component::Normal(name) => current.push(name),
        }
        match fs::symlink_metadata(&current) {
            Ok(metadata) if is_link_or_reparse(&metadata) => return true,
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(_) => return true,
        }
    }
    false
}

fn is_link_or_reparse(metadata: &fs::Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0000_0400;
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    }
    #[cfg(not(windows))]
    {
        false
    }
}

pub fn platform_path_key(path: &Path) -> String {
    path_components_key(&normalize_extended_prefix(path)).join("/")
}

fn path_components_key(path: &Path) -> Vec<String> {
    path.components().map(path_component_key).collect()
}

fn path_component_key(component: Component<'_>) -> String {
    let (tag, value) = match component {
        Component::Prefix(prefix) => ("P", prefix.as_os_str()),
        Component::RootDir => ("R", OsStr::new("/")),
        Component::CurDir => ("C", OsStr::new(".")),
        Component::ParentDir => ("U", OsStr::new("..")),
        Component::Normal(value) => ("N", value),
    };
    format!("{tag}{}", encoded_os_str(value))
}

#[cfg(unix)]
fn encoded_os_str(value: &OsStr) -> String {
    use std::os::unix::ffi::OsStrExt;
    hex_bytes(value.as_bytes())
}

#[cfg(windows)]
fn encoded_os_str(value: &OsStr) -> String {
    use std::os::windows::ffi::OsStrExt;
    let mut output = String::new();
    for unit in value.encode_wide() {
        let normalized = if (u16::from(b'A')..=u16::from(b'Z')).contains(&unit) {
            unit + (u16::from(b'a') - u16::from(b'A'))
        } else {
            unit
        };
        output.push_str(&format!("{normalized:04x}"));
    }
    output
}

#[cfg(not(any(unix, windows)))]
fn encoded_os_str(value: &OsStr) -> String {
    value.to_string_lossy().to_string()
}

#[cfg(not(windows))]
fn normalize_extended_prefix(path: &Path) -> PathBuf {
    path.to_path_buf()
}

#[cfg(windows)]
fn normalize_extended_prefix(path: &Path) -> PathBuf {
    let text = path.to_string_lossy();
    if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
        PathBuf::from(format!(r"\\{}", rest))
    } else if let Some(rest) = text.strip_prefix(r"\\?\") {
        PathBuf::from(rest)
    } else {
        path.to_path_buf()
    }
}

#[cfg(unix)]
fn hex_bytes(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn resolve_git_path(root: &Path, text: &str) -> Result<PathBuf> {
    let path = PathBuf::from(text.trim());
    let path = if path.is_absolute() { path } else { root.join(path) };
    fs::canonicalize(&path).wrap_err_with(|| format!("canonicalizing Git path {}", path.display()))
}

fn read_git_line_required(root: &Path, args: &[&str]) -> Result<String> {
    let output = run_git_output(root, args)?;
    if !output.status.success() {
        bail!("git {:?} failed: {}", args, String::from_utf8_lossy(&output.stderr).trim());
    }
    String::from_utf8(output.stdout)
        .map(|text| text.trim().to_string())
        .map_err(|error| eyre!("git {:?} emitted invalid UTF-8: {error}", args))
}

fn run_git_output(root: &Path, args: &[&str]) -> Result<std::process::Output> {
    let mut command = Command::new("git");
    let disabled_hooks = format!("core.hooksPath={}", disabled_hooks_path());
    command
        .current_dir(root)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .args(["-c", "core.fsmonitor=false", "-c", disabled_hooks.as_str()])
        .args(args)
        .stdin(Stdio::null());
    run_bounded_process(command, format!("read-only git command git {args:?}"), GIT_OUTPUT_TIMEOUT)
}

fn disabled_hooks_path() -> &'static str {
    #[cfg(windows)]
    {
        "NUL"
    }
    #[cfg(unix)]
    {
        "/dev/null"
    }
    #[cfg(not(any(unix, windows)))]
    {
        ""
    }
}

#[cfg(windows)]
struct ProcessContainment {
    handle: winapi::shared::ntdef::HANDLE,
}

#[cfg(unix)]
struct ProcessContainment {
    process_group: libc::pid_t,
}

#[cfg(not(any(unix, windows)))]
struct ProcessContainment;

#[cfg(windows)]
impl ProcessContainment {
    fn prepare(_command: &mut Command, _description: &str) -> Result<()> {
        Ok(())
    }
}

#[cfg(unix)]
impl ProcessContainment {
    fn prepare(command: &mut Command, _description: &str) -> Result<()> {
        use std::os::unix::process::CommandExt;

        // SAFETY: this pre-exec hook performs only the async-signal-safe
        // setpgid operation before the child runs the requested program.
        unsafe {
            command.pre_exec(|| {
                if libc::setpgid(0, 0) == -1 { Err(io::Error::last_os_error()) } else { Ok(()) }
            });
        }
        Ok(())
    }
}

#[cfg(not(any(unix, windows)))]
impl ProcessContainment {
    fn prepare(_command: &mut Command, description: &str) -> Result<()> {
        bail!("process containment is unavailable on this platform for {description}")
    }
}

#[cfg(windows)]
impl ProcessContainment {
    fn new(child: &Child, description: &str) -> Result<Self> {
        use std::mem::size_of;
        use std::os::windows::io::AsRawHandle;
        use std::ptr::null_mut;
        use winapi::um::jobapi2::{
            AssignProcessToJobObject, CreateJobObjectW, SetInformationJobObject,
        };
        use winapi::um::winnt::{
            JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
            JobObjectExtendedLimitInformation,
        };

        let handle = unsafe { CreateJobObjectW(null_mut(), null_mut()) };
        if handle.is_null() {
            return Err(eyre!(
                "creating Windows process job for {description}: {}",
                io::Error::last_os_error()
            ));
        }
        let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        let configured = unsafe {
            SetInformationJobObject(
                handle,
                JobObjectExtendedLimitInformation,
                &mut limits as *mut _ as *mut _,
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
        };
        if configured == 0 {
            unsafe { winapi::um::handleapi::CloseHandle(handle) };
            return Err(eyre!(
                "configuring Windows process job for {description}: {}",
                io::Error::last_os_error()
            ));
        }
        let assigned = unsafe {
            AssignProcessToJobObject(handle, child.as_raw_handle() as winapi::shared::ntdef::HANDLE)
        };
        if assigned == 0 {
            unsafe { winapi::um::handleapi::CloseHandle(handle) };
            return Err(eyre!(
                "containing {description} in a Windows process job: {}",
                io::Error::last_os_error()
            ));
        }
        Ok(Self { handle })
    }

    fn terminate(self) {
        unsafe {
            let _ = winapi::um::jobapi2::TerminateJobObject(self.handle, 1);
        }
    }
}

#[cfg(windows)]
impl Drop for ProcessContainment {
    fn drop(&mut self) {
        unsafe {
            let _ = winapi::um::handleapi::CloseHandle(self.handle);
        }
    }
}

#[cfg(not(windows))]
impl ProcessContainment {
    #[cfg(unix)]
    fn new(child: &Child, _description: &str) -> Result<Self> {
        let process_group = libc::pid_t::try_from(child.id())
            .map_err(|_| eyre!("child pid does not fit in a Unix process-group id"))?;
        Ok(Self { process_group })
    }

    #[cfg(unix)]
    fn terminate(self) {
        // SAFETY: the negative pid targets only the process group created by
        // the child pre-exec hook; ESRCH is harmless during normal exit.
        unsafe {
            let _ = libc::kill(-self.process_group, libc::SIGKILL);
        }
    }
}

#[cfg(not(any(unix, windows)))]
impl ProcessContainment {
    fn new(_child: &Child, description: &str) -> Result<Self> {
        bail!("process containment is unavailable on this platform for {description}")
    }

    fn terminate(self) {}
}

fn terminate_process(child: &mut Child, containment: &mut Option<ProcessContainment>) {
    if let Some(job) = containment.take() {
        job.terminate();
    }
    let _ = child.kill();
    let _ = child.wait();
}

fn run_bounded_process(
    mut command: Command,
    description: String,
    timeout: Duration,
) -> Result<std::process::Output> {
    ProcessContainment::prepare(&mut command, &description)?;
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .wrap_err_with(|| format!("running {description}"))?;
    let mut containment = match ProcessContainment::new(&child, &description) {
        Ok(value) => Some(value),
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error);
        }
    };
    let stdout = child.stdout.take().ok_or_else(|| eyre!("{description} did not expose stdout"))?;
    let stderr = child.stderr.take().ok_or_else(|| eyre!("{description} did not expose stderr"))?;
    let overflow = Arc::new(AtomicBool::new(false));
    let stdout_overflow = Arc::clone(&overflow);
    let stderr_overflow = Arc::clone(&overflow);
    let stdout_thread = std::thread::spawn(move || {
        read_pipe_bounded(stdout, MAX_GIT_OUTPUT_BYTES, stdout_overflow)
    });
    let stderr_thread = std::thread::spawn(move || {
        read_pipe_bounded(stderr, MAX_GIT_OUTPUT_BYTES, stderr_overflow)
    });
    let started = Instant::now();
    let mut status = None;
    let mut timed_out = false;
    loop {
        if overflow.load(AtomicOrdering::Relaxed) {
            terminate_process(&mut child, &mut containment);
            break;
        }
        match child.try_wait().wrap_err_with(|| format!("waiting for {description}")) {
            Ok(Some(value)) => {
                status = Some(value);
                if let Some(job) = containment.take() {
                    job.terminate();
                }
                break;
            }
            Ok(None) if started.elapsed() >= timeout => {
                timed_out = true;
                terminate_process(&mut child, &mut containment);
                break;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(10)),
            Err(error) => {
                terminate_process(&mut child, &mut containment);
                return Err(error);
            }
        }
    }
    let (stdout, stdout_exceeded) =
        stdout_thread.join().map_err(|_| eyre!("{description} stdout reader panicked"))??;
    let (stderr, stderr_exceeded) =
        stderr_thread.join().map_err(|_| eyre!("{description} stderr reader panicked"))??;
    if timed_out {
        bail!("{description} exceeded bounded producer time of {:?}", timeout);
    }
    if stdout_exceeded || stderr_exceeded || overflow.load(AtomicOrdering::Relaxed) {
        bail!("{description} exceeded bounded output size of {MAX_GIT_OUTPUT_BYTES} bytes");
    }
    let status = status.ok_or_else(|| eyre!("{description} ended without a process status"))?;
    Ok(std::process::Output { status, stdout, stderr })
}

fn read_pipe_bounded<R: Read>(
    mut reader: R,
    max_bytes: u64,
    overflow: Arc<AtomicBool>,
) -> io::Result<(Vec<u8>, bool)> {
    let limit = usize::try_from(max_bytes)
        .map_err(|_| io::Error::other("bounded output size does not fit in usize"))?;
    let mut bytes = Vec::new();
    let mut exceeded = false;
    let mut buffer = [0_u8; 8192];
    loop {
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        let remaining = limit.saturating_sub(bytes.len());
        if count > remaining {
            bytes.extend_from_slice(&buffer[..remaining]);
            exceeded = true;
            overflow.store(true, AtomicOrdering::Relaxed);
            return Ok((bytes, exceeded));
        } else {
            bytes.extend_from_slice(&buffer[..count]);
        }
    }
    Ok((bytes, exceeded))
}

fn is_missing_path(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .map(|_| false)
        .unwrap_or_else(|error| error.kind() == io::ErrorKind::NotFound)
}

fn digest_plan(plan: &RecoveryPlan) -> Result<String> {
    let mut copy = plan.clone();
    copy.observed_at.clear();
    copy.plan_digest.clear();
    let bytes = serde_json::to_vec(&copy).wrap_err("serializing evidence for digest")?;
    Ok(sha256(&bytes))
}

fn sha256(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn display_repository(observation: &Observation<RepositoryIdentity>) -> String {
    match &observation.value {
        Some(value) => value.repository_root.display().to_string(),
        None => format!("UNKNOWN ({})", observation.detail),
    }
}

fn display_candidate(observation: &Observation<CandidateIdentity>) -> String {
    match &observation.value {
        Some(value) => value.canonical_path.display().to_string(),
        None => format!("UNKNOWN ({})", observation.detail),
    }
}

fn os_str_order(left: &OsStr, right: &OsStr) -> Ordering {
    left.to_string_lossy().cmp(&right.to_string_lossy())
}

#[cfg(test)]
mod tests {
    use super::*;
    use color_eyre::eyre::{Result, bail, ensure};
    use std::cell::Cell;
    use std::io::Write;
    use tempfile::tempdir;

    fn positive_evidence() -> RecoveryEvidence {
        RecoveryEvidence {
            repository_identity: Observation::observed(RepositoryIdentity {
                requested_path: PathBuf::from("repo"),
                repository_root: PathBuf::from("repo"),
                common_dir: PathBuf::from("repo/.git"),
                path_key: String::from("repo"),
                git_version: Some(String::from("git version fixture")),
            }),
            candidate_identity: Observation::observed(CandidateIdentity {
                requested_path: PathBuf::from("candidate"),
                canonical_path: PathBuf::from("candidate"),
                path_key: String::from("candidate"),
            }),
            pointer: Observation::observed(PointerEvidence {
                sha256: String::from("pointer"),
                bytes: 1,
                administrative_path: PathBuf::from("repo/.git/worktrees/candidate"),
            }),
            administrative_gitdir: Observation::observed(PathBuf::from("candidate/.git")),
            administrative_commondir: Observation::observed(PathBuf::from("repo/.git")),
            administration: AdministrationState::Present,
            head: HeadEvidence::Attached {
                reference: String::from("refs/heads/feature"),
                oid: String::from("0123456789012345678901234567890123456789"),
            },
            reference: ReferenceEvidence::Resolved {
                reference: String::from("refs/heads/feature"),
                oid: String::from("0123456789012345678901234567890123456789"),
            },
            index: IndexEvidence::Present { bytes: 1, sha256: String::from("index") },
            config: ConfigEvidence::Present {
                path: String::from("config.worktree"),
                bytes: 1,
                sha256: String::from("config"),
            },
            unique_work: UniqueWorkEvidence::None,
            active_use: ActiveUseEvidence::Inactive,
            source_manifest: ManifestEvidence {
                files: Vec::new(),
                complete: true,
                directories_seen: 1,
                detail: String::from("complete"),
            },
            unknowns: Vec::new(),
            contradictions: Vec::new(),
            instrument_failures: Vec::new(),
        }
    }

    fn assert_same_size_change_rejected(result: StableRead, label: &str) -> Result<()> {
        #[cfg(windows)]
        match result {
            StableRead::Unstable(detail) => ensure!(
                detail.contains("RACE_DETECTED"),
                "{label} lacked race diagnostic: {detail}"
            ),
            StableRead::Unavailable(detail) => ensure!(
                detail.contains("stable Windows file identity is unavailable"),
                "{label} did not fail closed on Windows: {detail}"
            ),
            other => bail!("{label} was accepted: {other:?}"),
        }
        #[cfg(not(windows))]
        match result {
            StableRead::Unstable(detail) => ensure!(
                detail.contains("RACE_DETECTED"),
                "{label} lacked race diagnostic: {detail}"
            ),
            other => bail!("{label} was accepted: {other:?}"),
        }
        Ok(())
    }

    #[test]
    fn pure_evidence_reaches_every_required_classification() -> Result<()> {
        let cases = [
            (RecoveryClassification::CleanReconstructable, positive_evidence()),
            (
                RecoveryClassification::SalvageRequired,
                RecoveryEvidence {
                    unique_work: UniqueWorkEvidence::Present { status_lines: 1 },
                    ..positive_evidence()
                },
            ),
            (
                RecoveryClassification::DirtyOrIndexUnknown,
                RecoveryEvidence { index: IndexEvidence::Missing, ..positive_evidence() },
            ),
            (
                RecoveryClassification::DetachedOrHeadUnknown,
                RecoveryEvidence {
                    head: HeadEvidence::Detached {
                        value: String::from("deadbeef"),
                        reachable: true,
                    },
                    ..positive_evidence()
                },
            ),
            (
                RecoveryClassification::IdentityConflict,
                RecoveryEvidence {
                    contradictions: vec![String::from("pointer mismatch")],
                    ..positive_evidence()
                },
            ),
            (
                RecoveryClassification::ActiveOrLocked,
                RecoveryEvidence {
                    active_use: ActiveUseEvidence::Locked {
                        paths: vec![String::from("index.lock")],
                    },
                    ..positive_evidence()
                },
            ),
            (
                RecoveryClassification::ForensicInstrumentUnavailable,
                RecoveryEvidence {
                    instrument_failures: vec![String::from("metadata denied")],
                    ..positive_evidence()
                },
            ),
            (
                RecoveryClassification::NotProven,
                RecoveryEvidence {
                    active_use: ActiveUseEvidence::Unknown(String::from("processes unavailable")),
                    ..positive_evidence()
                },
            ),
        ];
        for (expected, evidence) in cases {
            ensure!(
                classify(&evidence) == expected,
                "expected {}, got {}",
                expected.as_str(),
                classify(&evidence).as_str()
            );
        }
        Ok(())
    }

    #[test]
    fn missing_admin_cannot_be_clean() -> Result<()> {
        let evidence = RecoveryEvidence {
            administration: AdministrationState::Missing,
            ..positive_evidence()
        };
        ensure!(
            classify(&evidence) == RecoveryClassification::DirtyOrIndexUnknown,
            "missing administration was classified clean"
        );
        Ok(())
    }

    #[test]
    fn missing_index_and_config_cannot_be_clean() -> Result<()> {
        let missing_index =
            RecoveryEvidence { index: IndexEvidence::Missing, ..positive_evidence() };
        let missing_config =
            RecoveryEvidence { config: ConfigEvidence::Missing, ..positive_evidence() };
        ensure!(
            classify(&missing_index) == RecoveryClassification::DirtyOrIndexUnknown,
            "missing index was classified clean"
        );
        ensure!(
            classify(&missing_config) == RecoveryClassification::DirtyOrIndexUnknown,
            "missing config was classified clean"
        );
        Ok(())
    }

    #[test]
    fn optionally_absent_worktree_config_can_be_clean() -> Result<()> {
        let optional_absent = RecoveryEvidence {
            config: ConfigEvidence::OptionalAbsent {
                detail: String::from(
                    "config.worktree is legitimately absent: extensions.worktreeConfig is not enabled",
                ),
            },
            ..positive_evidence()
        };
        ensure!(
            classify(&optional_absent) == RecoveryClassification::CleanReconstructable,
            "optional absent config.worktree must not block a clean classification"
        );
        Ok(())
    }

    #[test]
    fn worktree_config_extension_parser_matches_git_truthy_keys() {
        let enabled_cases = [
            "[core]\n\trepositoryformatversion = 0\n[extensions]\n\tworktreeConfig = true\n",
            "[extensions]\n\tworktreeconfig\n",
            "[extensions]\n\tWORKTREECONFIG = yes\n",
            "[extensions]\n\tworktreeconfig = 1\n",
            "[extensions]\n\tworktreeconfig = on\n",
        ];
        for case in enabled_cases {
            assert!(
                extensions_worktree_config_enabled(case.as_bytes()),
                "expected enabled: {case:?}"
            );
        }
        let disabled_cases = [
            "[core]\n\trepositoryformatversion = 0\n",
            "[extensions]\n\tworktreeConfig = false\n",
            "[extensions]\n\tworktreeconfig = 0\n",
            "[extensions \"other\"]\n\tworktreeconfig = true\n",
            "; comment\n[extensions]\n",
        ];
        for case in disabled_cases {
            assert!(
                !extensions_worktree_config_enabled(case.as_bytes()),
                "expected disabled: {case:?}"
            );
        }
    }

    #[test]
    fn changed_unique_work_outranks_ignored_content() {
        assert!(matches!(
            decide_unique_work_evidence(Vec::new(), vec![String::from("target/")], 2),
            UniqueWorkEvidence::Present { status_lines: 2 }
        ));
        assert!(matches!(
            decide_unique_work_evidence(Vec::new(), vec![String::from("target/")], 0),
            UniqueWorkEvidence::IgnoredContent { .. }
        ));
        assert!(matches!(
            decide_unique_work_evidence(vec![String::from("lib/old.pm")], Vec::new(), 3),
            UniqueWorkEvidence::IgnoredSource { .. }
        ));
        assert!(matches!(
            decide_unique_work_evidence(Vec::new(), Vec::new(), 0),
            UniqueWorkEvidence::None
        ));
    }

    #[test]
    fn unique_work_status_pins_submodule_evidence_args() {
        assert_eq!(
            unique_work_status_args(),
            vec![
                "status",
                "--porcelain=v1",
                "--ignored=matching",
                "--untracked-files=all",
                "--ignore-submodules=none",
            ]
        );
    }

    #[test]
    fn missing_identity_observations_cannot_be_classified_clean() -> Result<()> {
        for (label, evidence) in [
            (
                "repository",
                RecoveryEvidence {
                    repository_identity: Observation::unavailable("missing repository"),
                    ..positive_evidence()
                },
            ),
            (
                "candidate",
                RecoveryEvidence {
                    candidate_identity: Observation::unavailable("missing candidate"),
                    ..positive_evidence()
                },
            ),
            (
                "pointer",
                RecoveryEvidence {
                    pointer: Observation::unavailable("missing pointer"),
                    ..positive_evidence()
                },
            ),
        ] {
            ensure!(
                classify(&evidence) == RecoveryClassification::ForensicInstrumentUnavailable,
                "missing {label} identity was not refused"
            );
        }
        Ok(())
    }

    #[test]
    fn pointer_parser_rejects_unknown_and_duplicate_records() -> Result<()> {
        let candidate = Path::new("repo/candidate");
        ensure!(
            parse_pointer(candidate, "gitdir: admin\nunknown: value\n").is_err(),
            "unknown pointer record accepted"
        );
        ensure!(
            parse_pointer(candidate, "gitdir: admin\ngitdir: other\n").is_err(),
            "duplicate pointer accepted"
        );
        Ok(())
    }

    #[test]
    fn manifest_limits_bound_directories_and_depth() -> Result<()> {
        let temporary = tempdir()?;
        let root = temporary.path().join("candidate");
        fs::create_dir_all(root.join("one/two"))?;
        fs::write(root.join("one/two/source.pl"), "source\n")?;
        fs::write(root.join("second.txt"), "second\n")?;
        let manifest = collect_manifest(
            &root,
            TraversalLimits { max_directories: 1, max_depth: 32, ..TraversalLimits::default() },
            &FilesystemReader,
        );
        ensure!(!manifest.complete, "directory bound did not make the manifest incomplete");

        let manifest = collect_manifest(
            &root,
            TraversalLimits { max_depth: 0, ..TraversalLimits::default() },
            &FilesystemReader,
        );
        ensure!(!manifest.complete, "depth bound did not make the manifest incomplete");

        let manifest = collect_manifest(
            &root,
            TraversalLimits { max_entries_per_directory: 1, ..TraversalLimits::default() },
            &FilesystemReader,
        );
        ensure!(
            !manifest.complete,
            "per-directory entry bound did not make the manifest incomplete"
        );
        Ok(())
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn directory_replacement_during_manifest_read_is_rejected() -> Result<()> {
        struct ReplacingReader {
            root: PathBuf,
            replacement: PathBuf,
            replaced: Cell<bool>,
        }

        impl StableFileReader for ReplacingReader {
            fn read_twice(&self, path: &Path, _max_bytes: u64) -> io::Result<(Vec<u8>, Vec<u8>)> {
                if !self.replaced.replace(true) {
                    let backup = self.root.with_extension("old");
                    fs::rename(&self.root, &backup)?;
                    fs::rename(&self.replacement, &self.root)?;
                }
                let bytes = fs::read(path)?;
                Ok((bytes.clone(), bytes))
            }
        }

        let temporary = tempdir()?;
        let root = temporary.path().join("candidate");
        let replacement = temporary.path().join("candidate-replacement");
        fs::create_dir_all(root.join("nested"))?;
        fs::create_dir_all(replacement.join("nested"))?;
        fs::write(root.join("nested/source.pl"), "old\n")?;
        fs::write(replacement.join("nested/source.pl"), "new\n")?;

        let manifest = collect_manifest(
            &root,
            TraversalLimits::default(),
            &ReplacingReader { root: root.clone(), replacement, replaced: Cell::new(false) },
        );
        ensure!(!manifest.complete, "directory replacement was accepted as a complete manifest");
        ensure!(
            manifest.detail.contains("RACE_DETECTED directory identity"),
            "directory replacement lacked an identity race diagnostic: {manifest:?}"
        );
        Ok(())
    }

    #[test]
    fn bounded_file_read_refuses_oversized_administration_content() -> Result<()> {
        let temporary = tempdir()?;
        let file = temporary.path().join("HEAD");
        fs::write(&file, b"12345")?;
        let mut handle = fs::File::open(&file)?;
        let result = read_at_most(&mut handle, 4);
        ensure!(
            matches!(result, Err(ref error) if error.to_string().contains("bounded observation size")),
            "oversized administration content was not refused: {result:?}"
        );
        Ok(())
    }

    #[test]
    fn disabled_hooks_path_is_platform_specific() -> Result<()> {
        #[cfg(windows)]
        ensure!(disabled_hooks_path() == "NUL", "Windows hooks path was not disabled with NUL");
        #[cfg(unix)]
        ensure!(
            disabled_hooks_path() == "/dev/null",
            "Unix hooks path was not disabled with /dev/null"
        );
        Ok(())
    }

    #[cfg(windows)]
    #[test]
    fn bounded_git_capture_terminates_pathological_producer() -> Result<()> {
        let mut command = Command::new("cmd.exe");
        command.args(["/C", "for /L %i in (1,1,2147483647) do @echo x"]);
        let result = run_bounded_process(
            command,
            String::from("pathological producer fixture"),
            Duration::from_secs(1),
        );
        let error =
            result.err().ok_or_else(|| eyre!("pathological producer was not terminated"))?;
        ensure!(
            error.to_string().contains("bounded output")
                || error.to_string().contains("bounded producer time"),
            "pathological producer had an unexpected result: {error}"
        );
        Ok(())
    }

    #[cfg(windows)]
    #[test]
    fn bounded_git_capture_closes_descendant_pipes() -> Result<()> {
        let mut command = Command::new("cmd.exe");
        command.args(["/C", "start \"\" /B cmd.exe /C \"ping -n 30 127.0.0.1 >NUL\""]);
        let started = Instant::now();
        let result = run_bounded_process(
            command,
            String::from("descendant pipe fixture"),
            Duration::from_secs(2),
        );
        ensure!(
            started.elapsed() < Duration::from_secs(5),
            "descendant pipe fixture was not terminated promptly: {result:?}"
        );
        let error = match result {
            Err(error) => error,
            Ok(output) => bail!("descendant pipe fixture was not bounded: {output:?}"),
        };
        ensure!(
            error.to_string().contains("bounded producer time"),
            "descendant pipe fixture had an unexpected result: {error}"
        );
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn bounded_git_capture_closes_unix_descendant_pipes() -> Result<()> {
        let mut command = Command::new("sh");
        command.args(["-c", "(sleep 30) & exit 0"]);
        let started = Instant::now();
        let result = run_bounded_process(
            command,
            String::from("Unix descendant pipe fixture"),
            Duration::from_secs(2),
        );
        ensure!(
            started.elapsed() < Duration::from_secs(5),
            "Unix descendant pipe fixture was not terminated promptly: {result:?}"
        );
        let output = result?;
        ensure!(output.status.success(), "Unix descendant fixture failed: {output:?}");
        Ok(())
    }

    #[test]
    fn same_size_flapping_reader_is_rejected() -> Result<()> {
        struct FlappingReader {
            calls: Cell<usize>,
        }
        impl StableFileReader for FlappingReader {
            fn read_twice(&self, _path: &Path, _max_bytes: u64) -> io::Result<(Vec<u8>, Vec<u8>)> {
                let call = self.calls.get();
                self.calls.set(call.saturating_add(1));
                if call.is_multiple_of(2) {
                    Ok((b"AAAA".to_vec(), b"BBBB".to_vec()))
                } else {
                    Ok((b"BBBB".to_vec(), b"AAAA".to_vec()))
                }
            }
        }

        let temporary = tempdir()?;
        let file = temporary.path().join("same-size.txt");
        let mut handle = fs::File::create(&file)?;
        handle.write_all(b"AAAA")?;
        let result =
            read_stable_file(&file, &FlappingReader { calls: Cell::new(0) }, MAX_ADMIN_FILE_BYTES);
        assert_same_size_change_rejected(result, "same-size byte replacement")?;
        Ok(())
    }

    #[test]
    fn same_size_identity_replacement_is_rejected() -> Result<()> {
        struct IdentityReplacingReader {
            replacement: PathBuf,
            replaced: Cell<bool>,
        }

        impl StableFileReader for IdentityReplacingReader {
            fn read_twice(&self, path: &Path, _max_bytes: u64) -> io::Result<(Vec<u8>, Vec<u8>)> {
                if !self.replaced.replace(true) {
                    fs::remove_file(path)?;
                    fs::rename(&self.replacement, path)?;
                }
                let bytes = fs::read(path)?;
                Ok((bytes.clone(), bytes))
            }
        }

        let temporary = tempdir()?;
        let file = temporary.path().join("same-size.txt");
        let replacement = temporary.path().join("replacement.txt");
        fs::write(&file, b"AAAA")?;
        fs::write(&replacement, b"BBBB")?;
        let result = read_stable_file(
            &file,
            &IdentityReplacingReader { replacement, replaced: Cell::new(false) },
            MAX_ADMIN_FILE_BYTES,
        );
        assert_same_size_change_rejected(result, "same-size identity replacement")?;
        Ok(())
    }

    #[cfg(windows)]
    #[test]
    fn extended_and_normal_admin_paths_share_namespace() -> Result<()> {
        let temporary = tempdir()?;
        let common_dir = temporary.path().join("repository").join(".git");
        let administrative = common_dir.join("worktrees").join("lost");
        let extended_common = PathBuf::from(format!(r"\\?\{}", common_dir.display()));
        ensure!(
            is_in_admin_namespace(&extended_common, &administrative),
            "extended repository common dir was not matched with normal admin path"
        );
        Ok(())
    }

    #[cfg(not(windows))]
    #[test]
    fn case_sensitive_platform_keys_do_not_fold_case() -> Result<()> {
        ensure!(
            platform_path_key(Path::new("Foo")) != platform_path_key(Path::new("foo")),
            "case-sensitive identity was folded"
        );
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn unix_path_keys_preserve_backslashes_and_non_utf8_bytes() -> Result<()> {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;

        let literal_backslash = PathBuf::from(OsString::from_vec(b"dir\\name".to_vec()));
        let separator = PathBuf::from("dir/name");
        let non_utf8 = PathBuf::from(OsString::from_vec(vec![b'x', 0xff]));
        let replacement = PathBuf::from("x?");
        ensure!(
            platform_path_key(&literal_backslash) != platform_path_key(&separator),
            "Unix literal backslash collided with a path separator"
        );
        ensure!(
            platform_path_key(&non_utf8) != platform_path_key(&replacement),
            "non-UTF-8 path identity was lossy"
        );
        Ok(())
    }
}
