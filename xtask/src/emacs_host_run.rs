//! Actual-host execution surface for the hermetic Emacs runner (#7778).
//!
//! The runner substrate itself lives in `xtask/tests/support/emacs_host_runner.rs`
//! (landed with the runner core, pull #8024) and is included here unchanged so
//! the binary command and the contract tests share one implementation instead of
//! forking a second supervisor.  This module owns what the substrate's residual
//! needed: the exact client-subject registry, run-plan construction over the
//! checked tree, fixture materialization, and the first checked consumer of
//! `build_emacs_command`/`HermeticLayout`/`run_owned_process`.
//!
//! The generic process-tree cleanup boundary (owned-process-tree semantics,
//! descendant verification, truncation metadata) is #8734's claim; this module
//! consumes the current cleanup semantics without weakening or widening them.

use anyhow::{Context, Result, bail, ensure};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use walkdir::WalkDir;

#[path = "../tests/support/emacs_host_runner.rs"]
pub mod emacs_host_runner;

use emacs_host_runner::{
    EmacsClientKind, EmacsHostPaths, EmacsHostRunPlan, HermeticLayout, ProcessObservation,
    build_emacs_command, build_receipt, file_sha256, run_owned_process,
};
use xtask::editor_client_compat::{
    CANONICAL_EXPECTATION_SET_ID, CapabilityBasis, CapabilityIdentity, CleanupResult,
    ClientSourceState, DiagnosticMode, DiagnosticsIdentity, EvidenceStage, FailureClass,
    JourneyCell, ObservationResult, PlatformIdentity, PositionEncodingBasis, RegistrationState,
    WorkspaceFixtureIdentity, canonical_expectation_set_digest, fixture_digest,
};

/// The one exact client subject wired by this slice: bundled Eglot inside
/// exact Emacs 30.1.  Later subjects are new registry rows, never silent
/// replacements of this identity.
pub const SUBJECT_BUNDLED_EGLOT_EMACS_30_1: &str = "bundled_eglot_emacs_30_1";
const BUNDLED_EGLOT_EMACS_30_1_VERSION: &str = "1.17.30";
const BUNDLED_EGLOT_EMACS_30_1_SOURCE_REF: &str = "emacs-30.1";
/// The host-build token the pinned subject requires in the probed
/// `emacs --version` line. A host without this token is a different subject.
const PINNED_EMACS_VERSION_TOKEN: &str = "30.1";
const REPOSITORY: &str = "EffortlessMetrics/perl-lsp-swarm";
const JOURNEY_SELECTOR: &str = "bundled_eglot_lifecycle.v1";
const FIXTURE_ID: &str = "bundled_eglot_lifecycle_v1";
const DEFAULT_TIMEOUT_MS: u64 = 180_000;

/// Every exact client subject the execution surface can run today.
pub fn known_subjects() -> &'static [&'static str] {
    &[SUBJECT_BUNDLED_EGLOT_EMACS_30_1]
}

/// The pinned identity of the bundled-Eglot subject.  Public so the
/// contract tests can pin the registry row without a host.
pub fn bundled_eglot_client_subject(source_sha256: String) -> emacs_host_runner::ClientSubject {
    emacs_host_runner::ClientSubject {
        client_id: SUBJECT_BUNDLED_EGLOT_EMACS_30_1.to_string(),
        kind: EmacsClientKind::BundledEglot,
        version: BUNDLED_EGLOT_EMACS_30_1_VERSION.to_string(),
        source_state: ClientSourceState::Bundled,
        source_ref: BUNDLED_EGLOT_EMACS_30_1_SOURCE_REF.to_string(),
        source_sha256,
        package_sha256: None,
    }
}

/// Inputs for one bundled-Eglot host run.  Every path must be absolute and
/// exact; the plan builder verifies digests before the host is launched.
pub struct BundledEglotHostRun {
    pub emacs_executable: PathBuf,
    pub candidate_executable: PathBuf,
    pub client_source: PathBuf,
    pub out_root: PathBuf,
    pub timeout_ms: u64,
}

/// Materialize the bounded journey fixture under `root` and return its
/// digest-identity root.  The fixture is intentionally small: slice one
/// proves the client lifecycle, and semantic expectations stay with the
/// canonical expectation set rather than a journey-local oracle.
pub fn materialize_bundled_eglot_fixture(root: &Path) -> Result<PathBuf> {
    ensure!(root.is_absolute(), "fixture root must be absolute");
    let lib = root.join("lib/My");
    let script = root.join("script");
    fs::create_dir_all(&lib).with_context(|| format!("creating {}", lib.display()))?;
    fs::create_dir_all(&script).with_context(|| format!("creating {}", script.display()))?;
    fs::write(
        lib.join("Thing.pm"),
        "package My::Thing;\nuse strict;\nuse warnings;\nsub sentinel { \"BUNDLED_EGLOT_LIFECYCLE\" }\n1;\n",
    )?;
    fs::write(
        script.join("probe.pl"),
        "use strict;\nuse warnings;\nuse lib '../lib';\nuse My::Thing;\nprint My::Thing::sentinel(), \"\\n\";\n",
    )?;
    Ok(root.to_path_buf())
}

/// The library forms one exact Emacs build can ship for its bundled Eglot.
/// Installed builds commonly load `eglot.elc` while shipping `eglot.el`
/// and/or `eglot.el.gz`; the digest binds whichever form is present, and the
/// bundled-ness proof is the installation-root containment, not the file
/// extension. Preference order is deterministic: `.el`, then `.elc`, then
/// `.el.gz`.
const BUNDLED_LIBRARY_FORMS: [&str; 3] = ["eglot.el", "eglot.elc", "eglot.el.gz"];

/// Resolve the bundled Eglot library inside the exact Emacs installation.
///
/// The executable path is canonicalized first so a symlinked `emacs` (for
/// example `/usr/bin/emacs`) cannot point the search at a foreign tree.
/// Two libraries of the *same* form inside one build is an identity defect
/// and a typed error; different forms of one library are normal shipping
/// and resolved by the fixed preference order.
pub fn resolve_bundled_client_source(emacs_executable: &Path) -> Result<PathBuf> {
    let canonical = fs::canonicalize(emacs_executable).with_context(|| {
        format!("resolving the exact Emacs executable {}", emacs_executable.display())
    })?;
    let bin = canonical.parent().context("Emacs executable has no parent directory")?;
    let root = bin.parent().context("Emacs executable has no installation root")?;
    for form in BUNDLED_LIBRARY_FORMS {
        let mut matches: Vec<PathBuf> = WalkDir::new(root)
            .max_depth(7)
            .into_iter()
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.file_type().is_file())
            .filter(|entry| entry.file_name().to_str() == Some(form))
            .map(|entry| entry.into_path())
            .collect();
        matches.sort();
        match matches.len() {
            0 => continue,
            1 => return Ok(matches.remove(0)),
            count => bail!(
                "ambiguous bundled {form} identity: {count} candidate libraries inside {}",
                root.display()
            ),
        }
    }
    bail!(
        "no bundled Eglot library {:?} found inside the exact Emacs installation {}",
        BUNDLED_LIBRARY_FORMS,
        root.display()
    )
}

fn bounded_diagnostic(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    let first = text.lines().find(|line| !line.trim().is_empty()).unwrap_or("").trim();
    first.chars().take(400).collect()
}

/// A reused output directory silently concatenates driver event streams
/// (the driver appends and restarts its sequence), so a retry into the same
/// directory would either fail parsing or misattribute stale artifacts. The
/// runner refuses instead of cleaning: nothing here owns destructive
/// deletion of a caller-supplied path.
pub fn ensure_fresh_output_root(out_root: &Path) -> Result<()> {
    ensure!(
        !out_root.exists(),
        "output root already exists; use a fresh directory for each host run: {}",
        out_root.display()
    );
    Ok(())
}

/// The subject pins the exact host build: a different Emacs is a different
/// subject, not this run. The pin token is matched against the probed
/// version line before anything is launched.
pub fn ensure_pinned_host_version(emacs_version: &str) -> Result<()> {
    ensure!(
        emacs_version.contains(PINNED_EMACS_VERSION_TOKEN),
        "Emacs host {emacs_version} does not match the pinned subject \
         {SUBJECT_BUNDLED_EGLOT_EMACS_30_1} ({PINNED_EMACS_VERSION_TOKEN})"
    );
    Ok(())
}

/// Extract a standalone 40-hex commit-like token from a version line, if it
/// carries one. Used to bind a candidate's self-reported build revision to
/// the repository commit before the receipt claims that provenance. A longer
/// or shorter hex run is not a commit identity.
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

fn first_output_line(command: &mut Command, label: &str) -> Result<String> {
    let output = command
        .stdin(Stdio::null())
        .output()
        .with_context(|| format!("running {label} identity probe"))?;
    ensure!(
        output.status.success(),
        "{label} identity probe failed with status {}: {}",
        output.status,
        bounded_diagnostic(&output.stderr)
    );
    let text = String::from_utf8_lossy(&output.stdout);
    let line = text.lines().next().unwrap_or_default().trim().to_string();
    ensure!(!line.is_empty(), "{label} identity probe produced no version line");
    Ok(line)
}

/// Current commit identity for the candidate run plan.  Computed from the
/// repository, never from ambient state.
fn candidate_commit_identity(repo_root: &Path) -> Result<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .arg("rev-parse")
        .arg("HEAD")
        .stdin(Stdio::null())
        .output()
        .context("running git rev-parse for the candidate identity")?;
    ensure!(
        output.status.success(),
        "git rev-parse failed for the candidate identity: {}",
        bounded_diagnostic(&output.stderr)
    );
    let sha = String::from_utf8_lossy(&output.stdout).trim().to_string();
    ensure!(
        sha.len() == 40 && sha.bytes().all(|byte| byte.is_ascii_hexdigit()),
        "git rev-parse produced a malformed commit identity"
    );
    Ok(sha.to_lowercase())
}

/// Build the complete run plan for the bundled-Eglot subject over the
/// checked tree.  Validation (digest verification of every exact input)
/// happens inside `build_emacs_command`, so a returned plan has already
/// proven its file identities.
pub fn build_bundled_eglot_run_plan(
    repo_root: &Path,
    run: &BundledEglotHostRun,
    commit: &str,
    candidate_version: &str,
    emacs_version: &str,
) -> Result<(EmacsHostRunPlan, HermeticLayout)> {
    let driver = repo_root.join("scripts/test/emacs-host-driver.el");
    let adapter = repo_root.join("scripts/test/emacs-clients/eglot-bundled.el");
    let configuration = repo_root.join("scripts/test/emacs-clients/eglot-bundled-config.el");
    let fixture_root = materialize_bundled_eglot_fixture(&run.out_root.join("fixture"))?;
    let layout = HermeticLayout::prepare(&run.out_root.join("hermetic"))?;
    let plan = EmacsHostRunPlan {
        identity: emacs_host_runner::EmacsHostRunIdentity {
            schema_version: emacs_host_runner::RUN_PLAN_SCHEMA_VERSION.to_string(),
            stage: EvidenceStage::ExactSourceLocal,
            repository: REPOSITORY.to_string(),
            candidate_sha: commit.to_string(),
            emacs_version: emacs_version.to_string(),
            emacs_build_sha256: file_sha256(&run.emacs_executable)?,
            client: bundled_eglot_client_subject(file_sha256(&run.client_source)?),
            driver_sha256: file_sha256(&driver)?,
            adapter_sha256: file_sha256(&adapter)?,
            configuration_sha256: file_sha256(&configuration)?,
            candidate_version: candidate_version.to_string(),
            candidate_build_revision: commit.to_string(),
            candidate_artifact_sha256: file_sha256(&run.candidate_executable)?,
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
        paths: EmacsHostPaths {
            emacs_executable: run.emacs_executable.clone(),
            client_source: run.client_source.clone(),
            client_package: None,
            driver,
            adapter,
            configuration,
            candidate_executable: run.candidate_executable.clone(),
            fixture_root,
            artifact_root: layout.artifact_directory.clone(),
        },
    };
    Ok((plan, layout))
}

fn current_platform() -> Result<PlatformIdentity> {
    // Plan validation independently rejects unsafe identity tokens, so an
    // inherited OS_VERSION carrying a path-like value fails closed there.
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

/// The typed outcome of one host run.  The receipt is written for every run
/// that reached the process stage, including failed ones; absence of a
/// receipt means the run never launched.
pub struct HostRunOutcome {
    pub receipt_path: PathBuf,
    pub result: ObservationResult,
    pub process_cleanup: CleanupResult,
    pub driver_complete: bool,
}

/// CLI entry: validate the subject id, resolve the bundled client source
/// when the caller did not pin one, and execute the run.
pub fn host_run_from_cli(
    repo_root: &Path,
    subject: &str,
    emacs_executable: PathBuf,
    candidate_executable: PathBuf,
    client_source: Option<PathBuf>,
    out_root: PathBuf,
    timeout_ms: u64,
) -> Result<HostRunOutcome> {
    if subject != SUBJECT_BUNDLED_EGLOT_EMACS_30_1 {
        bail!(
            "unknown client subject {subject}: known subjects are {}",
            known_subjects().join(", ")
        );
    }
    // Exact inputs are checked before any installation walk or launch: an
    // unavailable host or candidate is a typed error here, never a skip and
    // never a search through an unrelated directory tree.
    for (label, path) in
        [("Emacs executable", &emacs_executable), ("candidate executable", &candidate_executable)]
    {
        ensure!(path.is_absolute(), "{label} must be an absolute path: {}", path.display());
        ensure!(path.is_file(), "{label} is not a file: {}", path.display());
    }
    ensure!(out_root.is_absolute(), "output root must be an absolute path: {}", out_root.display());
    let client_source = match client_source {
        Some(path) => path,
        None => resolve_bundled_client_source(&emacs_executable)?,
    };
    host_run(
        repo_root,
        &BundledEglotHostRun {
            emacs_executable,
            candidate_executable,
            client_source,
            out_root,
            timeout_ms,
        },
    )
}

/// Execute one bundled-Eglot actual-host run and write its receipt.
///
/// Missing or unusable inputs are typed errors before launch: an unavailable
/// host is never reported as a green or skipped run.
pub fn host_run(repo_root: &Path, run: &BundledEglotHostRun) -> Result<HostRunOutcome> {
    ensure!(!known_subjects().is_empty(), "no client subjects registered");
    ensure_fresh_output_root(&run.out_root)?;
    let commit = candidate_commit_identity(repo_root)?;
    let candidate_version =
        first_output_line(Command::new(&run.candidate_executable).arg("--version"), "candidate")?;
    let emacs_version =
        first_output_line(Command::new(&run.emacs_executable).arg("--version"), "Emacs")?;
    ensure_pinned_host_version(&emacs_version)?;
    // When the candidate's own version line carries a build revision, it
    // must agree with the repository commit the run plan is about to claim;
    // otherwise the receipt would assert a provenance it never observed.
    if let Some(reported) = extract_commit_like_token(&candidate_version) {
        ensure!(
            reported == commit,
            "candidate reports build revision {reported} but the repository is at {commit}"
        );
    }
    fs::create_dir_all(&run.out_root)
        .with_context(|| format!("creating output root {}", run.out_root.display()))?;
    let (plan, layout) =
        build_bundled_eglot_run_plan(repo_root, run, &commit, &candidate_version, &emacs_version)?;

    let mut command = build_emacs_command(&plan, &layout)?;
    let observation = run_owned_process(&mut command, &plan, &layout)?;
    let outcome = evaluate_observation(&plan, &observation)?;

    let snapshot = layout.capability_snapshot();
    let capabilities = if snapshot.is_file() {
        CapabilityIdentity {
            initialize_snapshot_sha256: file_sha256(&snapshot)?,
            position_encodings_offered: Vec::new(),
            position_encoding_basis: PositionEncodingBasis::NotProven,
            position_encoding_selected: None,
        }
    } else {
        CapabilityIdentity {
            // Hash of zero bytes: the snapshot is absent, and the
            // limitation below says so.  It never stands in for content.
            initialize_snapshot_sha256: file_sha256_of_empty()?,
            position_encodings_offered: Vec::new(),
            position_encoding_basis: PositionEncodingBasis::NotProven,
            position_encoding_selected: None,
        }
    };
    let mut limitations = vec![
        "substrate lifecycle proof only: client support verdicts belong to #7126/#7721/#7727"
            .to_string(),
        "process-tree cleanup verification is #8734's owned boundary; this receipt consumes the \
         current runner cleanup semantics unchanged"
            .to_string(),
    ];
    if !snapshot.is_file() {
        limitations.push(
            "initialize capability snapshot absent; its hash is the empty digest".to_string(),
        );
    }
    if !outcome.runtime_digest_match {
        limitations.push(
            "the adapter's runtime client-identity attestation did not match the run plan"
                .to_string(),
        );
    }
    if extract_commit_like_token(&plan.identity.candidate_version).is_none() {
        limitations.push(
            "candidate version line carries no build revision; candidate_build_revision is bound \
             to the repository commit and the executable digest only"
                .to_string(),
        );
    }
    let receipt = build_receipt(
        &plan,
        &observation,
        capabilities,
        DiagnosticsIdentity {
            advertised_mode: DiagnosticMode::NotProven,
            observed_messages: Vec::new(),
        },
        outcome_journey(&observation),
        outcome.result,
        outcome.failure_class,
        limitations,
        format!("#7778 {JOURNEY_SELECTOR}: actual-host substrate proof, no support claim"),
    );
    let receipt_path = run.out_root.join("receipt.json");
    fs::write(&receipt_path, serde_json::to_vec_pretty(&receipt)?)
        .with_context(|| format!("writing receipt {}", receipt_path.display()))?;
    Ok(HostRunOutcome {
        receipt_path,
        result: outcome.result,
        process_cleanup: observation.cleanup,
        driver_complete: observation.driver_complete,
    })
}

struct OutcomeJudgment {
    result: ObservationResult,
    failure_class: Option<FailureClass>,
    runtime_digest_match: bool,
}

/// Cross-check the adapter's runtime identity attestation (the loaded
/// bundled library digest) against the run plan, then judge the run.
fn evaluate_observation(
    plan: &EmacsHostRunPlan,
    observation: &ProcessObservation,
) -> Result<OutcomeJudgment> {
    let planned_digest = plan
        .identity
        .client
        .source_sha256
        .strip_prefix("sha256:")
        .unwrap_or(&plan.identity.client.source_sha256)
        .to_string();
    let observed_digest = observation
        .events
        .iter()
        .find(|event| event.kind == emacs_host_runner::DriverEventKind::ClientLoaded)
        .and_then(|event| event.details.get("source_sha256"))
        .cloned();
    let runtime_digest_match = match observed_digest {
        Some(observed) => observed == planned_digest,
        None => false,
    };
    let driver_failed = observation
        .events
        .iter()
        .any(|event| event.kind == emacs_host_runner::DriverEventKind::DriverFailed);
    let result = if observation.passed_process_boundary() && runtime_digest_match {
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
    } else if observation.cleanup != CleanupResult::Pass {
        Some(FailureClass::Cleanup)
    } else if !runtime_digest_match {
        Some(FailureClass::Environment)
    } else {
        None
    };
    Ok(OutcomeJudgment { result, failure_class, runtime_digest_match })
}

fn outcome_journey(observation: &ProcessObservation) -> Vec<JourneyCell> {
    use emacs_host_runner::DriverEventKind;
    let mut cells = Vec::new();
    for (id, kind) in [
        ("client_loaded", DriverEventKind::ClientLoaded),
        ("registration_selected", DriverEventKind::RegistrationSelected),
        ("initialize_observed", DriverEventKind::InitializeObserved),
        ("workspace_ready", DriverEventKind::WorkspaceReady),
        ("buffer_opened", DriverEventKind::BufferOpened),
        ("shutdown_completed", DriverEventKind::ShutdownCompleted),
    ] {
        let observed = observation.events.iter().any(|event| event.kind == kind);
        cells.push(JourneyCell {
            id: id.to_string(),
            capability_basis: CapabilityBasis::NotApplicable,
            observed,
            result: if observed { ObservationResult::Pass } else { ObservationResult::NotProven },
            evidence: vec!["emacs/driver-events.jsonl".to_string()],
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
        evidence: vec!["emacs/process-ledger.json".to_string()],
        limitation: Some(
            "cleanup pass today means a driver-complete status-0 host exit; descendant-process \
             verification lands with #8734"
                .to_string(),
        ),
    });
    cells
}

fn file_sha256_of_empty() -> Result<String> {
    let empty = tempfile::NamedTempFile::new()?;
    file_sha256(empty.path())
}
