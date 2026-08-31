//! Content-bound Vim + vim-lsp host-toolchain acquisition and identity
//! substrate (`vim_vim_lsp_host_toolchain.v1`, #11372).
//!
//! This module owns exactly one claim: local/CI tests reproducibly obtain and
//! prove one exact Vim + vim-lsp test instrument. It acquires the pinned Vim
//! release bytes and the #11369-pinned vim-lsp subject, verifies both against
//! independent digests, caches them under exact-identity keys, revalidates
//! every restore, and hands off the resolved roles for consumption by the
//! hermetic host runner (#10944).
//!
//! Claim ceiling (#11372): test-instrument acquisition, verification, cache
//! identity, and role handoff only. It never chooses support ranges
//! (#10966), launches semantic journeys (#10944), creates actual-editor
//! receipts (#7777/#10527), or manages user installation. Provisioned bytes
//! prove no Vim/vim-lsp/perllsp behavior.
//!
//! Identity law:
//!
//! - the vim-lsp subject is consumed by reference from the governed
//!   `.ci/editor-clients/vim-vim-lsp-subject.v1.json` authority (#11369);
//!   its bytes are never copied or re-derived here;
//! - the Vim subject is a pinned immutable release archive plus the pinned
//!   inner console executable digest; upstream publishes no checksum
//!   sidecars, so the recorded archive/executable digests are observed once
//!   in this reviewed change and recomputed on every later use;
//! - durable identity is content/build metadata only: manifests carry zero
//!   absolute machine paths (enforced by a leak scan at write and verify)
//!   and no timestamps;
//! - caching is an optimization over exact identities: the cache key covers
//!   every load-bearing input, a hit is never proof, and any drift deletes
//!   and rebuilds instead of silently satisfying the role.

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt::Write as _;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use walkdir::WalkDir;

// The shared host-runner substrate is included exactly once per crate (in
// `vim_host_run`, #10944); this module consumes that single instance rather
// than loading the file a second time.
use crate::vim_host_run::vim_host_runner::{
    VimLspCheckoutIdentity, VimLspEntryFile, VimLspExpectedContent, VimLspSubjectManifest,
    VimLspTreeDigest, VimLspUpstream, bytes_sha256, file_sha256, verify_vim_lsp_checkout,
};
use crate::vim_host_run::{REQUIRED_VIM_FEATURES, verify_vim_features};

// ---------------------------------------------------------------------------
// Governed pins (reviewed constants; bumping any of these changes identity)
// ---------------------------------------------------------------------------

/// The governed vim-lsp subject authority consumed by reference (#11369).
pub const SUBJECT_AUTHORITY_REPO_PATH: &str = ".ci/editor-clients/vim-vim-lsp-subject.v1.json";

/// Pinned Vim Windows release subject for the initial `vim_vim_lsp_host`
/// role. Upstream (vim/vim-win32-installer) publishes release assets without
/// checksum sidecars, so the digests below are the reviewed observation of
/// the exact immutable tag assets; every later use recomputes them.
pub const VIM_RELEASE_TAG: &str = "v9.2.0995";
pub const VIM_ARCHIVE_URL: &str =
    "https://github.com/vim/vim-win32-installer/releases/download/v9.2.0995/gvim_9.2.0995_x64.zip";
pub const VIM_ARCHIVE_SHA256: &str =
    "sha256:a33b4e1f25ea1a1d976250750d0ef2ef29f6093a3d072153817f91780fad05a1";
/// Inner console executable inside the archive whose bytes are the executed
/// subject identity.
pub const VIM_ARCHIVE_ENTRY: &str = "vim/vim92/vim.exe";
/// Archive subtree extracted into the cache (portable runtime files needed
/// beside the executable at launch time).
pub const VIM_ARCHIVE_RUNTIME_SUBTREE: &str = "vim/vim92/";
/// Executable digest observed for `v9.2.0995` `vim/vim92/vim.exe`.
pub const VIM_EXECUTABLE_SHA256: &str =
    "sha256:649b65454ce6cc1e2bc3e814b8e2498d5f89bbad8ab066a85cfbb07f33a0ce13";

/// Transport features vim-lsp requires from its host at runtime — the same
/// law the #10944 runner enforces before launch (`verify_vim_features`,
/// imported above). This substrate refuses to provision an instrument that
/// cannot run the pinned client.
pub const SCHEMA_VERSION: &str = "vim_vim_lsp_host_toolchain.v1";
pub const INSTRUMENT_VERSION: u32 = 1;
pub const HOST_ROLE: &str = "vim_vim_lsp_host";
pub const MANIFEST_FILE_NAME: &str = "vim_vim_lsp_host_toolchain.v1.json";
const LOAD_MODE_CALLER_PINNED_CHECKOUT: &str = "caller_pinned_git_checkout_via_runtimepath_prepend";
const VIM_LSP_UPSTREAM_URL: &str = "https://github.com/prabirshrestha/vim-lsp.git";

/// Bounded probe limits: an explicit or PATH `vim` that hangs or streams
/// output indefinitely must never hold provisioning hostage.
const VERSION_PROBE_TIMEOUT: Duration = Duration::from_secs(30);
/// Deadline applied to every acquisition subprocess (git init/remote/fetch/
/// checkout). A wedged transport must time out, not hang the provisioner.
const GIT_ACQUISITION_TIMEOUT: Duration = Duration::from_mins(5);
/// curl self-bounds with `--max-time` plus stall detection (`--speed-limit`
/// bytes over `--speed-time` seconds); the values leave room for slow CI
/// mirrors while guaranteeing termination.
const CURL_MAX_TIME_SECS: u64 = 900;
const CURL_SPEED_LIMIT_BYTES: u64 = 1024;
const CURL_SPEED_TIME_SECS: u64 = 60;
/// Output cap shared by every bounded subprocess probe.
const SUBPROCESS_OUTPUT_CAP_BYTES: usize = 512 * 1024;
/// Deflate-bomb guard for archive extraction.
const EXTRACTION_TOTAL_CAP_BYTES: u64 = 1024 * 1024 * 1024;
const EXTRACTION_MAX_ENTRIES: usize = 20_000;

// ---------------------------------------------------------------------------
// Typed instrument failures
// ---------------------------------------------------------------------------

/// Failure taxonomy for acquisition/verification. Every failure of this
/// substrate is an instrument outcome (`instrument_failed(<class>)`), never
/// product/client/host behavior and never a skipped green.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstrumentFailureClass {
    /// A governed authority manifest is missing, unreadable, or violates its
    /// own schema.
    AuthorityUnreadable,
    /// Download, network, git transport, or extraction input failed before
    /// any identity could be judged.
    AcquisitionUnavailable,
    /// Acquired or restored bytes/probe output disagree with the pinned or
    /// recorded identity.
    IdentityMismatch,
    /// An acquired artifact does not carry the pinned subject shape (for
    /// example the archive lacks the pinned entry path).
    SubjectUnresolved,
}

impl InstrumentFailureClass {
    pub fn token(self) -> &'static str {
        match self {
            InstrumentFailureClass::AuthorityUnreadable => "authority_unreadable",
            InstrumentFailureClass::AcquisitionUnavailable => "acquisition_unavailable",
            InstrumentFailureClass::IdentityMismatch => "identity_mismatch",
            InstrumentFailureClass::SubjectUnresolved => "subject_unresolved",
        }
    }
}

/// A typed instrument failure whose `Display` renders the stable
/// `instrument_failed(<class>)` prefix CI branches on.
#[derive(Debug)]
pub struct InstrumentFailure {
    pub class: InstrumentFailureClass,
    pub detail: String,
}

impl InstrumentFailure {
    pub fn new(class: InstrumentFailureClass, detail: impl Into<String>) -> Self {
        Self { class, detail: detail.into() }
    }

    fn wrap(class: InstrumentFailureClass, context: &str, error: impl std::fmt::Display) -> Self {
        Self::new(class, format!("{context}: {error:#}"))
    }
}

impl std::fmt::Display for InstrumentFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "instrument_failed({}): {}", self.class.token(), self.detail)
    }
}

impl std::error::Error for InstrumentFailure {}

type ToolchainResult<T> = Result<T, InstrumentFailure>;

fn classify_authority(error: anyhow::Error) -> InstrumentFailure {
    InstrumentFailure::wrap(
        InstrumentFailureClass::AuthorityUnreadable,
        "governed authority",
        error,
    )
}

fn mismatch(detail: impl Into<String>) -> InstrumentFailure {
    InstrumentFailure::new(InstrumentFailureClass::IdentityMismatch, detail)
}

fn unresolved(detail: impl Into<String>) -> InstrumentFailure {
    InstrumentFailure::new(InstrumentFailureClass::SubjectUnresolved, detail)
}

fn acquisition_failure(detail: impl Into<String>) -> InstrumentFailure {
    InstrumentFailure::new(InstrumentFailureClass::AcquisitionUnavailable, detail)
}

/// First non-empty stderr line, hard-capped, for embedding in typed failure
/// messages. Bounded diagnostics name the failing subject without turning a
/// chatty subprocess into an unbounded error dump.
fn bounded_first_stderr_line(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    let first = text.lines().find(|line| !line.trim().is_empty()).unwrap_or_default();
    let trimmed = first.trim();
    if trimmed.chars().count() > 160 {
        let bounded: String = trimmed.chars().take(160).collect();
        format!("{bounded}...")
    } else {
        trimmed.to_string()
    }
}

// ---------------------------------------------------------------------------
// Manifest schema v1 (deny_unknown_fields; deterministic serialization)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolchainManifest {
    pub schema_version: String,
    pub instrument_version: u32,
    pub host_role: String,
    pub platform: PlatformFields,
    pub cache_key: String,
    pub vim: VimToolchainIdentity,
    pub vim_lsp: VimLspToolchainIdentity,
    pub isolation: IsolationPolicy,
    /// Durable fact: this toolchain was fully acquired and verified at write
    /// time. Cache-hit versus fresh-build is runtime status on stdout, never
    /// a manifest field — the bytes stay identical across roots and reruns.
    pub provision_result: ProvisionResult,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum ProvisionResult {
    Provisioned,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlatformFields {
    pub os: String,
    pub arch: String,
    pub execution_environment: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VimToolchainIdentity {
    pub acquisition: VimAcquisitionIdentity,
    pub executable_sha256: String,
    pub runtime_tree_sha256: String,
    pub version_summary: String,
    pub version_text_sha256: String,
    pub required_features: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VimAcquisitionIdentity {
    pub mode: String,
    pub source_url: String,
    pub tag: String,
    pub archive_sha256: String,
    pub archive_entry: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VimLspToolchainIdentity {
    pub subject_authority_path: String,
    pub subject_authority_sha256: String,
    pub selected_commit: String,
    pub tree_digest: Option<String>,
    pub load_mode: String,
    pub entry_files: Vec<ManifestEntryFile>,
    pub verified_entry_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManifestEntryFile {
    pub path: String,
    pub git_blob_sha1: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IsolationPolicy {
    pub ambient_user_vimrc: String,
    pub ambient_user_plugins: String,
}

impl IsolationPolicy {
    pub fn governed() -> Self {
        Self {
            ambient_user_vimrc: "excluded".to_string(),
            ambient_user_plugins: "excluded".to_string(),
        }
    }
}

// ---------------------------------------------------------------------------
// Cache-key inputs
// ---------------------------------------------------------------------------

/// Every load-bearing input of the toolchain identity. Any change produces a
/// different cache key and therefore a different exact-identity directory.
///
/// Transitive binding note: `VIM_EXECUTABLE_SHA256` is deliberately not a
/// key input. Under the current acquisition mode the executable's bytes are
/// fully determined by the pinned archive (`archive_sha256` fixes the bytes,
/// and `install_vim_executable` rebinds the inner-entry digest on every
/// fresh build), so adding it would be redundant. Before ANY acquisition-
/// mode change (a mode whose executable is not derived solely from the
/// archive), this digest MUST join these inputs in the same commit, or a
/// stale entry could satisfy a role whose executable law changed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CacheKeyInputs<'a> {
    pub schema_version: &'a str,
    pub instrument_version: u32,
    pub host_role: &'a str,
    pub platform: &'a PlatformFields,
    pub vim_acquisition: &'a VimAcquisitionIdentity,
    pub vim_required_features: Vec<String>,
    pub vim_lsp_subject_authority_path: &'a str,
    pub vim_lsp_subject_authority_sha256: &'a str,
    pub vim_lsp_selected_commit: &'a str,
    pub vim_lsp_load_mode: &'a str,
    pub isolation: &'a IsolationPolicy,
}

pub fn cache_key(inputs: &CacheKeyInputs<'_>) -> Result<String> {
    let canonical = serde_json::to_vec(inputs).context("canonicalizing cache-key inputs")?;
    bytes_sha256(&canonical)
}

/// The cache-key identity string is `sha256:<hex>`; the on-disk entry name
/// carries exactly the hex part so Windows paths stay valid.
fn cache_dir_name(key: &str) -> Result<&str> {
    key.strip_prefix("sha256:")
        .ok_or_else(|| anyhow::anyhow!("cache key {key} is not a sha256:<hex> identity"))
}

/// Reproduce the recorded cache key purely from one manifest's own fields.
/// This closes false-subject control 4: a key that omitted a load-bearing
/// input cannot validate against the reproduction.
fn reproduce_cache_key(manifest: &ToolchainManifest) -> Result<String> {
    let inputs = CacheKeyInputs {
        schema_version: &manifest.schema_version,
        instrument_version: manifest.instrument_version,
        host_role: &manifest.host_role,
        platform: &manifest.platform,
        vim_acquisition: &manifest.vim.acquisition,
        vim_required_features: manifest.vim.required_features.clone(),
        vim_lsp_subject_authority_path: &manifest.vim_lsp.subject_authority_path,
        vim_lsp_subject_authority_sha256: &manifest.vim_lsp.subject_authority_sha256,
        vim_lsp_selected_commit: &manifest.vim_lsp.selected_commit,
        vim_lsp_load_mode: &manifest.vim_lsp.load_mode,
        isolation: &manifest.isolation,
    };
    cache_key(&inputs)
}

// ---------------------------------------------------------------------------
// Bounded subprocess probes
// ---------------------------------------------------------------------------

/// Run one command with stdin closed, a hard deadline, and an output cap.
/// Output past the cap aborts the probe instead of buffering indefinitely;
/// a hanging subject cannot hold provisioning hostage.
fn bounded_output(command: &mut Command, label: &str) -> Result<String> {
    let (stdout, _stderr) = bounded_output_with(command, label, VERSION_PROBE_TIMEOUT)?;
    Ok(stdout)
}

/// [`bounded_output`] with an explicit deadline; acquisition subprocesses
/// (git transport) and identity probes share the cap semantics but not the
/// same time budget. Returns `(stdout, stderr)`; each stream is capped.
fn bounded_output_with(
    command: &mut Command,
    label: &str,
    timeout: Duration,
) -> Result<(String, String)> {
    command.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command.spawn().with_context(|| format!("spawning {label} probe"))?;
    let stdout_pipe = child.stdout.take().context(format!("{label} probe has no stdout"))?;
    let stderr_pipe = child.stderr.take().context(format!("{label} probe has no stderr"))?;

    fn drain_pipe(
        mut pipe: impl Read + Send + 'static,
        label: String,
        stream: &'static str,
    ) -> std::thread::JoinHandle<Result<Vec<u8>>> {
        std::thread::spawn(move || {
            let mut buffered = Vec::new();
            let mut chunk = [0u8; 8192];
            loop {
                match pipe.read(&mut chunk) {
                    Ok(0) => break,
                    Ok(read) => {
                        buffered.extend_from_slice(&chunk[..read]);
                        if buffered.len() > SUBPROCESS_OUTPUT_CAP_BYTES {
                            return Err(anyhow::anyhow!(
                                "{label} probe exceeded the output cap on {stream}"
                            ));
                        }
                    }
                    Err(error) => {
                        return Err(anyhow::anyhow!("{label} probe {stream} read failed: {error}"));
                    }
                }
            }
            Ok(buffered)
        })
    }

    let stdout_reader = drain_pipe(stdout_pipe, label.to_string(), "stdout");
    let stderr_reader = drain_pipe(stderr_pipe, label.to_string(), "stderr");
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if Instant::now() >= deadline => break None,
            Ok(None) => std::thread::sleep(Duration::from_millis(25)),
            Err(error) => {
                let _ = child.kill();
                let _ = stdout_reader.join();
                let _ = stderr_reader.join();
                return Err(
                    anyhow::Error::from(error).context(format!("waiting for {label} probe"))
                );
            }
        }
    };
    let status = match status {
        Some(status) => status,
        None => {
            let _ = child.kill();
            let _ = child.wait();
            let _ = stdout_reader.join();
            let _ = stderr_reader.join();
            anyhow::bail!("{label} probe exceeded its {}s deadline", timeout.as_secs());
        }
    };
    let stdout = stdout_reader
        .join()
        .map_err(|_| anyhow::anyhow!("{label} probe stdout reader failed"))??;
    let stderr = stderr_reader
        .join()
        .map_err(|_| anyhow::anyhow!("{label} probe stderr reader failed"))??;
    let stderr_text = String::from_utf8_lossy(&stderr).into_owned();
    ensure!(status.success(), "{label} probe failed with status {status}: {}", stderr_text.trim());
    let stdout = String::from_utf8(stdout)
        .with_context(|| format!("{label} probe produced non-UTF-8 stdout"))?;
    Ok((stdout, stderr_text))
}

/// The production version probe: bounded `vim --version`. Version probing
/// sources no vimrc and reads no ambient plugin state.
pub fn probe_vim_version(executable: &Path) -> Result<String> {
    let mut command = Command::new(executable);
    command.arg("--version");
    let output = bounded_output(&mut command, "Vim --version")?;
    ensure!(!output.trim().is_empty(), "Vim --version probe produced no output");
    Ok(output)
}

// ---------------------------------------------------------------------------
// Safe archive extraction
// ---------------------------------------------------------------------------

/// Extract exactly the pinned runtime subtree of `archive` into `dest`,
/// refusing path traversal, absolute entries, drive letters, symlinks, and
/// oversized payloads. Only regular directories and files are admitted:
/// entries carrying symlink mode bits fail explicitly (the doc claim is
/// enforced by `is_symlink`, not by incidental file/dir classification).
/// Payload bytes are streamed to disk under a running cap on ACTUAL
/// decompressed bytes, so a header that lies about its declared size cannot
/// smuggle a deflate bomb past [`EXTRACTION_TOTAL_CAP_BYTES`].
fn extract_runtime_subtree(archive_path: &Path, subtree: &str, dest: &Path) -> Result<usize> {
    let file = fs::File::open(archive_path)
        .with_context(|| format!("opening archive {}", archive_path.display()))?;
    let mut archive = zip::ZipArchive::new(file)
        .with_context(|| format!("reading archive {}", archive_path.display()))?;
    anyhow::ensure!(subtree.ends_with('/'), "runtime subtree must end with '/': {subtree}");
    if dest.exists() {
        fs::remove_dir_all(dest).context("clearing prior extraction destination")?;
    }
    fs::create_dir_all(dest).with_context(|| format!("creating {}", dest.display()))?;
    let mut extracted = 0usize;
    let mut total_bytes = 0u64;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).context("reading archive entry")?;
        anyhow::ensure!(extracted < EXTRACTION_MAX_ENTRIES, "archive exceeds the entry cap");
        let raw_name = entry.name().to_string();
        let normalized = raw_name.replace('\\', "/");
        anyhow::ensure!(
            !normalized.contains('\0') && !normalized.contains(".."),
            "archive entry {raw_name} contains a path escape"
        );
        let Some(relative) = normalized.strip_prefix(subtree) else {
            continue;
        };
        if relative.is_empty() {
            // An explicit directory entry for the subtree root itself: the
            // destination already exists, nothing to extract.
            continue;
        }
        anyhow::ensure!(
            !Path::new(relative).is_absolute() && !relative.contains(':'),
            "archive entry {raw_name} is not a relative subtree path"
        );
        let target = dest.join(relative);
        if entry.is_dir() {
            fs::create_dir_all(&target)
                .with_context(|| format!("creating extracted directory {}", target.display()))?;
            continue;
        }
        // Explicit symlink rejection before any regular-file admission, so
        // the doc claim is enforced by name rather than incidentally.
        anyhow::ensure!(
            !entry.is_symlink(),
            "archive entry {raw_name} carries symlink metadata; symlinks are refused"
        );
        anyhow::ensure!(
            entry.is_file(),
            "archive entry {raw_name} is neither a regular file nor a directory"
        );
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
        }
        let mut payload = fs::File::create(&target)
            .with_context(|| format!("creating extracted {}", target.display()))?;
        let mut chunk = [0u8; 16 * 1024];
        loop {
            let read = entry.read(&mut chunk).context("reading archive entry bytes")?;
            if read == 0 {
                break;
            }
            total_bytes += read as u64;
            anyhow::ensure!(
                total_bytes <= EXTRACTION_TOTAL_CAP_BYTES,
                "archive exceeds the total extraction size cap while streaming actual bytes"
            );
            payload.write_all(&chunk[..read]).context("writing extracted archive bytes")?;
        }
        extracted += 1;
    }
    anyhow::ensure!(extracted > 0, "archive carried no entries under {subtree}");
    Ok(extracted)
}

/// Hash the complete extracted runtime deterministically. The relative path,
/// entry kind, and file bytes are length-prefixed so path boundaries and
/// empty directories cannot collide. WalkDir does not promise ordering, so
/// entries are sorted before the canonical stream is hashed.
///
/// The runtime root itself is rejected when it is a symlink: WalkDir yields
/// the root entry before its descendants and this walk skips that entry by
/// path, so a replaced root would otherwise be walked as ordinary storage
/// (external mutable bytes presented as the cached runtime) without ever
/// tripping the per-entry symlink rejection below.
fn runtime_tree_sha256(runtime_root: &Path) -> Result<String> {
    let root_metadata = fs::symlink_metadata(runtime_root)
        .with_context(|| format!("statting the runtime root {}", runtime_root.display()))?;
    anyhow::ensure!(
        !root_metadata.file_type().is_symlink(),
        "runtime root {} is a symlink",
        runtime_root.display()
    );
    let mut entries = Vec::new();
    for entry in WalkDir::new(runtime_root).follow_links(false) {
        let entry = entry.context("walking the extracted Vim runtime")?;
        if entry.path() == runtime_root {
            continue;
        }
        let relative = entry
            .path()
            .strip_prefix(runtime_root)
            .context("computing an extracted runtime relative path")?
            .to_string_lossy()
            .replace('\\', "/");
        let kind = entry.file_type();
        anyhow::ensure!(!kind.is_symlink(), "extracted runtime contains a symlink: {relative}");
        anyhow::ensure!(
            kind.is_dir() || kind.is_file(),
            "extracted runtime contains unsupported entry: {relative}"
        );
        entries.push((relative, kind.is_dir(), entry.into_path()));
    }
    entries.sort_by(|left, right| left.0.cmp(&right.0));

    let mut hasher = Sha256::new();
    for (relative, is_dir, path) in entries {
        hasher.update([if is_dir { b'd' } else { b'f' }]);
        hasher.update((relative.len() as u64).to_le_bytes());
        hasher.update(relative.as_bytes());
        if !is_dir {
            let length = fs::metadata(&path)
                .with_context(|| format!("statting extracted runtime file {}", path.display()))?
                .len();
            hasher.update(length.to_le_bytes());
            let mut file = fs::File::open(&path)
                .with_context(|| format!("opening extracted runtime file {}", path.display()))?;
            let mut buffer = [0u8; 64 * 1024];
            loop {
                let read = file.read(&mut buffer).with_context(|| {
                    format!("reading extracted runtime file {}", path.display())
                })?;
                if read == 0 {
                    break;
                }
                hasher.update(&buffer[..read]);
            }
        }
    }
    let digest = hasher.finalize();
    let mut identity = String::with_capacity("sha256:".len() + 64);
    identity.push_str("sha256:");
    for byte in digest {
        write!(&mut identity, "{byte:02x}")?;
    }
    Ok(identity)
}

// ---------------------------------------------------------------------------
// Authority loading
// ---------------------------------------------------------------------------

/// Where the governed #11369 subject authority comes from. Production reads
/// it from the repository by reference; hermetic fixtures inject equivalent
/// inline bytes so tests exercise the full pipeline with no network.
#[derive(Debug, Clone)]
pub enum SubjectAuthoritySource {
    RepoRoot(PathBuf),
    Inline { display_path: String, bytes: Vec<u8> },
}

impl SubjectAuthoritySource {
    fn load(&self) -> Result<(VimLspSubjectManifest, String)> {
        match self {
            SubjectAuthoritySource::RepoRoot(repo_root) => {
                let path = repo_root.join(SUBJECT_AUTHORITY_REPO_PATH);
                let bytes = fs::read(&path).with_context(|| {
                    format!("reading the governed subject authority {}", path.display())
                })?;
                let manifest = VimLspSubjectManifest::parse(&bytes)
                    .map_err(|error| error.context("the governed subject authority"))?;
                Ok((manifest, file_sha256(&path)?))
            }
            SubjectAuthoritySource::Inline { display_path, bytes } => {
                let manifest = VimLspSubjectManifest::parse(bytes)
                    .map_err(|error| error.context(display_path.clone()))?;
                Ok((manifest, bytes_sha256(bytes)?))
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Provisioning inputs and outcomes
// ---------------------------------------------------------------------------

/// Inputs for one provisioning run.
pub struct ProvisionInputs {
    /// Output/cache root. Layout beneath it is fixed:
    /// `<output>/<cache-key>/manifest.json|vim/|vim-lsp/|downloads/`.
    pub output_root: PathBuf,
    /// Repository root used to resolve the governed authority when the
    /// source is [`SubjectAuthoritySource::RepoRoot`] and to anchor the
    /// machine-path leak scan.
    pub repo_root: PathBuf,
    pub authority: SubjectAuthoritySource,
    /// Offline vim-lsp acquisition: clone the pinned commit from this local
    /// checkout instead of the governed upstream URL.
    pub vim_lsp_source: Option<PathBuf>,
    /// Offline Vim acquisition: use this local archive instead of curling
    /// [`VIM_ARCHIVE_URL`]. Tests inject a tiny checked-in fixture so the
    /// whole provision path runs with zero network. When set, both expected
    /// digests below MUST be set too; the trio is validated together.
    pub vim_archive_source: Option<PathBuf>,
    /// Expected digest of the injected archive (replaces the pinned
    /// [`VIM_ARCHIVE_SHA256`] for this run).
    pub vim_archive_expected_sha256: Option<String>,
    /// Expected digest of the pinned inner executable inside the injected
    /// archive (replaces the pinned [`VIM_EXECUTABLE_SHA256`]).
    pub vim_executable_expected_sha256: Option<String>,
    /// Execution-environment label recorded in identity (`local_runner`,
    /// `ci_runner`, ...).
    pub execution_environment: String,
}

/// Outcome of one provisioning run.
pub struct ProvisionOutcome {
    pub manifest_path: PathBuf,
    pub manifest: ToolchainManifest,
    pub manifest_sha256: String,
    /// True when an existing cache entry fully revalidated (pure cache hit);
    /// false when subjects were acquired or rebuilt.
    pub cache_hit: bool,
    pub vim_executable_role: PathBuf,
    pub vim_lsp_runtimepath_role: PathBuf,
}

fn platform_fields(execution_environment: &str) -> PlatformFields {
    PlatformFields {
        os: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
        execution_environment: execution_environment.to_string(),
    }
}

/// The effective Vim identity pins for one provisioning run: the pinned
/// release subject by default, or the injected offline archive when tests
/// supply one. Whatever is resolved here flows into the cache key, the
/// manifest, and both digest checks, so an injected run can never collide
/// with (or silently stand in for) a production-pinned entry.
struct VimPins {
    acquisition: VimAcquisitionIdentity,
    executable_sha256: String,
}

/// Resolve [`VimPins`] from the inputs. The archive source and both expected
/// digests are all-or-nothing; a partial injection is a typed input error.
fn resolve_vim_pins(inputs: &ProvisionInputs) -> ToolchainResult<VimPins> {
    match (
        &inputs.vim_archive_source,
        &inputs.vim_archive_expected_sha256,
        &inputs.vim_executable_expected_sha256,
    ) {
        (None, None, None) => Ok(VimPins {
            acquisition: VimAcquisitionIdentity {
                mode: "pinned_release_archive".to_string(),
                source_url: VIM_ARCHIVE_URL.to_string(),
                tag: VIM_RELEASE_TAG.to_string(),
                archive_sha256: VIM_ARCHIVE_SHA256.to_string(),
                archive_entry: VIM_ARCHIVE_ENTRY.to_string(),
            },
            executable_sha256: VIM_EXECUTABLE_SHA256.to_string(),
        }),
        (Some(archive), Some(archive_digest), Some(executable_digest)) => {
            let file_name = archive
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| "archive".to_string());
            // Only the file name enters durable identity: absolute machine
            // paths are excluded from manifests by law, and the content
            // digests carry the real binding anyway.
            Ok(VimPins {
                acquisition: VimAcquisitionIdentity {
                    mode: "offline_archive_injection".to_string(),
                    source_url: format!("local:{file_name}"),
                    tag: format!("{VIM_RELEASE_TAG}-shape-fixture"),
                    archive_sha256: archive_digest.clone(),
                    archive_entry: VIM_ARCHIVE_ENTRY.to_string(),
                },
                executable_sha256: executable_digest.clone(),
            })
        }
        _ => Err(acquisition_failure(
            "incomplete offline archive injection: vim_archive_source and both expected \
             digests must be provided together",
        )),
    }
}

fn required_feature_list() -> Vec<String> {
    REQUIRED_VIM_FEATURES.iter().map(|value| value.to_string()).collect()
}

fn serialize_manifest(manifest: &ToolchainManifest) -> Result<Vec<u8>> {
    let mut bytes = serde_json::to_vec_pretty(manifest).context("serializing the manifest")?;
    bytes.push(b'\n');
    Ok(bytes)
}

/// Refuse to publish durable identity that leaks machine-specific absolute
/// paths (false-subject control 9). The scan anchors on the concrete roots
/// involved plus generic Windows-drive and Unix-home prefixes.
fn assert_no_machine_paths(manifest_bytes: &[u8], roots: &[&Path]) -> Result<()> {
    let text = std::str::from_utf8(manifest_bytes).context("manifest bytes are not UTF-8")?;
    let lowered = text.to_lowercase();
    for root in roots {
        let needle = root.to_string_lossy().trim_end_matches(['/', '\\']).to_lowercase();
        if needle.len() > 2 && lowered.contains(&needle) {
            anyhow::bail!("manifest leaks the machine path {needle}");
        }
    }
    for generic in ["c:\\\\", "\\users\\", "/home/", "/users/", "/tmp/"] {
        if lowered.contains(generic) {
            anyhow::bail!("manifest leaks a machine-specific path pattern ({generic})");
        }
    }
    Ok(())
}

/// Verify-time path law (cheaper structural option over persisting scan
/// roots, which would leak machine paths into the manifest it protects):
/// every durable field that names a location must be a relative forward-
/// slash reference, and the acquisition source must be an explicit non-file
/// URL scheme. Combined with [`assert_no_machine_paths`] at write time this
/// makes verify-time scanning exactly as strong as write-time scanning,
/// because no durable string can structurally carry an absolute path.
fn assert_durable_strings_relative(manifest: &ToolchainManifest) -> Result<()> {
    let rel = |field: &str, value: &str| -> Result<()> {
        anyhow::ensure!(
            !value.is_empty()
                && !value.starts_with('/')
                && !value.contains('\\')
                && !value.contains(':')
                && !value.contains("..")
                && !Path::new(value).is_absolute(),
            "durable identity field {field} is not a relative reference: {value}"
        );
        Ok(())
    };
    rel("vim_lsp.subject_authority_path", &manifest.vim_lsp.subject_authority_path)?;
    rel("vim.acquisition.archive_entry", &manifest.vim.acquisition.archive_entry)?;
    for entry in &manifest.vim_lsp.entry_files {
        rel("vim_lsp.entry_files[].path", &entry.path)?;
    }
    let url = &manifest.vim.acquisition.source_url;
    anyhow::ensure!(
        url.starts_with("https://") || url.starts_with("local:"),
        "durable identity field vim.acquisition.source_url must be an https or local: \
         reference, not a filesystem path: {url}"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Acquisition stages
// ---------------------------------------------------------------------------

/// Acquire the Vim archive into `downloads` (skipped when a digest-correct
/// copy is already present), returning the archive path. `expected_sha256`
/// is the effective pin for this run (pinned release or injected fixture
/// expectation).
fn acquire_vim_archive(downloads_dir: &Path, expected_sha256: &str) -> ToolchainResult<PathBuf> {
    let archive_path = downloads_dir.join(format!("gvim_{VIM_RELEASE_TAG}.zip"));
    if archive_path.exists() {
        return match file_sha256(&archive_path) {
            Ok(actual) if actual == expected_sha256 => Ok(archive_path),
            // This function does not rebuild anything: it fails typed and
            // the caller deletes the whole cache entry and rebuilds from the
            // pinned sources.
            Ok(_) => Err(mismatch(format!(
                "cached download {} drifted from the expected archive digest {expected_sha256}; \
                 the whole cache entry will be rebuilt",
                archive_path.display()
            ))),
            Err(error) => Err(acquisition_failure(format!(
                "hashing the cached download {}: {error:#}",
                archive_path.display()
            ))),
        };
    }
    fs::create_dir_all(downloads_dir)
        .map_err(|error| acquisition_failure(format!("creating downloads dir: {error}")))?;
    // Concurrent provisions must not share one fixed partial path; the
    // unique suffix makes each writer own its file and the rename below
    // stays atomic per producer.
    let partial = downloads_dir.join(format!(
        "download.{}.{:x}.partial",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|since| since.as_nanos())
            .unwrap_or_default()
    ));
    let fetched = Command::new("curl")
        .args([
            "--fail",
            "--location",
            "--silent",
            "--show-error",
            "--max-time",
            &CURL_MAX_TIME_SECS.to_string(),
            "--speed-limit",
            &CURL_SPEED_LIMIT_BYTES.to_string(),
            "--speed-time",
            &CURL_SPEED_TIME_SECS.to_string(),
            "--output",
        ])
        .arg(&partial)
        .arg(VIM_ARCHIVE_URL)
        .stdin(Stdio::null())
        .output()
        .map_err(|error| {
            acquisition_failure(format!("running curl for the pinned Vim archive: {error}"))
        })?;
    if !fetched.status.success() {
        let _ = fs::remove_file(&partial);
        return Err(acquisition_failure(format!(
            "curl could not fetch the pinned Vim archive (status {}): {}",
            fetched.status,
            bounded_first_stderr_line(&fetched.stderr)
        )));
    }
    let actual = file_sha256(&partial).map_err(|error| {
        acquisition_failure(format!("hashing the downloaded archive: {error:#}"))
    })?;
    if actual != expected_sha256 {
        let _ = fs::remove_file(&partial);
        return Err(mismatch(format!(
            "downloaded Vim archive digest {actual} does not match the expected {expected_sha256}"
        )));
    }
    fs::rename(&partial, &archive_path).map_err(|error| {
        acquisition_failure(format!("finalizing the downloaded archive: {error}"))
    })?;
    Ok(archive_path)
}

/// Verify the pinned console executable inside the extracted portable
/// runtime, binding its digest to the run's expected pin. The executable
/// stays inside the runtime directory: the Windows VIMDLL build loads
/// `vim92.dll` from its own directory, so isolating the bare binary would
/// break launch.
fn install_vim_executable(
    archive: &Path,
    cache_vim_dir: &Path,
    expected_executable_sha256: &str,
) -> ToolchainResult<PathBuf> {
    let runtime_dir = cache_vim_dir.join("vim92");
    extract_runtime_subtree(archive, VIM_ARCHIVE_RUNTIME_SUBTREE, &runtime_dir)
        .map_err(|error| unresolved(format!("extracting the pinned runtime subtree: {error:#}")))?;
    let installed = runtime_dir.join("vim.exe");
    if !installed.is_file() {
        return Err(unresolved(format!(
            "the verified archive carries no console executable at {VIM_ARCHIVE_ENTRY}"
        )));
    }
    let actual = file_sha256(&installed)
        .map_err(|error| unresolved(format!("hashing the pinned executable: {error:#}")))?;
    if actual != expected_executable_sha256 {
        return Err(mismatch(format!(
            "extracted Vim executable digest {actual} does not match the expected \
             {expected_executable_sha256}"
        )));
    }
    Ok(installed)
}

/// Clone the pinned vim-lsp commit into `checkout_dir` from the governed
/// upstream URL or the offline local source, then verify it against the
/// parsed authority.
fn install_vim_lsp_checkout(
    checkout_dir: &Path,
    authority: &VimLspSubjectManifest,
    local_source: Option<&Path>,
) -> ToolchainResult<VimLspCheckoutIdentity> {
    if checkout_dir.exists() {
        fs::remove_dir_all(checkout_dir)
            .map_err(|error| acquisition_failure(format!("clearing prior checkout: {error}")))?;
    }
    fs::create_dir_all(checkout_dir)
        .map_err(|error| acquisition_failure(format!("creating checkout root: {error}")))?;
    let remote = local_source
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_else(|| VIM_LSP_UPSTREAM_URL.to_string());
    let dir_arg = checkout_dir.to_string_lossy().into_owned();
    let commit = authority.upstream.selected_commit.clone();
    let steps: [(&str, Vec<String>); 4] = [
        ("init", vec!["init".into(), dir_arg.clone()]),
        (
            "remote",
            vec![
                "-C".into(),
                dir_arg.clone(),
                "remote".into(),
                "add".into(),
                "origin".into(),
                remote,
            ],
        ),
        (
            "fetch",
            vec![
                "-C".into(),
                dir_arg.clone(),
                "fetch".into(),
                "--depth".into(),
                "1".into(),
                "--no-tags".into(),
                "origin".into(),
                commit,
            ],
        ),
        (
            "checkout",
            vec!["-C".into(), dir_arg, "checkout".into(), "--detach".into(), "FETCH_HEAD".into()],
        ),
    ];
    for (label, args) in steps {
        let mut command = Command::new("git");
        command.args(&args);
        // Bounded like every other acquisition subprocess: a wedged git
        // transport or an endlessly chatty step cannot hold provisioning
        // hostage.
        if let Err(error) =
            bounded_output_with(&mut command, &format!("git {label}"), GIT_ACQUISITION_TIMEOUT)
        {
            return Err(acquisition_failure(format!(
                "git {label} failed for the pinned vim-lsp subject: {error:#}"
            )));
        }
    }
    verify_vim_lsp_checkout(checkout_dir, authority).map_err(|error| {
        InstrumentFailure::wrap(
            InstrumentFailureClass::IdentityMismatch,
            "the acquired vim-lsp subject failed verification",
            error,
        )
    })
}

// ---------------------------------------------------------------------------
// Verification core
// ---------------------------------------------------------------------------

/// Verify one provisioned layout end to end against its manifest.
/// Deterministic and offline: no network, no authority reread, no PATH
/// discovery. Any drift is a typed identity failure.
pub fn verify_layout(
    manifest_path: &Path,
    probe: &dyn Fn(&Path) -> Result<String>,
) -> ToolchainResult<()> {
    let manifest_bytes = fs::read(manifest_path)
        .map_err(|error| mismatch(format!("reading {}: {error}", manifest_path.display())))?;
    let manifest: ToolchainManifest = serde_json::from_slice(&manifest_bytes).map_err(|error| {
        mismatch(format!("manifest does not satisfy {SCHEMA_VERSION}: {error}"))
    })?;
    let layout_root = manifest_path.parent().ok_or_else(|| {
        mismatch(format!("manifest {} has no parent directory", manifest_path.display()))
    })?;
    verify_manifest_core(&manifest, &manifest_bytes, layout_root, probe)
}

fn verify_manifest_core(
    manifest: &ToolchainManifest,
    manifest_bytes: &[u8],
    layout_root: &Path,
    probe: &dyn Fn(&Path) -> Result<String>,
) -> ToolchainResult<()> {
    if manifest.schema_version != SCHEMA_VERSION {
        return Err(mismatch(format!(
            "schema {} is not {SCHEMA_VERSION}",
            manifest.schema_version
        )));
    }
    if manifest.host_role != HOST_ROLE {
        return Err(mismatch(format!(
            "host role {} is not {HOST_ROLE}; one role cannot be relabeled another",
            manifest.host_role
        )));
    }
    if manifest.instrument_version != INSTRUMENT_VERSION {
        return Err(mismatch(format!(
            "instrument version {} is not {INSTRUMENT_VERSION}",
            manifest.instrument_version
        )));
    }
    let reproduced =
        reproduce_cache_key(manifest).map_err(|error| mismatch(format!("{error:#}")))?;
    if reproduced != manifest.cache_key {
        return Err(mismatch(format!(
            "cache key {} does not reproduce from the recorded inputs ({reproduced})",
            manifest.cache_key
        )));
    }
    assert_no_machine_paths(manifest_bytes, &[]).map_err(|error| mismatch(format!("{error:#}")))?;
    // Structural path law: durable location fields are relative references,
    // so verify-time scanning is as strong as write-time scanning without
    // persisting machine roots anywhere.
    assert_durable_strings_relative(manifest).map_err(|error| mismatch(format!("{error:#}")))?;

    let vim_executable = layout_root.join("vim").join("vim92").join("vim.exe");
    let actual_exe = file_sha256(&vim_executable)
        .map_err(|error| mismatch(format!("hashing the cached Vim executable: {error:#}")))?;
    if actual_exe != manifest.vim.executable_sha256 {
        return Err(mismatch(format!(
            "cached Vim executable digest {actual_exe} does not match the recorded {}",
            manifest.vim.executable_sha256
        )));
    }
    let runtime_root = vim_executable
        .parent()
        .ok_or_else(|| mismatch("cached Vim executable has no runtime parent".to_string()))?;
    let actual_runtime = runtime_tree_sha256(runtime_root)
        .map_err(|error| mismatch(format!("hashing the cached Vim runtime: {error:#}")))?;
    if actual_runtime != manifest.vim.runtime_tree_sha256 {
        return Err(mismatch(format!(
            "cached Vim runtime digest {actual_runtime} does not match the recorded {}",
            manifest.vim.runtime_tree_sha256
        )));
    }
    let version_text = probe(&vim_executable)
        .map_err(|error| mismatch(format!("cached Vim identity probe failed: {error:#}")))?;
    let actual_text =
        bytes_sha256(version_text.as_bytes()).map_err(|error| mismatch(format!("{error:#}")))?;
    if actual_text != manifest.vim.version_text_sha256 {
        return Err(mismatch(format!(
            "cached Vim version-text digest {actual_text} does not match the recorded {}; \
             same-version/different-bytes substitution fails closed",
            manifest.vim.version_text_sha256
        )));
    }
    let summary = version_text.lines().next().unwrap_or_default().trim();
    if summary != manifest.vim.version_summary {
        return Err(mismatch("cached Vim version summary line drifted".to_string()));
    }
    for feature in &manifest.vim.required_features {
        if !version_text.contains(&format!("+{feature}")) {
            return Err(mismatch(format!("cached Vim lacks the required feature +{feature}")));
        }
    }

    let synthetic_authority = VimLspSubjectManifest {
        schema_version: "vim_lsp_subject.v1".to_string(),
        upstream: VimLspUpstream {
            selected_commit: manifest.vim_lsp.selected_commit.clone(),
            tree_digest: manifest
                .vim_lsp
                .tree_digest
                .clone()
                .map(|value| VimLspTreeDigest { algorithm: "git-tree-sha1".to_string(), value }),
        },
        expected_content_identity: VimLspExpectedContent {
            entry_files: manifest
                .vim_lsp
                .entry_files
                .iter()
                .map(|entry| VimLspEntryFile {
                    path: entry.path.clone(),
                    git_blob_sha1: entry.git_blob_sha1.clone(),
                })
                .collect(),
        },
    };
    let checkout = layout_root.join("vim-lsp");
    verify_vim_lsp_checkout(&checkout, &synthetic_authority).map(|_verified_identity| ()).map_err(
        |error| {
            InstrumentFailure::wrap(
                InstrumentFailureClass::IdentityMismatch,
                "cached vim-lsp subject failed verification",
                error,
            )
        },
    )
}

/// Upgrade a pre-runtime-digest manifest without reacquiring an already
/// verified vim-lsp checkout. The schema version intentionally remains v1, so
/// caches written before this field existed need a local, fully verified
/// migration rather than being treated as a network-required rebuild.
///
/// Identity sourcing law: the migrated digest comes from the digest-verified
/// pinned archive extracted into a process-private staging directory, while
/// the live runtime tree is never cleared or rewritten by migration. A legacy
/// tree that does not already match the archive-derived digest, including a
/// symlinked runtime root, declines to the ordinary rebuild path, which
/// publishes through staging and rename. The retained vim-lsp checkout is left
/// in place so migration stays offline, and the migrated manifest is published
/// by atomic rename.
fn migrate_legacy_manifest(
    manifest_path: &Path,
    manifest_bytes: &[u8],
    key: &str,
    entry_root: &Path,
    probe: &dyn Fn(&Path) -> Result<String>,
) -> Option<ProvisionOutcome> {
    let mut value = serde_json::from_slice::<serde_json::Value>(manifest_bytes).ok()?;
    let vim = value.get_mut("vim")?.as_object_mut()?;
    if vim.contains_key("runtime_tree_sha256") {
        return None;
    }
    let expected_archive_sha256 =
        vim.get("acquisition")?.as_object()?.get("archive_sha256")?.as_str()?;
    let archive_path = entry_root.join("downloads").join(format!("gvim_{VIM_RELEASE_TAG}.zip"));
    if file_sha256(&archive_path).ok()?.as_str() != expected_archive_sha256 {
        return None;
    }
    let entry_name = match entry_root.file_name() {
        Some(name) => name.to_string_lossy(),
        None => return None,
    };
    let staging_root =
        entry_root.with_file_name(format!("{entry_name}.migrate.{}", std::process::id()));
    let _ = fs::remove_dir_all(&staging_root);
    let staged: Result<String> = (|| {
        let runtime_root = staging_root.join("vim92");
        extract_runtime_subtree(&archive_path, VIM_ARCHIVE_RUNTIME_SUBTREE, &runtime_root)?;
        runtime_tree_sha256(&runtime_root)
    })();
    let _ = fs::remove_dir_all(&staging_root);
    let digest = staged.ok()?;
    let runtime_root = entry_root.join("vim").join("vim92");
    if runtime_tree_sha256(&runtime_root).ok()?.as_str() != digest {
        return None;
    }
    vim.insert("runtime_tree_sha256".to_string(), serde_json::Value::String(digest));
    // Publish through the same canonical serializer as a fresh build so the
    // manifest bytes (and therefore manifest_sha256) depend only on identity,
    // not on migration history.
    let manifest: ToolchainManifest = serde_json::from_value(value).ok()?;
    let migrated_bytes = serialize_manifest(&manifest).ok()?;
    let manifest = serde_json::from_slice::<ToolchainManifest>(&migrated_bytes).ok()?;
    if manifest.cache_key != key
        || verify_manifest_core(&manifest, &migrated_bytes, entry_root, probe).is_err()
    {
        return None;
    }
    let migrated_manifest_tmp =
        manifest_path.with_file_name(format!("{MANIFEST_FILE_NAME}.tmp.{}", std::process::id()));
    fs::write(&migrated_manifest_tmp, &migrated_bytes).ok()?;
    fs::rename(&migrated_manifest_tmp, manifest_path).ok()?;
    Some(ProvisionOutcome {
        manifest_sha256: bytes_sha256(&migrated_bytes).ok()?,
        manifest_path: manifest_path.to_path_buf(),
        manifest,
        cache_hit: true,
        vim_executable_role: entry_root.join("vim").join("vim92").join("vim.exe"),
        vim_lsp_runtimepath_role: entry_root.join("vim-lsp"),
    })
}

// ---------------------------------------------------------------------------
// Provision
// ---------------------------------------------------------------------------

/// Provision (or revalidate) the full toolchain into the output root.
///
/// Network is touched only when immutable subjects are missing from the
/// cache; an existing valid entry is a pure offline revalidation. Any
/// revalidation failure deletes the entry and rebuilds from the pinned
/// sources — a warm cache containing different bytes can never satisfy the
/// role silently.
/// The output root as an absolute directory. Every derived role (entry
/// layout, vim-lsp checkout, handoff paths) is consumed by absolute-path
/// contracts, so a relative `--output target/vim-toolchain` invocation must
/// be anchored to the caller's working directory before any layout math,
/// never after a successful download.
fn absolutized_output_root(raw: &Path) -> ToolchainResult<PathBuf> {
    if raw.is_absolute() {
        return Ok(raw.to_path_buf());
    }
    std::env::current_dir()
        .map(|cwd| cwd.join(raw))
        .map_err(|error| acquisition_failure(format!("resolving the current directory: {error}")))
}

pub fn provision(
    inputs: &ProvisionInputs,
    probe: &dyn Fn(&Path) -> Result<String>,
) -> ToolchainResult<ProvisionOutcome> {
    let (authority, authority_sha256) = inputs.authority.load().map_err(classify_authority)?;
    let platform = platform_fields(&inputs.execution_environment);
    let pins = resolve_vim_pins(inputs)?;
    let key_inputs = CacheKeyInputs {
        schema_version: SCHEMA_VERSION,
        instrument_version: INSTRUMENT_VERSION,
        host_role: HOST_ROLE,
        platform: &platform,
        vim_acquisition: &pins.acquisition,
        vim_required_features: required_feature_list(),
        vim_lsp_subject_authority_path: SUBJECT_AUTHORITY_REPO_PATH,
        vim_lsp_subject_authority_sha256: &authority_sha256,
        vim_lsp_selected_commit: &authority.upstream.selected_commit,
        vim_lsp_load_mode: LOAD_MODE_CALLER_PINNED_CHECKOUT,
        isolation: &IsolationPolicy::governed(),
    };
    let key = cache_key(&key_inputs).map_err(classify_authority)?;
    let dir_name = cache_dir_name(&key).map_err(classify_authority)?.to_string();
    let output_root = absolutized_output_root(&inputs.output_root)?;
    let entry_root = output_root.join(&dir_name);
    let manifest_path = entry_root.join(MANIFEST_FILE_NAME);

    if manifest_path.exists() {
        let cached = fs::read(&manifest_path)
            .map_err(|error| mismatch(format!("reading the cached manifest: {error}")));
        if let Ok(manifest_bytes) = cached {
            if let Ok(manifest) = serde_json::from_slice::<ToolchainManifest>(&manifest_bytes)
                // The requested key is the governed identity of THIS run: a
                // foreign manifest dropped into this directory still reproduces
                // its own key, so only agreement with the currently requested
                // identity may admit a cache hit (never the directory alone).
                && manifest.cache_key == key
                && verify_manifest_core(&manifest, &manifest_bytes, &entry_root, probe).is_ok()
            {
                return Ok(ProvisionOutcome {
                    manifest_sha256: file_sha256(&manifest_path)
                        .map_err(|error| mismatch(format!("{error:#}")))?,
                    manifest_path,
                    manifest,
                    cache_hit: true,
                    vim_executable_role: entry_root.join("vim").join("vim92").join("vim.exe"),
                    vim_lsp_runtimepath_role: entry_root.join("vim-lsp"),
                });
            }
            if let Some(migrated) =
                migrate_legacy_manifest(&manifest_path, &manifest_bytes, &key, &entry_root, probe)
            {
                return Ok(migrated);
            }
        }
    }

    rebuild_entry(
        inputs,
        &output_root,
        &authority,
        authority_sha256,
        platform,
        key,
        &dir_name,
        &entry_root,
        pins,
        probe,
    )
}

#[allow(clippy::too_many_arguments)]
fn rebuild_entry(
    inputs: &ProvisionInputs,
    output_root: &Path,
    authority: &VimLspSubjectManifest,
    authority_sha256: String,
    platform: PlatformFields,
    key: String,
    dir_name: &str,
    entry_root: &Path,
    pins: VimPins,
    probe: &dyn Fn(&Path) -> Result<String>,
) -> ToolchainResult<ProvisionOutcome> {
    // Concurrent provisions must not race on the fixed per-key path: the
    // whole entry is built in a process-private staging directory and only
    // swapped into place once it is complete and verified. Only this
    // process's own stale staging leftovers are swept; a concurrent
    // writer's staging directory is never touched.
    let staging_root = output_root.join(format!("{dir_name}.staging.{}", std::process::id()));
    let _ = fs::remove_dir_all(&staging_root);
    // Healing law: a rebuild replaces derived state, not verified immutable
    // subjects. Carry the digest-valid pinned archive from the superseded
    // entry into the staging layout so offline healing stays possible;
    // anything missing or drifted degrades silently into normal acquisition.
    preserve_verified_archive(entry_root, &staging_root, &pins.acquisition.archive_sha256);
    match build_fresh_entry(
        inputs,
        authority,
        &authority_sha256,
        &platform,
        &key,
        &pins,
        &staging_root,
        probe,
    ) {
        Ok(outcome) => {
            if entry_root.exists() {
                fs::remove_dir_all(entry_root).map_err(|error| {
                    acquisition_failure(format!("clearing the superseded cache entry: {error}"))
                })?;
            }
            fs::rename(&staging_root, entry_root).map_err(|error| {
                acquisition_failure(format!("publishing the rebuilt cache entry: {error}"))
            })?;
            Ok(ProvisionOutcome {
                manifest_path: entry_root.join(MANIFEST_FILE_NAME),
                vim_executable_role: entry_root.join("vim").join("vim92").join("vim.exe"),
                vim_lsp_runtimepath_role: entry_root.join("vim-lsp"),
                ..outcome
            })
        }
        Err(failure) => {
            // A failed acquisition never leaves a half-built entry behind
            // that a later run could mistake for a provisioned subject.
            let _ = fs::remove_dir_all(&staging_root);
            Err(failure)
        }
    }
}

/// Carry the digest-verified pinned archive of a superseded entry into a
/// fresh staging layout. Best-effort by contract: any absence, hashing
/// problem, drift, or copy failure is a silent no-op and the subsequent
/// acquisition proceeds exactly as if nothing existed — this path only ever
/// prevents an unnecessary re-download, never substitutes identity evidence
/// (the acquired/kept archive is still digest-bound before use).
fn preserve_verified_archive(
    superseded_entry: &Path,
    staging_root: &Path,
    expected_archive_sha256: &str,
) {
    let legacy = superseded_entry.join("downloads").join(format!("gvim_{VIM_RELEASE_TAG}.zip"));
    if !legacy.is_file() {
        return;
    }
    let Ok(actual) = file_sha256(&legacy) else { return };
    if actual != expected_archive_sha256 {
        return;
    }
    let Some(file_name) = legacy.file_name() else { return };
    let target_dir = staging_root.join("downloads");
    let target = target_dir.join(file_name);
    if target.exists() {
        return;
    }
    if fs::create_dir_all(&target_dir).is_err() {
        return;
    }
    let _ = fs::copy(&legacy, &target);
}

#[allow(clippy::too_many_arguments)]
fn build_fresh_entry(
    inputs: &ProvisionInputs,
    authority: &VimLspSubjectManifest,
    authority_sha256: &str,
    platform: &PlatformFields,
    key: &str,
    pins: &VimPins,
    layout_root: &Path,
    probe: &dyn Fn(&Path) -> Result<String>,
) -> ToolchainResult<ProvisionOutcome> {
    let downloads_dir = layout_root.join("downloads");
    let cache_vim_dir = layout_root.join("vim");
    let checkout_dir = layout_root.join("vim-lsp");
    fs::create_dir_all(&cache_vim_dir).map_err(|error| {
        acquisition_failure(format!(
            "creating the cache entry {}: {error}",
            cache_vim_dir.display()
        ))
    })?;
    // Offline injection replaces the network download entirely: the local
    // archive is digest-bound against the run's expected value before any
    // use, so an injected fixture obeys exactly the same identity law as a
    // downloaded pinned release.
    let archive = match &inputs.vim_archive_source {
        Some(source) => {
            let actual = file_sha256(source).map_err(|error| {
                acquisition_failure(format!(
                    "hashing the injected Vim archive {}: {error:#}",
                    source.display()
                ))
            })?;
            if actual != pins.acquisition.archive_sha256 {
                return Err(mismatch(format!(
                    "injected Vim archive digest {actual} does not match the expected {}",
                    pins.acquisition.archive_sha256
                )));
            }
            source.clone()
        }
        None => acquire_vim_archive(&downloads_dir, &pins.acquisition.archive_sha256)?,
    };
    let vim_executable = install_vim_executable(&archive, &cache_vim_dir, &pins.executable_sha256)?;
    let version_text = probe(&vim_executable)
        .map_err(|error| mismatch(format!("provisioned Vim identity probe failed: {error:#}")))?;
    // Same feature law the #10944 runner enforces before launch.
    verify_vim_features(&version_text)
        .map_err(|error| mismatch(format!("provisioned Vim failed the feature law: {error:#}")))?;
    install_vim_lsp_checkout(&checkout_dir, authority, inputs.vim_lsp_source.as_deref())?;

    let executable_digest = file_sha256(&vim_executable)
        .map_err(|error| unresolved(format!("hashing the pinned executable: {error:#}")))?;
    let runtime_tree_digest = runtime_tree_sha256(vim_executable.parent().ok_or_else(|| {
        unresolved("provisioned Vim executable has no runtime parent".to_string())
    })?)
    .map_err(|error| unresolved(format!("hashing the provisioned Vim runtime: {error:#}")))?;
    let version_summary = version_text.lines().next().unwrap_or_default().trim().to_string();
    let manifest = ToolchainManifest {
        schema_version: SCHEMA_VERSION.to_string(),
        instrument_version: INSTRUMENT_VERSION,
        host_role: HOST_ROLE.to_string(),
        platform: platform.clone(),
        cache_key: key.to_string(),
        vim: VimToolchainIdentity {
            acquisition: pins.acquisition.clone(),
            executable_sha256: executable_digest,
            runtime_tree_sha256: runtime_tree_digest,
            version_summary,
            version_text_sha256: bytes_sha256(version_text.as_bytes())
                .map_err(|error| unresolved(format!("{error:#}")))?,
            required_features: required_feature_list(),
        },
        vim_lsp: VimLspToolchainIdentity {
            subject_authority_path: SUBJECT_AUTHORITY_REPO_PATH.to_string(),
            subject_authority_sha256: authority_sha256.to_string(),
            selected_commit: authority.upstream.selected_commit.clone(),
            tree_digest: authority.upstream.tree_digest.as_ref().map(|tree| tree.value.clone()),
            load_mode: LOAD_MODE_CALLER_PINNED_CHECKOUT.to_string(),
            entry_files: authority
                .expected_content_identity
                .entry_files
                .iter()
                .map(|entry| ManifestEntryFile {
                    path: entry.path.clone(),
                    git_blob_sha1: entry.git_blob_sha1.clone(),
                })
                .collect(),
            verified_entry_count: authority.expected_content_identity.entry_files.len(),
        },
        isolation: IsolationPolicy::governed(),
        provision_result: ProvisionResult::Provisioned,
    };
    let manifest_bytes = serialize_manifest(&manifest)
        .map_err(|error| unresolved(format!("serializing the manifest: {error:#}")))?;
    let scratch_root = std::env::temp_dir();
    let scan_roots: [&Path; 3] =
        [inputs.output_root.as_path(), inputs.repo_root.as_path(), &scratch_root];
    assert_no_machine_paths(&manifest_bytes, &scan_roots)
        .map_err(|error| mismatch(format!("{error:#}")))?;
    // Post-provision identity verification is mandatory, never skipped
    // (false-subject control 8): the freshly built layout must pass the same
    // offline verifier any later consumer runs.
    verify_manifest_core(&manifest, &manifest_bytes, layout_root, probe)?;
    let published = layout_root.join(MANIFEST_FILE_NAME);
    let staging = layout_root.join(format!("{MANIFEST_FILE_NAME}.tmp"));
    fs::write(&staging, &manifest_bytes)
        .map_err(|error| unresolved(format!("writing the manifest: {error}")))?;
    fs::rename(&staging, &published)
        .map_err(|error| unresolved(format!("publishing the manifest: {error}")))?;
    Ok(ProvisionOutcome {
        manifest_sha256: bytes_sha256(&manifest_bytes)
            .map_err(|error| unresolved(format!("{error:#}")))?,
        manifest_path: published,
        manifest,
        cache_hit: false,
        vim_executable_role: vim_executable,
        vim_lsp_runtimepath_role: checkout_dir.to_path_buf(),
    })
}

/// Render the ephemeral role handoff for the #10944 consumer. These lines
/// carry absolute runtime paths by design; they are stdout state, never part
/// of the durable manifest.
pub fn render_handoff(outcome: &ProvisionOutcome) -> String {
    format!(
        "toolchain: {SCHEMA_VERSION} status: {} cache_key: {}\nrole vim_executable: {}\nrole \
         vim_lsp_runtimepath_source: {}\nidentity_manifest: {}\nidentity_manifest_sha256: {}\n\
         handoff: consumer #10944 launches these exact roles; ambient PATH, vimrc, and plugin \
         state stay excluded",
        if outcome.cache_hit { "cache_hit_revalidated" } else { "acquired_and_verified" },
        outcome.manifest.cache_key,
        outcome.vim_executable_role.display(),
        outcome.vim_lsp_runtimepath_role.display(),
        outcome.manifest_path.display(),
        outcome.manifest_sha256,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Result;
    #[cfg(unix)]
    use std::os::unix::fs::MetadataExt;

    const FULL_FEATURE_TEXT: &str = "VIM - Vi IMproved 9.2 (fixture build)\n+channel +job +timers \
                                     huge version with every required transport feature\n";

    /// One hermetic vim-lsp subject fixture: a real git repository at a
    /// pinned commit plus the equivalent inline authority bytes. Building
    /// real git objects keeps the whole identity pipeline honest without
    /// touching the network.
    struct SubjectFixture {
        source_dir: PathBuf,
        authority_bytes: Vec<u8>,
    }

    fn git(repo: &Path, args: &[&str]) -> Result<String> {
        let output = Command::new("git")
            .args(["-c", "user.email=fixture@example.invalid", "-c", "user.name=fixture"])
            .arg("-C")
            .arg(repo)
            .args(args)
            .stdin(Stdio::null())
            .output()
            .with_context(|| format!("running fixture git {args:?}"))?;
        ensure!(
            output.status.success(),
            "fixture git {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    fn build_vimlsp_fixture(root: &Path, name: &str) -> Result<SubjectFixture> {
        let repo = root.join(name);
        fs::create_dir_all(repo.join("plugin").join("lsp"))?;
        fs::create_dir_all(repo.join("autoload").join("lsp"))?;
        let entry_bodies: [(&str, &str); 3] = [
            (
                "plugin/lsp.vim",
                "\" fixture pinned plugin entry\nfunction! lsp#enable() abort\nendfunction\n",
            ),
            ("autoload/lsp.vim", "\" fixture autoload root\n"),
            ("autoload/lsp/utils.vim", "\" fixture utils\n"),
        ];
        for (path, body) in entry_bodies {
            fs::write(repo.join(path), body)?;
        }
        git(&repo, &["init", "--quiet"])?;
        git(&repo, &["add", "--all"])?;
        git(&repo, &["commit", "--quiet", "--no-gpg-sign", "-m", "fixture subject pin"])?;
        let commit = git(&repo, &["rev-parse", "HEAD"])?;
        let tree = git(&repo, &["rev-parse", "HEAD^{tree}"])?;
        let mut entries = Vec::new();
        for (path, _) in entry_bodies {
            entries.push(serde_json::json!({
                "path": path,
                "git_blob_sha1": git(&repo, &["hash-object", "--", path])?,
            }));
        }
        let authority = serde_json::json!({
            "schema_version": "vim_lsp_subject.v1",
            "upstream": {
                "selected_commit": commit,
                "tree_digest": {"algorithm": "git-tree-sha1", "value": tree},
            },
            "expected_content_identity": {"entry_files": entries},
        });
        Ok(SubjectFixture { source_dir: repo, authority_bytes: serde_json::to_vec(&authority)? })
    }

    fn static_probe(version_text: &'static str) -> impl Fn(&Path) -> Result<String> {
        move |_executable| Ok(version_text.to_string())
    }

    /// The checked-in minimal Vim runtime fixture: a tiny valid zip carrying
    /// exactly the required entry names (`vim/vim92/vim.exe` plus a runtime
    /// support file) and nothing else. It exists so the whole provision path
    /// — acquisition, digest binding, extraction, executable verification,
    /// caching, revalidation — runs with zero network in every default test.
    const FIXTURE_ZIP_REPO_PATH: &str =
        "tests/fixtures/vim_host_toolchain/pinned_runtime_fixture.zip";

    fn install_fixture_archive(scratch: &Path) -> Result<ArchiveFixture> {
        let archive_path = scratch.join("pinned_runtime_fixture.zip");
        fs::copy(
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(FIXTURE_ZIP_REPO_PATH),
            &archive_path,
        )
        .with_context(|| format!("copying the checked-in fixture {}", FIXTURE_ZIP_REPO_PATH))?;
        // Digests are computed at test time from the copy and injected as
        // the run's expected values, so the fixture bytes stay authoritative
        // without any hardcoded digest drifting out of sync.
        let archive_sha256 = file_sha256(&archive_path)?;
        let mut zip_file = fs::File::open(&archive_path)?;
        let mut archive =
            zip::ZipArchive::new(&mut zip_file).with_context(|| "reading the fixture archive")?;
        let mut entry = archive
            .by_name(VIM_ARCHIVE_ENTRY)
            .with_context(|| format!("fixture lacks pinned entry {VIM_ARCHIVE_ENTRY}"))?;
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).context("reading fixture executable bytes")?;
        let executable_sha256 = bytes_sha256(&bytes)?;
        Ok(ArchiveFixture { archive_path, archive_sha256, executable_sha256 })
    }

    struct ArchiveFixture {
        archive_path: PathBuf,
        archive_sha256: String,
        executable_sha256: String,
    }

    fn inline_authority(fixture: &SubjectFixture) -> SubjectAuthoritySource {
        SubjectAuthoritySource::Inline {
            display_path: SUBJECT_AUTHORITY_REPO_PATH.to_string(),
            bytes: fixture.authority_bytes.clone(),
        }
    }

    /// Fully offline provision inputs: inline authority, local vim-lsp
    /// checkout source, and the injected archive fixture with its test-time
    /// digests. No default test may construct provision inputs without the
    /// archive injection (the only exceptions fail before acquisition or
    /// are the env-gated `live_network` test).
    fn provision_inputs(
        output_root: &Path,
        fixture: &SubjectFixture,
        archive: &ArchiveFixture,
        probe_text: &'static str,
    ) -> (ProvisionInputs, impl Fn(&Path) -> Result<String>) {
        (
            ProvisionInputs {
                output_root: output_root.to_path_buf(),
                repo_root: output_root.to_path_buf(),
                authority: inline_authority(fixture),
                vim_lsp_source: Some(fixture.source_dir.clone()),
                vim_archive_source: Some(archive.archive_path.clone()),
                vim_archive_expected_sha256: Some(archive.archive_sha256.clone()),
                vim_executable_expected_sha256: Some(archive.executable_sha256.clone()),
                execution_environment: "local_runner".to_string(),
            },
            static_probe(probe_text),
        )
    }

    fn assert_class(error: &InstrumentFailure, class: InstrumentFailureClass) {
        assert_eq!(error.class, class, "unexpected failure: {error}");
    }

    /// The production (pinned-release) identity pins, resolved through the
    /// same path provisioning uses when nothing is injected.
    fn production_pins() -> Result<VimPins> {
        resolve_vim_pins(&no_injection_inputs()).map_err(|error| anyhow::anyhow!("{error}"))
    }

    /// The offline-injection identity for a stand-in archive, without
    /// needing a real fixture on disk.
    fn fixture_archive_pins() -> Result<VimAcquisitionIdentity> {
        Ok(VimAcquisitionIdentity {
            mode: "offline_archive_injection".to_string(),
            source_url: "local:fixture.zip".to_string(),
            tag: format!("{VIM_RELEASE_TAG}-shape-fixture"),
            archive_sha256: "sha256:fixture-archive-digest".to_string(),
            archive_entry: VIM_ARCHIVE_ENTRY.to_string(),
        })
    }

    fn no_injection_inputs() -> ProvisionInputs {
        ProvisionInputs {
            output_root: PathBuf::from("unused"),
            repo_root: PathBuf::from("unused"),
            authority: SubjectAuthoritySource::Inline {
                display_path: SUBJECT_AUTHORITY_REPO_PATH.to_string(),
                bytes: Vec::new(),
            },
            vim_lsp_source: None,
            vim_archive_source: None,
            vim_archive_expected_sha256: None,
            vim_executable_expected_sha256: None,
            execution_environment: "local_runner".to_string(),
        }
    }

    #[test]
    fn roundtrip_provision_and_offline_verify_hermetic() -> Result<()> {
        let scratch = tempfile::tempdir()?;
        let fixture = build_vimlsp_fixture(scratch.path(), "source")?;
        let archive = install_fixture_archive(scratch.path())?;
        let output = scratch.path().join("out");
        let (inputs, probe) = provision_inputs(&output, &fixture, &archive, FULL_FEATURE_TEXT);

        let first = provision(&inputs, &probe).map_err(|error| anyhow::anyhow!("{error}"))?;
        assert!(!first.cache_hit);
        assert_eq!(first.manifest.host_role, HOST_ROLE);
        assert_eq!(
            first.manifest.vim.acquisition.mode, "offline_archive_injection",
            "the hermetic roundtrip must run on the injected archive, never the network"
        );
        assert_eq!(
            first.manifest.vim.executable_sha256,
            file_sha256(first.vim_executable_role.as_path())?
        );
        assert_eq!(first.manifest.vim_lsp.verified_entry_count, 3);
        verify_layout(&first.manifest_path, &probe).map_err(|error| anyhow::anyhow!("{error}"))?;

        // The offline verifier is standalone: it re-runs with no authority
        // reread and no network, from the manifest alone.
        let second_verify = verify_layout(&first.manifest_path, &probe);
        assert!(second_verify.is_ok());
        Ok(())
    }

    #[test]
    fn manifest_bytes_are_identical_across_independent_roots() -> Result<()> {
        let scratch = tempfile::tempdir()?;
        let fixture = build_vimlsp_fixture(scratch.path(), "source")?;
        let archive = install_fixture_archive(scratch.path())?;
        let left_inputs = ProvisionInputs {
            output_root: scratch.path().join("left"),
            repo_root: scratch.path().to_path_buf(),
            authority: inline_authority(&fixture),
            vim_lsp_source: Some(fixture.source_dir.clone()),
            vim_archive_source: Some(archive.archive_path.clone()),
            vim_archive_expected_sha256: Some(archive.archive_sha256.clone()),
            vim_executable_expected_sha256: Some(archive.executable_sha256.clone()),
            execution_environment: "ci_runner".to_string(),
        };
        let right_inputs = ProvisionInputs {
            output_root: scratch.path().join("right"),
            repo_root: scratch.path().to_path_buf(),
            authority: inline_authority(&fixture),
            vim_lsp_source: Some(fixture.source_dir.clone()),
            vim_archive_source: Some(archive.archive_path.clone()),
            vim_archive_expected_sha256: Some(archive.archive_sha256.clone()),
            vim_executable_expected_sha256: Some(archive.executable_sha256.clone()),
            execution_environment: "ci_runner".to_string(),
        };
        let probe = static_probe(FULL_FEATURE_TEXT);
        let left = provision(&left_inputs, &probe).map_err(|error| anyhow::anyhow!("{error}"))?;
        let right = provision(&right_inputs, &probe).map_err(|error| anyhow::anyhow!("{error}"))?;
        let left_bytes = fs::read(&left.manifest_path)?;
        let right_bytes = fs::read(&right.manifest_path)?;
        assert_eq!(left_bytes, right_bytes, "durable identity must be machine-path independent");
        assert!(!left_bytes.is_empty());
        Ok(())
    }

    #[test]
    fn cache_key_covers_every_load_bearing_input() -> Result<()> {
        let base = PlatformFields {
            os: "windows".into(),
            arch: "x86_64".into(),
            execution_environment: "local_runner".into(),
        };
        let acquisition = production_pins()?.acquisition;
        let isolation = IsolationPolicy::governed();
        let key_of = |platform: &PlatformFields,
                      acquisition: &VimAcquisitionIdentity,
                      features: &[String],
                      authority: &str,
                      commit: &str,
                      load_mode: &str,
                      isolation: &IsolationPolicy| {
            cache_key(&CacheKeyInputs {
                schema_version: SCHEMA_VERSION,
                instrument_version: INSTRUMENT_VERSION,
                host_role: HOST_ROLE,
                platform,
                vim_acquisition: acquisition,
                vim_required_features: features.to_vec(),
                vim_lsp_subject_authority_path: SUBJECT_AUTHORITY_REPO_PATH,
                vim_lsp_subject_authority_sha256: authority,
                vim_lsp_selected_commit: commit,
                vim_lsp_load_mode: load_mode,
                isolation,
            })
        };
        let baseline = key_of(
            &base,
            &acquisition,
            &required_feature_list(),
            "authority-digest",
            "0123456789abcdef0123456789abcdef01234567",
            LOAD_MODE_CALLER_PINNED_CHECKOUT,
            &isolation,
        )?;

        let mutated_platform = PlatformFields { arch: "aarch64".into(), ..base.clone() };
        let mutated_features = vec!["channel".to_string()];
        let mutated_acquisition =
            VimAcquisitionIdentity { archive_sha256: "ff".repeat(32), ..acquisition.clone() };
        let mutated_isolation =
            IsolationPolicy { ambient_user_plugins: "admitted".into(), ..isolation.clone() };

        assert_ne!(
            baseline,
            key_of(
                &mutated_platform,
                &acquisition,
                &required_feature_list(),
                "authority-digest",
                "0123456789abcdef0123456789abcdef01234567",
                LOAD_MODE_CALLER_PINNED_CHECKOUT,
                &isolation
            )?
        );
        assert_ne!(
            baseline,
            key_of(
                &base,
                &mutated_acquisition,
                &required_feature_list(),
                "authority-digest",
                "0123456789abcdef0123456789abcdef01234567",
                LOAD_MODE_CALLER_PINNED_CHECKOUT,
                &isolation
            )?
        );
        assert_ne!(
            baseline,
            key_of(
                &base,
                &acquisition,
                &mutated_features,
                "authority-digest",
                "0123456789abcdef0123456789abcdef01234567",
                LOAD_MODE_CALLER_PINNED_CHECKOUT,
                &isolation
            )?
        );
        assert_ne!(
            baseline,
            key_of(
                &base,
                &acquisition,
                &required_feature_list(),
                "other-authority",
                "0123456789abcdef0123456789abcdef01234567",
                LOAD_MODE_CALLER_PINNED_CHECKOUT,
                &isolation
            )?
        );
        assert_ne!(
            baseline,
            key_of(
                &base,
                &acquisition,
                &required_feature_list(),
                "authority-digest",
                "fedcba9876543210fedcba9876543210fedcba98",
                LOAD_MODE_CALLER_PINNED_CHECKOUT,
                &isolation
            )?
        );
        assert_ne!(
            baseline,
            key_of(
                &base,
                &acquisition,
                &required_feature_list(),
                "authority-digest",
                "0123456789abcdef0123456789abcdef01234567",
                "some_other_load_mode",
                &isolation
            )?
        );
        assert_ne!(
            baseline,
            key_of(
                &base,
                &acquisition,
                &required_feature_list(),
                "authority-digest",
                "0123456789abcdef0123456789abcdef01234567",
                LOAD_MODE_CALLER_PINNED_CHECKOUT,
                &mutated_isolation
            )?
        );
        // The offline-injection identity is a different subject from the
        // pinned release: an injected test entry can never collide with (or
        // silently stand in for) a production cache entry.
        let injected = fixture_archive_pins()?;
        assert_ne!(
            baseline,
            key_of(
                &base,
                &injected,
                &required_feature_list(),
                "authority-digest",
                "0123456789abcdef0123456789abcdef01234567",
                LOAD_MODE_CALLER_PINNED_CHECKOUT,
                &isolation
            )?
        );
        Ok(())
    }

    #[test]
    fn warm_cache_second_run_hits_and_keeps_manifest_stable() -> Result<()> {
        let scratch = tempfile::tempdir()?;
        let fixture = build_vimlsp_fixture(scratch.path(), "source")?;
        let archive = install_fixture_archive(scratch.path())?;
        let output = scratch.path().join("out");
        let (inputs, probe) = provision_inputs(&output, &fixture, &archive, FULL_FEATURE_TEXT);
        let first = provision(&inputs, &probe).map_err(|error| anyhow::anyhow!("{error}"))?;
        let before = fs::read(&first.manifest_path)?;
        let second = provision(&inputs, &probe).map_err(|error| anyhow::anyhow!("{error}"))?;
        assert!(second.cache_hit, "identical rerun must be an exact-identity cache hit");
        assert_eq!(first.manifest_path, second.manifest_path);
        assert_eq!(before, fs::read(&second.manifest_path)?);
        Ok(())
    }

    #[test]
    fn legacy_manifest_migrates_without_vim_lsp_reacquisition() -> Result<()> {
        let scratch = tempfile::tempdir()?;
        let fixture = build_vimlsp_fixture(scratch.path(), "source")?;
        let archive = install_fixture_archive(scratch.path())?;
        let output = scratch.path().join("out");
        let (mut inputs, probe) = provision_inputs(&output, &fixture, &archive, FULL_FEATURE_TEXT);
        let first = provision(&inputs, &probe).map_err(|error| anyhow::anyhow!("{error}"))?;
        let entry_root = first
            .manifest_path
            .parent()
            .ok_or_else(|| anyhow::anyhow!("manifest path has no parent directory"))?
            .to_path_buf();
        let fresh_bytes = legacyize_entry(&entry_root, &first.manifest_path, &archive)?;

        // With no local vim-lsp source, any rebuild would need network access.
        // Successful migration therefore proves the existing checkout was
        // retained and reverified locally.
        inputs.vim_lsp_source = None;
        let migrated = provision(&inputs, &probe).map_err(|error| anyhow::anyhow!("{error}"))?;
        assert!(migrated.cache_hit);
        assert!(migrated.manifest.vim.runtime_tree_sha256.starts_with("sha256:"));
        verify_layout(&migrated.manifest_path, &probe)
            .map_err(|error| anyhow::anyhow!("{error}"))?;
        // Canonical manifest-byte contract: a migrated entry and a freshly
        // provisioned entry with the same identity expose identical bytes,
        // so manifest_sha256 depends on identity, not on migration history.
        assert_eq!(fs::read(&migrated.manifest_path)?, fresh_bytes);
        assert_eq!(migrated.manifest_sha256, bytes_sha256(&fresh_bytes)?);
        Ok(())
    }

    #[test]
    fn legacy_runtime_corruption_declines_migration_and_rebuild_heals() -> Result<()> {
        let scratch = tempfile::tempdir()?;
        let fixture = build_vimlsp_fixture(scratch.path(), "source")?;
        let archive = install_fixture_archive(scratch.path())?;
        let output = scratch.path().join("out");
        let (inputs, probe) = provision_inputs(&output, &fixture, &archive, FULL_FEATURE_TEXT);
        let first = provision(&inputs, &probe).map_err(|error| anyhow::anyhow!("{error}"))?;
        let entry_root = first
            .manifest_path
            .parent()
            .ok_or_else(|| anyhow::anyhow!("manifest path has no parent directory"))?
            .to_path_buf();
        let support_file = first
            .vim_executable_role
            .parent()
            .ok_or_else(|| anyhow::anyhow!("provisioned Vim executable has no parent"))?
            .join("runtime/feature.txt");
        let original = fs::read(&support_file)?;
        let fresh_digest = first.manifest.vim.runtime_tree_sha256.clone();
        legacyize_entry(&entry_root, &first.manifest_path, &archive)?;

        // Mutate the legacy runtime AFTER legacyizing: these bytes were never
        // bound by the old manifest. A migration hashing the on-disk tree
        // would return cache_hit: true carrying a digest of corrupted bytes
        // (self-attestation).
        fs::write(&support_file, [original.as_slice(), b"corruption".as_slice()].concat())?;

        let rebuilt = provision(&inputs, &probe).map_err(|error| anyhow::anyhow!("{error}"))?;
        assert!(!rebuilt.cache_hit);
        assert_eq!(rebuilt.manifest.vim.runtime_tree_sha256, fresh_digest);
        assert_eq!(fs::read(&support_file)?, original);
        verify_layout(&rebuilt.manifest_path, &probe)
            .map_err(|error| anyhow::anyhow!("{error}"))?;
        Ok(())
    }

    #[test]
    fn legacy_corruption_without_verified_archive_cannot_cache_hit() -> Result<()> {
        let scratch = tempfile::tempdir()?;
        let fixture = build_vimlsp_fixture(scratch.path(), "source")?;
        let archive = install_fixture_archive(scratch.path())?;
        let output = scratch.path().join("out");
        let (inputs, probe) = provision_inputs(&output, &fixture, &archive, FULL_FEATURE_TEXT);
        let first = provision(&inputs, &probe).map_err(|error| anyhow::anyhow!("{error}"))?;
        let entry_root = first
            .manifest_path
            .parent()
            .ok_or_else(|| anyhow::anyhow!("manifest path has no parent directory"))?
            .to_path_buf();
        let support_file = first
            .vim_executable_role
            .parent()
            .ok_or_else(|| anyhow::anyhow!("provisioned Vim executable has no parent"))?
            .join("runtime/feature.txt");
        let original = fs::read(&support_file)?;
        let fresh_digest = first.manifest.vim.runtime_tree_sha256.clone();
        legacyize_entry(&entry_root, &first.manifest_path, &archive)?;

        // Corrupt the legacy runtime AND remove the retained archive, so the
        // migration has no digest-verified identity source and must decline
        // rather than bless the mutated tree; the ordinary rebuild path (kept
        // offline here through the injected vim-lsp source) heals the entry.
        fs::write(&support_file, [original.as_slice(), b"corruption".as_slice()].concat())?;
        fs::remove_file(entry_root.join("downloads").join(format!("gvim_{VIM_RELEASE_TAG}.zip")))?;

        let rebuilt = provision(&inputs, &probe).map_err(|error| anyhow::anyhow!("{error}"))?;
        assert!(!rebuilt.cache_hit);
        assert_eq!(rebuilt.manifest.vim.runtime_tree_sha256, fresh_digest);
        verify_layout(&rebuilt.manifest_path, &probe)
            .map_err(|error| anyhow::anyhow!("{error}"))?;
        Ok(())
    }

    fn create_dir_symlink(target: &Path, link: &Path) -> std::io::Result<()> {
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(target, link)
        }
        #[cfg(windows)]
        {
            std::os::windows::fs::symlink_dir(target, link)
        }
    }

    #[test]
    fn runtime_tree_sha256_rejects_symlinked_root() -> Result<()> {
        let scratch = tempfile::tempdir()?;
        let real = scratch.path().join("real");
        fs::create_dir_all(real.join("runtime"))?;
        fs::write(real.join("runtime").join("feature.txt"), b"payload")?;
        let link = scratch.path().join("link");
        // Symlink creation requires platform privileges; an environment that
        // refuses it cannot exercise this invariant here and skips, while the
        // ordinary-root control below still runs everywhere.
        if create_dir_symlink(&real, &link).is_err() {
            return Ok(());
        }
        let error =
            runtime_tree_sha256(&link).expect_err("symlinked runtime root must be rejected");
        assert!(format!("{error:#}").contains("is a symlink"), "{error}");
        let real_digest = runtime_tree_sha256(&real)?;
        assert!(real_digest.starts_with("sha256:"));
        Ok(())
    }

    #[test]
    fn legacy_symlinked_runtime_root_declines_migration_without_touching_target() -> Result<()> {
        let scratch = tempfile::tempdir()?;
        let fixture = build_vimlsp_fixture(scratch.path(), "source")?;
        let archive = install_fixture_archive(scratch.path())?;
        let output = scratch.path().join("out");
        let (inputs, probe) = provision_inputs(&output, &fixture, &archive, FULL_FEATURE_TEXT);
        let first = provision(&inputs, &probe).map_err(|error| anyhow::anyhow!("{error}"))?;
        let entry_root = first
            .manifest_path
            .parent()
            .ok_or_else(|| anyhow::anyhow!("manifest path has no parent directory"))?
            .to_path_buf();
        let runtime_root = entry_root.join("vim").join("vim92");
        let target = scratch.path().join("legacy-runtime-target");
        let support_file = target.join("runtime/feature.txt");
        let original = fs::read(runtime_root.join("runtime/feature.txt"))?;
        legacyize_entry(&entry_root, &first.manifest_path, &archive)?;
        fs::rename(&runtime_root, &target)?;
        // Symlink creation requires platform privileges; an environment that
        // refuses it cannot exercise this invariant here and skips.
        if create_dir_symlink(&target, &runtime_root).is_err() {
            return Ok(());
        }

        let rebuilt = provision(&inputs, &probe).map_err(|error| anyhow::anyhow!("{error}"))?;
        assert!(!rebuilt.cache_hit);
        assert!(target.is_dir());
        assert_eq!(fs::read(&support_file)?, original);
        verify_layout(&rebuilt.manifest_path, &probe)
            .map_err(|error| anyhow::anyhow!("{error}"))?;
        Ok(())
    }

    #[test]
    fn legacy_migration_does_not_rewrite_live_runtime_or_leave_staging() -> Result<()> {
        let scratch = tempfile::tempdir()?;
        let fixture = build_vimlsp_fixture(scratch.path(), "source")?;
        let archive = install_fixture_archive(scratch.path())?;
        let output = scratch.path().join("out");
        let (mut inputs, probe) = provision_inputs(&output, &fixture, &archive, FULL_FEATURE_TEXT);
        let first = provision(&inputs, &probe).map_err(|error| anyhow::anyhow!("{error}"))?;
        let entry_root = first
            .manifest_path
            .parent()
            .ok_or_else(|| anyhow::anyhow!("manifest path has no parent directory"))?
            .to_path_buf();
        let runtime_root = entry_root.join("vim").join("vim92");
        let mut snapshots = Vec::new();
        for entry in WalkDir::new(&runtime_root).follow_links(false) {
            let entry = entry?;
            if !entry.file_type().is_file() {
                continue;
            }
            let metadata = fs::metadata(entry.path())?;
            #[cfg(unix)]
            let inode = Some(metadata.ino());
            #[cfg(not(unix))]
            let inode: Option<u64> = None;
            snapshots.push((entry.path().to_path_buf(), metadata.modified()?, inode));
        }
        legacyize_entry(&entry_root, &first.manifest_path, &archive)?;

        inputs.vim_lsp_source = None;
        let migrated = provision(&inputs, &probe).map_err(|error| anyhow::anyhow!("{error}"))?;
        assert!(migrated.cache_hit);
        for (path, modified, inode) in snapshots {
            let metadata = fs::metadata(path)?;
            assert_eq!(metadata.modified()?, modified);
            #[cfg(unix)]
            let current_inode = Some(metadata.ino());
            #[cfg(not(unix))]
            let current_inode: Option<u64> = None;
            assert_eq!(current_inode, inode, "runtime file identity changed during migration");
        }
        let parent = entry_root
            .parent()
            .ok_or_else(|| anyhow::anyhow!("entry path has no parent directory"))?;
        for sibling in fs::read_dir(parent)? {
            let sibling = sibling?;
            let name = sibling.file_name().to_string_lossy().into_owned();
            assert!(
                !name.contains(".migrate."),
                "migration staging residue remained: {}",
                sibling.path().display()
            );
        }
        Ok(())
    }

    /// Re-create a production-shaped legacy entry: the pinned archive stays
    /// under the entry's downloads directory (every acquired entry has one)
    /// and the manifest predates the runtime-digest field. Returns the fresh
    /// manifest bytes for canonical-byte comparisons.
    fn legacyize_entry(
        entry_root: &Path,
        manifest_path: &Path,
        archive: &ArchiveFixture,
    ) -> Result<Vec<u8>> {
        let downloads = entry_root.join("downloads");
        fs::create_dir_all(&downloads)?;
        fs::copy(&archive.archive_path, downloads.join(format!("gvim_{VIM_RELEASE_TAG}.zip")))?;
        let fresh_bytes = fs::read(manifest_path)?;
        let mut legacy = serde_json::from_slice::<serde_json::Value>(&fresh_bytes)?;
        legacy
            .get_mut("vim")
            .and_then(serde_json::Value::as_object_mut)
            .and_then(|vim| vim.remove("runtime_tree_sha256"))
            .ok_or_else(|| anyhow::anyhow!("fresh manifest did not contain runtime digest"))?;
        fs::write(manifest_path, serde_json::to_vec_pretty(&legacy)?)?;
        Ok(fresh_bytes)
    }

    #[test]
    fn corrupt_cached_byte_fails_verification_then_rebuild_heals() -> Result<()> {
        let scratch = tempfile::tempdir()?;
        let fixture = build_vimlsp_fixture(scratch.path(), "source")?;
        let archive = install_fixture_archive(scratch.path())?;
        let output = scratch.path().join("out");
        let (inputs, probe) = provision_inputs(&output, &fixture, &archive, FULL_FEATURE_TEXT);
        let first = provision(&inputs, &probe).map_err(|error| anyhow::anyhow!("{error}"))?;

        // Mutation: flip one cached executable byte.
        let exe = first.vim_executable_role.clone();
        let mut bytes = fs::read(&exe)?;
        let last = bytes.len() - 1;
        bytes[last] ^= 0xFF;
        fs::write(&exe, &bytes)?;

        let drifted = verify_layout(&first.manifest_path, &probe).unwrap_err();
        assert_class(&drifted, InstrumentFailureClass::IdentityMismatch);

        // Provision over the drifted cache must rebuild, never satisfy
        // silently. Healing needs no network in either shape: the injected
        // source is offline by construction, and a production rebuild
        // carries the digest-valid archive forward into the staging layout
        // (preserve_verified_archive) instead of deleting it.
        let healed = provision(&inputs, &probe).map_err(|error| anyhow::anyhow!("{error}"))?;
        assert!(!healed.cache_hit);
        verify_layout(&healed.manifest_path, &probe).map_err(|error| anyhow::anyhow!("{error}"))?;
        Ok(())
    }

    #[test]
    fn corrupt_cached_runtime_support_file_is_rejected() -> Result<()> {
        let scratch = tempfile::tempdir()?;
        let fixture = build_vimlsp_fixture(scratch.path(), "source")?;
        let archive = install_fixture_archive(scratch.path())?;
        let output = scratch.path().join("out");
        let (inputs, probe) = provision_inputs(&output, &fixture, &archive, FULL_FEATURE_TEXT);
        let first = provision(&inputs, &probe).map_err(|error| anyhow::anyhow!("{error}"))?;

        let support_file = first
            .vim_executable_role
            .parent()
            .ok_or_else(|| anyhow::anyhow!("provisioned Vim executable has no parent"))?
            .join("runtime/feature.txt");
        let mut bytes = fs::read(&support_file)?;
        bytes.push(b'\n');
        fs::write(&support_file, bytes)?;

        let error = verify_layout(&first.manifest_path, &probe).unwrap_err();
        assert_class(&error, InstrumentFailureClass::IdentityMismatch);
        assert!(error.detail.contains("runtime digest"), "{error}");
        Ok(())
    }

    fn copy_tree(src: &Path, dst: &Path) -> Result<()> {
        fs::create_dir_all(dst)?;
        for entry in fs::read_dir(src)? {
            let from = entry?.path();
            let Some(name) = from.file_name() else { continue };
            let to = dst.join(name);
            if from.is_dir() {
                copy_tree(&from, &to)?;
            } else {
                fs::copy(&from, &to)?;
            }
        }
        Ok(())
    }

    /// The exact codex P1 scenario: material from another governed identity
    /// placed under this run's key directory must never be reported as a
    /// cache hit merely because it internally reproduces its own key.
    #[test]
    fn cache_hit_requires_the_currently_requested_key() -> Result<()> {
        let scratch = tempfile::tempdir()?;
        let fixture = build_vimlsp_fixture(scratch.path(), "source")?;
        let archive = install_fixture_archive(scratch.path())?;
        let output = scratch.path().join("out");
        let mk_inputs = |environment: &str| ProvisionInputs {
            output_root: output.clone(),
            repo_root: output.clone(),
            authority: inline_authority(&fixture),
            vim_lsp_source: Some(fixture.source_dir.clone()),
            vim_archive_source: Some(archive.archive_path.clone()),
            vim_archive_expected_sha256: Some(archive.archive_sha256.clone()),
            vim_executable_expected_sha256: Some(archive.executable_sha256.clone()),
            execution_environment: environment.to_string(),
        };
        let probe = static_probe(FULL_FEATURE_TEXT);

        let installed = provision(&mk_inputs("local_runner"), &probe)
            .map_err(|error| anyhow::anyhow!("{error}"))?;
        let foreign_material = installed
            .manifest_path
            .parent()
            .ok_or_else(|| anyhow::anyhow!("manifest path has no parent directory"))?
            .to_path_buf();
        let foreign_bytes = fs::read(&installed.manifest_path)?;

        let target = provision(&mk_inputs("ci_runner"), &probe)
            .map_err(|error| anyhow::anyhow!("{error}"))?;
        let target_key = target.manifest.cache_key.clone();
        let target_dir = target
            .manifest_path
            .parent()
            .ok_or_else(|| anyhow::anyhow!("manifest path has no parent directory"))?
            .to_path_buf();

        // Hostile placement: the foreign (different-key) entry replaces the
        // content of the currently requested key's directory.
        let target_bytes = fs::read(&target.manifest_path)?;
        assert_ne!(target_bytes, foreign_bytes);
        fs::remove_dir_all(&target_dir)?;
        copy_tree(&foreign_material, &target_dir)?;
        assert_eq!(fs::read(&target.manifest_path)?, foreign_bytes);

        let reprovisioned = provision(&mk_inputs("ci_runner"), &probe)
            .map_err(|error| anyhow::anyhow!("{error}"))?;
        assert!(
            !reprovisioned.cache_hit,
            "foreign internal-key-consistent material must not satisfy another key"
        );
        assert_eq!(reprovisioned.manifest.cache_key, target_key);
        verify_layout(&reprovisioned.manifest_path, &probe)
            .map_err(|error| anyhow::anyhow!("{error}"))?;
        Ok(())
    }

    /// The healing law: only a digest-valid pinned download may be carried
    /// across a rebuild, and any absence/drift degrades to plain acquisition.
    #[test]
    fn preserve_verified_archive_carries_only_digest_valid_downloads() -> Result<()> {
        let scratch = tempfile::tempdir()?;
        let superseded = scratch.path().join("entry");
        let staging_valid = scratch.path().join("staging-valid");
        let staging_drifted = scratch.path().join("staging-drifted");
        let archive_name = format!("gvim_{VIM_RELEASE_TAG}.zip");
        let legacy = superseded.join("downloads").join(&archive_name);
        fs::create_dir_all(
            legacy
                .parent()
                .ok_or_else(|| anyhow::anyhow!("archive path has no parent directory"))?,
        )?;
        fs::write(&legacy, b"verified pinned archive bytes")?;
        let pinned_digest = file_sha256(&legacy)?;

        preserve_verified_archive(&superseded, &staging_valid, &pinned_digest);
        let carried = staging_valid.join("downloads").join(&archive_name);
        assert_eq!(fs::read(&carried)?, b"verified pinned archive bytes");

        fs::write(&legacy, b"drifted immutable subject bytes")?;
        preserve_verified_archive(&superseded, &staging_drifted, &pinned_digest);
        assert!(!staging_drifted.join("downloads").exists());

        preserve_verified_archive(
            &scratch.path().join("missing"),
            &staging_drifted,
            &pinned_digest,
        );
        Ok(())
    }

    /// A relative `--output` invocation must be anchored to the working
    /// directory before any layout math, or the absolute-path consumption
    /// contracts fail only after a full network acquisition.
    #[test]
    fn relative_output_roots_are_anchored_before_layout_math() -> Result<()> {
        let anchored = absolutized_output_root(Path::new("target/vim-toolchain"))
            .map_err(|error| anyhow::anyhow!("{error}"))?;
        let cwd = std::env::current_dir()?;
        assert!(anchored.is_absolute());
        assert_eq!(anchored, cwd.join("target/vim-toolchain"));
        let already = absolutized_output_root(&cwd.join("elsewhere/out"))
            .map_err(|error| anyhow::anyhow!("{error}"))?;
        assert_eq!(already, cwd.join("elsewhere/out"));
        Ok(())
    }

    #[test]
    fn sequential_provisions_sharing_root_stay_deterministic() -> Result<()> {
        let scratch = tempfile::tempdir()?;
        let fixture = build_vimlsp_fixture(scratch.path(), "source")?;
        let archive = install_fixture_archive(scratch.path())?;
        let output = scratch.path().join("out");
        let (inputs, probe) = provision_inputs(&output, &fixture, &archive, FULL_FEATURE_TEXT);

        let first = provision(&inputs, &probe).map_err(|error| anyhow::anyhow!("{error}"))?;
        let first_bytes = fs::read(&first.manifest_path)?;
        let first_key = first.manifest.cache_key.clone();

        // Sharing the root is a pure revalidation.
        let second = provision(&inputs, &probe).map_err(|error| anyhow::anyhow!("{error}"))?;
        assert!(second.cache_hit);

        // After the whole entry is wiped, the rebuild lands on the exact
        // same key and byte-identical durable identity: no timestamps, no
        // machine paths, no staging residue in what gets published.
        fs::remove_dir_all(&output)?;
        let third = provision(&inputs, &probe).map_err(|error| anyhow::anyhow!("{error}"))?;
        assert!(!third.cache_hit);
        assert_eq!(third.manifest.cache_key, first_key);
        assert_eq!(fs::read(&third.manifest_path)?, first_bytes);
        Ok(())
    }

    #[test]
    fn same_version_text_with_different_bytes_is_rejected() -> Result<()> {
        let scratch = tempfile::tempdir()?;
        let fixture = build_vimlsp_fixture(scratch.path(), "source")?;
        let archive = install_fixture_archive(scratch.path())?;
        let output = scratch.path().join("out");
        let (inputs, probe) = provision_inputs(&output, &fixture, &archive, FULL_FEATURE_TEXT);
        let first = provision(&inputs, &probe).map_err(|error| anyhow::anyhow!("{error}"))?;

        // Substitute different executable bytes while the version probe text
        // stays byte-for-byte identical: false-subject control 3.
        let subject = first.vim_executable_role.clone();
        let original_len = fs::metadata(&subject)?.len() as usize;
        fs::write(&subject, vec![0xEEu8; original_len])?;

        let rejected = verify_layout(&first.manifest_path, &probe).unwrap_err();
        assert_class(&rejected, InstrumentFailureClass::IdentityMismatch);
        assert!(rejected.detail.contains("executable digest"), "{rejected}");
        Ok(())
    }

    #[test]
    fn unknown_manifest_field_fails_closed() -> Result<()> {
        let scratch = tempfile::tempdir()?;
        let fixture = build_vimlsp_fixture(scratch.path(), "source")?;
        let archive = install_fixture_archive(scratch.path())?;
        let output = scratch.path().join("out");
        let (inputs, probe) = provision_inputs(&output, &fixture, &archive, FULL_FEATURE_TEXT);
        let first = provision(&inputs, &probe).map_err(|error| anyhow::anyhow!("{error}"))?;
        let mut text = fs::read_to_string(&first.manifest_path)?;
        text = text.replace('{', "{\n  \"future_field\": 1,");
        let tampered = first.manifest_path.with_file_name("tampered.json");
        fs::write(&tampered, &text)?;
        let error = verify_layout(&tampered, &probe).unwrap_err();
        assert_class(&error, InstrumentFailureClass::IdentityMismatch);
        Ok(())
    }

    #[test]
    fn role_relabel_is_rejected_before_anything_else_matters() -> Result<()> {
        let scratch = tempfile::tempdir()?;
        let fixture = build_vimlsp_fixture(scratch.path(), "source")?;
        let archive = install_fixture_archive(scratch.path())?;
        let output = scratch.path().join("out");
        let (inputs, probe) = provision_inputs(&output, &fixture, &archive, FULL_FEATURE_TEXT);
        let first = provision(&inputs, &probe).map_err(|error| anyhow::anyhow!("{error}"))?;
        let relabeled = format!("\"host_role\": \"{}\"", "neovim_nvim_lsp_host");
        let relabeled = fs::read_to_string(&first.manifest_path)?
            .replace(&format!("\"host_role\": \"{HOST_ROLE}\""), &relabeled);
        let tampered = first.manifest_path.with_file_name("relabeled.json");
        fs::write(&tampered, &relabeled)?;
        let error = verify_layout(&tampered, &probe).unwrap_err();
        assert_class(&error, InstrumentFailureClass::IdentityMismatch);
        assert!(error.detail.contains("relabeled"), "{error}");
        Ok(())
    }

    #[test]
    fn missing_required_feature_refuses_provision() -> Result<()> {
        const NO_JOB_TEXT: &str = "VIM - Vi IMproved 9.2 (fixture build)\n+channel -job +timers\n";
        let scratch = tempfile::tempdir()?;
        let fixture = build_vimlsp_fixture(scratch.path(), "source")?;
        let archive = install_fixture_archive(scratch.path())?;
        let output = scratch.path().join("out");
        let (inputs, probe) = provision_inputs(&output, &fixture, &archive, NO_JOB_TEXT);
        let failure = provision(&inputs, &probe)
            .err()
            .ok_or_else(|| anyhow::anyhow!("provisioning must fail without +job"))?;
        assert_class(&failure, InstrumentFailureClass::IdentityMismatch);
        assert!(failure.detail.contains("+job"), "{failure}");
        Ok(())
    }

    #[test]
    fn garbage_authority_is_authority_unreadable() -> Result<()> {
        let scratch = tempfile::tempdir()?;
        // Fails at authority load, before any acquisition stage: no archive
        // injection needed and no network is reachable from this path.
        let inputs = ProvisionInputs {
            output_root: scratch.path().join("out"),
            repo_root: scratch.path().to_path_buf(),
            authority: SubjectAuthoritySource::Inline {
                display_path: "inline".to_string(),
                bytes: b"{ not the governed schema".to_vec(),
            },
            vim_lsp_source: None,
            vim_archive_source: None,
            vim_archive_expected_sha256: None,
            vim_executable_expected_sha256: None,
            execution_environment: "local_runner".to_string(),
        };
        let failure = provision(&inputs, &static_probe(FULL_FEATURE_TEXT))
            .err()
            .ok_or_else(|| anyhow::anyhow!("garbage authority must fail provisioning"))?;
        assert_class(&failure, InstrumentFailureClass::AuthorityUnreadable);
        Ok(())
    }

    #[test]
    fn unavailable_local_source_is_acquisition_unavailable() -> Result<()> {
        let scratch = tempfile::tempdir()?;
        let fixture = build_vimlsp_fixture(scratch.path(), "source")?;
        let archive = install_fixture_archive(scratch.path())?;
        let inputs = ProvisionInputs {
            output_root: scratch.path().join("out"),
            repo_root: scratch.path().to_path_buf(),
            authority: inline_authority(&fixture),
            vim_lsp_source: Some(scratch.path().join("missing-source")),
            vim_archive_source: Some(archive.archive_path.clone()),
            vim_archive_expected_sha256: Some(archive.archive_sha256.clone()),
            vim_executable_expected_sha256: Some(archive.executable_sha256.clone()),
            execution_environment: "local_runner".to_string(),
        };
        let failure = provision(&inputs, &static_probe(FULL_FEATURE_TEXT))
            .err()
            .ok_or_else(|| anyhow::anyhow!("unavailable source must fail"))?;
        assert_class(&failure, InstrumentFailureClass::AcquisitionUnavailable);
        assert!(failure.to_string().starts_with("instrument_failed(acquisition_unavailable)"));
        Ok(())
    }

    #[test]
    fn archive_traversal_entry_is_refused() -> Result<()> {
        let scratch = tempfile::tempdir()?;
        let archive_path = scratch.path().join("hostile.zip");
        let file = fs::File::create(&archive_path)?;
        let mut writer = zip::ZipWriter::new(file);
        writer
            .start_file("../evil.vim", zip::write::SimpleFileOptions::default())
            .map_err(|error| anyhow::anyhow!("{error}"))?;
        std::io::Write::write_all(&mut writer, b"payload")
            .map_err(|error| anyhow::anyhow!("{error}"))?;
        writer.finish().map_err(|error| anyhow::anyhow!("{error}"))?;
        let dest = scratch.path().join("extracted");
        let error = extract_runtime_subtree(&archive_path, VIM_ARCHIVE_RUNTIME_SUBTREE, &dest)
            .err()
            .ok_or_else(|| anyhow::anyhow!("traversal entry must be refused"))?;
        assert!(error.to_string().contains("path escape"), "{error}");
        Ok(())
    }

    #[test]
    fn machine_absolute_paths_are_refused_in_durable_identity() -> Result<()> {
        let scratch = tempfile::tempdir()?;
        let leaky = format!(
            "{{\"schema_version\":\"{SCHEMA_VERSION}\",\"note\":\"{}\"}}",
            scratch.path().display()
        );
        let error = assert_no_machine_paths(leaky.as_bytes(), &[scratch.path()])
            .err()
            .ok_or_else(|| anyhow::anyhow!("machine-path leak must be refused"))?;
        assert!(error.to_string().contains("machine path"), "{error}");
        Ok(())
    }

    #[test]
    fn archive_symlink_entry_is_refused() -> Result<()> {
        let scratch = tempfile::tempdir()?;
        let archive_path = scratch.path().join("symlink.zip");
        let file = fs::File::create(&archive_path)?;
        let mut writer = zip::ZipWriter::new(file);
        // An entry that carries unix symlink mode bits inside the pinned
        // subtree: extraction must reject it explicitly, matching the doc
        // claim, instead of relying on incidental file/dir classification.
        writer
            .add_symlink(
                "vim/vim92/link.vim",
                "../evil.vim",
                zip::write::SimpleFileOptions::default(),
            )
            .map_err(|error| anyhow::anyhow!("{error}"))?;
        writer.finish().map_err(|error| anyhow::anyhow!("{error}"))?;
        let dest = scratch.path().join("extracted");
        let error = extract_runtime_subtree(&archive_path, VIM_ARCHIVE_RUNTIME_SUBTREE, &dest)
            .err()
            .ok_or_else(|| anyhow::anyhow!("symlink entry must be refused"))?;
        assert!(error.to_string().contains("symlink"), "{error}");
        Ok(())
    }

    /// The single opt-in live-network test: it downloads the real pinned
    /// Vim release archive and fetches the real pinned vim-lsp commit.
    /// CI default stays offline — the test exits early unless
    /// `XTASK_VIM_TOOLCHAIN_LIVE_NETWORK=1`.
    #[test]
    fn live_network_provisions_the_real_pinned_subjects() -> Result<()> {
        if std::env::var_os("XTASK_VIM_TOOLCHAIN_LIVE_NETWORK").is_none() {
            let _ = std::io::Write::write_all(
                &mut std::io::stderr().lock(),
                b"skipping live_network test: set XTASK_VIM_TOOLCHAIN_LIVE_NETWORK=1 to \
                  exercise the real pinned acquisition\n",
            );
            return Ok(());
        }
        let scratch = tempfile::tempdir()?;
        let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
        let inputs = ProvisionInputs {
            output_root: scratch.path().join("out"),
            repo_root: repo_root.clone(),
            authority: SubjectAuthoritySource::RepoRoot(repo_root),
            vim_lsp_source: None,
            vim_archive_source: None,
            vim_archive_expected_sha256: None,
            vim_executable_expected_sha256: None,
            execution_environment: "local_runner".to_string(),
        };
        let outcome =
            provision(&inputs, &probe_vim_version).map_err(|error| anyhow::anyhow!("{error}"))?;
        assert!(!outcome.cache_hit);
        assert_eq!(outcome.manifest.vim.acquisition.source_url, VIM_ARCHIVE_URL);
        assert_eq!(outcome.manifest.vim.acquisition.archive_sha256, VIM_ARCHIVE_SHA256);
        verify_layout(&outcome.manifest_path, &probe_vim_version)
            .map_err(|error| anyhow::anyhow!("{error}"))?;
        Ok(())
    }
}
