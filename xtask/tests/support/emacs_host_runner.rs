use anyhow::{Context, Result, bail, ensure};
use chrono::Utc;
use regex::Regex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::{OsStr, OsString};
use std::fmt::Write as _;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::OnceLock;
use std::thread;
use std::time::{Duration, Instant};
use xtask::editor_client_compat::{
    ArtifactKind, CANONICAL_EXPECTATION_SET_ID, CapabilityIdentity, ClientSourceState,
    DiagnosticMode, DiagnosticsIdentity, EditorClientCompatReceipt, EvidenceArtifact,
    EvidenceStage, FailureClass, HostIdentity, IntegrationIdentity, IntegrationMode, JourneyCell,
    PlatformIdentity, RegistrationState, SCHEMA_VERSION as RECEIPT_SCHEMA_VERSION, ServerIdentity,
    WorkspaceFixtureIdentity, canonical_expectation_set_digest, fixture_digest,
};

// Supervision consumers name the fail-closed result vocabulary directly, so
// the runner re-exports the canonical definitions instead of wrapping them.
pub use xtask::editor_client_compat::{CleanupResult, ObservationResult};

pub const RUN_PLAN_SCHEMA_VERSION: &str = "emacs_host_run_plan.v1";
pub const DRIVER_SCHEMA_VERSION: &str = "emacs_host_driver.v1";
pub const CAPTURE_BOUNDS_SCHEMA_VERSION: &str = "emacs_host_capture_bounds.v1";
const MAX_CAPTURE_BYTES: usize = 1024 * 1024;

/// Environment switch selecting the fake-host behavior for the deterministic
/// supervision fixture (`run_fake_host_entry`). Without it the child behaves
/// as an ordinary test-harness invocation.
pub const FAKE_HOST_MODE_ENV: &str = "PERL_LSP_FAKE_HOST_MODE";
const FAKE_HOST_DESCENDANT_READY_ENV: &str = "PERL_LSP_FAKE_DESCENDANT_READY";
/// Best-effort safety cap so a mis-supervised descendant can never linger for
/// longer than one CI job even if every explicit stop path failed.
const DESCENDANT_LIFETIME_CAP_MS: u64 = 120_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EmacsClientKind {
    BundledEglot,
    ExternalEglot,
    LspMode,
}

impl EmacsClientKind {
    fn product(self) -> &'static str {
        match self {
            Self::BundledEglot | Self::ExternalEglot => "eglot",
            Self::LspMode => "lsp-mode",
        }
    }

    fn token(self) -> &'static str {
        match self {
            Self::BundledEglot => "bundled_eglot",
            Self::ExternalEglot => "external_eglot",
            Self::LspMode => "lsp_mode",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientSubject {
    pub client_id: String,
    pub kind: EmacsClientKind,
    pub version: String,
    pub source_state: ClientSourceState,
    pub source_ref: String,
    pub source_sha256: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub package_sha256: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmacsHostPaths {
    pub emacs_executable: PathBuf,
    pub client_source: PathBuf,
    pub client_package: Option<PathBuf>,
    pub driver: PathBuf,
    pub adapter: PathBuf,
    pub configuration: PathBuf,
    pub candidate_executable: PathBuf,
    pub fixture_root: PathBuf,
    pub artifact_root: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmacsHostRunIdentity {
    pub schema_version: String,
    pub stage: EvidenceStage,
    pub repository: String,
    pub candidate_sha: String,
    pub emacs_version: String,
    pub emacs_build_sha256: String,
    pub client: ClientSubject,
    pub driver_sha256: String,
    pub adapter_sha256: String,
    pub configuration_sha256: String,
    pub candidate_version: String,
    pub candidate_build_revision: String,
    pub candidate_artifact_sha256: String,
    pub fixture: WorkspaceFixtureIdentity,
    pub journey_selector: String,
    pub platform: PlatformIdentity,
    pub registration_state: RegistrationState,
    pub timeout_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmacsHostRunPlan {
    pub identity: EmacsHostRunIdentity,
    pub paths: EmacsHostPaths,
}

impl EmacsHostRunPlan {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.identity.schema_version == RUN_PLAN_SCHEMA_VERSION,
            "unexpected Emacs host run-plan schema"
        );
        validate_safe_identity(&self.identity.repository, "repository")?;
        ensure!(
            is_lower_hex(&self.identity.candidate_sha, 40),
            "candidate_sha must be 40 lowercase hex chars"
        );
        validate_safe_identity(&self.identity.emacs_version, "emacs_version")?;
        validate_sha256(&self.identity.emacs_build_sha256, "emacs_build_sha256")?;
        validate_client_subject(&self.identity.client)?;
        validate_sha256(&self.identity.driver_sha256, "driver_sha256")?;
        validate_sha256(&self.identity.adapter_sha256, "adapter_sha256")?;
        validate_sha256(&self.identity.configuration_sha256, "configuration_sha256")?;
        validate_safe_identity(&self.identity.candidate_version, "candidate_version")?;
        ensure!(
            is_lower_hex(&self.identity.candidate_build_revision, 40),
            "candidate_build_revision must be 40 lowercase hex chars"
        );
        validate_sha256(&self.identity.candidate_artifact_sha256, "candidate_artifact_sha256")?;
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
            ("emacs executable", &self.paths.emacs_executable),
            ("client source", &self.paths.client_source),
            ("driver", &self.paths.driver),
            ("adapter", &self.paths.adapter),
            ("configuration", &self.paths.configuration),
            ("candidate executable", &self.paths.candidate_executable),
        ] {
            ensure!(path.is_absolute(), "{label} path must be absolute");
            ensure!(path.is_file(), "{label} path is not a file: {}", path.display());
        }
        if let Some(package) = self.paths.client_package.as_deref() {
            ensure!(package.is_absolute(), "client package path must be absolute");
            ensure!(package.is_file(), "client package path is not a file: {}", package.display());
        }
        ensure!(
            self.paths.fixture_root.is_absolute() && self.paths.fixture_root.is_dir(),
            "fixture_root must be an absolute directory"
        );
        ensure!(self.paths.artifact_root.is_absolute(), "artifact_root must be absolute");
        ensure!(
            is_perllsp_filename(&self.paths.candidate_executable),
            "candidate executable file name must be perllsp or perllsp.exe"
        );

        verify_file_sha256(
            &self.paths.emacs_executable,
            &self.identity.emacs_build_sha256,
            "Emacs executable",
        )?;
        verify_file_sha256(
            &self.paths.client_source,
            &self.identity.client.source_sha256,
            "client source",
        )?;
        match (self.paths.client_package.as_deref(), self.identity.client.package_sha256.as_deref())
        {
            (Some(path), Some(expected)) => {
                verify_file_sha256(path, expected, "client package")?;
            }
            (None, None) => {}
            _ => bail!("client package path and package identity must be present together"),
        }
        verify_file_sha256(&self.paths.driver, &self.identity.driver_sha256, "driver")?;
        verify_file_sha256(&self.paths.adapter, &self.identity.adapter_sha256, "adapter")?;
        verify_file_sha256(
            &self.paths.configuration,
            &self.identity.configuration_sha256,
            "configuration",
        )?;
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
}

fn validate_client_subject(subject: &ClientSubject) -> Result<()> {
    ensure!(is_reason_token(&subject.client_id), "client_id must be a stable reason token");
    validate_safe_identity(&subject.version, "client.version")?;
    validate_safe_identity(&subject.source_ref, "client.source_ref")?;
    validate_sha256(&subject.source_sha256, "client.source_sha256")?;
    if let Some(package_sha256) = subject.package_sha256.as_deref() {
        validate_sha256(package_sha256, "client.package_sha256")?;
    }
    match subject.kind {
        EmacsClientKind::BundledEglot => {
            ensure!(
                subject.source_state == ClientSourceState::Bundled,
                "bundled Eglot must use bundled source state"
            );
            ensure!(
                subject.package_sha256.is_none(),
                "bundled Eglot cannot carry a separate package identity"
            );
        }
        EmacsClientKind::ExternalEglot | EmacsClientKind::LspMode => {
            ensure!(
                subject.source_state != ClientSourceState::Bundled,
                "external Eglot/lsp-mode cannot use bundled source state"
            );
            if subject.source_state == ClientSourceState::Released {
                ensure!(
                    subject.package_sha256.is_some(),
                    "released external client requires package identity"
                );
            }
        }
    }
    Ok(())
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HermeticLayout {
    pub root: PathBuf,
    pub home: PathBuf,
    pub user_emacs_directory: PathBuf,
    pub xdg_config_home: PathBuf,
    pub xdg_cache_home: PathBuf,
    pub xdg_data_home: PathBuf,
    pub package_directory: PathBuf,
    pub native_comp_directory: PathBuf,
    pub temp_directory: PathBuf,
    pub raw_directory: PathBuf,
    pub artifact_directory: PathBuf,
}

impl HermeticLayout {
    pub fn prepare(root: &Path) -> Result<Self> {
        ensure!(root.is_absolute(), "hermetic root must be absolute");
        let layout = Self {
            root: root.to_path_buf(),
            home: root.join("home"),
            user_emacs_directory: root.join("home/.emacs.d"),
            xdg_config_home: root.join("xdg/config"),
            xdg_cache_home: root.join("xdg/cache"),
            xdg_data_home: root.join("xdg/data"),
            package_directory: root.join("packages"),
            native_comp_directory: root.join("native-comp"),
            temp_directory: root.join("tmp"),
            raw_directory: root.join("raw"),
            artifact_directory: root.join("artifacts"),
        };
        for directory in [
            &layout.home,
            &layout.user_emacs_directory,
            &layout.xdg_config_home,
            &layout.xdg_cache_home,
            &layout.xdg_data_home,
            &layout.package_directory,
            &layout.native_comp_directory,
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

    pub fn client_log(&self) -> PathBuf {
        self.raw_directory.join("client.log")
    }

    pub fn server_stderr(&self) -> PathBuf {
        self.raw_directory.join("perllsp.stderr")
    }

    pub fn capability_snapshot(&self) -> PathBuf {
        self.raw_directory.join("initialize.json")
    }

    /// Raw before-run process-table snapshot, retained as run evidence even
    /// when the cleanup comparison itself could not be made.
    pub fn process_snapshot_before(&self) -> PathBuf {
        self.raw_directory.join("process-snapshot-before.txt")
    }

    /// Raw after-run process-table snapshot (same retention rule).
    pub fn process_snapshot_after(&self) -> PathBuf {
        self.raw_directory.join("process-snapshot-after.txt")
    }

    pub fn environment(&self, plan: &EmacsHostRunPlan) -> Result<BTreeMap<OsString, OsString>> {
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
        environment.insert(OsString::from("HOME"), self.home.as_os_str().to_owned());
        environment.insert(OsString::from("USERPROFILE"), self.home.as_os_str().to_owned());
        environment
            .insert(OsString::from("XDG_CONFIG_HOME"), self.xdg_config_home.as_os_str().to_owned());
        environment
            .insert(OsString::from("XDG_CACHE_HOME"), self.xdg_cache_home.as_os_str().to_owned());
        environment
            .insert(OsString::from("XDG_DATA_HOME"), self.xdg_data_home.as_os_str().to_owned());
        for key in ["TMPDIR", "TEMP", "TMP"] {
            environment.insert(OsString::from(key), self.temp_directory.as_os_str().to_owned());
        }
        environment.insert(
            OsString::from("PERL_LSP_EMACS_EVENT_FILE"),
            self.event_file().into_os_string(),
        );
        environment.insert(
            OsString::from("PERL_LSP_EMACS_CLIENT_LOG"),
            self.client_log().into_os_string(),
        );
        environment.insert(
            OsString::from("PERL_LSP_EMACS_SERVER_STDERR"),
            self.server_stderr().into_os_string(),
        );
        environment.insert(
            OsString::from("PERL_LSP_EMACS_CAPABILITY_SNAPSHOT"),
            self.capability_snapshot().into_os_string(),
        );
        environment.insert(
            OsString::from("PERL_LSP_EMACS_USER_DIR"),
            self.user_emacs_directory.as_os_str().to_owned(),
        );
        environment.insert(
            OsString::from("PERL_LSP_EMACS_PACKAGE_DIR"),
            self.package_directory.as_os_str().to_owned(),
        );
        environment.insert(
            OsString::from("PERL_LSP_EMACS_NATIVE_COMP_DIR"),
            self.native_comp_directory.as_os_str().to_owned(),
        );
        environment.insert(
            OsString::from("PERL_LSP_EMACS_CLIENT_KIND"),
            OsString::from(plan.identity.client.kind.token()),
        );
        environment.insert(
            OsString::from("PERL_LSP_EMACS_CLIENT_VERSION"),
            OsString::from(&plan.identity.client.version),
        );
        environment.insert(
            OsString::from("PERL_LSP_EMACS_CANDIDATE"),
            plan.paths.candidate_executable.as_os_str().to_owned(),
        );
        environment.insert(
            OsString::from("PERL_LSP_EMACS_CLIENT_SOURCE"),
            plan.paths.client_source.as_os_str().to_owned(),
        );
        if let Some(client_package) = plan.paths.client_package.as_ref() {
            environment.insert(
                OsString::from("PERL_LSP_EMACS_CLIENT_PACKAGE"),
                client_package.as_os_str().to_owned(),
            );
        }
        environment.insert(
            OsString::from("PERL_LSP_EMACS_FIXTURE_ROOT"),
            plan.paths.fixture_root.as_os_str().to_owned(),
        );
        environment.insert(
            OsString::from("PERL_LSP_EMACS_CONFIGURATION"),
            plan.paths.configuration.as_os_str().to_owned(),
        );
        Ok(environment)
    }
}

pub fn build_emacs_command(plan: &EmacsHostRunPlan, layout: &HermeticLayout) -> Result<Command> {
    plan.validate()?;
    let mut command = Command::new(&plan.paths.emacs_executable);
    command.env_clear();
    for (key, value) in layout.environment(plan)? {
        command.env(key, value);
    }
    command
        .arg("-Q")
        .arg("--no-site-file")
        .arg("--batch")
        .arg("--eval")
        .arg(format!("(setq user-emacs-directory {})", lisp_string(&layout.user_emacs_directory)?))
        .arg("--load")
        .arg(&plan.paths.driver)
        .arg("--load")
        .arg(&plan.paths.adapter)
        .arg("--funcall")
        .arg("perl-lsp-test-run");
    Ok(command)
}

fn lisp_string(path: &Path) -> Result<String> {
    let value = path.to_str().with_context(|| format!("path is not UTF-8: {}", path.display()))?;
    let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
    Ok(format!("\"{escaped}\""))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DriverEventKind {
    HostStarted,
    ClientLoaded,
    RegistrationSelected,
    InitializeObserved,
    WorkspaceReady,
    BufferOpened,
    HostActionStarted,
    HostActionCompleted,
    EditApplied,
    DriverFailed,
    ShutdownStarted,
    ShutdownCompleted,
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

pub fn validate_driver_events(events: &[DriverEvent], require_complete: bool) -> Result<()> {
    ensure!(!events.is_empty(), "driver emitted no events");
    let mut singleton = BTreeSet::new();
    let mut open_actions = BTreeSet::new();
    let mut last_lifecycle_rank = 0_u8;

    for (index, event) in events.iter().enumerate() {
        ensure!(event.schema_version == DRIVER_SCHEMA_VERSION, "unexpected driver event schema");
        ensure!(event.sequence == (index + 1) as u64, "driver event sequence is not contiguous");
        for (key, value) in &event.details {
            ensure!(is_reason_token(key), "driver detail key is not a reason token");
            validate_safe_identity(value, "driver detail value")?;
        }

        match event.kind {
            DriverEventKind::HostActionStarted => {
                ensure!(
                    singleton.contains(&DriverEventKind::BufferOpened),
                    "host action started before a buffer opened"
                );
                let action = event
                    .details
                    .get("action_id")
                    .context("host_action_started omitted action_id")?;
                ensure!(
                    open_actions.insert(action.as_str()),
                    "host action started twice without completion"
                );
            }
            DriverEventKind::HostActionCompleted => {
                let action = event
                    .details
                    .get("action_id")
                    .context("host_action_completed omitted action_id")?;
                ensure!(
                    open_actions.remove(action.as_str()),
                    "host action completed without a matching start"
                );
            }
            DriverEventKind::EditApplied => {
                ensure!(
                    singleton.contains(&DriverEventKind::BufferOpened),
                    "edit_applied preceded buffer_opened"
                );
            }
            DriverEventKind::DriverFailed => {
                ensure!(
                    singleton.insert(DriverEventKind::DriverFailed),
                    "duplicate driver_failed event"
                );
                ensure!(event.details.contains_key("reason"), "driver_failed omitted reason");
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
    ensure!(open_actions.is_empty(), "driver left host actions incomplete");

    if require_complete {
        ensure!(
            !singleton.contains(&DriverEventKind::DriverFailed),
            "complete host run reported driver failure"
        );
        for required in [
            DriverEventKind::HostStarted,
            DriverEventKind::ClientLoaded,
            DriverEventKind::RegistrationSelected,
            DriverEventKind::InitializeObserved,
            DriverEventKind::WorkspaceReady,
            DriverEventKind::BufferOpened,
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
        DriverEventKind::InitializeObserved => 4,
        DriverEventKind::WorkspaceReady => 5,
        DriverEventKind::BufferOpened => 6,
        DriverEventKind::ShutdownStarted => 7,
        DriverEventKind::ShutdownCompleted => 8,
        DriverEventKind::HostActionStarted
        | DriverEventKind::HostActionCompleted
        | DriverEventKind::EditApplied
        | DriverEventKind::DriverFailed => 6,
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct ProcessLedger {
    pid: u32,
    timed_out: bool,
    kill_requested: bool,
    exit_code: Option<i32>,
    cleanup: CleanupResult,
    cleanup_detail: String,
    process_probe: String,
    last_completed_barrier: Option<String>,
    surviving_processes: Vec<LedgerSurvivor>,
    event_count: usize,
    driver_complete: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct LedgerSurvivor {
    pid: u32,
    args: String,
}

/// One evidence row of the capture-bounds document: for every retained
/// capture it distinguishes full from truncated evidence instead of letting
/// a silent truncation masquerade as completeness.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct CaptureBoundsRow {
    id: String,
    kind: String,
    full_stream_sha256: String,
    original_byte_count: u64,
    retained_byte_count: u64,
    truncated: bool,
}

#[derive(Debug, Clone, Serialize)]
struct CaptureBoundsDocument {
    schema_version: String,
    captures: Vec<CaptureBoundsRow>,
}

#[derive(Debug, Clone)]
pub struct SurvivorProcess {
    pub pid: u32,
    pub args: String,
}

#[derive(Debug, Clone)]
pub struct ProcessObservation {
    pub status_code: Option<i32>,
    pub timed_out: bool,
    pub kill_requested: bool,
    pub cleanup: CleanupResult,
    pub cleanup_detail: String,
    pub events: Vec<DriverEvent>,
    pub driver_complete: bool,
    pub last_completed_barrier: Option<String>,
    pub surviving_processes: Vec<SurvivorProcess>,
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

/// The lifecycle barrier token for a singleton driver event, or `None` for
/// non-lifecycle events (actions, edits, failures).
fn completed_barrier_token(kind: DriverEventKind) -> Option<&'static str> {
    match kind {
        DriverEventKind::HostStarted => Some("host_started"),
        DriverEventKind::ClientLoaded => Some("client_loaded"),
        DriverEventKind::RegistrationSelected => Some("registration_selected"),
        DriverEventKind::InitializeObserved => Some("initialize_observed"),
        DriverEventKind::WorkspaceReady => Some("workspace_ready"),
        DriverEventKind::BufferOpened => Some("buffer_opened"),
        DriverEventKind::ShutdownStarted => Some("shutdown_started"),
        DriverEventKind::ShutdownCompleted => Some("shutdown_completed"),
        DriverEventKind::HostActionStarted
        | DriverEventKind::HostActionCompleted
        | DriverEventKind::EditApplied
        | DriverEventKind::DriverFailed => None,
    }
}

fn last_completed_barrier(events: &[DriverEvent]) -> Option<String> {
    let mut best: Option<(u8, &'static str)> = None;
    for event in events {
        if let Some(token) = completed_barrier_token(event.kind) {
            let rank = lifecycle_rank(event.kind);
            match best {
                Some((current_rank, _)) if rank < current_rank => {}
                _ => best = Some((rank, token)),
            }
        }
    }
    best.map(|(_, token)| token.to_string())
}

pub fn run_owned_process(
    command: &mut Command,
    plan: &EmacsHostRunPlan,
    layout: &HermeticLayout,
) -> Result<ProcessObservation> {
    // The needle binds this run's exact candidate identity. The plan path is
    // unique per run, so unrelated same-name processes (another checkout's
    // real host run) are never attributed here.
    let needle = if cfg!(windows) {
        plan.paths
            .candidate_executable
            .file_name()
            .and_then(OsStr::to_str)
            .unwrap_or("perllsp")
            .to_string()
    } else {
        plan.paths.candidate_executable.to_string_lossy().into_owned()
    };
    let parse_probe = |text: &str| {
        if cfg!(windows) {
            parse_windows_process_snapshot(text)
        } else {
            parse_process_snapshot(text)
        }
    };
    let probe_before = probe_process_table();
    // A before-probe that cannot be parsed is recorded with its typed cause:
    // treating it as an empty set would fabricate survivors later, and a
    // generic refusal would hide whether the table was unreadable, absent,
    // or malformed. The comparison refuses to judge cleanup either way.
    let before_diagnostic = diagnostic_probe_failure("before", &probe_before);
    let before_lines = match &probe_before {
        Some(Ok(text)) => parse_probe(text).unwrap_or_default(),
        _ => Vec::new(),
    };

    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command.spawn().context("spawning Emacs host subject")?;
    let pid = child.id();
    let mut stdout = child.stdout.take().context("capturing host stdout")?;
    let mut stderr = child.stderr.take().context("capturing host stderr")?;
    let stdout_reader = thread::spawn(move || -> std::io::Result<Vec<u8>> {
        let mut bytes = Vec::new();
        stdout.read_to_end(&mut bytes)?;
        Ok(bytes)
    });
    let stderr_reader = thread::spawn(move || -> std::io::Result<Vec<u8>> {
        let mut bytes = Vec::new();
        stderr.read_to_end(&mut bytes)?;
        Ok(bytes)
    });

    let deadline = Instant::now() + Duration::from_millis(plan.identity.timeout_ms);
    let mut timed_out = false;
    let mut kill_requested = false;
    let status: ExitStatus = loop {
        if let Some(status) = child.try_wait().context("polling Emacs host process")? {
            break status;
        }
        if Instant::now() >= deadline {
            timed_out = true;
            child.kill().context("killing timed-out Emacs host process")?;
            kill_requested = true;
            break child.wait().context("reaping timed-out Emacs host process")?;
        }
        thread::sleep(Duration::from_millis(10));
    };

    let stdout = join_reader(stdout_reader, "host stdout")?;
    let stderr = join_reader(stderr_reader, "host stderr")?;

    // Cleanup law (#8734): pass requires settled termination AND an
    // independently observed absence of this run's owned process tree.
    // A survivor matching the exact candidate needle that was absent before
    // the run is an observed leak (`fail`). An unavailable or unparseable
    // probe leaves the judgment not-proven with a typed detail. A forced
    // kill or abnormal exit skipped the driver's own shutdown path, so even
    // a clean-looking table degrades to not-proven rather than pass.
    let probe_after = probe_process_table();
    let after_diagnostic = diagnostic_probe_failure("after", &probe_after);
    let (mut cleanup, mut cleanup_detail, survivors) = if let Some(before_error) =
        before_diagnostic
    {
        (CleanupResult::NotProven, before_error, Vec::new())
    } else {
        match (&probe_before, &probe_after) {
            (Some(Ok(_)), Some(Ok(after_text))) => match parse_probe(after_text) {
                Ok(after_lines) => {
                    let survivors = surviving_processes(&before_lines, &after_lines, &needle);
                    if survivors.is_empty() {
                        (
                            CleanupResult::Pass,
                            "process-set comparison clean".to_string(),
                            survivors,
                        )
                    } else {
                        (
                            CleanupResult::Fail,
                            format!(
                                "process-set comparison observed {} surviving candidate \
                                 process(es) after the run",
                                survivors.len()
                            ),
                            survivors,
                        )
                    }
                }
                Err(error) => (
                    CleanupResult::NotProven,
                    format!("after-process probe unparseable: {error:#}"),
                    Vec::new(),
                ),
            },
            _ => (
                CleanupResult::NotProven,
                after_diagnostic.unwrap_or_else(|| {
                    "process probe unavailable on this platform; cleanup not observed".to_string()
                }),
                Vec::new(),
            ),
        }
    };
    // Retain both raw snapshots as run evidence even when the comparison
    // itself could not be made. These stay outside the sanitized artifact
    // list: they contain host-wide observations beyond this run's boundary.
    let _ = fs::write(layout.process_snapshot_before(), render_process_snapshot(&before_lines));
    let _ = fs::write(
        layout.process_snapshot_after(),
        match &probe_after {
            Some(Ok(text)) => text.clone(),
            _ => String::new(),
        },
    );
    if (timed_out || kill_requested || status.code() != Some(0)) && cleanup == CleanupResult::Pass
    {
        cleanup = CleanupResult::NotProven;
        cleanup_detail =
            "host exit skipped the driver shutdown path; orderly client shutdown not observed"
                .to_string();
    }

    let event_bytes = fs::read(layout.event_file()).unwrap_or_default();
    let events = parse_driver_events(&event_bytes, false).unwrap_or_default();
    let driver_complete = validate_driver_events(&events, true).is_ok();
    let last_barrier = last_completed_barrier(&events);

    let mut bounds: BTreeMap<String, CaptureBoundsRow> = BTreeMap::new();
    let mut artifacts = Vec::new();
    artifacts.push(write_sanitized_artifact(
        &layout.artifact_directory,
        "emacs/driver-stdout.log",
        ArtifactKind::DriverOutput,
        &stdout,
        plan,
        layout,
        &mut bounds,
    )?);
    artifacts.push(write_sanitized_artifact(
        &layout.artifact_directory,
        "emacs/driver-stderr.log",
        ArtifactKind::DriverOutput,
        &stderr,
        plan,
        layout,
        &mut bounds,
    )?);
    artifacts.push(write_sanitized_artifact(
        &layout.artifact_directory,
        "emacs/driver-events.jsonl",
        ArtifactKind::DriverOutput,
        &event_bytes,
        plan,
        layout,
        &mut bounds,
    )?);

    for (path, id, kind) in [
        (layout.client_log(), "emacs/client.log", ArtifactKind::ClientLog),
        (layout.server_stderr(), "emacs/perllsp.stderr", ArtifactKind::ServerStderr),
        (layout.capability_snapshot(), "emacs/initialize.json", ArtifactKind::CapabilitySnapshot),
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
                &mut bounds,
            )?);
        }
    }

    let ledger = ProcessLedger {
        pid,
        timed_out,
        kill_requested,
        exit_code: status.code(),
        cleanup,
        cleanup_detail: cleanup_detail.clone(),
        process_probe: if matches!((&probe_before, &probe_after), (Some(Ok(_)), Some(Ok(_)))) {
            "available".to_string()
        } else {
            "unavailable".to_string()
        },
        last_completed_barrier: last_barrier.clone(),
        surviving_processes: survivors
            .iter()
            .map(|line| LedgerSurvivor { pid: line.pid, args: line.args.clone() })
            .collect(),
        event_count: events.len(),
        driver_complete,
    };
    let ledger_bytes = serde_json::to_vec_pretty(&ledger)?;
    artifacts.push(write_sanitized_artifact(
        &layout.artifact_directory,
        "emacs/process-ledger.json",
        ArtifactKind::ProcessLedger,
        &ledger_bytes,
        plan,
        layout,
        &mut bounds,
    )?);

    let bounds_document = CaptureBoundsDocument {
        schema_version: CAPTURE_BOUNDS_SCHEMA_VERSION.to_string(),
        captures: bounds.values().cloned().collect(),
    };
    let bounds_bytes = serde_json::to_vec_pretty(&bounds_document)?;
    artifacts.push(write_sanitized_artifact(
        &layout.artifact_directory,
        "emacs/capture-bounds.json",
        ArtifactKind::Other,
        &bounds_bytes,
        plan,
        layout,
        &mut bounds,
    )?);

    Ok(ProcessObservation {
        status_code: status.code(),
        timed_out,
        kill_requested,
        cleanup,
        cleanup_detail,
        events,
        driver_complete,
        last_completed_barrier: last_barrier,
        surviving_processes: survivors
            .iter()
            .map(|line| SurvivorProcess { pid: line.pid, args: line.args.clone() })
            .collect(),
        artifacts,
    })
}

fn join_reader(
    handle: thread::JoinHandle<std::io::Result<Vec<u8>>>,
    label: &str,
) -> Result<Vec<u8>> {
    handle
        .join()
        .map_err(|_| anyhow::anyhow!("{label} reader thread panicked"))?
        .with_context(|| format!("reading {label}"))
}

#[allow(clippy::too_many_arguments)]
fn write_sanitized_artifact(
    artifact_root: &Path,
    id: &str,
    kind: ArtifactKind,
    bytes: &[u8],
    plan: &EmacsHostRunPlan,
    layout: &HermeticLayout,
    bounds: &mut BTreeMap<String, CaptureBoundsRow>,
) -> Result<EvidenceArtifact> {
    validate_safe_identity(id, "artifact id")?;
    let sanitized = sanitize_text(bytes, plan, layout);
    let sanitized_bytes = sanitized.as_bytes();
    // The full-stream identity is taken over the complete sanitized content
    // BEFORE bounding, so a truncated retention can never present its prefix
    // hash as the identity of the full source stream (#8734).
    let full_stream_sha256 = bytes_sha256(sanitized_bytes)?;
    let original_byte_count = sanitized_bytes.len() as u64;
    let bounded = bound_capture(sanitized_bytes);
    let truncated = bounded.len() < sanitized_bytes.len();
    let destination = artifact_root.join(id);
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&destination, bounded)
        .with_context(|| format!("writing sanitized artifact {}", destination.display()))?;
    let sha256 = file_sha256(&destination)?;
    bounds.insert(
        id.to_string(),
        CaptureBoundsRow {
            id: id.to_string(),
            kind: artifact_kind_token(kind).to_string(),
            full_stream_sha256,
            original_byte_count,
            retained_byte_count: bounded.len() as u64,
            truncated,
        },
    );
    Ok(EvidenceArtifact { kind, id: id.to_string(), sha256 })
}

fn artifact_kind_token(kind: ArtifactKind) -> &'static str {
    match kind {
        ArtifactKind::ClientLog => "client_log",
        ArtifactKind::ServerStderr => "server_stderr",
        ArtifactKind::DriverOutput => "driver_output",
        ArtifactKind::CapabilitySnapshot => "capability_snapshot",
        ArtifactKind::ProcessLedger => "process_ledger",
        ArtifactKind::FailureDiagnostics => "failure_diagnostics",
        ArtifactKind::Other => "other",
    }
}

fn sanitize_text(bytes: &[u8], plan: &EmacsHostRunPlan, layout: &HermeticLayout) -> String {
    let mut text = String::from_utf8_lossy(bytes).into_owned();
    // Every absolute path the run plan accepts has to appear here. An Emacs
    // load error or backtrace names the driver, adapter, configuration, and
    // package files directly, and these captures are written as sanitized
    // artifacts that may be uploaded, so a path omitted from this list leaks
    // the checkout or user directory it came from.
    let mut replacements = vec![
        (&layout.root, "<RUN_ROOT>"),
        (&plan.paths.artifact_root, "<ARTIFACT_ROOT>"),
        (&plan.paths.fixture_root, "<WORKSPACE>"),
        (&plan.paths.candidate_executable, "<CANDIDATE>"),
        (&plan.paths.emacs_executable, "<EMACS>"),
        (&plan.paths.client_source, "<CLIENT_SOURCE>"),
        (&plan.paths.driver, "<DRIVER>"),
        (&plan.paths.adapter, "<ADAPTER>"),
        (&plan.paths.configuration, "<CONFIGURATION>"),
    ];
    if let Some(client_package) = plan.paths.client_package.as_ref() {
        replacements.push((client_package, "<CLIENT_PACKAGE>"));
    }
    // Longest first: a run root is a prefix of the paths nested under it, and
    // replacing the prefix first would leave the longer path unrecognizable.
    replacements.sort_by_key(|(path, _)| std::cmp::Reverse(path.as_os_str().len()));
    for (path, token) in replacements {
        if let Some(value) = path.to_str() {
            text = text.replace(value, token);
            text = text.replace(&value.replace('\\', "/"), token);
        }
    }
    redact_resident_private_paths(&mut text);
    text
}

/// Defense-in-depth after the planned-path replacement: residual absolute
/// POSIX paths, drive-qualified Windows paths, and backslash path runs still
/// look private, so they are rejected generically into `<PATH>` tokens rather
/// than persisted verbatim into durable artifacts. Host-driver event detail
/// values remain strictly validated at parse time; this heuristic covers only
/// free-text captures such as logs and snapshots.
fn redact_resident_private_paths(text: &mut String) {
    static PATTERNS: OnceLock<Vec<Regex>> = OnceLock::new();
    let patterns = PATTERNS.get_or_init(|| {
        [
            // POSIX absolute path with at least two segments.
            r#"(?:^|(?<=[\s"'`(=,:;\[]))(/(?:[A-Za-z0-9._@+-]+/)+[A-Za-z0-9._@+-]*)"#,
            // Drive-qualified Windows path (C:\... or C:/...) with at least
            // one segment below the root.
            r#"(?:^|(?<=[\s"'`(=,:;\[]))[A-Za-z]:[/\\][A-Za-z0-9._@+-]+(?:[/\\][A-Za-z0-9._@+-]+)*"#,
            // Backslash path run with at least two segments (\Users\name).
            r#"(?:^|(?<=[\s"'`(=,:;\[]))(?:\\[A-Za-z0-9._@+-]+){2,}"#,
        ]
        .into_iter()
        .filter_map(|pattern| Regex::new(pattern).ok())
        .collect()
    });
    for pattern in patterns {
        *text = pattern.replace_all(text, "<PATH>").into_owned();
    }
}

fn bound_capture(bytes: &[u8]) -> &[u8] {
    if bytes.len() <= MAX_CAPTURE_BYTES { bytes } else { &bytes[..MAX_CAPTURE_BYTES] }
}

#[allow(clippy::too_many_arguments)]
pub fn build_receipt(
    plan: &EmacsHostRunPlan,
    observation: &ProcessObservation,
    capabilities: CapabilityIdentity,
    diagnostics: DiagnosticsIdentity,
    journey: Vec<JourneyCell>,
    result: ObservationResult,
    failure_class: Option<FailureClass>,
    limitations: Vec<String>,
    claim_boundary: String,
) -> EditorClientCompatReceipt {
    let source_ref = format!(
        "{}/{}/{}",
        plan.identity.client.kind.product(),
        plan.identity.client.source_ref,
        plan.identity.client.source_sha256
    );
    EditorClientCompatReceipt {
        schema_version: RECEIPT_SCHEMA_VERSION.to_string(),
        observed_at: Utc::now().to_rfc3339(),
        stage: plan.identity.stage,
        repository: plan.identity.repository.clone(),
        candidate_sha: plan.identity.candidate_sha.clone(),
        platform: plan.identity.platform.clone(),
        host: HostIdentity {
            client_id: plan.identity.client.client_id.clone(),
            product: "emacs".to_string(),
            version: plan.identity.emacs_version.clone(),
            source_state: plan.identity.client.source_state,
            source_ref,
            executable_sha256: plan.identity.emacs_build_sha256.clone(),
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

pub fn file_sha256(path: &Path) -> Result<String> {
    let bytes = fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    bytes_sha256(&bytes)
}

fn verify_file_sha256(path: &Path, expected: &str, label: &str) -> Result<()> {
    let actual = file_sha256(path)?;
    ensure!(actual == expected, "{label} hash mismatch");
    Ok(())
}

fn bytes_sha256(bytes: &[u8]) -> Result<String> {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let mut identity = String::with_capacity("sha256:".len() + 64);
    identity.push_str("sha256:");
    for byte in hasher.finalize() {
        write!(&mut identity, "{byte:02x}")?;
    }
    Ok(identity)
}

fn validate_sha256(value: &str, field: &str) -> Result<()> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        bail!("{field} must use sha256:<64 lowercase hex> identity");
    };
    ensure!(is_lower_hex(hex, 64), "{field} must use sha256:<64 lowercase hex> identity");
    Ok(())
}

fn validate_safe_identity(value: &str, field: &str) -> Result<()> {
    let value = value.trim();
    ensure!(!value.is_empty(), "{field} cannot be empty");
    ensure!(!value.starts_with('/'), "{field} must not expose an absolute path");
    ensure!(!value.starts_with('~'), "{field} must not expose a home-relative path");
    ensure!(!value.contains('\\'), "{field} must use normalized separators");
    ensure!(!value.contains("://"), "{field} must not expose a URI-qualified path");
    ensure!(
        !(value.len() >= 3
            && value.as_bytes()[0].is_ascii_alphabetic()
            && value.as_bytes()[1] == b':'
            && value.as_bytes()[2] == b'/'),
        "{field} must not expose a drive-qualified path"
    );
    ensure!(
        !value.split('/').any(|component| component == ".."),
        "{field} must not contain parent traversal"
    );
    Ok(())
}

fn is_lower_hex(value: &str, len: usize) -> bool {
    value.len() == len && value.bytes().all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

fn is_reason_token(value: &str) -> bool {
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

// ---------------------------------------------------------------------------
// Deterministic process-table probes (#8734)
//
// Same fail-closed family as the Vim sibling substrate; #10894 may extract
// them unchanged later. An unusable probe is evidence failure (`None` /
// parse error), never an empty set, so cleanup can degrade to not_proven but
// never fabricate a pass.
// ---------------------------------------------------------------------------

/// One observed process line from the platform probe.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ProcessProbeLine {
    pub pid: u32,
    pub args: String,
}

/// Parse a `ps -eo pid=,args=` style snapshot into deterministic lines.
pub fn parse_process_snapshot(text: &str) -> Result<Vec<ProcessProbeLine>> {
    let mut lines = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim_start();
        if trimmed.is_empty() {
            continue;
        }
        let mut split = trimmed.splitn(2, char::is_whitespace);
        let pid = split.next().unwrap_or_default();
        let args = split.next().unwrap_or_default().trim();
        let pid: u32 = pid
            .parse()
            .with_context(|| format!("process snapshot line is not `pid args`: {trimmed:?}"))?;
        lines.push(ProcessProbeLine { pid, args: args.to_string() });
    }
    lines.sort();
    Ok(lines)
}

/// Probe the current process table through the platform command. `None` means
/// the platform probe is unavailable — a typed limitation, never a pass.
pub fn probe_process_table() -> Option<Result<String>> {
    let output = if cfg!(windows) {
        Command::new("tasklist").arg("/FO").arg("CSV").arg("/NH").stdin(Stdio::null()).output()
    } else {
        Command::new("ps").args(["-eo", "pid=,args="]).stdin(Stdio::null()).output()
    };
    match output {
        Ok(output) if output.status.success() => {
            Some(Ok(String::from_utf8_lossy(&output.stdout).into_owned()))
        }
        Ok(output) => {
            // Bounded typed diagnostics: a failed probe must name its cause,
            // not collapse into a generic unparseable bucket.
            let stderr_head =
                String::from_utf8_lossy(&output.stderr[..usize::min(200, output.stderr.len())])
                    .into_owned();
            let stdout_head =
                String::from_utf8_lossy(&output.stdout[..usize::min(120, output.stdout.len())])
                    .into_owned();
            Some(Err(anyhow::anyhow!(
                "process probe failed with status {}; stderr head: {stderr_head:?}; \
                 stdout head: {stdout_head:?}",
                output.status
            )))
        }
        Err(error) => Some(Err(anyhow::Error::new(error))),
    }
}

/// Parse a Windows `tasklist /FO CSV /NH` snapshot into the same
/// `pid args` lines.
pub fn parse_windows_process_snapshot(text: &str) -> Result<Vec<ProcessProbeLine>> {
    let mut lines = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let fields: Vec<&str> = trimmed.split("\",\"").collect();
        if fields.len() < 2 {
            bail!("windows process snapshot row is not CSV: {trimmed:?}");
        }
        let image = fields[0].trim_start_matches('"');
        let pid: u32 = fields[1]
            .trim_end_matches('"')
            .parse()
            .with_context(|| format!("windows process snapshot pid is not numeric: {trimmed:?}"))?;
        lines.push(ProcessProbeLine { pid, args: image.to_string() });
    }
    lines.sort();
    Ok(lines)
}

/// The deterministic comparison: which `after` probe lines matching `needle`
/// were not present in the `before` probe. A survivor means the run leaked a
/// process it was responsible for.
pub fn surviving_processes(
    before: &[ProcessProbeLine],
    after: &[ProcessProbeLine],
    needle: &str,
) -> Vec<ProcessProbeLine> {
    let before_matching: BTreeSet<&ProcessProbeLine> =
        before.iter().filter(|line| line.args.contains(needle)).collect();
    after
        .iter()
        .filter(|line| line.args.contains(needle) && !before_matching.contains(line))
        .cloned()
        .collect()
}

/// Typed cause for an unusable probe snapshot: unavailable, failed, or
/// unparseable — never a silent empty set.
fn diagnostic_probe_failure(
    phase: &str,
    probe: &Option<Result<String>>,
) -> Option<String> {
    match probe {
        None => Some(format!(
            "{phase}-process probe unavailable on this platform; cleanup not observed"
        )),
        Some(Err(error)) => {
            Some(format!("{phase}-process probe failed: {error:#}"))
        }
        Some(Ok(text)) => {
            let parsed = if cfg!(windows) {
                parse_windows_process_snapshot(text)
            } else {
                parse_process_snapshot(text)
            };
            parsed.err().map(|error| format!("{phase}-process probe unparseable: {error:#}"))
        }
    }
}

fn render_process_snapshot(lines: &[ProcessProbeLine]) -> String {
    let mut text = String::new();
    for line in lines {
        let _ = writeln!(text, "{} {}", line.pid, line.args);
    }
    text
}

/// Best-effort hygiene for a knowingly leaked supervision descendant. The
/// tested behavior is the leak detection itself; removing the descendant is
/// test-environment care and never part of the judged evidence.
pub fn stop_test_descendant(pid: u32) {
    if cfg!(windows) {
        let _ = Command::new("taskkill")
            .args(["/F", "/PID", &pid.to_string()])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    } else {
        let _ = Command::new("kill")
            .args(["-9", &pid.to_string()])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}

// ---------------------------------------------------------------------------
// Supervision plan + fake-host fixture (#8734)
//
// The fixture re-enters this same test binary through the harness entry the
// contract registers, so every scenario exercises the real
// `run_owned_process` / artifact / receipt seam with a deterministic
// process-emitting subject instead of a second test-only supervisor.
// Candidate identities are unique per scenario so concurrent scenarios in one
// binary cannot attribute each other's processes; real plan validation
// continues to pin `perllsp[.exe]` names where it applies.
// ---------------------------------------------------------------------------

const FAKE_HOST_ENTRY_ENV: &str = "PERL_LSP_FAKE_ENTRY_TEST";

fn synthetic_sha256(seed: u8) -> String {
    format!("sha256:{}", [seed; 64].iter().map(|byte| format!("{byte:x}")).collect::<String>())
}

fn standalone_forty_hex(seed: u8) -> String {
    [seed; 40].iter().map(|byte| format!("{byte:x}")).collect()
}

pub fn supervision_plan(
    root: &Path,
    tag: &str,
    timeout_ms: u64,
) -> Result<(EmacsHostRunPlan, HermeticLayout)> {
    ensure!(root.is_absolute(), "supervision root must be absolute");
    let layout = HermeticLayout::prepare(root)?;
    let bin = root.join("bin");
    fs::create_dir_all(&bin).context("creating supervision bin directory")?;
    let fixture_root = root.join("fixture");
    fs::create_dir_all(&fixture_root).context("creating supervision fixture directory")?;

    let executable_suffix = if cfg!(windows) { ".exe" } else { "" };
    let candidate_executable = bin.join(format!("perllsp-{tag}{executable_suffix}"));
    let emacs_executable = bin.join("host-fixture");
    let client_source = bin.join("client-source.el");
    let driver = bin.join("host-driver.el");
    let adapter = bin.join("client-adapter.el");
    let configuration = bin.join("client-config.el");
    for (path, label) in [
        (&candidate_executable, &b"supervised candidate"[..]),
        (&emacs_executable, &b"supervised host"[..]),
        (&client_source, &b";; supervised client"[..]),
        (&driver, &b";; supervised driver"[..]),
        (&adapter, &b";; supervised adapter"[..]),
        (&configuration, &b";; supervised configuration"[..]),
    ] {
        fs::write(path, label)
            .with_context(|| format!("writing supervision input {}", path.display()))?;
    }

    let identity = EmacsHostRunIdentity {
        schema_version: RUN_PLAN_SCHEMA_VERSION.to_string(),
        stage: EvidenceStage::ExactSourceLocal,
        repository: "repo/supervision".to_string(),
        candidate_sha: standalone_forty_hex(1),
        emacs_version: "GNU Emacs 30.1 (supervision fixture)".to_string(),
        emacs_build_sha256: synthetic_sha256(2),
        client: ClientSubject {
            client_id: format!("fake_eglot_{tag}"),
            kind: EmacsClientKind::BundledEglot,
            version: "1.17.30".to_string(),
            source_state: ClientSourceState::Bundled,
            source_ref: "fixture".to_string(),
            source_sha256: synthetic_sha256(3),
            package_sha256: None,
        },
        driver_sha256: synthetic_sha256(4),
        adapter_sha256: synthetic_sha256(5),
        configuration_sha256: synthetic_sha256(6),
        candidate_version: "perllsp supervision".to_string(),
        candidate_build_revision: standalone_forty_hex(7),
        candidate_artifact_sha256: synthetic_sha256(8),
        fixture: WorkspaceFixtureIdentity {
            id: format!("fixture_{tag}"),
            digest: synthetic_sha256(9),
            expectation_set_id: CANONICAL_EXPECTATION_SET_ID.to_string(),
            expectation_set_digest: synthetic_sha256(10),
        },
        journey_selector: "supervision_lifecycle.v1".to_string(),
        platform: PlatformIdentity {
            os: "supervision".to_string(),
            os_version: "fixture".to_string(),
            arch: "fixture".to_string(),
        },
        registration_state: RegistrationState::ManualClientRegistration,
        timeout_ms,
    };
    let plan = EmacsHostRunPlan {
        identity,
        paths: EmacsHostPaths {
            emacs_executable,
            client_source,
            client_package: None,
            driver,
            adapter,
            configuration,
            candidate_executable,
            fixture_root,
            artifact_root: layout.artifact_directory.clone(),
        },
    };
    Ok((plan, layout))
}

/// Build the fake-host command with the same environment surface the real
/// command builder applies, so the fixture consumes one env contract.
pub fn supervision_command(
    host_executable: &Path,
    entry_test: &str,
    plan: &EmacsHostRunPlan,
    layout: &HermeticLayout,
    mode: &str,
) -> Result<Command> {
    let mut command = Command::new(host_executable);
    // --nocapture: the fixture's own stdout/stderr are the supervised
    // evidence streams, so the child harness must not swallow them.
    command.arg("--exact").arg(entry_test).arg("--nocapture");
    for (key, value) in layout.environment(plan)? {
        command.env(key, value);
    }
    command.env(FAKE_HOST_MODE_ENV, mode);
    command.env(FAKE_HOST_ENTRY_ENV, entry_test);
    Ok(command)
}

// -- fake-host child implementation -----------------------------------------

fn child_required_env(name: &str) -> Result<PathBuf> {
    std::env::var_os(name)
        .map(PathBuf::from)
        .with_context(|| format!("supervision child missing {name}"))
}

fn child_emit(
    event_file: &Path,
    sequence: &mut u64,
    event: &str,
    details: &[(&str, &str)],
) -> Result<()> {
    *sequence += 1;
    let mut detail_map = BTreeMap::new();
    for (key, value) in details {
        detail_map.insert((*key).to_string(), (*value).to_string());
    }
    let payload = serde_json::json!({
        "schema_version": DRIVER_SCHEMA_VERSION,
        "sequence": sequence,
        "event": event,
        "details": detail_map,
    });
    use std::io::Write as _;
    let mut file = fs::OpenOptions::new().create(true).append(true).open(event_file)?;
    writeln!(file, "{payload}")?;
    Ok(())
}

fn child_emit_lifecycle(
    event_file: &Path,
    sequence: &mut u64,
    stop_after_barrier: Option<&str>,
) -> Result<()> {
    let ladder: [(&str, Vec<(&str, &str)>); 11] = [
        ("host_started", vec![("subject", "emacs"), ("client_kind", "bundled_eglot")]),
        ("client_loaded", vec![]),
        ("registration_selected", vec![]),
        ("initialize_observed", vec![]),
        ("workspace_ready", vec![]),
        ("buffer_opened", vec![]),
        ("host_action_started", vec![("action_id", "rename_module")]),
        ("host_action_completed", vec![("action_id", "rename_module")]),
        ("edit_applied", vec![]),
        ("shutdown_started", vec![]),
        ("shutdown_completed", vec![]),
    ];
    for (name, details) in ladder {
        child_emit(event_file, sequence, name, &details)?;
        if stop_after_barrier == Some(name) {
            return Ok(());
        }
    }
    Ok(())
}

/// Entry point the contract test invokes when re-entered by the fake host.
/// It never returns normally: it exits with the fixture's status code so the
/// supervised process boundary observes honest exit semantics.
pub fn run_fake_host_entry(mode: &str) -> ! {
    eprintln!("fixture-enter mode={mode} pid={}", std::process::id());
    match run_fake_host_mode(mode) {
        Ok(code) => {
            eprintln!("fixture-exit mode={mode} code={code}");
            std::process::exit(code)
        }
        Err(error) => {
            eprintln!("supervision fixture failed: {error:#}");
            std::process::exit(9);
        }
    }
}

fn run_fake_host_mode(mode: &str) -> Result<i32> {
    let event_file = child_required_env("PERL_LSP_EMACS_EVENT_FILE")?;
    eprintln!(
        "fixture mode={mode} event_file={} cand={}",
        event_file.display(),
        std::env::var("PERL_LSP_EMACS_CANDIDATE").unwrap_or_default(),
    );
    let mut sequence = 0_u64;
    match mode {
        "clean" => {
            for (name_env, content) in [
                ("PERL_LSP_EMACS_CLIENT_LOG", "client log supervision capture distinct"),
                ("PERL_LSP_EMACS_SERVER_STDERR", "server stderr supervision capture distinct"),
                ("PERL_LSP_EMACS_CAPABILITY_SNAPSHOT", "{\"capabilities\":{}}"),
            ] {
                fs::write(child_required_env(name_env)?, content)
                    .with_context(|| format!("writing distinct capture for {name_env}"))?;
            }
            println!("clean supervision stdout");
            child_emit_lifecycle(&event_file, &mut sequence, None)?;
            Ok(0)
        }
        "chatty_paths" => {
            let home = std::env::var_os("HOME")
                .map(|value| value.to_string_lossy().into_owned())
                .unwrap_or_default();
            println!("{home}");
            println!("/home/observer/.netrc");
            println!("C:\\Users\\observer\\secret-token.txt");
            println!("\\Users\\observer\\secret-token.txt");
            child_emit_lifecycle(&event_file, &mut sequence, None)?;
            Ok(0)
        }
        "oversize_output" => {
            use std::io::Write as _;
            let stdout = std::io::stdout();
            let mut lock = stdout.lock();
            let filler = [b'x'; 4096];
            let total = MAX_CAPTURE_BYTES + 4096;
            let mut written = 0_usize;
            while written < total {
                let chunk = usize::min(filler.len(), total - written);
                lock.write_all(&filler[..chunk])
                    .map(|_| written += chunk)
                    .map_err(anyhow::Error::from)?;
            }
            writeln!(lock, "/home/observer/.netrc leaked past bound")?;
            lock.flush()?;
            child_emit_lifecycle(&event_file, &mut sequence, None)?;
            Ok(0)
        }
        "garbage_events" => {
            use std::io::Write as _;
            let mut file =
                fs::OpenOptions::new().create(true).append(true).open(&event_file)?;
            write!(file, "{{\"schema_version\":\"{DRIVER_SCHEMA_VERSION}\"")?;
            drop(file);
            child_emit(&event_file, &mut sequence, "host_started", &[("subject", "emacs")])?;
            Ok(0)
        }
        "hang_after_workspace_ready" => {
            child_emit_lifecycle(&event_file, &mut sequence, Some("workspace_ready"))?;
            // The parent-owned deadline kills this fixture; the cap only
            // bounds residue in a mis-supervised environment.
            for _ in 0..6000 {
                thread::sleep(Duration::from_millis(100));
            }
            Ok(7)
        }
        "driver_failed_exit3" => {
            child_emit_lifecycle(&event_file, &mut sequence, Some("buffer_opened"))?;
            child_emit(
                &event_file,
                &mut sequence,
                "driver_failed",
                &[("reason", "candidate_refused")],
            )?;
            Ok(3)
        }
        "leak_descendant_clean_exit" => {
            let candidate = child_required_env("PERL_LSP_EMACS_CANDIDATE")?;
            let self_exe =
                std::env::current_exe().context("locating supervision fixture exe")?;
            eprintln!("fixture: staging descendant image");
            fs::copy(&self_exe, &candidate).with_context(|| {
                format!("staging descendant image at {}", candidate.display())
            })?;
            let ready_marker = event_file
                .parent()
                .context("event file must have a parent directory")?
                .join(format!("descendant-ready-{}", std::process::id()));
            let entry_test = std::env::var(FAKE_HOST_ENTRY_ENV)
                .context("supervision child missing entry test name")?;
            eprintln!("fixture: spawning descendant");
            // The descendant is deliberately detached (null stdio, never
            // waited): the whole point of this scenario is a host that exits
            // zero immediately while leaking a live candidate process for
            // the deterministic cleanup comparison to observe.
            let descendant = Command::new(&candidate)
                .args(["--exact", &entry_test])
                .env(FAKE_HOST_MODE_ENV, "descendant_sleep")
                .env(FAKE_HOST_DESCENDANT_READY_ENV, &ready_marker)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .context("spawning leak-scenario descendant")?;
            let descendant_pid = descendant.id();
            drop(descendant);
            let mut became_ready = ready_marker.is_file();
            for _ in 0..300 {
                if became_ready {
                    break;
                }
                thread::sleep(Duration::from_millis(50));
                became_ready = ready_marker.is_file();
            }
            eprintln!("fixture: descendant {descendant_pid} ready={became_ready}");
            if !became_ready {
                bail!("leak-scenario descendant {descendant_pid} never signaled readiness");
            }
            child_emit_lifecycle(&event_file, &mut sequence, None)?;
            Ok(0)
        }
        "descendant_sleep" => {
            let ready = child_required_env(FAKE_HOST_DESCENDANT_READY_ENV)?;
            fs::write(
                &ready,
                format!("ready pid={}", std::process::id()).as_bytes(),
            )
            .with_context(|| format!("writing {}", ready.display()))?;
            for _ in 0..(DESCENDANT_LIFETIME_CAP_MS / 50) {
                thread::sleep(Duration::from_millis(50));
                // Heartbeat: refresh the marker so a diagnostic observer can
                // distinguish a living sleeper from one that died early.
                let _ = fs::write(&ready, format!("alive pid={}", std::process::id()));
            }
            Ok(0)
        }
        other => bail!("unknown supervision fixture mode: {other}"),
    }
}

// ---------------------------------------------------------------------------
// Supervised receipt construction (#8734)
//
// Same production-receipt dialect as the plan-driven builder below, fed from
// a supervised observation instead of a validated exact-input run plan. It
// never manufactures observed cells: capabilities and diagnostics stay
// not_proven and the journey stays empty unless a real adapter supplies them,
// so only the production validator can decide what this evidence may claim.
// ---------------------------------------------------------------------------

pub struct SupervisionReceiptInputs {
    pub stage: EvidenceStage,
    pub repository: String,
    pub candidate_sha: String,
    pub platform: PlatformIdentity,
    pub client_id: String,
    pub emacs_version: String,
    pub source_state: ClientSourceState,
    pub source_ref: String,
    pub emacs_build_sha256: String,
    pub configuration_sha256: String,
    pub driver_sha256: String,
    pub candidate_version: String,
    pub candidate_build_revision: String,
    pub candidate_artifact_sha256: String,
    pub fixture: WorkspaceFixtureIdentity,
    pub journey_selector: String,
    pub result: ObservationResult,
    pub failure_class: Option<FailureClass>,
    pub limitations: Vec<String>,
    pub claim_boundary: String,
}

pub fn build_receipt_supervised(
    observation: &ProcessObservation,
    inputs: SupervisionReceiptInputs,
) -> EditorClientCompatReceipt {
    EditorClientCompatReceipt {
        schema_version: RECEIPT_SCHEMA_VERSION.to_string(),
        observed_at: Utc::now().to_rfc3339(),
        stage: inputs.stage,
        repository: inputs.repository,
        candidate_sha: inputs.candidate_sha,
        platform: inputs.platform,
        host: HostIdentity {
            client_id: inputs.client_id,
            product: "emacs".to_string(),
            version: inputs.emacs_version,
            source_state: inputs.source_state,
            // The fixture has no upstream client tree to digest; the
            // supervision prefix keeps that provenance visible instead of
            // borrowing an identity the run did not verify.
            source_ref: format!("supervision/{}", inputs.source_ref),
            executable_sha256: inputs.emacs_build_sha256,
        },
        integration: IntegrationIdentity {
            mode: IntegrationMode::GenericLsp,
            registration_state: RegistrationState::ManualClientRegistration,
            configuration_sha256: inputs.configuration_sha256,
            driver_sha256: inputs.driver_sha256,
        },
        server: ServerIdentity {
            executable: "perllsp".to_string(),
            version: inputs.candidate_version,
            build_revision: inputs.candidate_build_revision,
            artifact_sha256: inputs.candidate_artifact_sha256,
            protocol_version: "3.17".to_string(),
            launch_command: vec!["perllsp".to_string(), "--stdio".to_string()],
        },
        workspace_fixture: inputs.fixture,
        capabilities: CapabilityIdentity {
            initialize_snapshot_sha256: synthetic_sha256(0),
            position_encodings_offered: Vec::new(),
            position_encoding_basis: xtask::editor_client_compat::PositionEncodingBasis::NotProven,
            position_encoding_selected: None,
        },
        diagnostics: default_not_proven_diagnostics(),
        // Receipt law requires at least one journey cell. The fixture
        // authors no catalog content: it derives a single fail-closed cell
        // from the run's own selector, observes no capability, and reports
        // not_proven so nothing semantic can be credited by construction.
        journey: vec![JourneyCell {
            id: inputs.journey_selector.clone(),
            capability_basis: xtask::editor_client_compat::CapabilityBasis::NotApplicable,
            observed: observation.passed_process_boundary(),
            result: ObservationResult::NotProven,
            evidence: Vec::new(),
            limitation: Some(
                "supervision fixture: process-boundary observation only; host lifecycle \
                 and client behavior stay unobserved"
                    .to_string(),
            ),
        }],
        protocol_evidence: None,
        process_cleanup: observation.cleanup,
        result: inputs.result,
        failure_class: inputs.failure_class,
        limitations: inputs.limitations,
        artifacts: observation.artifacts.clone(),
        // The journey selector rides on the claim boundary here because the
        // fixture emits no journey evidence of its own; real adapters attach
        // journey cells through their own builders.
        claim_boundary: format!("{} | {}", inputs.journey_selector, inputs.claim_boundary),
    }
}

