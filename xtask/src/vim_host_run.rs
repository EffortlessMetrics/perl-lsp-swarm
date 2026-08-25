//! Actual-host execution surface for the hermetic Vim + vim-lsp runner
//! (#10944).
//!
//! The runner substrate lives in `xtask/tests/support/vim_host_runner.rs` and
//! is included here unchanged (the same single-implementation pattern the
//! Emacs runner landed with #8024), so the binary command and the contract
//! tests share one supervisor instead of forking a second one. This module
//! owns what the substrate needed on top: consuming the #11369/#7762
//! authority manifests by reference, materializing the minimal harness
//! fixture, binding every exact identity before launch, and judging the run.
//!
//! Claim ceiling (#10944): harness architecture plus minimal exact-host
//! launch/attach/cleanup. No diagnostics/completion/navigation/edit semantic
//! cell is promoted — successor leaves (#10946 and the #11376/#11378
//! families) extend the thin adapter without reimplementing host startup.
//!
//! The historical #7810 shell harness (`scripts/ux/vim_vim_lsp_smoke.sh`)
//! retains its own receipts; this module is the canonical Rust-owned
//! orchestration going forward, and the Vimscript it launches is a thin
//! adapter (`scripts/test/vim-clients/vim-lsp-adapter.vim`) plus a bounded
//! driver (`scripts/test/vim-host-driver.vim`) — never a second orchestration
//! framework.

use anyhow::{Context, Result, bail, ensure};
use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[path = "../tests/support/vim_host_runner.rs"]
pub mod vim_host_runner;

use vim_host_runner::{
    DriverEventKind, HermeticVimLayout, ProcessObservation, VimHostPaths, VimHostRunIdentity,
    VimHostRunPlan, VimLspSubjectManifest, build_receipt, build_vim_command, bytes_sha256,
    file_sha256, run_owned_process, validate_driver_events, validate_receipt_binding,
    verify_vim_lsp_checkout,
};
use xtask::editor_client_compat::{
    CANONICAL_EXPECTATION_SET_ID, CapabilityBasis, CleanupResult, EvidenceStage, FailureClass,
    JourneyCell, ObservationResult, PlatformIdentity, RegistrationState, WorkspaceFixtureIdentity,
    canonical_expectation_set_digest, fixture_digest,
};

const REPOSITORY: &str = "EffortlessMetrics/perl-lsp-swarm";
const DEFAULT_TIMEOUT_MS: u64 = 240_000;
const JOURNEY_SELECTOR: &str = "vim_vim_lsp_host_lifecycle.v1";
const FIXTURE_ID: &str = "vim_vim_lsp_host_lifecycle_v1";

/// One exact client subject of the Vim host runner registry. The registry
/// admits exactly the #11369-pinned upstream vim-lsp commit; a newer upstream
/// head is a different subject and cannot run under this id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VimClientSubject {
    /// The pinned `prabirshrestha/vim-lsp` checkout selected by #11369
    /// (`e10d186452743beb7b43d2b3427020832f930c2b` at the manifest's
    /// observation instant).
    PinnedVimLspUpstream,
}

impl VimClientSubject {
    /// Parse a CLI subject id. Unknown ids are typed errors listing the
    /// registry, never a fallback to whatever matches loosely.
    pub fn from_id(id: &str) -> Result<Self> {
        match id {
            "pinned_vim_lsp_upstream" => Ok(Self::PinnedVimLspUpstream),
            _ => bail!(
                "unknown client subject {id}: known subjects are {}",
                Self::known_ids().join(", ")
            ),
        }
    }

    pub fn known_ids() -> &'static [&'static str] {
        &["pinned_vim_lsp_upstream"]
    }

    pub fn id(self) -> &'static str {
        "pinned_vim_lsp_upstream"
    }
}

// ---------------------------------------------------------------------------
// Authority-manifest consumption (#11369 configuration, #7762 activation)
// ---------------------------------------------------------------------------

/// The #11369 configuration-manifest fields this runner consumes. The values
/// are read from the governed artifact, never re-derived here; a manifest that
/// stops matching the canonical registration shape refuses the run.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ConfigurationManifest {
    pub schema_version: String,
    pub registration: RegistrationContract,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct RegistrationContract {
    pub server_name: String,
    pub command_identity: CommandIdentity,
    pub allowlist_filetypes: Vec<String>,
    pub root_uri_contract: RootUriContract,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct CommandIdentity {
    pub argv: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct RootUriContract {
    pub authority_manifest: String,
}

/// The #7762 activation-root manifest fields this runner consumes. Root
/// markers and filetype policy stay owned by that artifact; the driver only
/// receives the marker list and observes native detection.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ActivationRootManifest {
    pub schema_version: i64,
    pub root: RootContract,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct RootContract {
    pub markers: Vec<String>,
    pub selection: String,
    pub no_marker: String,
}

pub fn load_configuration_manifest(repo_root: &Path) -> Result<(ConfigurationManifest, String)> {
    let path = repo_root.join(".ci/editor-clients/vim-vim-lsp-configuration.v1.json");
    let bytes = fs::read(&path).with_context(|| {
        format!("reading the vim-lsp configuration manifest {}", path.display())
    })?;
    let manifest: ConfigurationManifest =
        serde_json::from_slice(&bytes).context("parsing the vim-lsp configuration manifest")?;
    ensure!(
        manifest.schema_version == "vim_lsp_configuration.v1",
        "unexpected configuration manifest schema {}",
        manifest.schema_version
    );
    // The configuration's own law: executable identity is exactly
    // `perllsp --stdio`; any other argv is a contract violation, not a
    // variant.
    ensure!(
        manifest.registration.command_identity.argv == ["perllsp", "--stdio"],
        "configuration manifest argv {} violates the canonical perllsp --stdio law",
        manifest.registration.command_identity.argv.join(" ")
    );
    ensure!(
        manifest.registration.allowlist_filetypes == ["perl"],
        "configuration manifest allowlist {} is not the canonical perl-only allowlist",
        manifest.registration.allowlist_filetypes.join(",")
    );
    ensure!(
        !manifest.registration.server_name.is_empty(),
        "configuration manifest carries no server name"
    );
    Ok((manifest, file_sha256(&path)?))
}

pub fn load_activation_root_manifest(
    repo_root: &Path,
) -> Result<(ActivationRootManifest, Vec<String>, String)> {
    let path = repo_root.join(".ci/editor-clients/vim-vim-lsp-activation-root.v1.json");
    let bytes = fs::read(&path)
        .with_context(|| format!("reading the activation-root manifest {}", path.display()))?;
    let manifest: ActivationRootManifest =
        serde_json::from_slice(&bytes).context("parsing the activation-root manifest")?;
    ensure!(!manifest.root.markers.is_empty(), "activation-root manifest pins no root markers");
    ensure!(
        manifest.root.selection == "nearest_parent_marker",
        "activation-root selection {} is not nearest_parent_marker",
        manifest.root.selection
    );
    ensure!(
        manifest.root.no_marker == "cwd_fallback",
        "activation-root no-marker policy {} is not cwd_fallback",
        manifest.root.no_marker
    );
    for marker in &manifest.root.markers {
        ensure!(
            !marker.is_empty() && !marker.contains('\\') && !marker.contains('/'),
            "activation root marker {marker} is not a plain relative file name"
        );
    }
    let markers = manifest.root.markers.clone();
    Ok((manifest, markers, file_sha256(&path)?))
}

// ---------------------------------------------------------------------------
// Fixture
// ---------------------------------------------------------------------------

/// Materialize the minimal hermetic harness fixture under `root` and return
/// its digest-bound root. The fixture carries exactly what the minimal host
/// proof needs — a #7762 root marker, a natively-detected Perl entry, and a
/// lib tree — and binds no semantic expectation: the governed
/// fixture/expectation contract (#10938, open) stays the authority successor
/// leaves will consume.
pub fn materialize_harness_fixture(root: &Path) -> Result<PathBuf> {
    ensure!(root.is_absolute(), "fixture root must be absolute");
    let lib = root.join("lib/My");
    fs::create_dir_all(&lib).with_context(|| format!("creating {}", lib.display()))?;
    // The first #7762 marker: root selection resolves here natively.
    fs::write(root.join(".perl-lsp.toml"), "# vim/vim-lsp hermetic host harness fixture\n")?;
    fs::write(
        lib.join("Widget.pm"),
        "package My::Widget;\nuse strict;\nuse warnings;\nsub answer { 42 }\n1;\n",
    )?;
    fs::write(
        root.join("main.pl"),
        "use strict;\nuse warnings;\nuse lib 'lib';\nuse My::Widget;\nmy $value = My::Widget::answer();\n",
    )?;
    Ok(root.to_path_buf())
}

// ---------------------------------------------------------------------------
// Identity probes
// ---------------------------------------------------------------------------

fn first_output_line(command: &mut Command, label: &str) -> Result<String> {
    let output = command
        .stdin(Stdio::null())
        .output()
        .with_context(|| format!("running {label} identity probe"))?;
    ensure!(
        output.status.success(),
        "{label} identity probe failed with status {}: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
            .lines()
            .find(|line| !line.trim().is_empty())
            .unwrap_or("")
            .chars()
            .take(300)
            .collect::<String>()
    );
    let text = String::from_utf8_lossy(&output.stdout);
    let line = text.lines().next().unwrap_or_default().trim().to_string();
    ensure!(!line.is_empty(), "{label} identity probe produced no version line");
    Ok(line)
}

fn full_output(command: &mut Command, label: &str) -> Result<String> {
    let output =
        command.stdin(Stdio::null()).output().with_context(|| format!("running {label} probe"))?;
    ensure!(output.status.success(), "{label} probe failed with status {}", output.status);
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn candidate_commit_identity(repo_root: &Path) -> Result<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .arg("rev-parse")
        .arg("HEAD")
        .stdin(Stdio::null())
        .output()
        .context("running git rev-parse for the candidate identity")?;
    ensure!(output.status.success(), "git rev-parse failed for the candidate identity");
    let sha = String::from_utf8_lossy(&output.stdout).trim().to_lowercase();
    ensure!(
        sha.len() == 40 && sha.bytes().all(|byte| byte.is_ascii_hexdigit()),
        "git rev-parse produced a malformed commit identity"
    );
    Ok(sha)
}

/// The Vim transport features vim-lsp requires at runtime. A host without
/// them is a typed environment failure, never a skip. Version *floors* are
/// deliberately not enforced here: the subject manifest's
/// `upstream_theoretical_prerequisites` are compatibility metadata, and the
/// maintained host-envelope rows belong to #10966.
const REQUIRED_VIM_FEATURES: [&str; 3] = ["channel", "job", "timers"];

pub fn verify_vim_features(version_output: &str) -> Result<()> {
    for feature in REQUIRED_VIM_FEATURES {
        ensure!(
            version_output.contains(&format!("+{feature}")),
            "Vim host lacks the required transport feature +{feature}; this host cannot run the \
             pinned vim-lsp client"
        );
    }
    Ok(())
}

/// Extract a standalone 40-hex commit-like token from a version line, if it
/// carries one, to cross-check the candidate's self-reported build revision
/// against the repository commit (same law as the Emacs runner).
pub fn extract_commit_like_token(line: &str) -> Option<String> {
    let bytes = line.as_bytes();
    let mut start = 0;
    while start < bytes.len() {
        if bytes[start].is_ascii_hexdigit() {
            let mut end = start;
            while end < bytes.len() && bytes[end].is_ascii_hexdigit() {
                end += 1;
            }
            if end - start == 40 {
                return Some(line[start..end].to_ascii_lowercase());
            }
            start = end;
        } else {
            start += 1;
        }
    }
    None
}

fn current_platform() -> Result<PlatformIdentity> {
    let os_version = std::env::var("OS_VERSION")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "unreported".to_string());
    Ok(PlatformIdentity {
        os: std::env::consts::OS.to_string(),
        os_version,
        arch: std::env::consts::ARCH.to_string(),
    })
}

// ---------------------------------------------------------------------------
// Inputs and run
// ---------------------------------------------------------------------------

/// Inputs for one host run. Every path must be absolute and exact; the plan
/// builder verifies digests and the pinned-subject manifest before the host
/// is launched.
pub struct VimHostRunInputs {
    pub vim_executable: PathBuf,
    pub vim_lsp_checkout: PathBuf,
    pub candidate_executable: PathBuf,
    pub out_root: PathBuf,
    pub timeout_ms: u64,
}

/// A reused output directory silently concatenates driver event streams and
/// inherits stale receipts, so the runner refuses instead of cleaning.
pub fn ensure_fresh_output_root(out_root: &Path) -> Result<()> {
    ensure!(
        !out_root.exists(),
        "output root already exists; use a fresh directory for each host run: {}",
        out_root.display()
    );
    Ok(())
}

/// The typed outcome of one host run. The receipt is written for every run
/// that reached the process stage, including failed ones; absence of a
/// receipt means the run never launched.
pub struct HostRunOutcome {
    pub receipt_path: PathBuf,
    pub result: ObservationResult,
    pub process_cleanup: CleanupResult,
    pub driver_complete: bool,
}

/// CLI entry: validate the subject id and the exact inputs, then execute the
/// run. Missing or relative inputs are typed errors here — an unavailable
/// host, client, or candidate is never a skipped green run.
pub fn host_run_from_cli(
    repo_root: &Path,
    subject_id: &str,
    vim_executable: PathBuf,
    vim_lsp_checkout: PathBuf,
    candidate_executable: PathBuf,
    out_root: PathBuf,
    timeout_ms: u64,
) -> Result<HostRunOutcome> {
    let subject = VimClientSubject::from_id(subject_id)?;
    let _ = subject;
    for (label, path) in [
        ("vim executable", &vim_executable),
        ("vim-lsp checkout", &vim_lsp_checkout),
        ("candidate executable", &candidate_executable),
    ] {
        ensure!(path.is_absolute(), "{label} must be an absolute path: {}", path.display());
    }
    ensure!(vim_executable.is_file(), "vim executable is not a file: {}", vim_executable.display());
    ensure!(
        candidate_executable.is_file(),
        "candidate executable is not a file: {}",
        candidate_executable.display()
    );
    ensure!(out_root.is_absolute(), "output root must be an absolute path: {}", out_root.display());
    host_run(
        repo_root,
        &VimHostRunInputs {
            vim_executable,
            vim_lsp_checkout,
            candidate_executable,
            out_root,
            timeout_ms,
        },
    )
}

/// Execute one hermetic actual-Vim host run and write its canonical receipt.
///
/// Every identity is bound before launch: the pinned vim-lsp subject is
/// verified against the #11369 manifest (commit, clean worktree, tree
/// digest, entry-file blob digests), the registration shape is consumed from
/// the #11369 configuration manifest, the root markers are consumed from the
/// #7762 activation-root manifest, and the candidate `perllsp` is digest- and
/// identity-packet-bound. Ambient PATH can never select another `perllsp`:
/// the registration uses the exact absolute executable this plan verified.
pub fn host_run(repo_root: &Path, run: &VimHostRunInputs) -> Result<HostRunOutcome> {
    ensure_fresh_output_root(&run.out_root)?;

    // Authority manifests first: the run consumes the governed bytes or
    // refuses, never re-derives them.
    let subject_manifest_path = repo_root.join(".ci/editor-clients/vim-vim-lsp-subject.v1.json");
    let subject_manifest = VimLspSubjectManifest::load(&subject_manifest_path)?;
    let subject_manifest_sha256 = file_sha256(&subject_manifest_path)?;
    let (configuration, configuration_sha256) = load_configuration_manifest(repo_root)?;
    let (_activation, root_markers, activation_root_sha256) =
        load_activation_root_manifest(repo_root)?;
    let checkout_identity = verify_vim_lsp_checkout(&run.vim_lsp_checkout, &subject_manifest)?;

    // Identity probes.
    let commit = candidate_commit_identity(repo_root)?;
    let candidate_version =
        first_output_line(Command::new(&run.candidate_executable).arg("--version"), "candidate")?;
    let identity_packet = full_output(
        Command::new(&run.candidate_executable).arg("--identity-json"),
        "candidate identity packet",
    )?;
    validate_identity_packet(&identity_packet)?;
    let vim_version_output =
        full_output(Command::new(&run.vim_executable).arg("--version"), "Vim")?;
    let vim_version = vim_version_output.lines().next().unwrap_or_default().trim().to_string();
    ensure!(!vim_version.is_empty(), "Vim identity probe produced no version line");
    verify_vim_features(&vim_version_output)?;
    if let Some(reported) = extract_commit_like_token(&candidate_version) {
        ensure!(
            reported == commit,
            "candidate reports build revision {reported} but the repository is at {commit}"
        );
    }

    fs::create_dir_all(&run.out_root)
        .with_context(|| format!("creating output root {}", run.out_root.display()))?;

    let driver = repo_root.join("scripts/test/vim-host-driver.vim");
    let adapter = repo_root.join("scripts/test/vim-clients/vim-lsp-adapter.vim");
    let fixture_root = materialize_harness_fixture(&run.out_root.join("fixture"))?;
    let layout = HermeticVimLayout::prepare(&run.out_root.join("hermetic"))?;

    let plan = VimHostRunPlan {
        identity: VimHostRunIdentity {
            schema_version: vim_host_runner::RUN_PLAN_SCHEMA_VERSION.to_string(),
            stage: EvidenceStage::ExactSourceLocal,
            repository: REPOSITORY.to_string(),
            candidate_sha: commit.clone(),
            vim_version: vim_version.clone(),
            vim_build_sha256: file_sha256(&run.vim_executable)?,
            vim_feature_digest: bytes_sha256(vim_version_output.as_bytes())?,
            vim_lsp_commit: checkout_identity.pinned_commit.clone(),
            vim_lsp_tree_digest: checkout_identity.tree_digest.clone(),
            vim_lsp_plugin_entry_sha256: checkout_identity.plugin_entry_sha256.clone(),
            driver_sha256: file_sha256(&driver)?,
            adapter_sha256: file_sha256(&adapter)?,
            configuration_sha256,
            activation_root_sha256,
            subject_manifest_sha256,
            candidate_version: candidate_version.clone(),
            candidate_build_revision: commit.clone(),
            candidate_artifact_sha256: file_sha256(&run.candidate_executable)?,
            candidate_identity_packet_sha256: bytes_sha256(identity_packet.as_bytes())?,
            fixture: WorkspaceFixtureIdentity {
                id: FIXTURE_ID.to_string(),
                digest: fixture_digest(&fixture_root)?,
                expectation_set_id: CANONICAL_EXPECTATION_SET_ID.to_string(),
                expectation_set_digest: canonical_expectation_set_digest()?,
            },
            journey_selector: JOURNEY_SELECTOR.to_string(),
            platform: current_platform()?,
            registration_state: RegistrationState::ManualClientRegistration,
            timeout_ms: if run.timeout_ms == 0 { DEFAULT_TIMEOUT_MS } else { run.timeout_ms },
        },
        paths: VimHostPaths {
            vim_executable: run.vim_executable.clone(),
            vim_lsp_checkout: run.vim_lsp_checkout.clone(),
            driver,
            adapter,
            candidate_executable: run.candidate_executable.clone(),
            fixture_root,
            artifact_root: layout.artifact_directory.clone(),
        },
    };

    let mut command =
        build_vim_command(&plan, &layout, &configuration.registration.server_name, &root_markers)?;
    let mut observation = run_owned_process(&mut command, &plan, &layout)?;

    // Mine the client log for the wire evidence the canonical receipt's
    // capability and diagnostics identities rest on, and retain it as
    // separately identified artifacts.
    let client_log_bytes = fs::read(layout.client_log()).unwrap_or_default();
    let wire = vim_host_runner::extract_wire_evidence(&client_log_bytes);
    observation
        .artifacts
        .extend(vim_host_runner::retain_wire_evidence_artifacts(&plan, &layout, &wire)?);

    let judgment = evaluate_observation(&plan, &observation, &wire)?;

    let snapshot = layout.capability_snapshot();
    let snapshot_sha256 = if snapshot.is_file() { Some(file_sha256(&snapshot)?) } else { None };
    let capabilities = vim_host_runner::capabilities_from_wire_evidence(&wire, snapshot_sha256)?;
    let diagnostics = vim_host_runner::diagnostics_from_wire_evidence(&wire);

    let mut limitations = vec![
        "harness substrate proof only: bootstrap through the thin adapter; no \
         diagnostics/completion/navigation/edit semantic cell is promoted (successor leaves)"
            .to_string(),
        "headless silent-ex Vim (-es): GUI-only client surfaces are not exercised by this harness"
            .to_string(),
        "actual_host_receipt.v1 protocol evidence is not composed here; the run retains the \
         initialize capability snapshot and wire-side client log instead"
            .to_string(),
        "#10938 governed fixture/expectation contract is open; this fixture is the minimal \
         harness fixture binding the #7762/#11369 authorities by digest"
            .to_string(),
    ];
    if !snapshot.is_file() {
        limitations.push(
            "initialize capability snapshot absent; its hash is the empty digest".to_string(),
        );
    }
    if observation.cleanup != CleanupResult::Pass {
        limitations.push(format!(
            "process cleanup {} ({})",
            match observation.cleanup {
                CleanupResult::Pass => "pass",
                CleanupResult::Fail => "fail",
                CleanupResult::NotProven => "not_proven",
            },
            observation.cleanup_detail
        ));
    }
    if !judgment.registration_digest_match {
        limitations.push(
            "the driver's registration attestation did not match the planned candidate digest"
                .to_string(),
        );
    }
    if plan.identity.platform.os == "windows" {
        limitations.push(
            "windows is a local probe platform for this harness; the maintained CI host row is \
             linux (vim availability and process probes are best-effort on windows)"
                .to_string(),
        );
    }
    // Observed on Git-for-Windows vim: killing the vim-lsp job while a
    // noblock channel write is pending can lose the editor's exit-callback
    // delivery, so `User lsp_server_exit` never fires even though the OS
    // process is gone. The driver fails that barrier honestly; when the
    // process-set comparison confirms no survivor, the receipt names the race
    // as the typed platform boundary it is instead of implying a leak.
    let stop_evidence_lost = observation.events.iter().any(|event| {
        event.kind == DriverEventKind::DriverFailed
            && event
                .details
                .get("reason")
                .is_some_and(|reason| reason.starts_with("server did not exit through the vim-lsp"))
    });
    let shutdown_barrier_reached =
        observation.events.iter().any(|event| event.kind == DriverEventKind::ShutdownCompleted);
    if stop_evidence_lost && shutdown_barrier_reached && observation.cleanup != CleanupResult::Fail
    {
        limitations.push(
            "the client's server-exit evidence was not observed within budget; on platforms where \
             the editor loses the job-exit callback in the kill/write race this names missing \
             client-side evidence, not a surviving process (the deterministic process-set \
             comparison remains the cleanup authority)"
                .to_string(),
        );
    }
    if !wire.saw_initialize || !wire.saw_initialized {
        limitations.push(
            "the client log carries no complete initialize/initialized attach identity; the run \
             cannot claim the attach wire sequence"
                .to_string(),
        );
    }

    let receipt = build_receipt(
        &plan,
        &observation,
        capabilities,
        diagnostics,
        outcome_journey(&observation),
        judgment.result,
        judgment.failure_class,
        limitations,
        format!(
            "#10944 {JOURNEY_SELECTOR}: hermetic actual-host substrate proof, no support claim"
        ),
    );
    let receipt_path = run.out_root.join("receipt.json");
    fs::write(&receipt_path, serde_json::to_vec_pretty(&receipt)?)
        .with_context(|| format!("writing receipt {}", receipt_path.display()))?;
    validate_receipt_binding(&receipt, &plan)
        .context("the emitted receipt failed its own freshness binding")?;
    Ok(HostRunOutcome {
        receipt_path,
        result: judgment.result,
        process_cleanup: observation.cleanup,
        driver_complete: observation.driver_complete,
    })
}

/// Validate the candidate's canonical identity packet
/// (`perl_lsp.binary_identity.v1`): executable identity, binary role, and a
/// well-formed schema. The packet binds the exact candidate bytes this run
/// launches.
pub fn validate_identity_packet(packet: &str) -> Result<()> {
    let value: serde_json::Value =
        serde_json::from_str(packet).context("parsing the candidate identity packet")?;
    ensure!(
        value.get("schema_version").and_then(|field| field.as_str())
            == Some("perl_lsp.binary_identity.v1"),
        "candidate identity packet is not perl_lsp.binary_identity.v1"
    );
    let binary =
        value.get("binary").context("candidate identity packet carries no binary section")?;
    ensure!(
        binary.get("executable").and_then(|field| field.as_str()) == Some("perllsp"),
        "candidate identity packet names another executable"
    );
    ensure!(
        binary.get("role").and_then(|field| field.as_str()) == Some("server"),
        "candidate identity packet is not the server binary role"
    );
    Ok(())
}

pub struct OutcomeJudgment {
    pub result: ObservationResult,
    pub failure_class: Option<FailureClass>,
    pub registration_digest_match: bool,
}

/// Cross-check the driver's runtime registration attestation against the run
/// plan, require the initialize/initialized attach identity in the mined
/// wire evidence, and judge the run. The `registration_selected` event must
/// carry the planned candidate artifact digest (the driver echoes the digest
/// Rust bound to the exact executable it registered), and every emitted
/// event stream must validate under the driver contract.
pub fn evaluate_observation(
    plan: &VimHostRunPlan,
    observation: &ProcessObservation,
    wire: &vim_host_runner::WireEvidence,
) -> Result<OutcomeJudgment> {
    let planned_digest = plan.identity.candidate_artifact_sha256.clone();
    let registration =
        observation.events.iter().find(|event| event.kind == DriverEventKind::RegistrationSelected);
    let registration_digest_match = registration
        .and_then(|event| event.details.get("candidate_sha256"))
        == Some(&planned_digest);
    let attach_identity_observed = wire.saw_initialize && wire.saw_initialized;
    let driver_failed =
        observation.events.iter().any(|event| event.kind == DriverEventKind::DriverFailed);
    let result = if observation.passed_process_boundary()
        && registration_digest_match
        && attach_identity_observed
        && validate_driver_events(&observation.events, true).is_ok()
    {
        ObservationResult::Pass
    } else if driver_failed
        || observation.timed_out
        || observation.status_code.is_some_and(|code| code != 0)
    {
        ObservationResult::Fail
    } else {
        ObservationResult::NotProven
    };
    let failure_class = if driver_failed {
        Some(FailureClass::HostClient)
    } else if observation.cleanup == CleanupResult::Fail {
        Some(FailureClass::Cleanup)
    } else if observation.timed_out {
        Some(FailureClass::Instrument)
    } else if !registration_digest_match || !attach_identity_observed {
        Some(FailureClass::Environment)
    } else if observation.status_code.is_some_and(|code| code != 0) {
        Some(FailureClass::HostClient)
    } else {
        None
    };
    Ok(OutcomeJudgment { result, failure_class, registration_digest_match })
}

fn outcome_journey(observation: &ProcessObservation) -> Vec<JourneyCell> {
    let mut cells = Vec::new();
    for (id, kind) in [
        ("host_started", DriverEventKind::HostStarted),
        ("client_loaded", DriverEventKind::ClientLoaded),
        ("registration_selected", DriverEventKind::RegistrationSelected),
        ("fixture_opened", DriverEventKind::FixtureOpened),
        ("server_initialized", DriverEventKind::ServerInitialized),
        ("buffer_enabled", DriverEventKind::BufferEnabled),
        ("initialize_observed", DriverEventKind::InitializeObserved),
        ("root_selected", DriverEventKind::RootSelected),
        ("shutdown_started", DriverEventKind::ShutdownStarted),
        ("shutdown_completed", DriverEventKind::ShutdownCompleted),
    ] {
        let observed = observation.events.iter().any(|event| event.kind == kind);
        cells.push(JourneyCell {
            id: id.to_string(),
            capability_basis: CapabilityBasis::NotApplicable,
            observed,
            result: if observed { ObservationResult::Pass } else { ObservationResult::NotProven },
            evidence: vec!["vim/driver-events.jsonl".to_string()],
            limitation: if observed {
                None
            } else {
                Some("lifecycle barrier never emitted".to_string())
            },
        });
    }
    cells.push(JourneyCell {
        id: "process_boundary".to_string(),
        capability_basis: CapabilityBasis::NotApplicable,
        observed: observation.status_code.is_some(),
        result: if observation.passed_process_boundary() {
            ObservationResult::Pass
        } else if observation.timed_out {
            ObservationResult::Fail
        } else {
            ObservationResult::NotProven
        },
        evidence: vec!["vim/process-ledger.json".to_string()],
        limitation: Some(
            "cleanup judgment combines driver-complete status-0 host exit with the deterministic \
             process-set comparison for the exact candidate executable"
                .to_string(),
        ),
    });
    cells
}
