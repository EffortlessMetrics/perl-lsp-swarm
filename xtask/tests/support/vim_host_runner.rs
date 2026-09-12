//! Hermetic actual-Vim host runner substrate (#10944).
//!
//! This substrate is the Vim-side sibling of the Emacs host runner substrate
//! (`emacs_host_runner.rs`, landed with #8024/#7778). It owns what must never
//! be owned by Vimscript: the exact-subject run plan and its fail-closed
//! validation, the hermetic isolation layout, the headless Vim command, the
//! bounded owned-process supervision with a deterministic process-set
//! comparison, the pinned-subject checkout verification, and the composition
//! of the repository's generic `editor_client_compat.v1` receipt.
//!
//! Ownership split (mirrors the issue contract):
//!
//! - Rust here owns orchestration, identity, boundedness, process ledgers,
//!   cleanup policy, and receipt policy. A Vimscript file is only a thin
//!   editor-native adapter (`scripts/test/vim-clients/vim-lsp-adapter.vim`)
//!   plus a bounded driver (`scripts/test/vim-host-driver.vim`).
//! - `.ci/editor-clients/vim-vim-lsp-subject.v1.json` (#11369) owns the exact
//!   upstream vim-lsp subject bytes; [`VimLspSubjectManifest`] parses it and
//!   [`verify_vim_lsp_checkout`] refuses any checkout that is not exactly the
//!   pinned commit with the pinned entry-file blob identities.
//! - `.ci/editor-clients/vim-vim-lsp-configuration.v1.json` (#11369) owns the
//!   client registration shape; the values this substrate forwards to the
//!   adapter are read from that manifest, never re-derived here.
//! - `.ci/editor-clients/vim-vim-lsp-activation-root.v1.json` (#7762) owns
//!   root markers and filetype policy; the driver observes native Vim
//!   detection and this substrate rejects pre-forced filetypes.
//! - The generic process-tree cleanup boundary (#8734) and the shared
//!   host-execution/receipt primitives (#10894) are consumed, not duplicated:
//!   bounded execution, process-ledger comparison, redaction/bounding,
//!   receipt-freshness refusal, and cleanup laws live in
//!   `xtask::editor_host` and this substrate keeps only Vim-specific subject,
//!   event, and journey knowledge.
//!
//! Fail-closed laws:
//!
//! - every identity input is digest-verified before launch; a missing or
//!   wrong Vim/vim-lsp/candidate is a typed error, never a skipped pass;
//! - the output root must be fresh; a stale receipt directory refuses the run;
//! - the deadline is parent-owned; a hung Vim is killed and reported;
//! - cleanup compares a deterministic before/after process set for the exact
//!   candidate executable and fails (survivor observed) or stays not-proven
//!   (probe unavailable), never silently passes;
//! - adapter events are validated (contiguous sequence, singleton lifecycle
//!   ordering, no dangling actions) before any receipt claims attach identity;
//! - a receipt whose registration detail does not bind the planned candidate
//!   digest, or whose buffer attachment was pre-forced, cannot pass.

use anyhow::{Context, Result, ensure};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use xtask::editor_client_compat::{
    ArtifactKind, CANONICAL_EXPECTATION_SET_ID, CapabilityIdentity, CleanupResult,
    ClientSourceState, DiagnosticMode, DiagnosticsIdentity, EditorClientCompatReceipt,
    EvidenceArtifact, EvidenceStage, FailureClass, HostIdentity, IntegrationIdentity,
    IntegrationMode, JourneyCell, ObservationResult, PlatformIdentity, PositionEncodingBasis,
    RegistrationState, SCHEMA_VERSION as RECEIPT_SCHEMA_VERSION, ServerIdentity,
    WorkspaceFixtureIdentity, canonical_expectation_set_digest, fixture_digest,
};
use xtask::editor_host::{
    BoundedRun, HostProcessLedger, PathRedaction, ProbeCapture, judge_cleanup,
    validate_safe_identity, write_artifact,
};

// The shared #10894 mechanics this substrate consumes from one authority.
pub use xtask::editor_host::{
    ProcessProbeLine, parse_process_snapshot, parse_windows_process_snapshot, probe_process_table,
    render_process_snapshot, surviving_processes,
};

pub const RUN_PLAN_SCHEMA_VERSION: &str = "vim_host_run_plan.v1";
pub const DRIVER_SCHEMA_VERSION: &str = "vim_host_driver.v1";

/// The one exact client subject this runner can execute today: the pinned
/// `prabirshrestha/vim-lsp` upstream commit selected by #11369. A newer
/// upstream head is a different subject, never a silent edit of this row.
pub const VIM_LSP_CLIENT_ID: &str = "vim-lsp";

// ---------------------------------------------------------------------------
// Pinned-subject manifest (#11369) parsing and checkout verification
// ---------------------------------------------------------------------------

/// The #11369 subject-manifest fields this substrate consumes. Parsing is
/// deliberately strict: unknown schema versions refuse the run instead of
/// being interpreted loosely.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VimLspSubjectManifest {
    pub schema_version: String,
    pub upstream: VimLspUpstream,
    pub expected_content_identity: VimLspExpectedContent,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VimLspUpstream {
    pub selected_commit: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tree_digest: Option<VimLspTreeDigest>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VimLspTreeDigest {
    pub algorithm: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VimLspExpectedContent {
    /// Required: the plugin entry this runtime must source (`plugin/lsp.vim`).
    pub entry_files: Vec<VimLspEntryFile>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VimLspEntryFile {
    pub path: String,
    pub git_blob_sha1: String,
}

impl VimLspSubjectManifest {
    /// Parse and structurally validate the #11369 manifest bytes.
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let manifest: VimLspSubjectManifest =
            serde_json::from_slice(bytes).context("parsing the vim-lsp subject manifest")?;
        ensure!(
            manifest.schema_version == "vim_lsp_subject.v1",
            "unexpected vim-lsp subject manifest schema {}",
            manifest.schema_version
        );
        ensure!(
            is_lower_hex(&manifest.upstream.selected_commit, 40),
            "pinned vim-lsp commit must be 40 lowercase hex chars"
        );
        if let Some(tree) = &manifest.upstream.tree_digest {
            ensure!(
                tree.algorithm == "git-tree-sha1",
                "unsupported vim-lsp tree digest algorithm {}",
                tree.algorithm
            );
            ensure!(
                is_lower_hex(&tree.value, 40),
                "vim-lsp tree digest must be 40 lowercase hex chars"
            );
        }
        ensure!(
            !manifest.expected_content_identity.entry_files.is_empty(),
            "subject manifest pins no vim-lsp entry files"
        );
        let mut paths = BTreeSet::new();
        for entry in &manifest.expected_content_identity.entry_files {
            ensure!(
                is_reason_token(&entry.path.replace('/', "_")),
                "entry path is not a governed relative path: {}",
                entry.path
            );
            ensure!(
                !entry.path.starts_with('/') && !entry.path.contains(".."),
                "entry path escapes the checkout: {}",
                entry.path
            );
            ensure!(paths.insert(entry.path.as_str()), "duplicate entry path {}", entry.path);
            ensure!(
                is_lower_hex(&entry.git_blob_sha1, 40),
                "entry {} blob digest must be 40 lowercase hex chars",
                entry.path
            );
        }
        Ok(manifest)
    }

    /// Load and parse the manifest from disk.
    pub fn load(path: &Path) -> Result<Self> {
        let bytes = fs::read(path)
            .with_context(|| format!("reading vim-lsp subject manifest {}", path.display()))?;
        Self::parse(&bytes)
    }
}

/// The verified identity of one vim-lsp checkout, produced only by
/// [`verify_vim_lsp_checkout`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VimLspCheckoutIdentity {
    pub pinned_commit: String,
    pub resolved_commit: String,
    pub tree_digest: String,
    /// `plugin/lsp.vim` sha256 — the client source identity the generic
    /// receipt carries as the loaded-client attestation target.
    pub plugin_entry_sha256: String,
    pub verified_entry_count: usize,
}

/// Verify that `checkout` is exactly the pinned #11369 subject: HEAD is the
/// selected commit, the worktree is clean, the resolved tree digest matches
/// the manifest observation, and every pinned entry file's git blob SHA1
/// matches. Any drift is a typed error before anything is launched; this is
/// the fail-closed consumption edge of the pin authority (never
/// latest-is-fine).
pub fn verify_vim_lsp_checkout(
    checkout: &Path,
    manifest: &VimLspSubjectManifest,
) -> Result<VimLspCheckoutIdentity> {
    ensure!(
        checkout.is_absolute() && checkout.is_dir(),
        "vim-lsp checkout must be an absolute directory: {}",
        checkout.display()
    );
    ensure!(
        checkout.join(".git").exists(),
        "vim-lsp subject must be a real git checkout: {}",
        checkout.display()
    );
    let resolved_commit = git_line(checkout, &["rev-parse", "HEAD"])
        .context("resolving the vim-lsp checkout HEAD")?;
    ensure!(is_lower_hex(&resolved_commit, 40), "vim-lsp checkout HEAD is not a commit identity");
    ensure!(
        resolved_commit == manifest.upstream.selected_commit,
        "vim-lsp checkout is {resolved_commit} but the pinned subject is {}; a drifting \
         checkout is a different subject, never this run",
        manifest.upstream.selected_commit
    );
    let status = git_output(checkout, &["status", "--porcelain"])
        .context("checking the vim-lsp checkout worktree state")?;
    ensure!(
        status.trim().is_empty(),
        "vim-lsp checkout has uncommitted changes; a dirty checkout is not the pinned subject"
    );
    let tree_digest =
        git_line(checkout, &["rev-parse", "HEAD^{tree}"]).context("resolving the checkout tree")?;
    if let Some(expected) = &manifest.upstream.tree_digest {
        ensure!(
            tree_digest == expected.value,
            "vim-lsp checkout tree {tree_digest} does not match the pinned tree {}",
            expected.value
        );
    }
    let mut verified = 0;
    for entry in &manifest.expected_content_identity.entry_files {
        let path = checkout.join(&entry.path);
        ensure!(
            path.is_file(),
            "pinned vim-lsp entry file {} is missing from the checkout",
            entry.path
        );
        let blob = git_line(checkout, &["hash-object", "--", &entry.path])
            .with_context(|| format!("hashing pinned entry {}", entry.path))?;
        ensure!(
            blob == entry.git_blob_sha1,
            "pinned vim-lsp entry {} has blob {blob}, expected {}",
            entry.path,
            entry.git_blob_sha1
        );
        verified += 1;
    }
    let plugin_entry = checkout.join("plugin/lsp.vim");
    ensure!(plugin_entry.is_file(), "vim-lsp checkout has no plugin/lsp.vim entry");
    Ok(VimLspCheckoutIdentity {
        pinned_commit: manifest.upstream.selected_commit.clone(),
        resolved_commit,
        tree_digest,
        plugin_entry_sha256: file_sha256(&plugin_entry)?,
        verified_entry_count: verified,
    })
}

fn git_output(dir: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .with_context(|| format!("running git {}", args.join(" ")))?;
    ensure!(
        output.status.success(),
        "git {} failed: {}",
        args.join(" "),
        bounded_first_line(&output.stderr)
    );
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn git_line(dir: &Path, args: &[&str]) -> Result<String> {
    let out = git_output(dir, args)?;
    let line = out.lines().next().unwrap_or_default().trim().to_lowercase();
    ensure!(!line.is_empty(), "git {} produced no output", args.join(" "));
    Ok(line)
}

fn bounded_first_line(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes)
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("")
        .trim()
        .chars()
        .take(300)
        .collect()
}

// ---------------------------------------------------------------------------
// Run plan
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VimHostPaths {
    pub vim_executable: PathBuf,
    pub vim_lsp_checkout: PathBuf,
    pub driver: PathBuf,
    pub adapter: PathBuf,
    pub candidate_executable: PathBuf,
    pub fixture_root: PathBuf,
    pub artifact_root: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VimHostRunIdentity {
    pub schema_version: String,
    pub stage: EvidenceStage,
    pub repository: String,
    pub candidate_sha: String,
    pub vim_version: String,
    pub vim_build_sha256: String,
    /// Digest of the full `vim --version` build-feature output: the compiled
    /// feature identity of the exact host, separate from the binary bytes.
    pub vim_feature_digest: String,
    pub vim_lsp_commit: String,
    pub vim_lsp_tree_digest: String,
    pub vim_lsp_plugin_entry_sha256: String,
    pub driver_sha256: String,
    pub adapter_sha256: String,
    pub configuration_sha256: String,
    pub activation_root_sha256: String,
    pub subject_manifest_sha256: String,
    pub candidate_version: String,
    pub candidate_build_revision: String,
    pub candidate_artifact_sha256: String,
    pub candidate_identity_packet_sha256: String,
    pub fixture: WorkspaceFixtureIdentity,
    pub journey_selector: String,
    pub platform: PlatformIdentity,
    pub registration_state: RegistrationState,
    pub timeout_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VimHostRunPlan {
    pub identity: VimHostRunIdentity,
    pub paths: VimHostPaths,
}

impl VimHostRunPlan {
    /// Fail-closed plan validation: every typed identity field, every exact
    /// input file digest, the fixture digest, and the canonical expectation
    /// set are verified before any launch is allowed.
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.identity.schema_version == RUN_PLAN_SCHEMA_VERSION,
            "unexpected Vim host run-plan schema"
        );
        validate_safe_identity(&self.identity.repository, "repository")?;
        ensure!(
            is_lower_hex(&self.identity.candidate_sha, 40),
            "candidate_sha must be 40 lowercase hex chars"
        );
        validate_safe_identity(&self.identity.vim_version, "vim_version")?;
        validate_sha256(&self.identity.vim_build_sha256, "vim_build_sha256")?;
        validate_sha256(&self.identity.vim_feature_digest, "vim_feature_digest")?;
        ensure!(
            is_lower_hex(&self.identity.vim_lsp_commit, 40),
            "vim_lsp_commit must be 40 lowercase hex chars"
        );
        ensure!(
            is_lower_hex(&self.identity.vim_lsp_tree_digest, 40),
            "vim_lsp_tree_digest must be 40 lowercase hex chars"
        );
        validate_sha256(&self.identity.vim_lsp_plugin_entry_sha256, "vim_lsp_plugin_entry_sha256")?;
        validate_sha256(&self.identity.driver_sha256, "driver_sha256")?;
        validate_sha256(&self.identity.adapter_sha256, "adapter_sha256")?;
        validate_sha256(&self.identity.configuration_sha256, "configuration_sha256")?;
        validate_sha256(&self.identity.activation_root_sha256, "activation_root_sha256")?;
        validate_sha256(&self.identity.subject_manifest_sha256, "subject_manifest_sha256")?;
        validate_safe_identity(&self.identity.candidate_version, "candidate_version")?;
        ensure!(
            is_lower_hex(&self.identity.candidate_build_revision, 40),
            "candidate_build_revision must be 40 lowercase hex chars"
        );
        validate_sha256(&self.identity.candidate_artifact_sha256, "candidate_artifact_sha256")?;
        validate_sha256(
            &self.identity.candidate_identity_packet_sha256,
            "candidate_identity_packet_sha256",
        )?;
        validate_fixture_identity(&self.identity.fixture)?;
        ensure!(
            is_reason_token(&self.identity.journey_selector),
            "journey_selector must be a stable reason token"
        );
        validate_platform(&self.identity.platform)?;
        ensure!(
            (1..=600_000).contains(&self.identity.timeout_ms),
            "timeout_ms must be between 1 and 600000"
        );

        for (label, path) in [
            ("vim executable", &self.paths.vim_executable),
            ("driver", &self.paths.driver),
            ("adapter", &self.paths.adapter),
            ("candidate executable", &self.paths.candidate_executable),
        ] {
            ensure!(path.is_absolute(), "{label} path must be absolute");
            ensure!(path.is_file(), "{label} path is not a file: {}", path.display());
        }
        ensure!(
            self.paths.vim_lsp_checkout.is_absolute()
                && self.paths.vim_lsp_checkout.join(".git").exists(),
            "vim-lsp checkout must be an absolute git checkout"
        );
        ensure!(
            self.paths.fixture_root.is_absolute() && self.paths.fixture_root.is_dir(),
            "fixture_root must be an absolute directory"
        );
        ensure!(self.paths.artifact_root.is_absolute(), "artifact_root must be absolute");
        ensure!(
            is_perllsp_filename(&self.paths.candidate_executable),
            "candidate executable file name must be perllsp or perllsp.exe"
        );
        ensure!(
            self.paths.vim_lsp_checkout.join("plugin/lsp.vim").is_file(),
            "vim-lsp checkout has no plugin/lsp.vim; a missing client is a typed failure, \
             never a skip"
        );

        verify_file_sha256(
            &self.paths.vim_executable,
            &self.identity.vim_build_sha256,
            "Vim executable",
        )?;
        verify_file_sha256(
            &self.paths.vim_lsp_checkout.join("plugin/lsp.vim"),
            &self.identity.vim_lsp_plugin_entry_sha256,
            "vim-lsp plugin entry",
        )?;
        verify_file_sha256(&self.paths.driver, &self.identity.driver_sha256, "driver")?;
        verify_file_sha256(&self.paths.adapter, &self.identity.adapter_sha256, "adapter")?;
        verify_file_sha256(
            &self.paths.candidate_executable,
            &self.identity.candidate_artifact_sha256,
            "candidate executable",
        )?;
        ensure!(
            fixture_digest(&self.paths.fixture_root)? == self.identity.fixture.digest,
            "fixture digest mismatch"
        );
        ensure!(
            self.identity.fixture.expectation_set_id == CANONICAL_EXPECTATION_SET_ID,
            "unexpected expectation-set identity"
        );
        ensure!(
            canonical_expectation_set_digest()? == self.identity.fixture.expectation_set_digest,
            "expectation-set digest mismatch"
        );
        Ok(())
    }

    /// The receipt-side client identity for the pinned subject. `source_ref`
    /// binds the pinned commit and tree so a receipt names its exact upstream
    /// bytes without a second pin table.
    pub fn client_source_ref(&self) -> String {
        format!(
            "{VIM_LSP_CLIENT_ID}/{}/{}",
            self.identity.vim_lsp_commit, self.identity.vim_lsp_tree_digest
        )
    }
}

fn validate_fixture_identity(fixture: &WorkspaceFixtureIdentity) -> Result<()> {
    ensure!(is_reason_token(&fixture.id), "fixture id must be a stable reason token");
    validate_sha256(&fixture.digest, "fixture digest")?;
    ensure!(
        is_reason_token(&fixture.expectation_set_id),
        "expectation set id must be a stable reason token"
    );
    validate_sha256(&fixture.expectation_set_digest, "expectation set digest")?;
    Ok(())
}

fn validate_platform(platform: &PlatformIdentity) -> Result<()> {
    validate_safe_identity(&platform.os, "platform.os")?;
    validate_safe_identity(&platform.os_version, "platform.os_version")?;
    validate_safe_identity(&platform.arch, "platform.arch")?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Hermetic isolation layout
// ---------------------------------------------------------------------------

/// Render an absolute path in the form Vim accepts everywhere: forward
/// slashes on every host. Windows Vim builds (including Git-for-Windows vim)
/// silently refuse backslash-qualified `-S`/`source`/`runtime` targets — the
/// editor exits 1 having sourced nothing — so every path the supervisor
/// hands to the editor side crosses this boundary normalized.
pub fn vim_path(path: &Path) -> OsString {
    let text = path.to_string_lossy().replace('\\', "/");
    OsString::from(text)
}

/// The isolated run root: the run consumes no user vimrc, plugins, viminfo,
/// swap/backup/session files, cache, or tags, and the only LSP client loaded
/// is the pinned checkout. Every target the run needs lives under `root`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HermeticVimLayout {
    pub root: PathBuf,
    pub home: PathBuf,
    pub xdg_config_home: PathBuf,
    pub xdg_cache_home: PathBuf,
    pub xdg_data_home: PathBuf,
    pub temp_directory: PathBuf,
    pub raw_directory: PathBuf,
    pub artifact_directory: PathBuf,
}

impl HermeticVimLayout {
    pub fn prepare(root: &Path) -> Result<Self> {
        ensure!(root.is_absolute(), "hermetic root must be absolute");
        let layout = Self {
            root: root.to_path_buf(),
            home: root.join("home"),
            xdg_config_home: root.join("xdg/config"),
            xdg_cache_home: root.join("xdg/cache"),
            xdg_data_home: root.join("xdg/data"),
            temp_directory: root.join("tmp"),
            raw_directory: root.join("raw"),
            artifact_directory: root.join("artifacts"),
        };
        for directory in [
            &layout.home,
            &layout.xdg_config_home,
            &layout.xdg_cache_home,
            &layout.xdg_data_home,
            &layout.temp_directory,
            &layout.raw_directory,
            &layout.artifact_directory,
        ] {
            fs::create_dir_all(directory)
                .with_context(|| format!("creating hermetic directory {}", directory.display()))?;
        }
        Ok(layout)
    }

    pub fn event_file(&self) -> PathBuf {
        self.raw_directory.join("driver-events.jsonl")
    }

    /// The vim-lsp client protocol log (`g:lsp_log_file`). Kept strictly
    /// separate from the server trace so client and server evidence cannot be
    /// conflated.
    pub fn client_log(&self) -> PathBuf {
        self.raw_directory.join("vim-lsp-client.log")
    }

    /// The server-side trace (`PERL_LSP_LOG_FILE` delivered through the
    /// registration `env` channel): perllsp's own log target.
    pub fn server_trace(&self) -> PathBuf {
        self.raw_directory.join("perllsp.log")
    }

    /// The initialize capability snapshot written by the driver from
    /// `lsp#get_server_capabilities`.
    pub fn capability_snapshot(&self) -> PathBuf {
        self.raw_directory.join("initialize.json")
    }

    pub fn process_snapshot_before(&self) -> PathBuf {
        self.raw_directory.join("processes-before.txt")
    }

    pub fn process_snapshot_after(&self) -> PathBuf {
        self.raw_directory.join("processes-after.txt")
    }

    /// The hermetic environment for the Vim process. The inherited allowlist
    /// is the same minimal OS-load-bearing set the Emacs runner admits; HOME,
    /// XDG state, and temp targets are redirected into the run root, and the
    /// run-bound variables the adapter contract requires are injected with
    /// exact absolute values resolved by Rust — including the exact candidate
    /// executable, so ambient PATH can never select another `perllsp`. Every
    /// path value is Vim-normalized through [`vim_path`].
    pub fn environment(
        &self,
        plan: &VimHostRunPlan,
        server_name: &str,
        root_markers: &[String],
    ) -> Result<BTreeMap<OsString, OsString>> {
        self.environment_with_extras(plan, server_name, root_markers, &[])
    }

    /// Same hermetic environment plus journey-scoped extras (#10946): the
    /// scenario module delivers its own fail-closed run contract (expected
    /// and decoy root identities, the governed defect coordinates) through
    /// the same explicit absolute-value channel, never through the ambient
    /// environment.
    pub fn environment_with_extras(
        &self,
        plan: &VimHostRunPlan,
        server_name: &str,
        root_markers: &[String],
        extras: &[(OsString, OsString)],
    ) -> Result<BTreeMap<OsString, OsString>> {
        let mut environment = BTreeMap::new();
        for key in [
            "PATH",
            "SYSTEMROOT",
            "WINDIR",
            "COMSPEC",
            "PATHEXT",
            "LD_LIBRARY_PATH",
            "DYLD_LIBRARY_PATH",
        ] {
            if let Some(value) = std::env::var_os(key) {
                environment.insert(OsString::from(key), value);
            }
        }
        environment.insert(OsString::from("HOME"), vim_path(&self.home));
        environment.insert(OsString::from("USERPROFILE"), vim_path(&self.home));
        environment.insert(OsString::from("XDG_CONFIG_HOME"), vim_path(&self.xdg_config_home));
        environment.insert(OsString::from("XDG_CACHE_HOME"), vim_path(&self.xdg_cache_home));
        environment.insert(OsString::from("XDG_DATA_HOME"), vim_path(&self.xdg_data_home));
        for key in ["TMPDIR", "TEMP", "TMP"] {
            environment.insert(OsString::from(key), vim_path(&self.temp_directory));
        }
        environment
            .insert(OsString::from("PERLLSP_VIM_HOST_EVENT_FILE"), vim_path(&self.event_file()));
        environment
            .insert(OsString::from("PERLLSP_VIM_HOST_CLIENT_LOG"), vim_path(&self.client_log()));
        environment.insert(
            OsString::from("PERLLSP_VIM_HOST_SERVER_TRACE"),
            vim_path(&self.server_trace()),
        );
        environment.insert(
            OsString::from("PERLLSP_VIM_HOST_CAPABILITY_SNAPSHOT"),
            vim_path(&self.capability_snapshot()),
        );
        environment.insert(
            OsString::from("PERLLSP_VIM_HOST_CANDIDATE"),
            vim_path(&plan.paths.candidate_executable),
        );
        environment.insert(
            OsString::from("PERLLSP_VIM_HOST_CANDIDATE_SHA256"),
            OsString::from(&plan.identity.candidate_artifact_sha256),
        );
        environment.insert(
            OsString::from("PERLLSP_VIM_HOST_VIM_LSP_DIR"),
            vim_path(&plan.paths.vim_lsp_checkout),
        );
        environment
            .insert(OsString::from("PERLLSP_VIM_HOST_ADAPTER"), vim_path(&plan.paths.adapter));
        environment.insert(
            OsString::from("PERLLSP_VIM_HOST_FIXTURE_ROOT"),
            vim_path(&plan.paths.fixture_root),
        );
        environment
            .insert(OsString::from("PERLLSP_VIM_HOST_SERVER_NAME"), OsString::from(server_name));
        environment.insert(
            OsString::from("PERLLSP_VIM_HOST_ROOT_MARKERS"),
            OsString::from(root_markers.join(",")),
        );
        // Per-barrier budget for the editor-side waits. Sized for a cold debug
        // candidate: the initialize handshake (including perllsp's workspace
        // indexing/configuration round trip) measured ~30-40s on a local Windows
        // debug build and CI runners are slower; 90s keeps every barrier honest
        // while the parent-owned run deadline still bounds the whole journey.
        environment.insert(OsString::from("PERLLSP_VIM_HOST_BUDGET_MS"), OsString::from("90000"));
        for (key, value) in extras {
            ensure!(
                key.to_str().is_some_and(|text| text.starts_with("PERLLSP_VIM_HOST_")),
                "journey extras must stay inside the PERLLSP_VIM_HOST_ channel: {:?}",
                key
            );
            environment.insert(key.clone(), value.clone());
        }
        Ok(environment)
    }
}

/// Build the headless hermetic Vim command for one validated plan.
///
/// `-Nu NONE` skips every user and system vimrc, `-U NONE` skips gvimrc, `-n`
/// disables swap files, `-i NONE` disables viminfo, and `-es` runs the
/// headless silent-ex driver mode. The only sourced files are the repository
/// driver and, through it, the thin adapter and the pinned plugin checkout.
pub fn build_vim_command(
    plan: &VimHostRunPlan,
    layout: &HermeticVimLayout,
    server_name: &str,
    root_markers: &[String],
) -> Result<Command> {
    build_vim_command_with_extras(plan, layout, server_name, root_markers, &[])
}

/// [`build_vim_command`] plus journey-scoped environment extras (#10946).
pub fn build_vim_command_with_extras(
    plan: &VimHostRunPlan,
    layout: &HermeticVimLayout,
    server_name: &str,
    root_markers: &[String],
    extras: &[(OsString, OsString)],
) -> Result<Command> {
    plan.validate()?;
    let mut command = Command::new(&plan.paths.vim_executable);
    command.env_clear();
    for (key, value) in layout.environment_with_extras(plan, server_name, root_markers, extras)? {
        command.env(key, value);
    }
    command
        .arg("-Nu")
        .arg("NONE")
        .arg("-U")
        .arg("NONE")
        .arg("-n")
        .arg("-i")
        .arg("NONE")
        .arg("-es")
        .arg("-S")
        .arg(vim_path(&plan.paths.driver))
        .stdin(Stdio::null());
    Ok(command)
}

// ---------------------------------------------------------------------------
// Driver events
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DriverEventKind {
    HostStarted,
    ClientLoaded,
    RegistrationSelected,
    ServerInitialized,
    BufferEnabled,
    InitializeObserved,
    RootSelected,
    FixtureOpened,
    DiagnosticsObserved,
    /// #10946 diagnostics-lifecycle events. They extend the minimal #10944
    /// journey (same ordering tier as `diagnostics_observed`); the minimal
    /// journey never emits them and `require_complete` still binds only the
    /// #10944 barriers, so the two journeys stay independently valid.
    DefectStateObserved,
    DefectFixApplied,
    CurrentStateObserved,
    /// #11390 freshness-generation events. Unlike every #10944/#10946 kind,
    /// these four may repeat within one journey: the freshness journey walks
    /// multiple source/config generations in a single host run. Each carries a
    /// monotone 1-based index detail with a per-kind cap, so an unordered,
    /// duplicated, or overlong barrier stream is rejected here even before the
    /// scenario judgment checks the exact phase sequence. The two earlier
    /// journeys never emit them.
    ExternalMutationApplied,
    StaleGenerationHeld,
    ClientMaterializationApplied,
    GenerationCurrentObserved,
    /// #11396 save-format events. Same repeating law as the #11390 kinds: the
    /// save journey walks several ordinary saves in one host run, each with
    /// exactly one configured-owner settlement, plus stale-result holds. Each
    /// carries a monotone 1-based per-kind index with a cap; the earlier
    /// journeys never emit them.
    SaveOwnerConfigured,
    SaveSettlementObserved,
    StaleResultHoldObserved,
    /// #11401 host-reopen lifecycle events. Like the freshness kinds these may
    /// repeat within one host session, with monotone 1-based indexes and
    /// per-kind caps enforced here. The buffer kinds bind the same-host
    /// close/wipe+reopen chain (old and new buffer numbers, unchanged server
    /// generation); the pending kinds bind a started pending action's wire
    /// identity, its identity-bound cancellation, and the observed rejection
    /// of a late old result against the replacement document instance. The
    /// earlier journeys never emit them.
    BufferWiped,
    BufferReopened,
    PendingActionStarted,
    PendingActionCancelled,
    LateResultRejected,
    /// One #11401 host session settled its own product result (the repeated
    /// denominator's per-iteration observation) and is about to exit through
    /// its designed exit path.
    SessionIterationSettled,
    /// The #11401 driver initiated the user-equivalent host exit (`:qa!`);
    /// the supervisor owns the actual exit observation.
    HostExitInitiated,
    ShutdownStarted,
    ShutdownCompleted,
    DriverFailed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DriverEvent {
    pub schema_version: String,
    pub sequence: u64,
    #[serde(rename = "event")]
    pub kind: DriverEventKind,
    #[serde(default)]
    pub details: BTreeMap<String, String>,
}

pub fn parse_driver_events(bytes: &[u8], require_complete: bool) -> Result<Vec<DriverEvent>> {
    let text = std::str::from_utf8(bytes).context("driver event stream is not UTF-8")?;
    let mut events = Vec::new();
    for (index, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let event: DriverEvent = serde_json::from_str(line)
            .with_context(|| format!("invalid driver event at line {}", index + 1))?;
        events.push(event);
    }
    validate_driver_events(&events, require_complete)?;
    Ok(events)
}

/// Validate one driver event stream. Laws beyond the Emacs-shared shape:
///
/// - `registration_selected` must bind the exact candidate digest and the
///   canonical `perllsp --stdio` argv token, so a run whose client registered
///   anything else (for example an ambient PATH `perllsp`) is rejected here;
/// - `buffer_enabled` must report native filetype detection; a pre-forced or
///   manual filetype (`detection` detail not `native_vim`) is a hard failure,
///   because #7762 activation may not be manufactured by the driver.
pub fn validate_driver_events(events: &[DriverEvent], require_complete: bool) -> Result<()> {
    ensure!(!events.is_empty(), "driver emitted no events");
    let mut singleton = BTreeSet::new();
    let mut last_lifecycle_rank = 0_u8;
    // Monotone last-seen indexes for the #11390 repeating freshness kinds.
    let mut freshness_mutation_index = 0_u32;
    let mut freshness_hold_index = 0_u32;
    let mut freshness_materialization_index = 0_u32;
    let mut freshness_generation_index = 0_u32;
    // Monotone last-seen indexes for the #11396 repeating save-format kinds.
    let mut save_owner_index = 0_u32;
    let mut save_settlement_index = 0_u32;
    let mut save_hold_index = 0_u32;
    // Monotone last-seen indexes for the #11401 repeating lifecycle kinds.
    let mut lifecycle_wipe_index = 0_u32;
    let mut lifecycle_reopen_index = 0_u32;
    let mut lifecycle_pending_index = 0_u32;
    let mut lifecycle_pending_cancel_index = 0_u32;
    let mut lifecycle_late_result_index = 0_u32;

    for (index, event) in events.iter().enumerate() {
        ensure!(event.schema_version == DRIVER_SCHEMA_VERSION, "unexpected driver event schema");
        ensure!(event.sequence == (index + 1) as u64, "driver event sequence is not contiguous");
        for (key, value) in &event.details {
            ensure!(is_reason_token(key), "driver detail key is not a reason token");
            validate_safe_identity(value, "driver detail value")?;
        }
        match event.kind {
            DriverEventKind::RegistrationSelected => {
                ensure!(
                    singleton.insert(DriverEventKind::RegistrationSelected),
                    "duplicate singleton driver event"
                );
                ensure!(
                    event.details.get("cmd") == Some(&"perllsp--stdio".to_string()),
                    "registration did not bind the canonical perllsp --stdio command identity"
                );
                let digest = event
                    .details
                    .get("candidate_sha256")
                    .context("registration_selected omitted candidate_sha256")?;
                validate_sha256(digest, "registration candidate_sha256")?;
                update_lifecycle_rank(event.kind, &mut last_lifecycle_rank)?;
            }
            DriverEventKind::BufferEnabled => {
                ensure!(
                    singleton.insert(DriverEventKind::BufferEnabled),
                    "duplicate singleton driver event"
                );
                let filetype =
                    event.details.get("filetype").context("buffer_enabled omitted filetype")?;
                ensure!(
                    filetype == "perl",
                    "buffer_enabled attached a non-Perl filetype: {filetype}"
                );
                ensure!(
                    event.details.get("detection") == Some(&"native_vim".to_string()),
                    "buffer_enabled filetype was not natively detected; a pre-forced filetype \
                     cannot manufacture #7762 activation"
                );
            }
            DriverEventKind::DriverFailed => {
                ensure!(
                    singleton.insert(DriverEventKind::DriverFailed),
                    "duplicate driver_failed event"
                );
                ensure!(event.details.contains_key("reason"), "driver_failed omitted reason");
            }
            DriverEventKind::DiagnosticsObserved => {
                ensure!(
                    singleton.insert(DriverEventKind::DiagnosticsObserved),
                    "duplicate singleton driver event"
                );
                ensure!(
                    event.details.get("mode") == Some(&"push".to_string()),
                    "diagnostics_observed must bind the observed push update path"
                );
                update_lifecycle_rank(event.kind, &mut last_lifecycle_rank)?;
            }
            DriverEventKind::DefectStateObserved => {
                ensure!(
                    singleton.insert(DriverEventKind::DefectStateObserved),
                    "duplicate singleton driver event"
                );
                ensure!(
                    event.details.get("state_source") == Some(&"client_state".to_string()),
                    "defect_state_observed must come from the client's own diagnostics state"
                );
                ensure!(
                    event.details.get("errors").is_some_and(|value| value.parse::<u32>().is_ok()),
                    "defect_state_observed must report the client-state error count"
                );
                update_lifecycle_rank(event.kind, &mut last_lifecycle_rank)?;
            }
            DriverEventKind::DefectFixApplied => {
                ensure!(
                    singleton.insert(DriverEventKind::DefectFixApplied),
                    "duplicate singleton driver event"
                );
                ensure!(
                    event.details.get("edit_path") == Some(&"buffer_did_change".to_string()),
                    "defect_fix_applied must bind the real buffer didChange flush path"
                );
                update_lifecycle_rank(event.kind, &mut last_lifecycle_rank)?;
            }
            DriverEventKind::CurrentStateObserved => {
                ensure!(
                    singleton.insert(DriverEventKind::CurrentStateObserved),
                    "duplicate singleton driver event"
                );
                ensure!(
                    event.details.get("state_source") == Some(&"client_state".to_string()),
                    "current_state_observed must come from the client's own diagnostics state"
                );
                ensure!(
                    event.details.get("discriminator_absent") == Some(&"1".to_string()),
                    "current_state_observed must prove the old discriminator absent"
                );
                ensure!(
                    event.details.get("barrier") == Some(&"diagnostics_event_and_wire".to_string()),
                    "current_state_observed must bind the deterministic currentness barrier"
                );
                update_lifecycle_rank(event.kind, &mut last_lifecycle_rank)?;
            }
            DriverEventKind::ExternalMutationApplied => {
                validate_repeating_freshness_event(
                    event,
                    "mutation_index",
                    EXTERNAL_MUTATION_CAP,
                    &mut freshness_mutation_index,
                )?;
                ensure!(
                    matches!(
                        event.details.get("mutation").map(String::as_str),
                        Some("in_place") | Some("atomic_replace")
                    ),
                    "external_mutation_applied must name in_place or atomic_replace"
                );
                ensure!(
                    matches!(
                        event.details.get("target").map(String::as_str),
                        Some("governed") | Some("decoy") | Some("project_config")
                    ),
                    "external_mutation_applied must name the governed, decoy, or project_config \
                     target"
                );
                ensure!(
                    event.details.contains_key("disk_generation"),
                    "external_mutation_applied must name the new on-disk generation"
                );
                update_lifecycle_rank(event.kind, &mut last_lifecycle_rank)?;
            }
            DriverEventKind::StaleGenerationHeld => {
                validate_repeating_freshness_event(
                    event,
                    "hold_index",
                    STALE_GENERATION_HOLD_CAP,
                    &mut freshness_hold_index,
                )?;
                ensure!(
                    event.details.contains_key("held_generation")
                        && event.details.contains_key("current_generation"),
                    "stale_generation_held must name the held and current generations"
                );
                ensure!(
                    event
                        .details
                        .get("window_ms")
                        .and_then(|value| value.parse::<u64>().ok())
                        .is_some_and(|window| window >= MIN_STALE_WINDOW_MS),
                    "stale_generation_held must carry a bounded observation window of at least \
                     {MIN_STALE_WINDOW_MS}ms"
                );
                ensure!(
                    event.details.get("state_held") == Some(&"1".to_string()),
                    "stale_generation_held must prove the client-state claim held for the whole                      window"
                );
                update_lifecycle_rank(event.kind, &mut last_lifecycle_rank)?;
            }
            DriverEventKind::ClientMaterializationApplied => {
                validate_repeating_freshness_event(
                    event,
                    "materialization_index",
                    CLIENT_MATERIALIZATION_CAP,
                    &mut freshness_materialization_index,
                )?;
                ensure!(
                    matches!(
                        event.details.get("materialization").map(String::as_str),
                        Some("client_close_reopen")
                            | Some("settings_push")
                            | Some("server_restart")
                    ),
                    "client_materialization_applied must name the exact client route that \
                     materialized the generation"
                );
                ensure!(
                    event.details.contains_key("picks_generation"),
                    "client_materialization_applied must name the generation it picks up"
                );
                update_lifecycle_rank(event.kind, &mut last_lifecycle_rank)?;
            }
            DriverEventKind::GenerationCurrentObserved => {
                validate_repeating_freshness_event(
                    event,
                    "generation_index",
                    GENERATION_CURRENT_CAP,
                    &mut freshness_generation_index,
                )?;
                ensure!(
                    event.details.get("state_source") == Some(&"client_state".to_string()),
                    "generation_current_observed must come from the client's own diagnostics state"
                );
                ensure!(
                    event.details.get("barrier") == Some(&"diagnostics_event_and_wire".to_string()),
                    "generation_current_observed must bind the deterministic currentness barrier"
                );
                ensure!(
                    event.details.get("errors").is_some_and(|value| value.parse::<u32>().is_ok())
                        && event
                            .details
                            .get("warnings")
                            .is_some_and(|value| value.parse::<u32>().is_ok()),
                    "generation_current_observed must report numeric client-state error and \
                     warning counts"
                );
                ensure!(
                    event.details.contains_key("generation"),
                    "generation_current_observed must name the accepted generation"
                );
                update_lifecycle_rank(event.kind, &mut last_lifecycle_rank)?;
            }
            DriverEventKind::SaveOwnerConfigured => {
                validate_repeating_save_event(
                    event,
                    "owner_index",
                    SAVE_OWNER_CONFIGURED_CAP,
                    &mut save_owner_index,
                )?;
                ensure!(
                    event
                        .details
                        .get("owner_count")
                        .is_some_and(|value| value.parse::<u32>().is_ok()),
                    "save_owner_configured must report a numeric owner_count"
                );
                ensure!(
                    matches!(
                        event.details.get("route").map(String::as_str),
                        Some("bufwritepre_autocmd") | Some("none")
                    ),
                    "save_owner_configured must name the bufwritepre_autocmd route or its absence"
                );
                ensure!(
                    event.details.get("action").map(String::as_str)
                        == Some("lsp_document_format_sync")
                        || event.details.get("route").map(String::as_str) == Some("none"),
                    "save_owner_configured must delegate to the canonical sync format action"
                );
                ensure!(
                    event
                        .details
                        .get("timeout_ms")
                        .is_some_and(|value| value.parse::<u64>().is_ok()),
                    "save_owner_configured must report a numeric bounded sync timeout"
                );
                update_lifecycle_rank(event.kind, &mut last_lifecycle_rank)?;
            }
            DriverEventKind::SaveSettlementObserved => {
                validate_repeating_save_event(
                    event,
                    "save_index",
                    SAVE_SETTLEMENT_CAP,
                    &mut save_settlement_index,
                )?;
                ensure!(
                    matches!(
                        event.details.get("trigger").map(String::as_str),
                        Some("bufwritepre_save") | Some("manual_comparator") | Some("none")
                    ),
                    "save_settlement_observed must name the bufwritepre_save, manual_comparator, \
                     or no trigger"
                );
                ensure!(
                    matches!(
                        event.details.get("disposition").map(String::as_str),
                        Some("applied")
                            | Some("no_change")
                            | Some("disabled")
                            | Some("refused")
                            | Some("failure")
                            | Some("stale_rejected")
                    ),
                    "save_settlement_observed must name a declared save disposition"
                );
                ensure!(
                    matches!(
                        event.details.get("response_kind").map(String::as_str),
                        Some("edits") | Some("empty") | Some("error") | Some("absent")
                    ),
                    "save_settlement_observed must name the settled response kind"
                );
                for key in ["requests_before", "requests_after", "owner_count"] {
                    ensure!(
                        event.details.get(key).is_some_and(|value| value.parse::<u32>().is_ok()),
                        "save_settlement_observed must report a numeric {key}"
                    );
                }
                for key in ["buffer_sha256", "file_sha256"] {
                    ensure!(
                        event
                            .details
                            .get(key)
                            .is_some_and(|value| validate_sha256(value, key).is_ok()),
                        "save_settlement_observed must report the exact {key} bytes identity"
                    );
                }
                update_lifecycle_rank(event.kind, &mut last_lifecycle_rank)?;
            }
            DriverEventKind::StaleResultHoldObserved => {
                validate_repeating_save_event(
                    event,
                    "hold_index",
                    STALE_RESULT_HOLD_CAP,
                    &mut save_hold_index,
                )?;
                ensure!(
                    event
                        .details
                        .get("window_ms")
                        .and_then(|value| value.parse::<u64>().ok())
                        .is_some_and(|window| window >= MIN_STALE_WINDOW_MS),
                    "stale_result_hold_observed must carry a bounded observation window of at \
                     least {MIN_STALE_WINDOW_MS}ms"
                );
                ensure!(
                    event.details.get("bytes_held") == Some(&"1".to_string()),
                    "stale_result_hold_observed must prove the byte state held for the whole window"
                );
                ensure!(
                    event.details.get("late_response_rejected") == Some(&"1".to_string()),
                    "stale_result_hold_observed must prove the late result was released and \
                     never applied"
                );
                update_lifecycle_rank(event.kind, &mut last_lifecycle_rank)?;
            }
            DriverEventKind::BufferWiped => {
                validate_repeating_lifecycle_event(
                    event,
                    "wipe_index",
                    BUFFER_WIPE_CAP,
                    &mut lifecycle_wipe_index,
                )?;
                ensure!(
                    event.details.get("didclose_sent") == Some(&"1".to_string()),
                    "buffer_wiped must bind the real client didClose flush path"
                );
                ensure!(
                    event
                        .details
                        .get("bufnr")
                        .and_then(|value| value.parse::<u32>().ok())
                        .is_some(),
                    "buffer_wiped must report the wiped buffer number"
                );
                update_lifecycle_rank(event.kind, &mut last_lifecycle_rank)?;
            }
            DriverEventKind::BufferReopened => {
                validate_repeating_lifecycle_event(
                    event,
                    "reopen_index",
                    BUFFER_REOPEN_CAP,
                    &mut lifecycle_reopen_index,
                )?;
                ensure!(
                    event.details.get("same_path") == Some(&"1".to_string()),
                    "buffer_reopened must bind the same governed path"
                );
                let old_bufnr =
                    event.details.get("old_bufnr").and_then(|value| value.parse::<u32>().ok());
                let new_bufnr =
                    event.details.get("new_bufnr").and_then(|value| value.parse::<u32>().ok());
                ensure!(
                    old_bufnr.is_some() && new_bufnr.is_some(),
                    "buffer_reopened must report numeric old and new buffer numbers"
                );
                ensure!(
                    old_bufnr != new_bufnr,
                    "buffer_reopened must bind a changed document instance; a same-buffer \
                     observation is not a reopen"
                );
                ensure!(
                    event
                        .details
                        .get("server_init_count")
                        .and_then(|value| value.parse::<u32>().ok())
                        .is_some(),
                    "buffer_reopened must report the server init generation at the reopen (the \
                     same-host law: no server restart may hide inside a buffer reopen)"
                );
                update_lifecycle_rank(event.kind, &mut last_lifecycle_rank)?;
            }
            DriverEventKind::PendingActionStarted => {
                validate_repeating_lifecycle_event(
                    event,
                    "pending_index",
                    PENDING_ACTION_CAP,
                    &mut lifecycle_pending_index,
                )?;
                ensure!(
                    event.details.get("method") == Some(&"textDocument/documentSymbol".to_string()),
                    "pending_action_started must name the deterministic pending observation \
                     method"
                );
                ensure!(
                    event
                        .details
                        .get("request_id")
                        .and_then(|value| value.parse::<u32>().ok())
                        .is_some_and(|id| id > 0),
                    "pending_action_started must bind the positive wire request identity"
                );
                ensure!(
                    event
                        .details
                        .get("target_bufnr")
                        .and_then(|value| value.parse::<u32>().ok())
                        .is_some(),
                    "pending_action_started must report the target buffer number"
                );
                update_lifecycle_rank(event.kind, &mut last_lifecycle_rank)?;
            }
            DriverEventKind::PendingActionCancelled => {
                validate_referencing_lifecycle_event(
                    event,
                    "cancel_index",
                    "pending_index",
                    PENDING_CANCEL_CAP,
                    &mut lifecycle_pending_cancel_index,
                    lifecycle_pending_index,
                )?;
                ensure!(
                    event.details.get("cancel_sent") == Some(&"1".to_string()),
                    "pending_action_cancelled must bind the identity-bound cancellation send"
                );
                ensure!(
                    event
                        .details
                        .get("request_id")
                        .and_then(|value| value.parse::<u32>().ok())
                        .is_some_and(|id| id > 0),
                    "pending_action_cancelled must bind the cancelled request identity"
                );
                ensure!(
                    event.details.get("notification_count") == Some(&"0".to_string()),
                    "pending_action_cancelled must prove zero admissions after cancellation; a \
                     cancelled result that was delivered is a contract violation, never a pass"
                );
                update_lifecycle_rank(event.kind, &mut last_lifecycle_rank)?;
            }
            DriverEventKind::LateResultRejected => {
                validate_referencing_lifecycle_event(
                    event,
                    "late_index",
                    "pending_index",
                    LATE_RESULT_CAP,
                    &mut lifecycle_late_result_index,
                    lifecycle_pending_index,
                )?;
                ensure!(
                    event.details.get("response_delivered") == Some(&"1".to_string()),
                    "late_result_rejected must prove the old operation completed (the response \
                     arrived); an uncompleted observation is not a late result"
                );
                ensure!(
                    event.details.get("replacement_state_unchanged") == Some(&"1".to_string()),
                    "late_result_rejected must prove the replacement instance stayed unchanged \
                     across the bounded observation window"
                );
                ensure!(
                    event
                        .details
                        .get("window_ms")
                        .and_then(|value| value.parse::<u64>().ok())
                        .is_some_and(|window| window >= MIN_STALE_WINDOW_MS),
                    "late_result_rejected must carry a bounded observation window of at least \
                     {MIN_STALE_WINDOW_MS}ms"
                );
                ensure!(
                    event
                        .details
                        .get("request_id")
                        .and_then(|value| value.parse::<u32>().ok())
                        .is_some_and(|id| id > 0),
                    "late_result_rejected must bind the late request identity"
                );
                update_lifecycle_rank(event.kind, &mut last_lifecycle_rank)?;
            }
            DriverEventKind::SessionIterationSettled => {
                ensure!(
                    singleton.insert(DriverEventKind::SessionIterationSettled),
                    "duplicate singleton driver event"
                );
                ensure!(
                    matches!(
                        event.details.get("session_role").map(String::as_str),
                        Some("full_lifecycle_session")
                            | Some("replacement_host_session")
                            | Some("assertion_failure_session")
                            | Some("timeout_interruption_session")
                            | Some("server_restart_relabel_session")
                    ),
                    "session_iteration_settled must name its exact designed session role"
                );
                ensure!(
                    event.details.contains_key("iteration_index")
                        && event.details.contains_key("product_result"),
                    "session_iteration_settled must bind its iteration denominator index and \
                     product result"
                );
                update_lifecycle_rank(event.kind, &mut last_lifecycle_rank)?;
            }
            DriverEventKind::HostExitInitiated => {
                ensure!(
                    singleton.insert(DriverEventKind::HostExitInitiated),
                    "duplicate singleton driver event"
                );
                ensure!(
                    event.details.get("exit_path") == Some(&"user_qa".to_string()),
                    "host_exit_initiated must bind the user-equivalent exit path"
                );
                update_lifecycle_rank(event.kind, &mut last_lifecycle_rank)?;
            }
            kind => {
                ensure!(singleton.insert(kind), "duplicate singleton driver event");
                let rank = lifecycle_rank(kind);
                ensure!(
                    rank >= last_lifecycle_rank,
                    "driver lifecycle events arrived out of order"
                );
                last_lifecycle_rank = rank;
            }
        }
    }

    if require_complete {
        ensure!(
            !singleton.contains(&DriverEventKind::DriverFailed),
            "complete host run reported driver failure"
        );
        for required in [
            DriverEventKind::HostStarted,
            DriverEventKind::ClientLoaded,
            DriverEventKind::RegistrationSelected,
            DriverEventKind::ServerInitialized,
            DriverEventKind::BufferEnabled,
            DriverEventKind::InitializeObserved,
            DriverEventKind::RootSelected,
            DriverEventKind::FixtureOpened,
            DriverEventKind::DiagnosticsObserved,
            DriverEventKind::ShutdownStarted,
            DriverEventKind::ShutdownCompleted,
        ] {
            ensure!(singleton.contains(&required), "complete host run omitted {required:?}");
        }
    }
    Ok(())
}

fn lifecycle_rank(kind: DriverEventKind) -> u8 {
    match kind {
        DriverEventKind::HostStarted => 1,
        DriverEventKind::ClientLoaded => 2,
        DriverEventKind::RegistrationSelected => 3,
        DriverEventKind::FixtureOpened => 4,
        DriverEventKind::ServerInitialized => 5,
        DriverEventKind::BufferEnabled => 6,
        DriverEventKind::InitializeObserved => 7,
        DriverEventKind::RootSelected => 8,
        // The #10946 diagnostics tier carries its own strict order: wire push
        // observed, then the client-state defect observation, then the fix
        // edit, then the post-edit current state — all strictly before
        // shutdown. Ranks are internal; only their order is contractual.
        DriverEventKind::DiagnosticsObserved => 40,
        DriverEventKind::DefectStateObserved => 41,
        DriverEventKind::DefectFixApplied => 42,
        DriverEventKind::CurrentStateObserved => 43,
        // The #11390 freshness-generation tier: the repeating kinds share one
        // rank because their phases legitimately interleave (a mutation, its
        // stale-hold window, its materialization, its current observation),
        // with per-kind monotone indexes carrying the order. The #11396
        // save-format kinds and the #11401 host-reopen kinds join the same
        // tier for the same reason: source/config mutations, materializations,
        // save settlements, pending actions, and buffer wipes/reopens all
        // interleave inside one journey. All strictly before the session
        // settlement and shutdown tiers.
        DriverEventKind::ExternalMutationApplied
        | DriverEventKind::StaleGenerationHeld
        | DriverEventKind::ClientMaterializationApplied
        | DriverEventKind::GenerationCurrentObserved
        | DriverEventKind::BufferWiped
        | DriverEventKind::BufferReopened
        | DriverEventKind::PendingActionStarted
        | DriverEventKind::PendingActionCancelled
        | DriverEventKind::LateResultRejected
        | DriverEventKind::SaveOwnerConfigured
        | DriverEventKind::SaveSettlementObserved
        | DriverEventKind::StaleResultHoldObserved => 44,
        // The session's own product result settles strictly before its
        // terminal path (orderly stop, forced failure, or the indefinite
        // barrier of the timeout shape).
        DriverEventKind::SessionIterationSettled => 49,
        DriverEventKind::ShutdownStarted => 50,
        DriverEventKind::ShutdownCompleted => 51,
        DriverEventKind::DriverFailed => 51,
        // The user-equivalent exit initiation is the last driver-side event of
        // an orderly session (the supervisor owns the exit observation).
        DriverEventKind::HostExitInitiated => 52,
    }
}

/// Per-kind occurrence caps for the #11390 repeating freshness events. The
/// caps bound the authored journeys (mutations across source and config
/// generations; holds for each stale window; materializations for each reload,
/// push, and restart; one current observation per accepted generation); a
/// stream exceeding a cap is an instrument fault, never evidence.
pub const EXTERNAL_MUTATION_CAP: u32 = 6;
pub const STALE_GENERATION_HOLD_CAP: u32 = 6;
pub const CLIENT_MATERIALIZATION_CAP: u32 = 10;
pub const GENERATION_CURRENT_CAP: u32 = 12;
/// Per-kind occurrence caps for the #11396 repeating save-format events: the
/// authored journey re-arms the owner a bounded number of times (single,
/// removed for the disabled leg, re-armed after each restart) and walks seven
/// settlements plus one stale-result hold.
pub const SAVE_OWNER_CONFIGURED_CAP: u32 = 8;
pub const SAVE_SETTLEMENT_CAP: u32 = 10;
pub const STALE_RESULT_HOLD_CAP: u32 = 4;
/// Per-kind occurrence caps for the #11401 repeating lifecycle events: one
/// wipe/reopen chain, up to three pending actions (identity-bound cancel,
/// late-result document route, in-flight-at-exit host route), one observed
/// late rejection per journey shape. A stream exceeding a cap is an
/// instrument fault, never evidence.
pub const BUFFER_WIPE_CAP: u32 = 2;
pub const BUFFER_REOPEN_CAP: u32 = 2;
pub const PENDING_ACTION_CAP: u32 = 3;
pub const PENDING_CANCEL_CAP: u32 = 2;
pub const LATE_RESULT_CAP: u32 = 2;
/// The minimum honest absence-observation window for a stale-generation hold:
/// below this the "no spontaneous republish" claim carries no observation.
pub const MIN_STALE_WINDOW_MS: u64 = 2000;

/// Validate one repeating #11390 freshness event: its index detail is numeric,
/// exactly one greater than the last seen index for its kind (monotone,
/// gap-free), and within the kind's cap.
fn validate_repeating_freshness_event(
    event: &DriverEvent,
    index_key: &str,
    cap: u32,
    last_index: &mut u32,
) -> Result<()> {
    validate_repeating_index(event, index_key, cap, last_index)
}

/// Validate one repeating #11396 save-format event with the same law.
fn validate_repeating_save_event(
    event: &DriverEvent,
    index_key: &str,
    cap: u32,
    last_index: &mut u32,
) -> Result<()> {
    validate_repeating_index(event, index_key, cap, last_index)
}

/// Validate one repeating #11401 lifecycle event: same monotone, gap-free,
/// capped index law as the freshness kinds.
fn validate_repeating_lifecycle_event(
    event: &DriverEvent,
    index_key: &str,
    cap: u32,
    last_index: &mut u32,
) -> Result<()> {
    validate_repeating_index(event, index_key, cap, last_index)
}

/// Validate one #11401 lifecycle event that references an already-started
/// pending action (a cancellation or a late-result observation of pending
/// action N): its per-kind occurrence index (`occurrence_key`) is monotone,
/// gap-free, and capped, and its referenced pending index
/// (`reference_key`) must name a pending action that already started — the
/// pending-action namespace is shared with `pending_action_started`, so a
/// cancel or late claim for an unstarted action is rejected here.
fn validate_referencing_lifecycle_event(
    event: &DriverEvent,
    occurrence_key: &str,
    reference_key: &str,
    cap: u32,
    last_index: &mut u32,
    started_index: u32,
) -> Result<()> {
    let referenced =
        event
            .details
            .get(reference_key)
            .and_then(|value| value.parse::<u32>().ok())
            .with_context(|| format!("lifecycle event omitted a numeric {reference_key}"))?;
    ensure!(
        referenced >= 1 && referenced <= started_index,
        "lifecycle event {reference_key} {referenced} references a pending action that has not \
         started (started so far: {started_index})"
    );
    validate_repeating_index(event, occurrence_key, cap, last_index)
}

fn validate_repeating_index(
    event: &DriverEvent,
    index_key: &str,
    cap: u32,
    last_index: &mut u32,
) -> Result<()> {
    let index = event
        .details
        .get(index_key)
        .and_then(|value| value.parse::<u32>().ok())
        .with_context(|| format!("repeating event omitted a numeric {index_key}"))?;
    ensure!(
        index == *last_index + 1,
        "repeating event {index_key} {} is not exactly one greater than the last seen {}",
        index,
        *last_index
    );
    ensure!(index <= cap, "repeating event {index_key} {index} exceeds the journey cap {cap}");
    *last_index = index;
    Ok(())
}

/// Advance the observed lifecycle rank, rejecting any event that arrives out
/// of order. Used by every dedicated match arm so no event kind can bypass
/// the ordering law (#10946 review finding: the dedicated arms must enforce
/// the same ordering the fallback arm enforces).
fn update_lifecycle_rank(kind: DriverEventKind, last_lifecycle_rank: &mut u8) -> Result<()> {
    let rank = lifecycle_rank(kind);
    ensure!(rank >= *last_lifecycle_rank, "driver lifecycle events arrived out of order");
    *last_lifecycle_rank = rank;
    Ok(())
}

// ---------------------------------------------------------------------------
// Owned-process supervision (mechanics owned by `xtask::editor_host`)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct ProcessObservation {
    pub status_code: Option<i32>,
    pub timed_out: bool,
    pub kill_requested: bool,
    pub cleanup: CleanupResult,
    pub cleanup_detail: String,
    pub events: Vec<DriverEvent>,
    pub driver_complete: bool,
    pub artifacts: Vec<EvidenceArtifact>,
}

impl ProcessObservation {
    pub fn passed_process_boundary(&self) -> bool {
        self.status_code == Some(0)
            && !self.timed_out
            && self.cleanup == CleanupResult::Pass
            && self.driver_complete
    }
}

/// Execute one owned host process under a parent-owned hard deadline with a
/// deterministic before/after process-set comparison for the exact candidate
/// executable. The mechanics — parent-owned deadline, separated captures,
/// numeric set comparison, forced-kill classification, orderly-exit law — are
/// owned by [`xtask::editor_host`]; this binding keeps only the Vim run plan,
/// needle policy, and evidence retention.
pub fn run_owned_process(
    command: &mut Command,
    plan: &VimHostRunPlan,
    layout: &HermeticVimLayout,
) -> Result<ProcessObservation> {
    // The needle binds the exact candidate. On platforms whose probe reports
    // full command lines (`ps -eo pid=,args=`) the Vim-normalized absolute
    // executable path is used, so an unrelated `perllsp` — a decoy on PATH or
    // a concurrent host run from another checkout — can never be attributed
    // to this run. On Windows `tasklist` exposes only the image name, so the
    // file name is the narrowest observable needle there.
    let needle = if cfg!(windows) {
        plan.paths
            .candidate_executable
            .file_name()
            .and_then(OsStr::to_str)
            .unwrap_or("perllsp")
            .to_string()
    } else {
        vim_path(&plan.paths.candidate_executable).to_string_lossy().into_owned()
    };

    let probe_before = ProbeCapture::take();
    let bounded: BoundedRun =
        xtask::editor_host::bounded_run(command, plan.identity.timeout_ms, "the Vim host subject")?;
    let probe_after = ProbeCapture::take();
    let probes_available = matches!(probe_before, ProbeCapture::Captured(_))
        && matches!(probe_after, ProbeCapture::Captured(_));
    let judgment = judge_cleanup(
        &probe_before,
        &probe_after,
        &needle,
        cfg!(windows),
        bounded.orderly_success(),
    );

    // Retain both raw snapshots as run evidence even when the comparison
    // itself could not be made. Retention is deliberately best-effort: a
    // locked or unwritable snapshot target must not abort the run before a
    // receipt exists — every run that reaches the process stage emits one,
    // and a post-launch retention failure stays distinguishable from a run
    // that never launched.
    if let Some(before_text) = &judgment.before_snapshot {
        let _ = fs::write(layout.process_snapshot_before(), before_text);
    }
    let _ = fs::write(
        layout.process_snapshot_after(),
        judgment.after_snapshot.clone().unwrap_or_default(),
    );

    let event_bytes = fs::read(layout.event_file()).unwrap_or_default();
    let events = parse_driver_events(&event_bytes, false).unwrap_or_default();
    let driver_complete = validate_driver_events(&events, true).is_ok();

    let mut artifacts = Vec::new();
    artifacts.push(write_sanitized_artifact(
        &layout.artifact_directory,
        "vim/driver-stdout.log",
        ArtifactKind::DriverOutput,
        &bounded.stdout,
        plan,
        layout,
    )?);
    artifacts.push(write_sanitized_artifact(
        &layout.artifact_directory,
        "vim/driver-stderr.log",
        ArtifactKind::DriverOutput,
        &bounded.stderr,
        plan,
        layout,
    )?);
    artifacts.push(write_sanitized_artifact(
        &layout.artifact_directory,
        "vim/driver-events.jsonl",
        ArtifactKind::DriverOutput,
        &event_bytes,
        plan,
        layout,
    )?);

    // The server trace is retained under its configured name, or under the
    // dated variant perllsp writes (`PERL_LSP_LOG_FILE` gains a `.YYYY-MM-DD`
    // suffix) — either satisfies the ServerStderr artifact obligation, and
    // the retained copy keeps the resolved file name.
    if let Some((path, id)) = resolve_server_trace(layout) {
        let bytes =
            fs::read(&path).with_context(|| format!("reading host artifact {}", path.display()))?;
        artifacts.push(write_sanitized_artifact(
            &layout.artifact_directory,
            &id,
            ArtifactKind::ServerStderr,
            &bytes,
            plan,
            layout,
        )?);
    }

    for (path, id, kind) in [
        (layout.client_log(), "vim/vim-lsp-client.log", ArtifactKind::ClientLog),
        (layout.capability_snapshot(), "vim/initialize.json", ArtifactKind::CapabilitySnapshot),
    ] {
        if path.is_file() {
            let bytes = fs::read(&path)
                .with_context(|| format!("reading host artifact {}", path.display()))?;
            artifacts.push(write_sanitized_artifact(
                &layout.artifact_directory,
                id,
                kind,
                &bytes,
                plan,
                layout,
            )?);
        }
    }

    artifacts.push(
        HostProcessLedger::record(
            &bounded,
            events.len(),
            driver_complete,
            probes_available,
            &judgment,
        )
        .artifact(
            &layout.artifact_directory,
            "vim/process-ledger.json",
            &vim_redactions(plan, layout),
        )?,
    );

    Ok(ProcessObservation {
        status_code: bounded.status_code,
        timed_out: bounded.timed_out,
        kill_requested: bounded.kill_requested,
        cleanup: judgment.result,
        cleanup_detail: judgment.detail,
        events,
        driver_complete,
        artifacts,
    })
}

/// Resolve the server trace file: the configured path when it exists, else
/// the dated variant perllsp writes next to it (`perllsp.log.YYYY-MM-DD`).
/// Returns the path and the artifact id preserving the resolved file name.
fn resolve_server_trace(layout: &HermeticVimLayout) -> Option<(PathBuf, String)> {
    let configured = layout.server_trace();
    if configured.is_file() {
        let name = configured.file_name()?.to_str()?.to_string();
        return Some((configured, format!("vim/{name}")));
    }
    let parent = configured.parent()?;
    let stem = configured.file_stem()?.to_str()?;
    let mut candidates: Vec<PathBuf> = fs::read_dir(parent)
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name().and_then(OsStr::to_str).is_some_and(|name| {
                name.starts_with(&format!("{stem}.")) && name.len() > stem.len() + 1
            })
        })
        .collect();
    candidates.sort();
    let path = candidates.into_iter().next()?;
    let name = path.file_name()?.to_str()?.to_string();
    Some((path, format!("vim/{name}")))
}

/// The Vim run plan's capture redaction map. Every absolute path the run plan
/// accepts must appear here: captures are written as sanitized artifacts that
/// may be uploaded, and an omitted path leaks the checkout or user directory
/// it came from.
fn vim_redactions(plan: &VimHostRunPlan, layout: &HermeticVimLayout) -> Vec<PathRedaction> {
    vec![
        PathRedaction { path: layout.root.clone(), token: "<RUN_ROOT>" },
        PathRedaction { path: plan.paths.artifact_root.clone(), token: "<ARTIFACT_ROOT>" },
        PathRedaction { path: plan.paths.fixture_root.clone(), token: "<WORKSPACE>" },
        PathRedaction { path: plan.paths.candidate_executable.clone(), token: "<CANDIDATE>" },
        PathRedaction { path: plan.paths.vim_executable.clone(), token: "<VIM>" },
        PathRedaction { path: plan.paths.vim_lsp_checkout.clone(), token: "<VIM_LSP_CHECKOUT>" },
        PathRedaction { path: plan.paths.driver.clone(), token: "<DRIVER>" },
        PathRedaction { path: plan.paths.adapter.clone(), token: "<ADAPTER>" },
    ]
}

/// Write one sanitized, bounded run artifact through the shared substrate.
fn write_sanitized_artifact(
    artifact_root: &Path,
    id: &str,
    kind: ArtifactKind,
    bytes: &[u8],
    plan: &VimHostRunPlan,
    layout: &HermeticVimLayout,
) -> Result<EvidenceArtifact> {
    write_artifact(artifact_root, id, kind, bytes, &vim_redactions(plan, layout))
}

// ---------------------------------------------------------------------------
// Wire-evidence extraction from the vim-lsp client log
// ---------------------------------------------------------------------------

/// One mined `textDocument/publishDiagnostics` batch from the client log
/// (#10946). The batch is the client's own record of what the server pushed:
/// which document (by file-name token), how many diagnostics, how many at
/// error severity, and how many carry a parser-family code (`PL0xx`), the
/// stable discriminator of a perllsp syntax defect. The line index preserves
/// wire ordering so a post-edit batch can be distinguished from a pre-edit
/// one by its position relative to the `textDocument/didChange` request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublishDiagnosticsBatch {
    pub line_index: usize,
    pub uri_file: String,
    pub diagnostics_count: usize,
    pub error_severity_count: usize,
    pub parser_code_count: usize,
}

/// The minimal LSP wire facts mined from the vim-lsp client log
/// (`g:lsp_log_file`), the same proven extraction surface the #7810 shell
/// harness used, now owned by Rust so the receipt rests on parsed evidence
/// rather than a shell pipeline.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WireEvidence {
    pub saw_initialize: bool,
    pub saw_initialized: bool,
    pub saw_shutdown: bool,
    pub saw_exit: bool,
    pub saw_publish_diagnostics: bool,
    /// The client logged its own job-exit handler (`s:on_exit` line) — the
    /// editor-side evidence that the client observed the server process end,
    /// including when it only arrives during the editor's teardown.
    pub saw_client_exit_log: bool,
    /// Line index of the first `textDocument/didChange` notification, for
    /// post-edit currentness ordering (#10946).
    pub did_change_line: Option<usize>,
    /// Every parsed `textDocument/publishDiagnostics` batch in wire order
    /// (#10946).
    pub publish_diagnostics_batches: Vec<PublishDiagnosticsBatch>,
    /// The whole first `initialize` request envelope, if the log carried one.
    pub initialize_request: Option<serde_json::Value>,
    /// The client capabilities object of the first `initialize` request, if
    /// the log carried one.
    pub client_capabilities: Option<serde_json::Value>,
}

/// Extract the wire facts from client-log bytes. Each log line may carry a
/// timestamp or label prefix (including earlier bracketed fields, exactly
/// like the real vim-lsp client log); the JSON payload is found by trying
/// every `[`/`{` start until one parses. The payload may be an object or an
/// envelope array; method fields are walked recursively, like the heritage
/// extraction. vim-lsp's own lifecycle trace lines (arrays whose first
/// element is a label such as `s:on_exit`) are recognized separately.
pub fn extract_wire_evidence(log: &[u8]) -> WireEvidence {
    let text = String::from_utf8_lossy(log);
    let mut evidence = WireEvidence::default();
    for (index, line) in text.lines().enumerate() {
        let Some(value) = parse_first_json_value(line) else {
            continue;
        };
        if let serde_json::Value::Array(items) = &value
            && items.first().and_then(serde_json::Value::as_str) == Some("s:on_exit")
        {
            evidence.saw_client_exit_log = true;
        }
        let mut first_initialize: Option<serde_json::Value> = None;
        walk_wire_value(&value, index, &mut evidence, &mut first_initialize);
        if let (Some(request), None) = (&first_initialize, &evidence.initialize_request) {
            evidence.initialize_request = Some(request.clone());
            evidence.client_capabilities =
                request.get("params").and_then(|params| params.get("capabilities")).cloned();
        }
    }
    evidence
}

fn parse_first_json_value(line: &str) -> Option<serde_json::Value> {
    for (index, byte) in line.bytes().enumerate() {
        if (byte == b'[' || byte == b'{')
            && let Ok(value) = serde_json::from_str::<serde_json::Value>(&line[index..])
        {
            return Some(value);
        }
    }
    None
}

#[allow(clippy::too_many_lines)]
fn walk_wire_value(
    value: &serde_json::Value,
    line_index: usize,
    evidence: &mut WireEvidence,
    first_initialize: &mut Option<serde_json::Value>,
) {
    match value {
        serde_json::Value::Object(map) => {
            if let Some(serde_json::Value::String(method)) = map.get("method") {
                match method.as_str() {
                    "initialize" => {
                        evidence.saw_initialize = true;
                        if first_initialize.is_none() {
                            *first_initialize = Some(serde_json::Value::Object(map.clone()));
                        }
                    }
                    "initialized" => evidence.saw_initialized = true,
                    "shutdown" => evidence.saw_shutdown = true,
                    "exit" => evidence.saw_exit = true,
                    "textDocument/publishDiagnostics" => {
                        evidence.saw_publish_diagnostics = true;
                        if let Some(batch) = mine_publish_diagnostics_batch(map, line_index) {
                            evidence.publish_diagnostics_batches.push(batch);
                        }
                    }
                    // Set-once latch on the FIRST didChange line: `get_or_insert`
                    // is the shape #12910 asks for here, and states the
                    // keep-the-earliest intent the nested `if` only implied.
                    "textDocument/didChange" => {
                        evidence.did_change_line.get_or_insert(line_index);
                    }
                    _ => {}
                }
            }
            for child in map.values() {
                walk_wire_value(child, line_index, evidence, first_initialize);
            }
        }
        serde_json::Value::Array(items) => {
            for child in items {
                walk_wire_value(child, line_index, evidence, first_initialize);
            }
        }
        _ => {}
    }
}

/// Mine one publishDiagnostics batch from its notification object: the
/// document's file-name token, diagnostic count, error-severity count, and
/// parser-code count. Batches whose params are absent or malformed are
/// skipped (the boolean `saw_publish_diagnostics` stays the only fact then).
fn mine_publish_diagnostics_batch(
    map: &serde_json::Map<String, serde_json::Value>,
    line_index: usize,
) -> Option<PublishDiagnosticsBatch> {
    let params = map.get("params")?;
    let uri = params.get("uri")?.as_str()?;
    let uri_file = uri.rsplit('/').next().unwrap_or("").to_string();
    if uri_file.is_empty() || uri_file.contains('\\') {
        return None;
    }
    let diagnostics = params.get("diagnostics")?.as_array()?;
    let mut error_severity_count = 0;
    let mut parser_code_count = 0;
    for diagnostic in diagnostics {
        if diagnostic.get("severity").and_then(serde_json::Value::as_i64) == Some(1) {
            error_severity_count += 1;
        }
        let code = diagnostic.get("code").and_then(serde_json::Value::as_str).unwrap_or("");
        if code.len() == 5 && code.starts_with("PL0") {
            parser_code_count += 1;
        }
    }
    Some(PublishDiagnosticsBatch {
        line_index,
        uri_file,
        diagnostics_count: diagnostics.len(),
        error_severity_count,
        parser_code_count,
    })
}

// ---------------------------------------------------------------------------
// Receipt
// ---------------------------------------------------------------------------

/// Retain the mined wire evidence as separately identified artifacts: the
/// first `initialize` request, its client-capabilities object, and the
/// lifecycle notification summary. These are the initialize/attach artifacts
/// the canonical receipt references; they are sanitized and digest-bound like
/// every other capture.
pub fn retain_wire_evidence_artifacts(
    plan: &VimHostRunPlan,
    layout: &HermeticVimLayout,
    evidence: &WireEvidence,
) -> Result<Vec<EvidenceArtifact>> {
    let mut artifacts = Vec::new();
    if let Some(request) = &evidence.initialize_request {
        let bytes = serde_json::to_vec_pretty(request)?;
        artifacts.push(write_sanitized_artifact(
            &layout.artifact_directory,
            "vim/initialize-request.json",
            ArtifactKind::Other,
            &bytes,
            plan,
            layout,
        )?);
    }
    if let Some(capabilities) = &evidence.client_capabilities {
        let bytes = serde_json::to_vec_pretty(capabilities)?;
        artifacts.push(write_sanitized_artifact(
            &layout.artifact_directory,
            "vim/client-capabilities.json",
            ArtifactKind::Other,
            &bytes,
            plan,
            layout,
        )?);
    }
    let lifecycle = serde_json::json!({
        "schema_version": "vim_host_wire_lifecycle.v1",
        "saw_initialize": evidence.saw_initialize,
        "saw_initialized": evidence.saw_initialized,
        "saw_shutdown": evidence.saw_shutdown,
        "saw_exit": evidence.saw_exit,
        "saw_publish_diagnostics": evidence.saw_publish_diagnostics,
    });
    artifacts.push(write_sanitized_artifact(
        &layout.artifact_directory,
        "vim/wire-lifecycle.json",
        ArtifactKind::Other,
        &serde_json::to_vec_pretty(&lifecycle)?,
        plan,
        layout,
    )?);
    Ok(artifacts)
}

/// Derive the receipt capability identity from the mined wire evidence: the
/// client's offered position encodings come from its own `initialize`
/// capabilities; an absent offer selects the protocol default utf-16 (LSP
/// 3.17). An absent initialize request leaves the basis not-proven.
pub fn capabilities_from_wire_evidence(
    evidence: &WireEvidence,
    snapshot_sha256: Option<String>,
) -> Result<CapabilityIdentity> {
    let offered = evidence
        .client_capabilities
        .as_ref()
        .and_then(|capabilities| capabilities.get("positionEncoding"))
        .and_then(|value| value.as_str())
        .map(|encoding| vec![encoding.to_string()]);
    match (offered, snapshot_sha256) {
        (Some(encodings), Some(digest)) => Ok(CapabilityIdentity {
            initialize_snapshot_sha256: digest,
            position_encodings_offered: encodings.clone(),
            position_encoding_basis: PositionEncodingBasis::Offered,
            position_encoding_selected: encodings.first().cloned(),
        }),
        (None, Some(digest)) => Ok(CapabilityIdentity {
            initialize_snapshot_sha256: digest,
            position_encodings_offered: Vec::new(),
            position_encoding_basis: PositionEncodingBasis::ProtocolDefault,
            position_encoding_selected: Some("utf-16".to_string()),
        }),
        (_, None) => Ok(CapabilityIdentity {
            // Hash of zero bytes: the snapshot is absent, and the receipt's
            // limitation says so. It never stands in for content.
            initialize_snapshot_sha256: bytes_sha256(&[])?,
            position_encodings_offered: Vec::new(),
            position_encoding_basis: PositionEncodingBasis::NotProven,
            position_encoding_selected: None,
        }),
    }
}

/// Derive the diagnostics identity from the mined wire evidence: observed
/// `textDocument/publishDiagnostics` notifications prove the push path; an
/// unobserved path stays not-proven.
pub fn diagnostics_from_wire_evidence(evidence: &WireEvidence) -> DiagnosticsIdentity {
    if evidence.saw_publish_diagnostics {
        DiagnosticsIdentity {
            advertised_mode: DiagnosticMode::Push,
            observed_messages: vec!["publish_diagnostics".to_string()],
        }
    } else {
        DiagnosticsIdentity {
            advertised_mode: DiagnosticMode::NotProven,
            observed_messages: Vec::new(),
        }
    }
}

/// Compose the canonical generic editor-client receipt. The Vim-specific
/// detail rides inside the shared schema (host identity, journey cells,
/// limitations); there is no second outer support schema.
#[allow(clippy::too_many_arguments)]
pub fn build_receipt(
    plan: &VimHostRunPlan,
    observation: &ProcessObservation,
    capabilities: CapabilityIdentity,
    diagnostics: DiagnosticsIdentity,
    journey: Vec<JourneyCell>,
    result: ObservationResult,
    failure_class: Option<FailureClass>,
    limitations: Vec<String>,
    claim_boundary: String,
) -> EditorClientCompatReceipt {
    EditorClientCompatReceipt {
        schema_version: RECEIPT_SCHEMA_VERSION.to_string(),
        observed_at: Utc::now().to_rfc3339(),
        stage: plan.identity.stage,
        repository: plan.identity.repository.clone(),
        candidate_sha: plan.identity.candidate_sha.clone(),
        platform: plan.identity.platform.clone(),
        host: HostIdentity {
            client_id: VIM_LSP_CLIENT_ID.to_string(),
            product: "vim".to_string(),
            version: plan.identity.vim_lsp_commit.clone(),
            source_state: ClientSourceState::UpstreamSource,
            source_ref: plan.client_source_ref(),
            executable_sha256: plan.identity.vim_build_sha256.clone(),
        },
        integration: IntegrationIdentity {
            mode: IntegrationMode::GenericLsp,
            registration_state: plan.identity.registration_state,
            configuration_sha256: plan.identity.configuration_sha256.clone(),
            driver_sha256: plan.identity.driver_sha256.clone(),
        },
        server: ServerIdentity {
            executable: "perllsp".to_string(),
            version: plan.identity.candidate_version.clone(),
            build_revision: plan.identity.candidate_build_revision.clone(),
            artifact_sha256: plan.identity.candidate_artifact_sha256.clone(),
            protocol_version: "3.17".to_string(),
            launch_command: vec!["perllsp".to_string(), "--stdio".to_string()],
        },
        workspace_fixture: plan.identity.fixture.clone(),
        capabilities,
        diagnostics,
        journey,
        protocol_evidence: None,
        process_cleanup: observation.cleanup,
        result,
        failure_class,
        limitations,
        artifacts: observation.artifacts.clone(),
        claim_boundary,
    }
}

pub fn default_not_proven_diagnostics() -> DiagnosticsIdentity {
    DiagnosticsIdentity {
        advertised_mode: DiagnosticMode::NotProven,
        observed_messages: Vec::new(),
    }
}

/// Validate that a receipt is fresh for `plan`: a receipt produced by another
/// run (different candidate, fixture, client bytes, or host build) cannot
/// satisfy the current run's obligations. This is the stale-receipt law; it
/// composes with the fresh-output-root refusal that prevents a prior run's
/// artifacts from being inherited at all.
pub fn validate_receipt_binding(
    receipt: &EditorClientCompatReceipt,
    plan: &VimHostRunPlan,
) -> Result<()> {
    receipt.validate()?;
    ensure!(
        receipt.host.product == "vim" && receipt.host.client_id == VIM_LSP_CLIENT_ID,
        "receipt subject is not the vim/vim-lsp host runner subject"
    );
    ensure!(
        receipt.host.version == plan.identity.vim_lsp_commit,
        "receipt binds vim-lsp {} but the run plan pins {}",
        receipt.host.version,
        plan.identity.vim_lsp_commit
    );
    ensure!(
        receipt.host.executable_sha256 == plan.identity.vim_build_sha256,
        "receipt binds a different Vim executable build"
    );
    ensure!(
        receipt.candidate_sha == plan.identity.candidate_sha,
        "receipt binds a different repository candidate commit"
    );
    ensure!(
        receipt.server.artifact_sha256 == plan.identity.candidate_artifact_sha256,
        "receipt binds a different perllsp candidate artifact"
    );
    ensure!(
        receipt.workspace_fixture.digest == plan.identity.fixture.digest,
        "receipt binds a different workspace fixture"
    );
    ensure!(
        receipt.integration.driver_sha256 == plan.identity.driver_sha256,
        "receipt binds a different driver"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Shared helpers (one-line delegates to the #10894 authority)
// ---------------------------------------------------------------------------

pub fn file_sha256(path: &Path) -> Result<String> {
    xtask::editor_host::sha256_file(path)
}

fn verify_file_sha256(path: &Path, expected: &str, label: &str) -> Result<()> {
    xtask::editor_host::verify_sha256_file(path, expected, label)
}

pub fn bytes_sha256(bytes: &[u8]) -> Result<String> {
    xtask::editor_host::sha256_bytes(bytes)
}

fn validate_sha256(value: &str, field: &str) -> Result<()> {
    xtask::editor_host::validate_sha256_field(value, field)
}

pub fn is_lower_hex(value: &str, len: usize) -> bool {
    xtask::editor_host::is_lower_hex(value, len)
}

pub fn is_reason_token(value: &str) -> bool {
    !value.is_empty()
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'_' | b'.' | b'-')
        })
        && value.as_bytes().first().is_some_and(u8::is_ascii_alphanumeric)
}

fn is_perllsp_filename(path: &Path) -> bool {
    path.file_name()
        .and_then(OsStr::to_str)
        .is_some_and(|name| name == "perllsp" || name == "perllsp.exe")
}
