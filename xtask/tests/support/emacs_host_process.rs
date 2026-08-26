//! Owned-process supervision, capture-bounds integrity, and durable redaction
//! for the shared Emacs host runner (#8734).
//!
//! Cleanup is independently observed. A status-0 host that emits
//! `shutdown_completed` is not proof that the test-owned candidate tree is
//! gone. An unavailable or unparseable probe is `not_proven`, never `pass`.

use super::{
    DriverEvent, DriverEventKind, EmacsHostRunPlan, HermeticLayout, MAX_CAPTURE_BYTES,
    bytes_sha256, file_sha256, lifecycle_rank, parse_driver_events, validate_driver_events,
    validate_safe_identity,
};
use anyhow::{Context, Result, bail};
use regex::Regex;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::fmt::Write as _;
use std::fs;
use std::io::Read;
use std::path::Path;
use std::process::{Command, ExitStatus, Stdio};
use std::sync::OnceLock;
use std::thread;
use std::time::{Duration, Instant};
use xtask::editor_client_compat::{ArtifactKind, CleanupResult, EvidenceArtifact};

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

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ProcessProbeLine {
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
    pub surviving_processes: Vec<ProcessProbeLine>,
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

fn candidate_needle(plan: &EmacsHostRunPlan) -> String {
    // Unix probes report full command lines, so the exact candidate path is
    // the identity. Windows `tasklist` exposes only the image name.
    if cfg!(windows) {
        plan.paths
            .candidate_executable
            .file_name()
            .and_then(OsStr::to_str)
            .unwrap_or("perllsp")
            .to_string()
    } else {
        plan.paths.candidate_executable.to_string_lossy().into_owned()
    }
}

fn parse_probe(text: &str) -> Result<Vec<ProcessProbeLine>> {
    if cfg!(windows) { parse_windows_process_snapshot(text) } else { parse_process_snapshot(text) }
}

/// Execute one owned host process under a parent-owned deadline. Cleanup
/// `pass` requires settled host termination and an independently observed
/// empty candidate process set. A leaked descendant is `fail` even when the
/// host exits 0 and emits `shutdown_completed`.
pub fn run_owned_process(
    command: &mut Command,
    plan: &EmacsHostRunPlan,
    layout: &HermeticLayout,
) -> Result<ProcessObservation> {
    let needle = candidate_needle(plan);
    let probe_before = probe_process_table();
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
    // Give the platform process table a bounded chance to publish a child
    // that outlived the host. This is not a cleanup grace period: survivors
    // still fail, and a missing probe still cannot pass.
    thread::sleep(Duration::from_millis(100));

    let probe_after = probe_process_table();
    let after_diagnostic = diagnostic_probe_failure("after", &probe_after);
    let (mut cleanup, mut cleanup_detail, survivors) = if let Some(before_error) = before_diagnostic
    {
        (CleanupResult::NotProven, before_error, Vec::new())
    } else {
        match (&probe_before, &probe_after) {
            (Some(Ok(_)), Some(Ok(after_text))) => match parse_probe(after_text) {
                Ok(after_lines) => {
                    let survivors = surviving_processes(&before_lines, &after_lines, &needle);
                    if survivors.is_empty() {
                        (CleanupResult::Pass, "process-set comparison clean".to_string(), survivors)
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
    let _ = fs::write(layout.process_snapshot_before(), render_process_snapshot(&before_lines));
    let _ = fs::write(
        layout.process_snapshot_after(),
        match &probe_after {
            Some(Ok(text)) => text.clone(),
            _ => String::new(),
        },
    );
    if (timed_out || kill_requested || status.code() != Some(0)) && cleanup == CleanupResult::Pass {
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
        schema_version: super::CAPTURE_BOUNDS_SCHEMA_VERSION.to_string(),
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
        surviving_processes: survivors,
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
    bounds: &mut BTreeMap<String, CaptureBoundsRow>,
) -> Result<EvidenceArtifact> {
    validate_safe_identity(id, "artifact id")?;
    let sanitized = sanitize_text(bytes, plan, layout);
    let sanitized_bytes = sanitized.as_bytes();
    // Hash the complete sanitized stream *before* bounding so a truncated
    // retention cannot present its prefix hash as the full-stream identity.
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

fn redact_resident_private_paths(text: &mut String) {
    // The `regex` crate cannot compile lookbehind. These patterns are
    // deliberately lookaround-free so they actually run: a failed compile
    // would otherwise silently leave private paths in durable artifacts.
    static PATTERNS: OnceLock<Vec<Regex>> = OnceLock::new();
    let patterns = PATTERNS.get_or_init(|| {
        [
            r#"/(?:[A-Za-z0-9._@+-]+/){1,}[A-Za-z0-9._@+-]*"#,
            r#"[A-Za-z]:[/\\][A-Za-z0-9._@+-]+(?:[/\\][A-Za-z0-9._@+-]+)*"#,
            r#"(?:\\[A-Za-z0-9._@+-]+){2,}"#,
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
            let stderr_head =
                String::from_utf8_lossy(&output.stderr[..usize::min(200, output.stderr.len())])
                    .into_owned();
            Some(Err(anyhow::anyhow!(
                "process probe failed with status {}; stderr head: {stderr_head:?}",
                output.status
            )))
        }
        Err(error) => Some(Err(anyhow::Error::new(error))),
    }
}

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

/// After-probe lines matching `needle` that were absent from the before-probe.
/// A survivor is a leak of this run's candidate identity.
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

fn diagnostic_probe_failure(phase: &str, probe: &Option<Result<String>>) -> Option<String> {
    match probe {
        None => Some(format!(
            "{phase}-process probe unavailable on this platform; cleanup not observed"
        )),
        Some(Err(error)) => Some(format!("{phase}-process probe failed: {error:#}")),
        Some(Ok(text)) => parse_probe(text)
            .err()
            .map(|error| format!("{phase}-process probe unparseable: {error:#}")),
    }
}

fn render_process_snapshot(lines: &[ProcessProbeLine]) -> String {
    let mut text = String::new();
    for line in lines {
        let _ = writeln!(text, "{} {}", line.pid, line.args);
    }
    text
}
