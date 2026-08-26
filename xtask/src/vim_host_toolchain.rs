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
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

// The shared host-runner substrate is included exactly once per crate (in
// `vim_host_run`, #10944); this module consumes that single instance rather
// than loading the file a second time.
use crate::vim_host_run::vim_host_runner::{
    VimLspCheckoutIdentity, VimLspEntryFile, VimLspExpectedContent, VimLspSubjectManifest,
    VimLspTreeDigest, VimLspUpstream, bytes_sha256, file_sha256, verify_vim_lsp_checkout,
};

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
/// law the #10944 runner enforces before launch. This substrate refuses to
/// provision an instrument that cannot run the pinned client.
pub const REQUIRED_VIM_FEATURES: [&str; 3] = ["channel", "job", "timers"];

pub const SCHEMA_VERSION: &str = "vim_vim_lsp_host_toolchain.v1";
pub const INSTRUMENT_VERSION: u32 = 1;
pub const HOST_ROLE: &str = "vim_vim_lsp_host";
pub const MANIFEST_FILE_NAME: &str = "vim_vim_lsp_host_toolchain.v1.json";
const LOAD_MODE_CALLER_PINNED_CHECKOUT: &str = "caller_pinned_git_checkout_via_runtimepath_prepend";
const VIM_LSP_UPSTREAM_URL: &str = "https://github.com/prabirshrestha/vim-lsp.git";

/// Bounded probe limits: an explicit or PATH `vim` that hangs or streams
/// output indefinitely must never hold provisioning hostage.
const VERSION_PROBE_TIMEOUT: Duration = Duration::from_secs(30);
const VERSION_PROBE_OUTPUT_CAP_BYTES: usize = 512 * 1024;
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
    command.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command.spawn().with_context(|| format!("spawning {label} probe"))?;
    let mut stdout_pipe = child.stdout.take().context(format!("{label} probe has no stdout"))?;
    let thread_label = label.to_string();
    let reader = std::thread::spawn(move || {
        let label = thread_label;
        let mut buffered = Vec::new();
        let mut chunk = [0u8; 8192];
        loop {
            match stdout_pipe.read(&mut chunk) {
                Ok(0) => break,
                Ok(read) => {
                    buffered.extend_from_slice(&chunk[..read]);
                    if buffered.len() > VERSION_PROBE_OUTPUT_CAP_BYTES {
                        return Err(anyhow::anyhow!("{label} probe exceeded the output cap"));
                    }
                }
                Err(error) => return Err(anyhow::anyhow!("{label} probe read failed: {error}")),
            }
        }
        Ok(buffered)
    });
    let deadline = Instant::now() + VERSION_PROBE_TIMEOUT;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if Instant::now() >= deadline => break None,
            Ok(None) => std::thread::sleep(Duration::from_millis(25)),
            Err(error) => {
                let _ = child.kill();
                let _ = reader.join();
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
            let _ = reader.join();
            anyhow::bail!(
                "{label} probe exceeded its {}s deadline",
                VERSION_PROBE_TIMEOUT.as_secs()
            );
        }
    };
    let stdout = reader.join().map_err(|_| anyhow::anyhow!("{label} probe reader failed"))??;
    ensure!(status.success(), "{label} probe failed with status {status}");
    String::from_utf8(stdout).with_context(|| format!("{label} probe produced non-UTF-8 output"))
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
/// oversized payloads. Only regular directories and files are admitted.
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
        anyhow::ensure!(
            entry.is_file(),
            "archive entry {raw_name} is neither a regular file nor a directory"
        );
        total_bytes += entry.size();
        anyhow::ensure!(
            total_bytes <= EXTRACTION_TOTAL_CAP_BYTES,
            "archive exceeds the total extraction size cap"
        );
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
        }
        let mut payload = Vec::new();
        entry.read_to_end(&mut payload).context("reading archive entry bytes")?;
        fs::write(&target, &payload)
            .with_context(|| format!("writing extracted {}", target.display()))?;
        extracted += 1;
    }
    anyhow::ensure!(extracted > 0, "archive carried no entries under {subtree}");
    Ok(extracted)
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

fn vim_acquisition_identity() -> VimAcquisitionIdentity {
    VimAcquisitionIdentity {
        mode: "pinned_release_archive".to_string(),
        source_url: VIM_ARCHIVE_URL.to_string(),
        tag: VIM_RELEASE_TAG.to_string(),
        archive_sha256: VIM_ARCHIVE_SHA256.to_string(),
        archive_entry: VIM_ARCHIVE_ENTRY.to_string(),
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

// ---------------------------------------------------------------------------
// Acquisition stages
// ---------------------------------------------------------------------------

/// Acquire the pinned Vim archive into `downloads` (skipped when a
/// digest-correct copy is already present), returning the archive path.
fn acquire_vim_archive(downloads_dir: &Path) -> ToolchainResult<PathBuf> {
    let archive_path = downloads_dir.join(format!("gvim_{}.zip", VIM_RELEASE_TAG));
    if archive_path.exists() {
        return match file_sha256(&archive_path) {
            Ok(actual) if actual == VIM_ARCHIVE_SHA256 => Ok(archive_path),
            Ok(_) => Err(mismatch(format!(
                "cached download {} drifted from the pinned archive digest; rebuilding",
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
    let partial = downloads_dir.join("download.partial");
    let fetched = Command::new("curl")
        .args(["--fail", "--location", "--silent", "--show-error", "--output"])
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
            "curl could not fetch the pinned Vim archive: {}",
            String::from_utf8_lossy(&fetched.stderr).trim()
        )));
    }
    let actual = file_sha256(&partial).map_err(|error| {
        acquisition_failure(format!("hashing the downloaded archive: {error:#}"))
    })?;
    if actual != VIM_ARCHIVE_SHA256 {
        let _ = fs::remove_file(&partial);
        return Err(mismatch(format!(
            "downloaded Vim archive digest {actual} does not match the pinned {VIM_ARCHIVE_SHA256}"
        )));
    }
    fs::rename(&partial, &archive_path).map_err(|error| {
        acquisition_failure(format!("finalizing the downloaded archive: {error}"))
    })?;
    Ok(archive_path)
}

/// Verify the pinned console executable inside the extracted portable
/// runtime, binding its digest to the pin. The executable stays inside the
/// runtime directory: the Windows VIMDLL build loads `vim92.dll` from its
/// own directory, so isolating the bare binary would break launch.
fn install_vim_executable(archive: &Path, cache_vim_dir: &Path) -> ToolchainResult<PathBuf> {
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
    if actual != VIM_EXECUTABLE_SHA256 {
        return Err(mismatch(format!(
            "extracted Vim executable digest {actual} does not match the pinned \
             {VIM_EXECUTABLE_SHA256}"
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
        let outcome = Command::new("git")
            .args(&args)
            .stdin(Stdio::null())
            .output()
            .map_err(|error| acquisition_failure(format!("running git {label}: {error}")))?;
        if !outcome.status.success() {
            return Err(acquisition_failure(format!(
                "git {label} failed for the pinned vim-lsp subject: {}",
                String::from_utf8_lossy(&outcome.stderr).trim()
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

    let vim_executable = layout_root.join("vim").join("vim92").join("vim.exe");
    let actual_exe = file_sha256(&vim_executable)
        .map_err(|error| mismatch(format!("hashing the cached Vim executable: {error:#}")))?;
    if actual_exe != manifest.vim.executable_sha256 {
        return Err(mismatch(format!(
            "cached Vim executable digest {actual_exe} does not match the recorded {}",
            manifest.vim.executable_sha256
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
pub fn provision(
    inputs: &ProvisionInputs,
    probe: &dyn Fn(&Path) -> Result<String>,
) -> ToolchainResult<ProvisionOutcome> {
    let (authority, authority_sha256) = inputs.authority.load().map_err(classify_authority)?;
    let platform = platform_fields(&inputs.execution_environment);
    let vim_identity = vim_acquisition_identity();
    let key_inputs = CacheKeyInputs {
        schema_version: SCHEMA_VERSION,
        instrument_version: INSTRUMENT_VERSION,
        host_role: HOST_ROLE,
        platform: &platform,
        vim_acquisition: &vim_identity,
        vim_required_features: required_feature_list(),
        vim_lsp_subject_authority_path: SUBJECT_AUTHORITY_REPO_PATH,
        vim_lsp_subject_authority_sha256: &authority_sha256,
        vim_lsp_selected_commit: &authority.upstream.selected_commit,
        vim_lsp_load_mode: LOAD_MODE_CALLER_PINNED_CHECKOUT,
        isolation: &IsolationPolicy::governed(),
    };
    let key = cache_key(&key_inputs).map_err(classify_authority)?;
    let dir_name = cache_dir_name(&key).map_err(classify_authority)?.to_string();
    let entry_root = inputs.output_root.join(dir_name);
    let manifest_path = entry_root.join(MANIFEST_FILE_NAME);

    if manifest_path.exists() {
        let cached = fs::read(&manifest_path)
            .map_err(|error| mismatch(format!("reading the cached manifest: {error}")));
        if let Ok(manifest_bytes) = cached
            && let Ok(manifest) = serde_json::from_slice::<ToolchainManifest>(&manifest_bytes)
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
    }

    rebuild_entry(inputs, &authority, authority_sha256, platform, key, &entry_root, probe)
}

fn rebuild_entry(
    inputs: &ProvisionInputs,
    authority: &VimLspSubjectManifest,
    authority_sha256: String,
    platform: PlatformFields,
    key: String,
    entry_root: &Path,
    probe: &dyn Fn(&Path) -> Result<String>,
) -> ToolchainResult<ProvisionOutcome> {
    if entry_root.exists() {
        fs::remove_dir_all(entry_root)
            .map_err(|error| acquisition_failure(format!("clearing the cache entry: {error}")))?;
    }
    let downloads_dir = entry_root.join("downloads");
    let cache_vim_dir = entry_root.join("vim");
    let checkout_dir = entry_root.join("vim-lsp");
    fs::create_dir_all(&cache_vim_dir).map_err(|error| {
        acquisition_failure(format!(
            "creating the cache entry {}: {error}",
            cache_vim_dir.display()
        ))
    })?;

    match build_fresh_entry(
        inputs,
        authority,
        &authority_sha256,
        &platform,
        &key,
        entry_root,
        &downloads_dir,
        &cache_vim_dir,
        &checkout_dir,
        probe,
    ) {
        Ok(outcome) => Ok(outcome),
        Err(failure) => {
            // A failed acquisition never leaves a half-built entry behind
            // that a later run could mistake for a provisioned subject.
            let _ = fs::remove_dir_all(entry_root);
            Err(failure)
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn build_fresh_entry(
    inputs: &ProvisionInputs,
    authority: &VimLspSubjectManifest,
    authority_sha256: &str,
    platform: &PlatformFields,
    key: &str,
    entry_root: &Path,
    downloads_dir: &Path,
    cache_vim_dir: &Path,
    checkout_dir: &Path,
    probe: &dyn Fn(&Path) -> Result<String>,
) -> ToolchainResult<ProvisionOutcome> {
    let archive = acquire_vim_archive(downloads_dir)?;
    let vim_executable = install_vim_executable(&archive, cache_vim_dir)?;
    let version_text = probe(&vim_executable)
        .map_err(|error| mismatch(format!("provisioned Vim identity probe failed: {error:#}")))?;
    for feature in REQUIRED_VIM_FEATURES {
        if !version_text.contains(&format!("+{feature}")) {
            return Err(mismatch(format!(
                "provisioned Vim lacks the required transport feature +{feature}; this build \
                 cannot run the pinned vim-lsp client"
            )));
        }
    }
    install_vim_lsp_checkout(checkout_dir, authority, inputs.vim_lsp_source.as_deref())?;

    let executable_digest = file_sha256(&vim_executable)
        .map_err(|error| unresolved(format!("hashing the pinned executable: {error:#}")))?;
    let version_summary = version_text.lines().next().unwrap_or_default().trim().to_string();
    let manifest = ToolchainManifest {
        schema_version: SCHEMA_VERSION.to_string(),
        instrument_version: INSTRUMENT_VERSION,
        host_role: HOST_ROLE.to_string(),
        platform: platform.clone(),
        cache_key: key.to_string(),
        vim: VimToolchainIdentity {
            acquisition: vim_acquisition_identity(),
            executable_sha256: executable_digest,
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
    verify_manifest_core(&manifest, &manifest_bytes, entry_root, probe)?;
    let published = entry_root.join(MANIFEST_FILE_NAME);
    let staging = entry_root.join(format!("{MANIFEST_FILE_NAME}.tmp"));
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

    fn inline_authority(fixture: &SubjectFixture) -> SubjectAuthoritySource {
        SubjectAuthoritySource::Inline {
            display_path: SUBJECT_AUTHORITY_REPO_PATH.to_string(),
            bytes: fixture.authority_bytes.clone(),
        }
    }

    fn provision_inputs(
        output_root: &Path,
        fixture: &SubjectFixture,
        probe_text: &'static str,
    ) -> (ProvisionInputs, impl Fn(&Path) -> Result<String>) {
        (
            ProvisionInputs {
                output_root: output_root.to_path_buf(),
                repo_root: output_root.to_path_buf(),
                authority: inline_authority(fixture),
                vim_lsp_source: Some(fixture.source_dir.clone()),
                execution_environment: "local_runner".to_string(),
            },
            static_probe(probe_text),
        )
    }

    fn assert_class(error: &InstrumentFailure, class: InstrumentFailureClass) {
        assert_eq!(error.class, class, "unexpected failure: {error}");
    }

    #[test]
    fn roundtrip_provision_and_offline_verify_hermetic() -> Result<()> {
        let scratch = tempfile::tempdir()?;
        let fixture = build_vimlsp_fixture(scratch.path(), "source")?;
        let output = scratch.path().join("out");
        let (inputs, probe) = provision_inputs(&output, &fixture, FULL_FEATURE_TEXT);

        let first = provision(&inputs, &probe).map_err(|error| anyhow::anyhow!("{error}"))?;
        assert!(!first.cache_hit);
        assert_eq!(first.manifest.host_role, HOST_ROLE);
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
        let left_inputs = ProvisionInputs {
            output_root: scratch.path().join("left"),
            repo_root: scratch.path().to_path_buf(),
            authority: inline_authority(&fixture),
            vim_lsp_source: Some(fixture.source_dir.clone()),
            execution_environment: "ci_runner".to_string(),
        };
        let right_inputs = ProvisionInputs {
            output_root: scratch.path().join("right"),
            repo_root: scratch.path().to_path_buf(),
            authority: inline_authority(&fixture),
            vim_lsp_source: Some(fixture.source_dir.clone()),
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
        let acquisition = vim_acquisition_identity();
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
        Ok(())
    }

    #[test]
    fn warm_cache_second_run_hits_and_keeps_manifest_stable() -> Result<()> {
        let scratch = tempfile::tempdir()?;
        let fixture = build_vimlsp_fixture(scratch.path(), "source")?;
        let output = scratch.path().join("out");
        let (inputs, probe) = provision_inputs(&output, &fixture, FULL_FEATURE_TEXT);
        let first = provision(&inputs, &probe).map_err(|error| anyhow::anyhow!("{error}"))?;
        let before = fs::read(&first.manifest_path)?;
        let second = provision(&inputs, &probe).map_err(|error| anyhow::anyhow!("{error}"))?;
        assert!(second.cache_hit, "identical rerun must be an exact-identity cache hit");
        assert_eq!(first.manifest_path, second.manifest_path);
        assert_eq!(before, fs::read(&second.manifest_path)?);
        Ok(())
    }

    #[test]
    fn corrupt_cached_byte_fails_verification_then_rebuild_heals() -> Result<()> {
        let scratch = tempfile::tempdir()?;
        let fixture = build_vimlsp_fixture(scratch.path(), "source")?;
        let output = scratch.path().join("out");
        let (inputs, probe) = provision_inputs(&output, &fixture, FULL_FEATURE_TEXT);
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
        // silently — and healing needs no network because the verified
        // archive copy is still digest-correct.
        let healed = provision(&inputs, &probe).map_err(|error| anyhow::anyhow!("{error}"))?;
        assert!(!healed.cache_hit);
        verify_layout(&healed.manifest_path, &probe).map_err(|error| anyhow::anyhow!("{error}"))?;
        Ok(())
    }

    #[test]
    fn same_version_text_with_different_bytes_is_rejected() -> Result<()> {
        let scratch = tempfile::tempdir()?;
        let fixture = build_vimlsp_fixture(scratch.path(), "source")?;
        let output = scratch.path().join("out");
        let (inputs, probe) = provision_inputs(&output, &fixture, FULL_FEATURE_TEXT);
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
        let output = scratch.path().join("out");
        let (inputs, probe) = provision_inputs(&output, &fixture, FULL_FEATURE_TEXT);
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
        let output = scratch.path().join("out");
        let (inputs, probe) = provision_inputs(&output, &fixture, FULL_FEATURE_TEXT);
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
        let output = scratch.path().join("out");
        let (inputs, probe) = provision_inputs(&output, &fixture, NO_JOB_TEXT);
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
        let inputs = ProvisionInputs {
            output_root: scratch.path().join("out"),
            repo_root: scratch.path().to_path_buf(),
            authority: SubjectAuthoritySource::Inline {
                display_path: "inline".to_string(),
                bytes: b"{ not the governed schema".to_vec(),
            },
            vim_lsp_source: None,
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
        let inputs = ProvisionInputs {
            output_root: scratch.path().join("out"),
            repo_root: scratch.path().to_path_buf(),
            authority: inline_authority(&fixture),
            vim_lsp_source: Some(scratch.path().join("missing-source")),
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
}
