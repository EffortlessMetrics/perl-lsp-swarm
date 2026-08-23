use anyhow::{Context, Result, bail, ensure};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::{OsStr, OsString};
use std::fmt::Write as _;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};
use xtask::editor_client_compat::{
    ArtifactKind, CANONICAL_EXPECTATION_SET_ID, CapabilityIdentity, CleanupResult,
    ClientSourceState, DiagnosticMode, DiagnosticsIdentity, EditorClientCompatReceipt,
    EvidenceArtifact, EvidenceStage, FailureClass, HostIdentity, IntegrationIdentity,
    IntegrationMode, JourneyCell, ObservationResult, PlatformIdentity, RegistrationState,
    SCHEMA_VERSION as RECEIPT_SCHEMA_VERSION, ServerIdentity, WorkspaceFixtureIdentity,
    canonical_expectation_set_digest, fixture_digest,
};

pub const RUN_PLAN_SCHEMA_VERSION: &str = "emacs_host_run_plan.v1";
pub const DRIVER_SCHEMA_VERSION: &str = "emacs_host_driver.v1";
const MAX_CAPTURE_BYTES: usize = 1024 * 1024;

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
    event_count: usize,
    driver_complete: bool,
}

#[derive(Debug, Clone)]
pub struct ProcessObservation {
    pub status_code: Option<i32>,
    pub timed_out: bool,
    pub kill_requested: bool,
    pub cleanup: CleanupResult,
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

pub fn run_owned_process(
    command: &mut Command,
    plan: &EmacsHostRunPlan,
    layout: &HermeticLayout,
) -> Result<ProcessObservation> {
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
    // `Child::kill` signals only the Emacs process. The adapter starts
    // `perllsp` as a descendant, so after a forced kill — or any exit that
    // skipped the driver's own shutdown path — nothing here has observed that
    // the server went away, and the server can outlive the run. Claiming
    // `pass` in the receipt would assert a cleanup that was never witnessed,
    // so only a host that terminated through its own shutdown path with a
    // success status may report a proven cleanup; everything else is
    // `not_proven` rather than `fail`, because a leak is unobserved here, not
    // demonstrated.
    let cleanup = if timed_out || kill_requested || status.code() != Some(0) {
        CleanupResult::NotProven
    } else {
        CleanupResult::Pass
    };
    let event_bytes = fs::read(layout.event_file()).unwrap_or_default();
    let events = parse_driver_events(&event_bytes, false).unwrap_or_default();
    let driver_complete = validate_driver_events(&events, true).is_ok();

    let mut artifacts = Vec::new();
    artifacts.push(write_sanitized_artifact(
        &layout.artifact_directory,
        "emacs/driver-stdout.log",
        ArtifactKind::DriverOutput,
        &stdout,
        plan,
        layout,
    )?);
    artifacts.push(write_sanitized_artifact(
        &layout.artifact_directory,
        "emacs/driver-stderr.log",
        ArtifactKind::DriverOutput,
        &stderr,
        plan,
        layout,
    )?);
    artifacts.push(write_sanitized_artifact(
        &layout.artifact_directory,
        "emacs/driver-events.jsonl",
        ArtifactKind::DriverOutput,
        &event_bytes,
        plan,
        layout,
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
            )?);
        }
    }

    let ledger = ProcessLedger {
        pid,
        timed_out,
        kill_requested,
        exit_code: status.code(),
        cleanup,
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
    )?);

    Ok(ProcessObservation {
        status_code: status.code(),
        timed_out,
        kill_requested,
        cleanup,
        events,
        driver_complete,
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

fn write_sanitized_artifact(
    artifact_root: &Path,
    id: &str,
    kind: ArtifactKind,
    bytes: &[u8],
    plan: &EmacsHostRunPlan,
    layout: &HermeticLayout,
) -> Result<EvidenceArtifact> {
    validate_safe_identity(id, "artifact id")?;
    let sanitized = sanitize_text(bytes, plan, layout);
    let bounded = bound_capture(sanitized.as_bytes());
    let destination = artifact_root.join(id);
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&destination, bounded)
        .with_context(|| format!("writing sanitized artifact {}", destination.display()))?;
    Ok(EvidenceArtifact { kind, id: id.to_string(), sha256: file_sha256(&destination)? })
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
    text
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
