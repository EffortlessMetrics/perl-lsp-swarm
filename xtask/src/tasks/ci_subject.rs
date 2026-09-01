//! Immutable GitHub-event subject authority for candidate-bound CI decisions (#8042).
//!
//! The receipt produced here contains only bounded semantic identity. Consumers
//! re-resolve the exact commit pair and verify the changed-input digest; they
//! never consult a mutable branch name after the event has been captured.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use color_eyre::eyre::{Context, Result, bail, eyre};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::tasks::change_set::{self, ArtifactIdentity, DiffMode};

const SCHEMA_VERSION: &str = "ci-subject.v1";
const PRODUCER: &str = "cargo-xtask-ci-subject";
const FETCH_TIMEOUT: Duration = Duration::from_secs(30);
const ZERO_SHA: &str = "0000000000000000000000000000000000000000";
// At most 256 scalar values keeps even fully JSON-escaped detail within the
// receipt's 2 KiB bound.
const MAX_FAILURE_DETAIL_CHARS: usize = 256;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CiEventKind {
    PullRequest,
    Push,
    MergeGroup,
    WorkflowDispatch,
    Explicit,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SubjectResolutionSource {
    GithubEvent,
    ExplicitInput,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SubjectDiffMode {
    MergeBase,
    Direct,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SubjectStatus {
    Resolved,
    NotProven,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SubjectErrorCode {
    MissingInput,
    MalformedSha,
    ZeroSha,
    RepositoryMismatch,
    ObjectUnavailable,
    NonCommitObject,
    DiffUnavailable,
    ContradictoryEmptyDiff,
    ReceiptInvalid,
    CheckoutMismatch,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CiSubjectReceipt {
    pub schema_version: String,
    pub producer: String,
    pub status: SubjectStatus,
    pub repository: String,
    pub event_kind: CiEventKind,
    pub resolution_source: SubjectResolutionSource,
    pub diff_mode: SubjectDiffMode,
    pub base_sha: String,
    pub head_sha: String,
    pub base_tree: String,
    pub head_tree: String,
    pub diff_base_sha: String,
    pub diff_base_tree: String,
    pub changed_file_count: usize,
    pub changed_input_digest: String,
    pub subject_digest: String,
    pub error_code: Option<SubjectErrorCode>,
}

#[derive(Debug, Serialize)]
struct CiSubjectFailureReceipt {
    schema_version: &'static str,
    producer: &'static str,
    status: SubjectStatus,
    error_code: SubjectErrorCode,
    detail: String,
}

#[derive(Debug)]
pub struct ResolvedCiSubject {
    pub receipt: CiSubjectReceipt,
    pub changed_paths: Vec<String>,
}

pub struct CiSubjectConfig {
    pub event_name: Option<String>,
    pub event_path: Option<PathBuf>,
    pub repository: Option<String>,
    pub github_sha: Option<String>,
    pub base_sha: Option<String>,
    pub head_sha: Option<String>,
    pub receipt: PathBuf,
    pub root: Option<PathBuf>,
}

#[derive(Debug)]
pub struct SubjectResolutionError {
    pub code: SubjectErrorCode,
    pub message: String,
}

impl std::fmt::Display for SubjectResolutionError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{:?}: {}", self.code, self.message)
    }
}

impl std::error::Error for SubjectResolutionError {}

type SubjectResult<T> = std::result::Result<T, SubjectResolutionError>;

#[derive(Debug, Clone)]
pub(crate) struct SubjectInput {
    pub(crate) repository: String,
    pub(crate) event_kind: CiEventKind,
    pub(crate) resolution_source: SubjectResolutionSource,
    pub(crate) diff_mode: SubjectDiffMode,
    pub(crate) base_sha: String,
    pub(crate) head_sha: String,
}

pub fn run(config: CiSubjectConfig) -> Result<()> {
    let root = match config.root.as_ref() {
        Some(root) => root.clone(),
        None => crate::utils::project_root()?,
    };
    let resolution = (|| -> SubjectResult<ResolvedCiSubject> {
        let input = input_from_config(&config)?;
        let local_repository = repository_identity(&root)?;
        ensure_repository(&input.repository, &local_repository)?;
        let subject = resolve_input(&root, input)?;
        ensure_checkout_head(&root, &subject.receipt.head_sha)?;
        Ok(subject)
    })();
    match resolution {
        Ok(subject) => {
            write_receipt(&config.receipt, &subject.receipt)?;
            println!("ci subject: RESOLVED ({})", config.receipt.display());
            Ok(())
        }
        Err(error) => {
            write_failure_receipt(&config.receipt, &error)?;
            Err(eyre!(error))
        }
    }
}

pub fn load_and_resolve(path: &Path, root: &Path) -> Result<ResolvedCiSubject> {
    let bytes = fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
    let receipt: CiSubjectReceipt = serde_json::from_slice(&bytes)
        .with_context(|| format!("failed to parse {}", path.display()))?;
    validate_receipt_shape(&receipt).map_err(|error| eyre!(error))?;

    let local_repository = repository_identity(root).map_err(|error| eyre!(error))?;
    if receipt.repository != local_repository {
        bail!(
            "ci subject repository mismatch: receipt={}, checkout={}",
            receipt.repository,
            local_repository
        );
    }
    let input = SubjectInput {
        repository: receipt.repository.clone(),
        event_kind: receipt.event_kind,
        resolution_source: receipt.resolution_source,
        diff_mode: receipt.diff_mode,
        base_sha: receipt.base_sha.clone(),
        head_sha: receipt.head_sha.clone(),
    };
    let resolved = resolve_input(root, input).map_err(|error| eyre!(error))?;
    if resolved.receipt != receipt {
        bail!("ci subject receipt does not match the exact objects and changed inputs");
    }
    ensure_checkout_head(root, &receipt.head_sha).map_err(|error| eyre!(error))?;
    Ok(resolved)
}

pub(crate) fn input_from_config(config: &CiSubjectConfig) -> SubjectResult<SubjectInput> {
    let event_name = config
        .event_name
        .clone()
        .or_else(|| std::env::var("GITHUB_EVENT_NAME").ok())
        .unwrap_or_else(|| "explicit".to_string());
    let repository =
        config.repository.clone().or_else(|| std::env::var("GITHUB_REPOSITORY").ok()).ok_or_else(
            || subject_error(SubjectErrorCode::MissingInput, "repository is required"),
        )?;

    match event_name.as_str() {
        "pull_request" | "push" | "merge_group" => {
            let event_path = config
                .event_path
                .clone()
                .or_else(|| std::env::var_os("GITHUB_EVENT_PATH").map(PathBuf::from))
                .ok_or_else(|| {
                    subject_error(SubjectErrorCode::MissingInput, "GitHub event path is required")
                })?;
            let event_bytes = fs::read(&event_path).map_err(|error| {
                subject_error(
                    SubjectErrorCode::MissingInput,
                    format!("could not read {}: {error}", event_path.display()),
                )
            })?;
            let event: Value = serde_json::from_slice(&event_bytes).map_err(|error| {
                subject_error(
                    SubjectErrorCode::MissingInput,
                    format!("invalid event JSON: {error}"),
                )
            })?;
            let github_sha = config.github_sha.clone().or_else(|| std::env::var("GITHUB_SHA").ok());
            input_from_event(&event_name, &repository, github_sha.as_deref(), &event)
        }
        "workflow_dispatch" => {
            let input = explicit_input(
                repository,
                CiEventKind::WorkflowDispatch,
                config.base_sha.clone(),
                config.head_sha.clone(),
            )?;
            if let Some(github_sha) =
                config.github_sha.clone().or_else(|| std::env::var("GITHUB_SHA").ok())
                && github_sha != input.head_sha
            {
                return Err(subject_error(
                    SubjectErrorCode::CheckoutMismatch,
                    format!(
                        "workflow-dispatch head {} contradicts GITHUB_SHA {github_sha}",
                        input.head_sha
                    ),
                ));
            }
            Ok(input)
        }
        "explicit" => explicit_input(
            repository,
            CiEventKind::Explicit,
            config.base_sha.clone(),
            config.head_sha.clone(),
        ),
        other => Err(subject_error(
            SubjectErrorCode::MissingInput,
            format!("unsupported event kind {other:?}"),
        )),
    }
}

fn input_from_event(
    event_name: &str,
    expected_repository: &str,
    github_sha: Option<&str>,
    event: &Value,
) -> SubjectResult<SubjectInput> {
    let event_repository = json_string(event, &["repository", "full_name"])?;
    ensure_repository(expected_repository, &event_repository)?;
    match event_name {
        "pull_request" => {
            let base_repository =
                json_string(event, &["pull_request", "base", "repo", "full_name"])?;
            let head_repository =
                json_string(event, &["pull_request", "head", "repo", "full_name"])?;
            ensure_repository(expected_repository, &base_repository)?;
            ensure_repository(expected_repository, &head_repository)?;
            Ok(SubjectInput {
                repository: expected_repository.to_string(),
                event_kind: CiEventKind::PullRequest,
                resolution_source: SubjectResolutionSource::GithubEvent,
                diff_mode: SubjectDiffMode::MergeBase,
                base_sha: json_string(event, &["pull_request", "base", "sha"])?,
                head_sha: json_string(event, &["pull_request", "head", "sha"])?,
            })
        }
        "push" => {
            let head_sha = github_sha
                .map(str::to_string)
                .or_else(|| json_optional_string(event, &["after"]))
                .ok_or_else(|| {
                    subject_error(SubjectErrorCode::MissingInput, "push head SHA is required")
                })?;
            if let Some(after) = json_optional_string(event, &["after"])
                && after != head_sha
            {
                return Err(subject_error(
                    SubjectErrorCode::RepositoryMismatch,
                    format!("GITHUB_SHA {head_sha} contradicts push after {after}"),
                ));
            }
            Ok(SubjectInput {
                repository: expected_repository.to_string(),
                event_kind: CiEventKind::Push,
                resolution_source: SubjectResolutionSource::GithubEvent,
                diff_mode: SubjectDiffMode::Direct,
                base_sha: json_string(event, &["before"])?,
                head_sha,
            })
        }
        "merge_group" => Ok(SubjectInput {
            repository: expected_repository.to_string(),
            event_kind: CiEventKind::MergeGroup,
            resolution_source: SubjectResolutionSource::GithubEvent,
            diff_mode: SubjectDiffMode::Direct,
            base_sha: json_string(event, &["merge_group", "base_sha"])?,
            head_sha: json_string(event, &["merge_group", "head_sha"])?,
        }),
        _ => Err(subject_error(SubjectErrorCode::MissingInput, "unsupported GitHub event")),
    }
}

fn explicit_input(
    repository: String,
    event_kind: CiEventKind,
    base_sha: Option<String>,
    head_sha: Option<String>,
) -> SubjectResult<SubjectInput> {
    Ok(SubjectInput {
        repository,
        event_kind,
        resolution_source: SubjectResolutionSource::ExplicitInput,
        diff_mode: SubjectDiffMode::Direct,
        base_sha: base_sha.ok_or_else(|| {
            subject_error(SubjectErrorCode::MissingInput, "explicit base SHA is required")
        })?,
        head_sha: head_sha.ok_or_else(|| {
            subject_error(SubjectErrorCode::MissingInput, "explicit head SHA is required")
        })?,
    })
}

fn resolve_input(root: &Path, input: SubjectInput) -> SubjectResult<ResolvedCiSubject> {
    validate_sha(&input.base_sha, "base")?;
    validate_sha(&input.head_sha, "head")?;
    ensure_commit(root, &input.base_sha)?;
    ensure_commit(root, &input.head_sha)?;
    let base_tree = tree_sha(root, &input.base_sha)?;
    let head_tree = tree_sha(root, &input.head_sha)?;
    let diff_base_sha = match input.diff_mode {
        SubjectDiffMode::MergeBase => strict_merge_base(root, &input.base_sha, &input.head_sha)?,
        SubjectDiffMode::Direct => input.base_sha.clone(),
    };
    let diff_base_tree = tree_sha(root, &diff_base_sha)?;
    let resolved = change_set::resolve_change_set_with_mode(
        ArtifactIdentity::CommitRange { base: diff_base_sha.clone(), head: input.head_sha.clone() },
        root,
        DiffMode::DirectTwoDot,
    )
    .map_err(|error| subject_error(SubjectErrorCode::DiffUnavailable, error.to_string()))?;
    let changed_paths = canonical_paths(resolved.changed_paths);
    validate_changed_inputs(&diff_base_tree, &head_tree, &changed_paths)?;
    let changed_input_digest = changed_input_digest(&changed_paths);
    let mut receipt = CiSubjectReceipt {
        schema_version: SCHEMA_VERSION.to_string(),
        producer: PRODUCER.to_string(),
        status: SubjectStatus::Resolved,
        repository: input.repository,
        event_kind: input.event_kind,
        resolution_source: input.resolution_source,
        diff_mode: input.diff_mode,
        base_sha: input.base_sha,
        head_sha: input.head_sha,
        base_tree,
        head_tree,
        diff_base_sha,
        diff_base_tree,
        changed_file_count: changed_paths.len(),
        changed_input_digest,
        subject_digest: String::new(),
        error_code: None,
    };
    receipt.subject_digest = subject_digest(&receipt);
    Ok(ResolvedCiSubject { receipt, changed_paths })
}

fn validate_changed_inputs(
    base_tree: &str,
    head_tree: &str,
    changed_paths: &[String],
) -> SubjectResult<()> {
    if base_tree != head_tree && changed_paths.is_empty() {
        return Err(subject_error(
            SubjectErrorCode::ContradictoryEmptyDiff,
            "different base/head trees produced an empty changed-input set",
        ));
    }
    Ok(())
}

fn validate_receipt_shape(receipt: &CiSubjectReceipt) -> SubjectResult<()> {
    if receipt.schema_version != SCHEMA_VERSION
        || receipt.producer != PRODUCER
        || receipt.status != SubjectStatus::Resolved
        || receipt.error_code.is_some()
    {
        return Err(subject_error(
            SubjectErrorCode::ReceiptInvalid,
            "unsupported or failed receipt",
        ));
    }
    validate_sha(&receipt.base_sha, "base")?;
    validate_sha(&receipt.head_sha, "head")?;
    validate_sha(&receipt.base_tree, "base tree")?;
    validate_sha(&receipt.head_tree, "head tree")?;
    validate_sha(&receipt.diff_base_sha, "diff base")?;
    validate_sha(&receipt.diff_base_tree, "diff base tree")?;
    if receipt.subject_digest != subject_digest(receipt) {
        return Err(subject_error(SubjectErrorCode::ReceiptInvalid, "subject digest mismatch"));
    }
    Ok(())
}

fn ensure_checkout_head(root: &Path, expected: &str) -> SubjectResult<()> {
    let actual = git_stdout(root, &["rev-parse", "--verify", "HEAD^{commit}"])?;
    if actual != expected {
        return Err(subject_error(
            SubjectErrorCode::CheckoutMismatch,
            format!("subject head {expected} does not match checkout HEAD {actual}"),
        ));
    }
    Ok(())
}

pub(crate) fn validate_sha(value: &str, label: &str) -> SubjectResult<()> {
    if value == ZERO_SHA {
        return Err(subject_error(SubjectErrorCode::ZeroSha, format!("{label} SHA is all zero")));
    }
    if value.len() != 40 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(subject_error(
            SubjectErrorCode::MalformedSha,
            format!("{label} SHA must be a full 40-character hexadecimal object ID"),
        ));
    }
    Ok(())
}

pub(crate) fn ensure_commit(root: &Path, sha: &str) -> SubjectResult<()> {
    match object_type(root, sha) {
        Ok(kind) if kind == "commit" => return Ok(()),
        Ok(kind) => {
            return Err(subject_error(
                SubjectErrorCode::NonCommitObject,
                format!("object {sha} is {kind}, not commit"),
            ));
        }
        Err(_) => {}
    }
    bounded_fetch(root, sha)?;
    match object_type(root, sha) {
        Ok(kind) if kind == "commit" => Ok(()),
        Ok(kind) => Err(subject_error(
            SubjectErrorCode::NonCommitObject,
            format!("object {sha} is {kind}, not commit"),
        )),
        Err(error) => Err(subject_error(
            SubjectErrorCode::ObjectUnavailable,
            format!("commit {sha} remains unavailable after bounded exact-SHA fetch: {error}"),
        )),
    }
}

fn strict_merge_base(root: &Path, base: &str, head: &str) -> SubjectResult<String> {
    if let Ok(merge_base) = git_stdout(root, &["merge-base", base, head]) {
        validate_sha(&merge_base, "merge base")?;
        return Ok(merge_base);
    }
    bounded_history_fetch(root, base, head).map_err(|error| {
        subject_error(
            SubjectErrorCode::DiffUnavailable,
            format!("could not deepen exact PR history within the bound: {error}"),
        )
    })?;
    let merge_base = git_stdout(root, &["merge-base", base, head]).map_err(|error| {
        subject_error(
            SubjectErrorCode::DiffUnavailable,
            format!("PR merge base remains unavailable after bounded exact-SHA deepening: {error}"),
        )
    })?;
    validate_sha(&merge_base, "merge base")?;
    Ok(merge_base)
}

fn bounded_fetch(root: &Path, sha: &str) -> SubjectResult<()> {
    bounded_git_fetch(
        root,
        &["fetch", "--no-tags", "--no-write-fetch-head", "--depth=1", "origin", sha],
    )
}

fn bounded_history_fetch(root: &Path, base: &str, head: &str) -> SubjectResult<()> {
    bounded_git_fetch(
        root,
        &["fetch", "--no-tags", "--no-write-fetch-head", "--depth=256", "origin", base, head],
    )
}

fn bounded_git_fetch(root: &Path, args: &[&str]) -> SubjectResult<()> {
    let mut child = Command::new("git")
        .args(args)
        .current_dir(root)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| {
            subject_error(
                SubjectErrorCode::ObjectUnavailable,
                format!("could not start fetch: {error}"),
            )
        })?;
    let start = Instant::now();
    loop {
        if let Some(status) = child.try_wait().map_err(|error| {
            subject_error(
                SubjectErrorCode::ObjectUnavailable,
                format!("could not wait for fetch: {error}"),
            )
        })? {
            if status.success() {
                return Ok(());
            }
            return Err(subject_error(
                SubjectErrorCode::ObjectUnavailable,
                format!("exact-SHA fetch exited with {status}"),
            ));
        }
        if start.elapsed() >= FETCH_TIMEOUT {
            let _ = child.kill();
            let _ = child.wait();
            return Err(subject_error(
                SubjectErrorCode::ObjectUnavailable,
                format!("exact-SHA fetch exceeded {} seconds", FETCH_TIMEOUT.as_secs()),
            ));
        }
        thread::sleep(Duration::from_millis(50));
    }
}

fn object_type(root: &Path, sha: &str) -> SubjectResult<String> {
    git_stdout(root, &["cat-file", "-t", sha])
}

fn tree_sha(root: &Path, commit: &str) -> SubjectResult<String> {
    let tree = git_stdout(root, &["rev-parse", "--verify", &format!("{commit}^{{tree}}")])?;
    validate_sha(&tree, "tree")?;
    Ok(tree)
}

pub(crate) fn git_stdout(root: &Path, args: &[&str]) -> SubjectResult<String> {
    let output = Command::new("git").args(args).current_dir(root).output().map_err(|error| {
        subject_error(
            SubjectErrorCode::ObjectUnavailable,
            format!("git {}: {error}", args.join(" ")),
        )
    })?;
    if !output.status.success() {
        return Err(subject_error(
            SubjectErrorCode::ObjectUnavailable,
            format!(
                "git {} failed: {}",
                args.join(" "),
                String::from_utf8_lossy(&output.stderr).trim()
            ),
        ));
    }
    String::from_utf8(output.stdout)
        .map(|value| value.trim().to_string())
        .map_err(|error| subject_error(SubjectErrorCode::ObjectUnavailable, error.to_string()))
}

fn repository_identity(root: &Path) -> SubjectResult<String> {
    let raw = git_stdout(root, &["config", "--get", "remote.origin.url"])?;
    let value = raw.trim_end_matches(".git");
    let repository = value
        .strip_prefix("git@github.com:")
        .or_else(|| value.strip_prefix("https://github.com/"))
        .or_else(|| value.strip_prefix("http://github.com/"))
        .ok_or_else(|| {
            subject_error(
                SubjectErrorCode::RepositoryMismatch,
                format!("unsupported origin repository URL {value:?}"),
            )
        })?;
    if repository.matches('/').count() != 1 || repository.is_empty() {
        return Err(subject_error(
            SubjectErrorCode::RepositoryMismatch,
            format!("origin did not resolve to owner/name: {value:?}"),
        ));
    }
    Ok(repository.to_string())
}

fn ensure_repository(expected: &str, observed: &str) -> SubjectResult<()> {
    if expected != observed {
        return Err(subject_error(
            SubjectErrorCode::RepositoryMismatch,
            format!("expected repository {expected}, observed {observed}"),
        ));
    }
    Ok(())
}

fn json_string(value: &Value, path: &[&str]) -> SubjectResult<String> {
    json_optional_string(value, path).ok_or_else(|| {
        subject_error(
            SubjectErrorCode::MissingInput,
            format!("event field {} is required", path.join(".")),
        )
    })
}

fn json_optional_string(value: &Value, path: &[&str]) -> Option<String> {
    let mut current = value;
    for segment in path {
        current = current.get(*segment)?;
    }
    current.as_str().map(str::to_string)
}

fn canonical_paths(paths: Vec<String>) -> Vec<String> {
    let mut paths = paths;
    paths.sort();
    paths.dedup();
    paths
}

fn changed_input_digest(paths: &[String]) -> String {
    let mut hasher = Sha256::new();
    for path in paths {
        hasher.update(path.as_bytes());
        hasher.update([0]);
    }
    hex_digest(hasher.finalize())
}

fn subject_digest(receipt: &CiSubjectReceipt) -> String {
    let mut hasher = Sha256::new();
    for value in [
        receipt.schema_version.as_str(),
        receipt.producer.as_str(),
        receipt.repository.as_str(),
        event_kind_name(receipt.event_kind),
        source_name(receipt.resolution_source),
        diff_mode_name(receipt.diff_mode),
        receipt.base_sha.as_str(),
        receipt.head_sha.as_str(),
        receipt.base_tree.as_str(),
        receipt.head_tree.as_str(),
        receipt.diff_base_sha.as_str(),
        receipt.diff_base_tree.as_str(),
        receipt.changed_input_digest.as_str(),
    ] {
        hasher.update(value.as_bytes());
        hasher.update([0]);
    }
    hasher.update(receipt.changed_file_count.to_string().as_bytes());
    hex_digest(hasher.finalize())
}

fn event_kind_name(kind: CiEventKind) -> &'static str {
    match kind {
        CiEventKind::PullRequest => "pull_request",
        CiEventKind::Push => "push",
        CiEventKind::MergeGroup => "merge_group",
        CiEventKind::WorkflowDispatch => "workflow_dispatch",
        CiEventKind::Explicit => "explicit",
    }
}

fn source_name(source: SubjectResolutionSource) -> &'static str {
    match source {
        SubjectResolutionSource::GithubEvent => "github_event",
        SubjectResolutionSource::ExplicitInput => "explicit_input",
    }
}

fn diff_mode_name(mode: SubjectDiffMode) -> &'static str {
    match mode {
        SubjectDiffMode::MergeBase => "merge_base",
        SubjectDiffMode::Direct => "direct",
    }
}

fn hex_digest(bytes: impl AsRef<[u8]>) -> String {
    bytes.as_ref().iter().map(|byte| format!("{byte:02x}")).collect()
}

fn write_receipt(path: &Path, receipt: &CiSubjectReceipt) -> Result<()> {
    if let Some(parent) = path.parent().filter(|parent| !parent.as_os_str().is_empty()) {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let bytes = serde_json::to_vec_pretty(receipt).context("failed to serialize CI subject")?;
    fs::write(path, bytes).with_context(|| format!("failed to write {}", path.display()))
}

fn write_failure_receipt(path: &Path, error: &SubjectResolutionError) -> Result<()> {
    if let Some(parent) = path.parent().filter(|parent| !parent.as_os_str().is_empty()) {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let receipt = CiSubjectFailureReceipt {
        schema_version: SCHEMA_VERSION,
        producer: PRODUCER,
        status: SubjectStatus::NotProven,
        error_code: error.code,
        detail: error.message.chars().take(MAX_FAILURE_DETAIL_CHARS).collect(),
    };
    let bytes = serde_json::to_vec_pretty(&receipt)
        .context("failed to serialize failed CI subject receipt")?;
    fs::write(path, bytes).with_context(|| format!("failed to write {}", path.display()))
}

pub(crate) fn subject_error(
    code: SubjectErrorCode,
    message: impl Into<String>,
) -> SubjectResolutionError {
    SubjectResolutionError { code, message: message.into() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use color_eyre::eyre::{Result, bail, ensure};
    use serde_json::json;

    #[test]
    fn rejects_branch_names_before_git_resolution() -> Result<()> {
        let Err(error) = validate_sha("origin/main", "base") else {
            bail!("branch names must fail before Git resolution");
        };
        ensure!(error.code == SubjectErrorCode::MalformedSha);
        Ok(())
    }

    #[test]
    fn rejects_zero_push_base() -> Result<()> {
        let Err(error) = validate_sha(ZERO_SHA, "base") else {
            bail!("zero base must fail");
        };
        ensure!(error.code == SubjectErrorCode::ZeroSha);
        Ok(())
    }

    #[test]
    fn workflow_dispatch_head_must_match_trusted_github_sha_before_git() -> Result<()> {
        let head = "b".repeat(40);
        let config = CiSubjectConfig {
            event_name: Some("workflow_dispatch".to_string()),
            event_path: None,
            repository: Some("owner/repo".to_string()),
            github_sha: Some(head.clone()),
            base_sha: Some("a".repeat(40)),
            head_sha: Some("candidate-branch".to_string()),
            receipt: PathBuf::from("unused.json"),
            root: None,
        };
        let Err(error) = input_from_config(&config) else {
            bail!("dispatch head that contradicts GITHUB_SHA must fail before Git");
        };
        ensure!(error.code == SubjectErrorCode::CheckoutMismatch);

        let accepted =
            input_from_config(&CiSubjectConfig { head_sha: Some(head.clone()), ..config })?;
        ensure!(accepted.head_sha == head);
        Ok(())
    }

    #[test]
    fn changed_input_digest_is_order_independent() -> Result<()> {
        let left = changed_input_digest(&canonical_paths(vec!["b.rs".into(), "a.rs".into()]));
        let right = changed_input_digest(&canonical_paths(vec!["a.rs".into(), "b.rs".into()]));
        ensure!(left == right);
        Ok(())
    }

    #[test]
    fn different_trees_cannot_collapse_to_an_empty_input_set() -> Result<()> {
        let Err(error) = validate_changed_inputs(&"a".repeat(40), &"b".repeat(40), &[]) else {
            bail!("different trees with no changed input must fail closed");
        };
        ensure!(error.code == SubjectErrorCode::ContradictoryEmptyDiff);
        Ok(())
    }

    #[test]
    fn local_blob_and_unavailable_commit_receive_distinct_typed_refusals() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let repo = tmp.path().join("repo");
        fs::create_dir_all(&repo)?;
        git_stdout(&repo, &["init"])?;
        fs::write(repo.join("blob.txt"), "not a commit\n")?;
        let blob = git_stdout(&repo, &["hash-object", "-w", "blob.txt"])?;
        let Err(blob_error) = ensure_commit(&repo, &blob) else {
            bail!("a blob object must not be accepted as a commit");
        };
        ensure!(blob_error.code == SubjectErrorCode::NonCommitObject);

        let missing_remote = tmp.path().join("missing.git").to_string_lossy().to_string();
        git_stdout(&repo, &["remote", "add", "origin", &missing_remote])?;
        let unavailable = "d".repeat(40);
        let Err(unavailable_error) = ensure_commit(&repo, &unavailable) else {
            bail!("an unavailable exact commit must fail closed");
        };
        ensure!(unavailable_error.code == SubjectErrorCode::ObjectUnavailable);
        Ok(())
    }

    #[test]
    fn failure_receipt_detail_is_bounded() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let receipt = tmp.path().join("failure.json");
        let error = subject_error(SubjectErrorCode::MalformedSha, "x".repeat(16_384));
        write_failure_receipt(&receipt, &error)?;
        let bytes = fs::read(&receipt)?;
        ensure!(bytes.len() < 1024, "failure receipt exceeded its bounded schema");
        let value: Value = serde_json::from_slice(&bytes)?;
        ensure!(
            value["detail"].as_str().is_some_and(|detail| detail.chars().count() == 256),
            "failure detail was not truncated to the semantic bound"
        );
        Ok(())
    }

    #[test]
    fn resolved_receipt_size_is_independent_of_large_changed_input_corpus() -> Result<()> {
        let paths: Vec<String> =
            (0..100_000).map(|index| format!("crates/demo/src/generated/{index}.rs")).collect();
        let mut receipt = CiSubjectReceipt {
            schema_version: SCHEMA_VERSION.to_string(),
            producer: PRODUCER.to_string(),
            status: SubjectStatus::Resolved,
            repository: "owner/repo".to_string(),
            event_kind: CiEventKind::Push,
            resolution_source: SubjectResolutionSource::GithubEvent,
            diff_mode: SubjectDiffMode::Direct,
            base_sha: "a".repeat(40),
            head_sha: "b".repeat(40),
            base_tree: "c".repeat(40),
            head_tree: "d".repeat(40),
            diff_base_sha: "a".repeat(40),
            diff_base_tree: "c".repeat(40),
            changed_file_count: paths.len(),
            changed_input_digest: changed_input_digest(&paths),
            subject_digest: String::new(),
            error_code: None,
        };
        receipt.subject_digest = subject_digest(&receipt);
        ensure!(
            receipt.changed_input_digest
                == "16adfbd99a3a87bce90dd3da112bb07da467f19e76996dbdc672788cbee83d44"
        );
        ensure!(
            receipt.subject_digest
                == "4fe1debc2ac5123e83f393b8f2e6e570ee1ca7c0b80164ecd10ef1a54f30dbb5"
        );
        let bytes = serde_json::to_vec_pretty(&receipt)?;
        ensure!(bytes.len() < 2048, "resolved receipt grew with the changed-input corpus");
        ensure!(!std::str::from_utf8(&bytes)?.contains("generated/99999.rs"));
        Ok(())
    }

    #[test]
    fn event_mapping_uses_exact_pr_push_and_merge_group_pairs() -> Result<()> {
        let base = "a".repeat(40);
        let head = "b".repeat(40);
        let repository = "owner/repo";
        let pr = json!({
            "repository": {"full_name": repository},
            "pull_request": {
                "base": {"sha": base, "repo": {"full_name": repository}},
                "head": {"sha": head, "repo": {"full_name": repository}}
            }
        });
        let pr_input = input_from_event("pull_request", repository, None, &pr)?;
        ensure!(pr_input.base_sha == base);
        ensure!(pr_input.head_sha == head);
        ensure!(pr_input.diff_mode == SubjectDiffMode::MergeBase);

        let push = json!({
            "repository": {"full_name": repository},
            "before": base,
            "after": head
        });
        let push_input = input_from_event("push", repository, Some(&head), &push)?;
        ensure!(push_input.base_sha == base);
        ensure!(push_input.head_sha == head);
        ensure!(push_input.diff_mode == SubjectDiffMode::Direct);

        let merge_group = json!({
            "repository": {"full_name": repository},
            "merge_group": {"base_sha": base, "head_sha": head}
        });
        let merge_input = input_from_event("merge_group", repository, None, &merge_group)?;
        ensure!(merge_input.base_sha == base);
        ensure!(merge_input.head_sha == head);
        ensure!(merge_input.diff_mode == SubjectDiffMode::Direct);
        Ok(())
    }

    #[test]
    fn fork_pr_and_push_sha_contradiction_fail_closed() -> Result<()> {
        let base = "a".repeat(40);
        let head = "b".repeat(40);
        let repository = "owner/repo";
        let fork = json!({
            "repository": {"full_name": repository},
            "pull_request": {
                "base": {"sha": base, "repo": {"full_name": repository}},
                "head": {"sha": head, "repo": {"full_name": "fork/repo"}}
            }
        });
        let Err(fork_error) = input_from_event("pull_request", repository, None, &fork) else {
            bail!("cross-repository PR must fail closed");
        };
        ensure!(fork_error.code == SubjectErrorCode::RepositoryMismatch);

        let push = json!({
            "repository": {"full_name": repository},
            "before": base,
            "after": head
        });
        let other_head = "c".repeat(40);
        let Err(push_error) = input_from_event("push", repository, Some(&other_head), &push) else {
            bail!("contradictory push head must fail closed");
        };
        ensure!(push_error.code == SubjectErrorCode::RepositoryMismatch);
        Ok(())
    }
}
