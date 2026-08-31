//! Owned-process supervision, capture-bounds integrity, and durable redaction
//! for the shared Emacs host runner (#8734).
//!
//! Cleanup is independently observed. A status-0 host that emits
//! `shutdown_completed` is not proof that the test-owned candidate tree is
//! gone. An unavailable or unparseable probe is `not_proven`, never `pass`.

use super::{
    DriverEvent, DriverEventKind, EmacsHostRunPlan, HermeticLayout, MAX_CAPTURE_BYTES,
    bytes_sha256, file_sha256, lifecycle_rank, parse_driver_event_prefix, validate_driver_events,
    validate_safe_identity,
};
use anyhow::{Context, Result, bail};
use regex::Regex;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::fmt::Write as _;
use std::fs;
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
    snapshot_persist: String,
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
    let stdout = child.stdout.take().context("capturing host stdout")?;
    let stderr = child.stderr.take().context("capturing host stderr")?;
    let stdout_reader = spawn_bounded_reader(stdout, StreamSanitizer::for_run(plan, layout));
    let stderr_reader = spawn_bounded_reader(stderr, StreamSanitizer::for_run(plan, layout));

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
    // Timeout/force cleanup owns this run's candidate identity, not only the
    // host PID. Kill needle-matching survivors that were absent from the
    // before-probe (never image-wide, never the pre-existing set). Descendants
    // spawned with null stdio do not hold the host pipes; join_capture still
    // unblocks on host EOF.
    if timed_out || kill_requested {
        reap_this_run_survivors(pid, &before_lines, &needle);
    }

    let stdout_capture = join_capture(stdout_reader, "host stdout")?;
    let stderr_capture = join_capture(stderr_reader, "host stderr")?;
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
        let pre_existing = matching_candidates(&before_lines, &needle);
        if !pre_existing.is_empty() {
            // A candidate process was alive before this run launched. This
            // run cannot attribute survivors (or their absence) to itself,
            // and the contract requires the test-owned candidate tree to be
            // created and reaped by this run alone: fail closed instead of
            // letting a leaked process hide in the before-baseline.
            (
                CleanupResult::NotProven,
                format!(
                    "{} candidate process(es) matching {needle:?} were already present \
                         before launch; cleanup cannot be attributed to this run",
                    pre_existing.len()
                ),
                pre_existing,
            )
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
                        "process probe unavailable on this platform; cleanup not observed"
                            .to_string()
                    }),
                    Vec::new(),
                ),
            }
        }
    };
    let after_raw = match &probe_after {
        Some(Ok(text)) => text.clone(),
        _ => String::new(),
    };
    let mut snapshot_persist_errors = Vec::new();
    for (path, raw) in [
        (layout.process_snapshot_before(), render_process_snapshot(&before_lines)),
        (layout.process_snapshot_after(), after_raw),
    ] {
        let sanitized = sanitize_text(raw.as_bytes(), plan, layout);
        if let Err(error) = persist_text(&path, &sanitized) {
            snapshot_persist_errors.push(format!("{}: {error:#}", path.display()));
        }
    }
    if (timed_out || kill_requested || status.code() != Some(0)) && cleanup == CleanupResult::Pass {
        cleanup = CleanupResult::NotProven;
        cleanup_detail =
            "host exit skipped the driver shutdown path; orderly client shutdown not observed"
                .to_string();
    }
    if !snapshot_persist_errors.is_empty() {
        if cleanup == CleanupResult::Pass {
            cleanup = CleanupResult::NotProven;
            cleanup_detail =
                format!("process snapshot persist failed: {}", snapshot_persist_errors.join("; "));
        } else {
            cleanup_detail = format!(
                "{cleanup_detail}; process snapshot persist failed: {}",
                snapshot_persist_errors.join("; ")
            );
        }
    }
    let snapshot_persist = if snapshot_persist_errors.is_empty() {
        "ok".to_string()
    } else {
        snapshot_persist_errors.join("; ")
    };

    let event_bytes = fs::read(layout.event_file()).unwrap_or_default();
    // Keep the valid JSONL prefix. A truncated trailing line must not wipe
    // barriers already observed; an invalid first line still yields no events.
    let events = parse_driver_event_prefix(&event_bytes);
    let driver_complete = validate_driver_events(&events, true).is_ok();
    let last_barrier = last_completed_barrier(&events);

    let mut bounds: BTreeMap<String, CaptureBoundsRow> = BTreeMap::new();
    let mut artifacts = Vec::new();
    artifacts.push(write_captured_stream_artifact(
        &layout.artifact_directory,
        "emacs/driver-stdout.log",
        ArtifactKind::DriverOutput,
        &stdout_capture,
        &mut bounds,
    )?);
    artifacts.push(write_captured_stream_artifact(
        &layout.artifact_directory,
        "emacs/driver-stderr.log",
        ArtifactKind::DriverOutput,
        &stderr_capture,
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
        snapshot_persist,
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

/// A host output stream captured with a bounded memory footprint.
#[derive(Debug, Clone)]
struct CapturedStream {
    /// Sanitized diagnostic window: at most `MAX_CAPTURE_BYTES` bytes.
    retained: Vec<u8>,
    /// Total sanitized bytes observed, including everything drained past the
    /// retention window.
    total_bytes: u64,
    /// `sha256:<hex>` over the complete sanitized stream, computed
    /// incrementally while draining, so the identity never depends on what
    /// was retained and a truncated retention cannot present its prefix hash
    /// as the full-stream identity.
    full_sha256: String,
}

/// Resolved per-run sanitization for streaming captures. Every replacement
/// target is a path or path-like token and the redaction patterns exclude
/// newlines, so sanitizing line-by-line is byte-identical to sanitizing the
/// whole stream while keeping the reader's memory bounded.
struct StreamSanitizer {
    replacements: Vec<(String, String)>,
}

impl StreamSanitizer {
    fn for_run(plan: &EmacsHostRunPlan, layout: &HermeticLayout) -> Self {
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
        Self {
            replacements: replacements
                .into_iter()
                .filter_map(|(path, token)| {
                    path.to_str().map(|value| (value.to_string(), token.to_string()))
                })
                .flat_map(|(value, token)| {
                    [(value.clone(), token.clone()), (value.replace('\\', "/"), token)]
                })
                .collect(),
        }
    }

    fn sanitize_line(&self, line: &str) -> String {
        let mut sanitized = line.to_string();
        for (needle, token) in &self.replacements {
            sanitized = sanitized.replace(needle, token);
        }
        redact_resident_private_paths(&mut sanitized);
        sanitized
    }
}

/// Drain one host output pipe with bounded memory: the full sanitized stream
/// is hashed and counted while only a bounded diagnostic window is retained.
/// Reading continues to EOF so the host can never block on a full pipe; a
/// hung host is still governed by the run deadline's kill, after which the
/// pipes reach EOF and the reader joins. A single line larger than the
/// retention window is flushed incrementally so no host output can grow the
/// reader's memory without bound.
fn spawn_bounded_reader(
    mut stream: impl std::io::Read + Send + 'static,
    sanitizer: StreamSanitizer,
) -> thread::JoinHandle<std::io::Result<CapturedStream>> {
    thread::spawn(move || {
        use sha2::Digest as _;
        let mut hasher = sha2::Sha256::new();
        let mut retained = Vec::new();
        let mut total_bytes = 0u64;
        let mut line: Vec<u8> = Vec::new();
        let mut chunk = [0u8; 8192];
        let absorb = |bytes: &[u8],
                      retained: &mut Vec<u8>,
                      total_bytes: &mut u64,
                      hasher: &mut sha2::Sha256|
         -> std::io::Result<()> {
            let sanitized = sanitizer.sanitize_line(&String::from_utf8_lossy(bytes));
            let sanitized = sanitized.as_bytes();
            hasher.update(sanitized);
            *total_bytes += sanitized.len() as u64;
            if retained.len() < MAX_CAPTURE_BYTES {
                let take = usize::min(MAX_CAPTURE_BYTES - retained.len(), sanitized.len());
                retained.extend_from_slice(&sanitized[..take]);
            }
            Ok(())
        };
        loop {
            let read = stream.read(&mut chunk)?;
            if read == 0 {
                break;
            }
            for byte in &chunk[..read] {
                if *byte == b'\n' {
                    line.push(b'\n');
                    absorb(&line, &mut retained, &mut total_bytes, &mut hasher)?;
                    line.clear();
                } else {
                    line.push(*byte);
                    if line.len() >= MAX_CAPTURE_BYTES {
                        absorb(&line, &mut retained, &mut total_bytes, &mut hasher)?;
                        line.clear();
                    }
                }
            }
        }
        if !line.is_empty() {
            absorb(&line, &mut retained, &mut total_bytes, &mut hasher)?;
        }
        let digest = hasher.finalize();
        let full_sha256 = format!(
            "sha256:{}",
            digest.iter().map(|byte| format!("{byte:02x}")).collect::<String>()
        );
        Ok(CapturedStream { retained, total_bytes, full_sha256 })
    })
}

fn join_capture(
    handle: thread::JoinHandle<std::io::Result<CapturedStream>>,
    label: &str,
) -> Result<CapturedStream> {
    handle
        .join()
        .map_err(|_| anyhow::anyhow!("{label} reader thread panicked"))?
        .with_context(|| format!("reading {label}"))
}

/// Persist one bounded captured stream as a durable artifact. The stream is
/// already sanitized by the reader; the bounds row stays honest about the
/// streaming capture: `full_stream_sha256` covers the complete sanitized
/// stream (independent of retention), `original_byte_count` is the total
/// sanitized bytes observed, and `truncated` records that the stream exceeded
/// the retention window.
fn write_captured_stream_artifact(
    artifact_root: &Path,
    id: &str,
    kind: ArtifactKind,
    captured: &CapturedStream,
    bounds: &mut BTreeMap<String, CaptureBoundsRow>,
) -> Result<EvidenceArtifact> {
    validate_safe_identity(id, "artifact id")?;
    let destination = artifact_root.join(id);
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&destination, &captured.retained)
        .with_context(|| format!("writing sanitized artifact {}", destination.display()))?;
    let sha256 = file_sha256(&destination)?;
    bounds.insert(
        id.to_string(),
        CaptureBoundsRow {
            id: id.to_string(),
            kind: artifact_kind_token(kind).to_string(),
            full_stream_sha256: captured.full_sha256.clone(),
            original_byte_count: captured.total_bytes,
            retained_byte_count: captured.retained.len() as u64,
            truncated: captured.total_bytes > captured.retained.len() as u64,
        },
    );
    Ok(EvidenceArtifact { kind, id: id.to_string(), sha256 })
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
    if lines.is_empty() {
        bail!("process probe captured zero rows; an empty snapshot is instrument failure");
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
    if lines.is_empty() {
        bail!("windows process probe captured zero rows; an empty snapshot is instrument failure");
    }
    lines.sort();
    Ok(lines)
}

/// Candidate-identity lines matching `needle`. On Unix the needle is the
/// exact candidate path, so a match is this suite's identity. On Windows the
/// probe exposes only image names, so any process with the candidate's image
/// basename matches: every failure direction here is conservative (cleanup is
/// `Fail` or `NotProven`, never a false `Pass`), and a match found in the
/// before-probe fails the run closed via the pre-existing check in
/// `run_owned_process`. Matching is component-bounded: `/run/perllsp-helper`
/// is not `/run/perllsp`, so timeout reap cannot kill a prefix-sharing
/// unrelated executable.
fn matching_candidates(lines: &[ProcessProbeLine], needle: &str) -> Vec<ProcessProbeLine> {
    lines.iter().filter(|line| matches_needle(&line.args, needle)).cloned().collect()
}

/// After-probe lines matching `needle` that were absent from the before-probe.
/// A survivor is a leak of this run's candidate identity. Pre-existing
/// candidate processes are handled separately and fail closed before this
/// comparison runs.
pub fn surviving_processes(
    before: &[ProcessProbeLine],
    after: &[ProcessProbeLine],
    needle: &str,
) -> Vec<ProcessProbeLine> {
    let before_matching: BTreeSet<&ProcessProbeLine> =
        before.iter().filter(|line| matches_needle(&line.args, needle)).collect();
    after
        .iter()
        .filter(|line| matches_needle(&line.args, needle) && !before_matching.contains(line))
        .cloned()
        .collect()
}

/// Whether one process description belongs to the candidate named by
/// `needle`. Component-boundary matching: the needle must sit at a whitespace
/// edge and must not continue into another token (`/run/perllsp-helper` is
/// not `/run/perllsp`). On Windows, image names fold case and may continue
/// into `.exe`. This is the same matching law as `editor_host`; the Emacs
/// runner keeps its own copy so this claim does not absorb that supervisor.
fn matches_needle(args: &str, needle: &str) -> bool {
    matches_needle_with(args, needle, cfg!(windows))
}

fn matches_needle_with(args: &str, needle: &str, fold: bool) -> bool {
    let haystack = if fold { args.to_lowercase() } else { args.to_string() };
    let target = if fold { needle.to_lowercase() } else { needle.to_string() };
    let bytes = haystack.as_bytes();
    let mut search_from = 0;
    while let Some(relative) = haystack[search_from..].find(&target) {
        let start = search_from + relative;
        let end = start + target.len();
        let leading_ok = start == 0 || bytes.get(start - 1).is_some_and(u8::is_ascii_whitespace);
        let trailing = bytes.get(end).copied();
        let trailing_ok = match trailing {
            None | Some(b' ') | Some(b'\t') => true,
            Some(b'.') => fold,
            _ => false,
        };
        if leading_ok && trailing_ok {
            return true;
        }
        let mut next = start + 1;
        while next < haystack.len() && !haystack.is_char_boundary(next) {
            next += 1;
        }
        if next >= haystack.len() {
            break;
        }
        search_from = next;
    }
    false
}

fn persist_text(path: &Path, text: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("creating process snapshot parent {}", parent.display()))?;
    }
    fs::write(path, text)
        .with_context(|| format!("writing process snapshot {}", path.display()))?;
    Ok(())
}

/// Kill this-run candidate survivors after a force/timeout host kill.
/// Selection is `surviving_processes` (needle match absent from the before-probe)
/// minus the already-waited host PID. This is not an image-wide Windows
/// `taskkill` and does not touch pre-existing matches.
fn reap_this_run_survivors(host_pid: u32, before: &[ProcessProbeLine], needle: &str) {
    for _ in 0..10 {
        let Some(Ok(text)) = probe_process_table() else {
            return;
        };
        let Ok(mid) = parse_probe(&text) else {
            return;
        };
        let remaining: Vec<ProcessProbeLine> = surviving_processes(before, &mid, needle)
            .into_iter()
            .filter(|line| line.pid != host_pid)
            .collect();
        if remaining.is_empty() {
            return;
        }
        for survivor in remaining {
            stop_owned_pid(survivor.pid);
        }
        thread::sleep(Duration::from_millis(50));
    }
}

fn stop_owned_pid(pid: u32) {
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

#[cfg(test)]
mod process_tests {
    use super::*;
    use anyhow::ensure;

    #[test]
    fn pre_existing_candidate_lines_are_detectable_in_the_before_probe() {
        let leaked = ProcessProbeLine { pid: 4242, args: "/opt/perllsp/bin/perllsp serve".into() };
        let before = vec![
            ProcessProbeLine { pid: 1, args: "/usr/bin/emacs --daemon".into() },
            leaked.clone(),
        ];
        let needle = "/opt/perllsp/bin/perllsp";
        let detected = matching_candidates(&before, needle);
        assert_eq!(detected, vec![leaked.clone()], "a pre-existing candidate must be detected");
        // ...and the survivor comparison alone must never re-report it,
        // which is exactly why run_owned_process fails closed on it instead.
        let after = vec![leaked];
        assert!(surviving_processes(&before, &after, needle).is_empty());
    }

    #[test]
    fn timeout_reap_targets_this_run_survivors_only() {
        let host = ProcessProbeLine { pid: 10, args: "/tmp/run/perllsp serve".into() };
        let leaked = ProcessProbeLine { pid: 20, args: "/tmp/run/perllsp --stdio".into() };
        let preexisting = ProcessProbeLine { pid: 5, args: "/tmp/run/perllsp older".into() };
        let decoy = ProcessProbeLine { pid: 30, args: "/another/checkout/perllsp --stdio".into() };
        let before = vec![preexisting.clone()];
        let mid = vec![preexisting, host.clone(), leaked.clone(), decoy];
        let targets: Vec<_> = surviving_processes(&before, &mid, "/tmp/run/perllsp")
            .into_iter()
            .filter(|line| line.pid != host.pid)
            .collect();
        assert_eq!(
            targets,
            vec![leaked],
            "timeout reap must kill this-run needle matches, never the host pid, pre-existing set, or a different executable"
        );
    }

    #[test]
    fn empty_process_snapshots_are_instrument_failure() -> Result<()> {
        let unix = parse_process_snapshot("").err().context("empty unix snapshot must fail")?;
        ensure!(
            unix.to_string().contains("zero rows"),
            "empty unix snapshot must name instrument failure, got {unix:#}"
        );
        let windows = parse_windows_process_snapshot(" \n")
            .err()
            .context("empty windows snapshot must fail")?;
        ensure!(
            windows.to_string().contains("zero rows"),
            "empty windows snapshot must name instrument failure, got {windows:#}"
        );
        Ok(())
    }

    #[test]
    fn candidate_match_requires_a_component_boundary() {
        assert!(
            matches_needle_with("/tmp/run/perllsp --stdio", "/tmp/run/perllsp", false),
            "the exact candidate path with arguments is this run"
        );
        assert!(
            !matches_needle_with("/tmp/run/perllsp-helper --stdio", "/tmp/run/perllsp", false),
            "a prefix-sharing executable is a different identity"
        );
        assert!(
            !matches_needle_with("cat /tmp/run/perllsp", "/tmp/run/perllsp", false),
            "a path appearing as a later argument is not the candidate image"
        );
        assert!(
            matches_needle_with("PERLLSP-TAG.EXE", "perllsp-tag.exe", true),
            "Windows image names fold case"
        );
        assert!(
            matches_needle_with("perllsp-tag.exe", "perllsp-tag", true),
            "Windows image names may continue into .exe"
        );
        assert!(
            !matches_needle_with("perllsp-tag-extra.exe", "perllsp-tag", true),
            "a different Windows image name is not this run"
        );
    }

    #[test]
    fn process_snapshot_text_redacts_private_paths() {
        let mut text = "4242 /home/observer/.netrc --flag\n".to_string();
        redact_resident_private_paths(&mut text);
        assert!(
            !text.contains("/home/observer/.netrc"),
            "durable process snapshots must not keep private path text"
        );
        assert!(text.contains("<PATH>"), "redaction must replace the private path token");
    }

    #[test]
    fn process_snapshot_write_failure_is_surfaced() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let blocker = tmp.path().join("not-a-directory");
        fs::write(&blocker, b"file")?;
        let dest = blocker.join("snapshot.txt");
        match persist_text(&dest, "1 /tmp/x\n") {
            Ok(()) => bail!("write through a file parent must fail"),
            Err(error) => {
                let rendered = format!("{error:#}");
                ensure!(
                    rendered.contains("writing process snapshot")
                        || rendered.contains("creating process snapshot parent"),
                    "the persist error must name the snapshot write, got {rendered}"
                );
            }
        }
        Ok(())
    }
}
