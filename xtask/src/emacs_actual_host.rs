use anyhow::{Context, Result, bail, ensure};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::env;
use std::ffi::OsString;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::TempDir;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EmacsClientSubject {
    BundledEglotEmacs294,
    BundledEglotEmacs301,
    StandaloneEglot123,
    StandaloneEglot124Source,
    LspMode1000,
    LspMode1001Source,
}

impl EmacsClientSubject {
    pub const ALL: [Self; 6] = [
        Self::BundledEglotEmacs294,
        Self::BundledEglotEmacs301,
        Self::StandaloneEglot123,
        Self::StandaloneEglot124Source,
        Self::LspMode1000,
        Self::LspMode1001Source,
    ];

    pub const fn client_kind(self) -> &'static str {
        match self {
            Self::BundledEglotEmacs294 | Self::BundledEglotEmacs301 => "bundled_eglot",
            Self::StandaloneEglot123 | Self::StandaloneEglot124Source => "external_eglot",
            Self::LspMode1000 | Self::LspMode1001Source => "lsp_mode",
        }
    }

    pub const fn client_version(self) -> &'static str {
        match self {
            Self::BundledEglotEmacs294 => "1.12.29",
            Self::BundledEglotEmacs301 => "1.17.30",
            Self::StandaloneEglot123 => "1.23",
            Self::StandaloneEglot124Source => "1.24",
            Self::LspMode1000 => "10.0.0",
            Self::LspMode1001Source => "10.0.1-dev",
        }
    }

    pub const fn source_state(self) -> &'static str {
        match self {
            Self::BundledEglotEmacs294 | Self::BundledEglotEmacs301 => "bundled",
            Self::StandaloneEglot123 | Self::LspMode1000 => "released",
            Self::StandaloneEglot124Source | Self::LspMode1001Source => "upstream_source",
        }
    }

    pub const fn required_emacs_ref(self) -> Option<&'static str> {
        match self {
            Self::BundledEglotEmacs294 => Some("emacs-29.4"),
            Self::BundledEglotEmacs301 => Some("emacs-30.1"),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClientIdentity {
    pub subject: EmacsClientSubject,
    pub source_ref: String,
    pub loaded_file: PathBuf,
    pub loaded_file_sha256: String,
    pub package_sha256: Option<String>,
}

impl ClientIdentity {
    pub fn validate(&self) -> Result<()> {
        ensure!(!self.source_ref.trim().is_empty(), "client source ref is empty");
        ensure!(self.loaded_file.is_absolute(), "client loaded file must be absolute");
        ensure!(
            !self.loaded_file_sha256.trim().is_empty(),
            "client loaded-file digest is empty"
        );
        match self.subject.source_state() {
            "released" => ensure!(
                self.package_sha256.as_deref().is_some_and(|value| !value.is_empty()),
                "released client subject requires an exact package digest"
            ),
            "upstream_source" => ensure!(
                self.source_ref != "HEAD" && self.source_ref != "main" && self.source_ref != "master",
                "upstream-source client subject requires an immutable commit/ref"
            ),
            "bundled" => {
                let expected = self
                    .subject
                    .required_emacs_ref()
                    .context("bundled subject missing Emacs ref contract")?;
                ensure!(
                    self.source_ref == expected,
                    "bundled client source ref `{}` does not match `{expected}`",
                    self.source_ref
                );
            }
            other => bail!("unsupported client source state `{other}`"),
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CandidateIdentity {
    pub path: PathBuf,
    pub version: String,
    pub sha256: String,
}

impl CandidateIdentity {
    pub fn verify_file(&self) -> Result<()> {
        ensure!(self.path.is_absolute(), "candidate path must be absolute");
        ensure!(self.path.is_file(), "candidate path is not a file: {}", self.path.display());
        ensure!(!self.version.trim().is_empty(), "candidate version is empty");
        ensure!(!self.sha256.trim().is_empty(), "candidate digest is empty");
        let actual = sha256_file(&self.path)?;
        ensure!(
            actual == self.sha256,
            "candidate digest mismatch: expected {}, found {actual}",
            self.sha256
        );
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EmacsIdentity {
    pub executable: PathBuf,
    pub version: String,
    pub build_ref: String,
    pub sha256: String,
}

impl EmacsIdentity {
    pub fn validate(&self) -> Result<()> {
        ensure!(self.executable.is_absolute(), "Emacs executable must be absolute");
        ensure!(!self.version.trim().is_empty(), "Emacs version is empty");
        ensure!(!self.build_ref.trim().is_empty(), "Emacs build/ref is empty");
        ensure!(!self.sha256.trim().is_empty(), "Emacs digest is empty");
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActualHostRunPlan {
    pub emacs: EmacsIdentity,
    pub client: ClientIdentity,
    pub candidate: CandidateIdentity,
    pub driver: PathBuf,
    pub driver_sha256: String,
    pub fixture_identity: String,
    pub journey: String,
    pub platform: String,
    pub architecture: String,
    pub timeout_seconds: u64,
}

impl ActualHostRunPlan {
    pub fn validate(&self) -> Result<()> {
        self.emacs.validate()?;
        self.client.validate()?;
        self.candidate.verify_file()?;
        ensure!(self.driver.is_absolute(), "checked Lisp driver must be absolute");
        ensure!(self.driver.is_file(), "checked Lisp driver does not exist");
        ensure!(!self.driver_sha256.trim().is_empty(), "driver digest is empty");
        ensure!(
            sha256_file(&self.driver)? == self.driver_sha256,
            "checked Lisp driver digest mismatch"
        );
        ensure!(!self.fixture_identity.trim().is_empty(), "fixture identity is empty");
        ensure!(!self.journey.trim().is_empty(), "journey selector is empty");
        ensure!(!self.platform.trim().is_empty(), "platform is empty");
        ensure!(!self.architecture.trim().is_empty(), "architecture is empty");
        ensure!(self.timeout_seconds > 0, "host timeout must be positive");
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum HostBarrier {
    HostStarted,
    ClientLoaded,
    RegistrationSelected,
    InitializeObserved,
    WorkspaceReady,
    BufferOpened,
    HostActionStarted,
    HostActionCompleted,
    EditApplied,
    ShutdownStarted,
    ShutdownCompleted,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct BarrierLedger {
    completed: Vec<HostBarrier>,
}

impl BarrierLedger {
    pub fn record(&mut self, barrier: HostBarrier) -> Result<()> {
        if let Some(last) = self.completed.last().copied() {
            ensure!(barrier > last, "host barrier regression or duplicate: {barrier:?} after {last:?}");
        }
        self.completed.push(barrier);
        Ok(())
    }

    pub fn last_completed(&self) -> Option<HostBarrier> {
        self.completed.last().copied()
    }

    pub fn completed(&self) -> &[HostBarrier] {
        &self.completed
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CleanupObservation {
    pub graceful_shutdown_completed: bool,
    pub emacs_exited: bool,
    pub candidate_exited: bool,
    pub descendant_pids_remaining: Vec<u32>,
    pub pending_host_actions: usize,
    pub pending_server_requests: usize,
    pub locked_test_artifacts: usize,
}

pub fn validate_cleanup(observation: &CleanupObservation) -> Result<()> {
    ensure!(
        observation.graceful_shutdown_completed,
        "graceful client/workspace shutdown did not complete"
    );
    ensure!(observation.emacs_exited, "Emacs process did not exit");
    ensure!(observation.candidate_exited, "exact candidate process remained alive");
    ensure!(
        observation.descendant_pids_remaining.is_empty(),
        "test-owned descendant processes remained alive: {:?}",
        observation.descendant_pids_remaining
    );
    ensure!(observation.pending_host_actions == 0, "pending host actions remain");
    ensure!(observation.pending_server_requests == 0, "pending server requests remain");
    ensure!(observation.locked_test_artifacts == 0, "test artifact/socket locks remain");
    Ok(())
}

#[derive(Debug)]
pub struct HermeticEmacsRunner {
    plan: ActualHostRunPlan,
    root: TempDir,
    home: PathBuf,
    user_emacs_directory: PathBuf,
    xdg_config: PathBuf,
    xdg_cache: PathBuf,
    xdg_data: PathBuf,
    package_dir: PathBuf,
    native_comp_cache: PathBuf,
    artifacts: PathBuf,
}

impl HermeticEmacsRunner {
    pub fn new(plan: ActualHostRunPlan) -> Result<Self> {
        plan.validate()?;
        let root = tempfile::Builder::new().prefix("perllsp-emacs-host-").tempdir()?;
        let home = root.path().join("home");
        let user_emacs_directory = root.path().join("emacs.d");
        let xdg_config = root.path().join("xdg-config");
        let xdg_cache = root.path().join("xdg-cache");
        let xdg_data = root.path().join("xdg-data");
        let package_dir = root.path().join("packages");
        let native_comp_cache = root.path().join("native-comp");
        let artifacts = root.path().join("artifacts");
        for directory in [
            &home,
            &user_emacs_directory,
            &xdg_config,
            &xdg_cache,
            &xdg_data,
            &package_dir,
            &native_comp_cache,
            &artifacts,
        ] {
            fs::create_dir_all(directory)?;
        }
        Ok(Self {
            plan,
            root,
            home,
            user_emacs_directory,
            xdg_config,
            xdg_cache,
            xdg_data,
            package_dir,
            native_comp_cache,
            artifacts,
        })
    }

    pub fn command(&self) -> Result<Command> {
        let mut command = Command::new(&self.plan.emacs.executable);
        command.env_clear();
        copy_platform_environment(&mut command);
        command
            .arg("--batch")
            .arg("--quick")
            .arg("--no-site-file")
            .arg("--load")
            .arg(&self.plan.driver)
            .env("HOME", &self.home)
            .env("XDG_CONFIG_HOME", &self.xdg_config)
            .env("XDG_CACHE_HOME", &self.xdg_cache)
            .env("XDG_DATA_HOME", &self.xdg_data)
            .env("PERLLSP_ACTUAL_HOST_PROFILE", &self.user_emacs_directory)
            .env("PERLLSP_ACTUAL_HOST_PACKAGE_DIR", &self.package_dir)
            .env("PERLLSP_ACTUAL_HOST_NATIVE_COMP_CACHE", &self.native_comp_cache)
            .env("PERLLSP_ACTUAL_HOST_ARTIFACT_DIR", &self.artifacts)
            .env("PERLLSP_ACTUAL_HOST_SUBJECT", &self.plan.candidate.path)
            .env("PERLLSP_ACTUAL_HOST_CLIENT_KIND", self.plan.client.subject.client_kind())
            .env("PERLLSP_ACTUAL_HOST_JOURNEY", &self.plan.journey)
            .env("PERLLSP_ACTUAL_HOST_FIXTURE", &self.plan.fixture_identity);

        let mut path_entries = Vec::new();
        if let Some(parent) = self.plan.emacs.executable.parent() {
            path_entries.push(parent.to_path_buf());
        }
        if let Some(parent) = self.plan.candidate.path.parent()
            && !path_entries.contains(&parent.to_path_buf())
        {
            path_entries.push(parent.to_path_buf());
        }
        let controlled_path = env::join_paths(path_entries).context("build controlled host PATH")?;
        command.env("PATH", controlled_path);
        Ok(command)
    }

    pub fn root(&self) -> &Path {
        self.root.path()
    }

    pub fn artifacts(&self) -> &Path {
        &self.artifacts
    }

    pub fn plan(&self) -> &ActualHostRunPlan {
        &self.plan
    }

    pub fn isolated_paths(&self) -> BTreeSet<&Path> {
        [
            self.home.as_path(),
            self.user_emacs_directory.as_path(),
            self.xdg_config.as_path(),
            self.xdg_cache.as_path(),
            self.xdg_data.as_path(),
            self.package_dir.as_path(),
            self.native_comp_cache.as_path(),
            self.artifacts.as_path(),
        ]
        .into_iter()
        .collect()
    }
}

pub fn sha256_file(path: &Path) -> Result<String> {
    let mut file = File::open(path).with_context(|| format!("open {} for digest", path.display()))?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 16 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(format!("{digest:x}"))
}

fn copy_platform_environment(command: &mut Command) {
    const ALLOWLIST: [&str; 9] = [
        "SystemRoot",
        "WINDIR",
        "COMSPEC",
        "PATHEXT",
        "TMP",
        "TEMP",
        "TMPDIR",
        "LANG",
        "LC_ALL",
    ];
    for key in ALLOWLIST {
        if let Some(value) = env::var_os(key) {
            command.env(OsString::from(key), value);
        }
    }
}
