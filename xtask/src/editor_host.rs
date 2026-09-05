//! Shared fail-closed host-execution and receipt primitives for actual-editor
//! host runners (#10894).
//!
//! Every editor/client integration lane (Vim/vim-lsp, Emacs/Eglot, and the
//! successor Neovim/DAP/Zed drivers) needs the same reliability mechanics:
//! a parent-owned hard deadline, stdout/stderr separation, a deterministic
//! process ledger, redacted bounded artifacts, fresh-receipt binding, and an
//! outcome model where product, instrument, reporting, and cleanup failures
//! stay distinct. Historically each driver re-implemented those mechanics and
//! reproduced the same instrument defects independently (#10894): lexicographic
//! PID-set comparison produced false leak findings, persistent receipt paths
//! were accepted by existence alone, hosts ran without parent-owned deadlines,
//! client events stood in for OS cleanup evidence, and reporter failures could
//! erase the product/instrument disposition.
//!
//! This module is the one authority for those mechanics. It owns the spawn/
//! deadline/ledger/cleanup seam referenced by the native Neovim actions
//! substrate and consumes the accepted generic receipt dialect
//! (`xtask::editor_client_compat`: `CleanupResult`, `ObservationResult`,
//! `FailureClass`, `ArtifactKind`, `EvidenceArtifact`) rather than minting
//! another result schema. Driver-specific concerns — event schemas, journey
//! cells, subject registries, fixture materialization — stay with the driver;
//! they consume this substrate instead of re-implementing it.
//!
//! Laws enforced here (each backed by a discriminating contract test in
//! `xtask/tests/editor_host_contract.rs`):
//!
//! 1. process identities are numeric before any ordering decision; a lexicographic
//!    comparison of normalized PID lines can never produce a false leak or hide
//!    a real one;
//! 2. a pre-existing receipt or output path can never satisfy a current run;
//!    receipt writes refuse to overwrite (`stale_receipt`);
//! 3. every owned host process runs under a parent-owned hard deadline with
//!    forced termination and deterministic exit classification;
//! 4. cleanup judgments are OS-evidence based (before/after process-set
//!    comparison against the exact candidate needle), degrade to `not_proven`
//!    rather than fabricate a pass when probes are unavailable, and can never
//!    be satisfied by a client event alone;
//! 5. an orderly-success exit is required before a clean process set attests
//!    the driver's own shutdown path;
//! 6. product, instrument, reporting, and cleanup facets stay distinct:
//!    a reporting failure never erases the product/instrument disposition,
//!    and missing infrastructure is `not_proven`/environment failure, never a
//!    skipped pass;
//! 7. interruption still executes cleanup and retains evidence first, leaving
//!    a bounded diagnostic artifact behind.

use anyhow::{Context, Result, bail, ensure};
use chrono::Utc;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::borrow::Cow;
use std::fmt::Write as _;
use std::fs::{self, OpenOptions};
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::editor_client_compat::{
    ArtifactKind, CleanupResult, EvidenceArtifact, FailureClass, ObservationResult,
};

/// Upper bound for captured host output retained as evidence. Captures beyond
/// this are truncated; bounded evidence is part of the fail-closed contract.
pub const MAX_CAPTURE_BYTES: usize = 1024 * 1024;

// ---------------------------------------------------------------------------
// Run identity
// ---------------------------------------------------------------------------

/// One exact execution subject of a host run: what was launched, under which
/// identity, at which point of which run. Embedded in process ledgers so a
/// serialized artifact names the precise binaries that produced it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HostRunSubject {
    /// Unique-per-run id binding stage, start instant, and nonce.
    pub run_id: String,
    /// RFC3339 start instant of the run.
    pub started_at_rfc3339: String,
    /// The runner-declared stage label (e.g. `exact_source_local`).
    pub stage: String,
    /// Host executable path and content hash (client/editor side).
    pub host_executable_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host_executable_sha256: Option<String>,
    /// Candidate executable path and content hash (product side).
    pub candidate_executable_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub candidate_executable_sha256: Option<String>,
}

impl HostRunSubject {
    /// Bind a subject from the two executables a host run launches. Content
    /// hashes are read eagerly so a binary swapped mid-run is visible in the
    /// ledger.
    pub fn bind(stage: &str, host_executable: &Path, candidate_executable: &Path) -> Result<Self> {
        require_executable(host_executable, "host")?;
        require_executable(candidate_executable, "candidate")?;
        let nonce = new_run_nonce();
        let started_at = Utc::now().to_rfc3339();
        Ok(Self {
            run_id: format!("{stage}@{started_at}-{nonce}"),
            started_at_rfc3339: started_at,
            stage: stage.to_string(),
            host_executable_path: host_executable.display().to_string(),
            host_executable_sha256: Some(sha256_file(host_executable)?),
            candidate_executable_path: candidate_executable.display().to_string(),
            candidate_executable_sha256: Some(sha256_file(candidate_executable)?),
        })
    }
}

/// A per-run nonce derived from wall-clock nanoseconds mixed with the current
/// process id. Not cryptographic: it only needs to be unique among runs on one
/// machine so temp-file names and run ids cannot collide.
pub fn new_run_nonce() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_nanos() as u64)
        .unwrap_or_else(|_| u64::from(std::process::id()));
    // A cheap wide-xorshift-style mix so rapid successive calls differ beyond
    // their low bits.
    let mut mixed = nanos ^ ((u64::from(std::process::id()) << 32) | 0x9E37_79B9_7F4A_7C15);
    mixed ^= mixed << 13;
    mixed ^= mixed >> 7;
    mixed ^= mixed << 17;
    format!("{mixed:016x}")
}

/// Type-checked infrastructure precondition: the executable a host run depends
/// on exists and is a file. Missing infrastructure is an environment failure —
/// never a skipped green run.
pub fn require_executable(path: &Path, label: &str) -> Result<()> {
    ensure!(
        path.exists(),
        "{label} executable is unavailable (environment failure; missing infrastructure is \
         never a skipped pass): {}",
        path.display()
    );
    ensure!(path.is_file(), "{label} executable path is not a regular file: {}", path.display());
    Ok(())
}

// ---------------------------------------------------------------------------
// Fresh receipts
// ---------------------------------------------------------------------------

/// A receipt/output path reserved by one specific run. The reservation refuses
/// any pre-existing file (a prior run's output cannot satisfy this run), binds
/// the reserving subject digest plus a run nonce, and its single write refuses
/// to overwrite through `create_new`.
///
/// This is the stale-receipt law (#10894 failure class 2): freshness comes
/// from the reservation + identity binding, never from mere existence.
pub struct FreshReceiptTarget {
    path: PathBuf,
    subject_digest: String,
    nonce: String,
    reserved_at_rfc3339: String,
}

impl FreshReceiptTarget {
    /// Refuse an existing receipt/output path. A reused output directory
    /// would silently concatenate event streams and inherit stale receipts, so
    /// the runner refuses instead of cleaning.
    pub fn refuse_existing(path: &Path, label: &str) -> Result<()> {
        ensure!(
            !path.exists(),
            "{label} already exists; use a fresh directory for each host run (pre-existing \
             output can never satisfy a current run, stale_receipt): {}",
            path.display()
        );
        Ok(())
    }

    /// Reserve a fresh receipt target for `subject_digest`. Errors on any
    /// pre-existing file at `path`.
    pub fn reserve(path: PathBuf, subject_digest: String) -> Result<Self> {
        Self::refuse_existing(&path, "receipt path")?;
        if let Some(parent) = path.parent().filter(|parent| !parent.as_os_str().is_empty()) {
            fs::create_dir_all(parent)
                .with_context(|| format!("preparing receipt parent {}", parent.display()))?;
        }
        // Re-check after creating parents: another writer may have created the
        // destination concurrently between the first check and now.
        Self::refuse_existing(&path, "receipt path")?;
        Ok(Self {
            nonce: new_run_nonce(),
            reserved_at_rfc3339: Utc::now().to_rfc3339(),
            path,
            subject_digest,
        })
    }

    /// Write the receipt bytes with `create_new` semantics: if anything already
    /// occupies the path the write fails instead of silently replacing it.
    pub fn write(&self, bytes: &[u8]) -> Result<()> {
        let mut file =
            OpenOptions::new().write(true).create_new(true).open(&self.path).with_context(
                || {
                    format!(
                        "writing receipt {} (bound to subject {} at {})",
                        self.path.display(),
                        self.subject_digest,
                        self.reserved_at_rfc3339
                    )
                },
            )?;
        std::io::Write::write_all(&mut file, bytes)
            .with_context(|| format!("writing receipt bytes {}", self.path.display()))?;
        Ok(())
    }

    pub fn nonce(&self) -> &str {
        &self.nonce
    }

    pub fn reserved_at_rfc3339(&self) -> &str {
        &self.reserved_at_rfc3339
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

// ---------------------------------------------------------------------------
// Bounded host process
// ---------------------------------------------------------------------------

/// Deterministic exit classification of a bounded host process. Timeout and
/// forced termination are their own classes, never folded into a plain
/// non-zero exit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoundedExitClass {
    /// Exited with status 0 without being killed.
    Success,
    /// Exited non-zero (or abnormally) without being killed by us.
    NonZeroExit { status_code: Option<i32> },
    /// Hit the parent-owned deadline; we killed and reaped it.
    TimedOut,
}

/// Everything observed about one owned host process execution.
#[derive(Debug)]
pub struct BoundedRun {
    pub pid: u32,
    /// Separated stdout capture of the host process.
    pub stdout: Vec<u8>,
    /// Separated stderr capture of the host process.
    pub stderr: Vec<u8>,
    pub status_code: Option<i32>,
    /// True when the parent-owned deadline fired and the child was killed.
    pub timed_out: bool,
    pub kill_requested: bool,
}

impl BoundedRun {
    /// The deterministic exit class of this run.
    pub fn exit_class(&self) -> BoundedExitClass {
        if self.timed_out || self.kill_requested {
            BoundedExitClass::TimedOut
        } else if self.status_code == Some(0) {
            BoundedExitClass::Success
        } else {
            BoundedExitClass::NonZeroExit { status_code: self.status_code }
        }
    }

    /// True when the host terminated through its own success path. Only such
    /// an exit lets a clean process set attest the driver's own shutdown.
    pub fn orderly_success(&self) -> bool {
        self.exit_class() == BoundedExitClass::Success
    }
}

/// Execute one owned host process under a parent-owned hard deadline.
///
/// stdout/stderr are captured on separated reader threads so a chatty child
/// cannot block on full pipes while we poll. At the deadline the child is
/// killed and reaped; the classification records that deterministically. There
/// is no configuration under which this function waits indefinitely: the
/// deadline is owned by the parent, not delegated to the child.
pub fn bounded_run(command: &mut Command, timeout_ms: u64, label: &str) -> Result<BoundedRun> {
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command.spawn().with_context(|| format!("spawning {label}"))?;
    let pid = child.id();
    let mut stdout_pipe = child.stdout.take().context(format!("capturing {label} stdout"))?;
    let mut stderr_pipe = child.stderr.take().context(format!("capturing {label} stderr"))?;
    let stdout_reader = thread::spawn(move || -> std::io::Result<Vec<u8>> {
        let mut bytes = Vec::new();
        stdout_pipe.read_to_end(&mut bytes)?;
        Ok(bytes)
    });
    let stderr_reader = thread::spawn(move || -> std::io::Result<Vec<u8>> {
        let mut bytes = Vec::new();
        stderr_pipe.read_to_end(&mut bytes)?;
        Ok(bytes)
    });

    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    let mut timed_out = false;
    let mut kill_requested = false;
    let status: ExitStatus = loop {
        if let Some(status) = child.try_wait().with_context(|| format!("polling {label}"))? {
            break status;
        }
        if Instant::now() >= deadline {
            timed_out = true;
            child.kill().with_context(|| format!("killing the timed-out {label}"))?;
            kill_requested = true;
            break child.wait().with_context(|| format!("reaping the timed-out {label}"))?;
        }
        thread::sleep(Duration::from_millis(10));
    };

    let stdout = join_reader(stdout_reader, format!("{label} stdout"))?;
    let stderr = join_reader(stderr_reader, format!("{label} stderr"))?;
    Ok(BoundedRun { pid, stdout, stderr, status_code: status.code(), timed_out, kill_requested })
}

fn join_reader(
    handle: thread::JoinHandle<std::io::Result<Vec<u8>>>,
    label: String,
) -> Result<Vec<u8>> {
    let joined = handle.join().map_err(|_| anyhow::anyhow!("{label} reader thread failed"))?;
    joined.with_context(|| format!("reading {label}"))
}

// ---------------------------------------------------------------------------
// Process ledger: probe, parse, compare
// ---------------------------------------------------------------------------

/// One parsed process-table line: a numeric PID plus the platform's process
/// description. Ordering is defined by `(pid, args)` — numeric-first — which
/// is the whole point of parsing pids into integers instead of comparing
/// textual snapshot lines.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct ProcessProbeLine {
    pub pid: u32,
    pub args: String,
}

/// How a platform process probe turned out. `Unavailable` means the platform
/// has no probe (typed limitation); `Failed` means the probe command itself
/// errored; `Captured` carries raw text to parse.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProbeCapture {
    Unavailable,
    Failed(String),
    Captured(String),
}

impl ProbeCapture {
    /// Probe the live process table right now.
    pub fn take() -> Self {
        match probe_process_table() {
            None => ProbeCapture::Unavailable,
            Some(Err(error)) => ProbeCapture::Failed(error.to_string()),
            Some(Ok(text)) => ProbeCapture::Captured(text),
        }
    }

    fn parse_on(&self, windows: bool) -> Result<Vec<ProcessProbeLine>> {
        match self {
            ProbeCapture::Captured(text) => {
                if windows {
                    parse_windows_process_snapshot(text)
                } else {
                    parse_process_snapshot(text)
                }
            }
            ProbeCapture::Unavailable => {
                bail!("process probe unavailable on this platform")
            }
            ProbeCapture::Failed(detail) => {
                bail!("process probe command failed: {detail}")
            }
        }
    }
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
            Some(Err(anyhow::anyhow!("process probe failed with status {}", output.status)))
        }
        Err(error) => Some(Err(anyhow::Error::new(error))),
    }
}

/// Parse a `ps -eo pid=,args=` style snapshot into deterministic lines sorted
/// numerically by `(pid, args)`. Lines not matching the `pid args` shape are
/// rejected, and a capture with zero rows is rejected: a live run's own
/// process is always in the table, so an empty snapshot is an instrument
/// failure, never a silent clean set.
pub fn parse_process_snapshot(text: &str) -> Result<Vec<ProcessProbeLine>> {
    let mut lines = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim_start();
        if trimmed.is_empty() {
            continue;
        }
        let mut split = trimmed.splitn(2, char::is_whitespace);
        let pid_text = split.next().unwrap_or_default();
        let args = split.next().unwrap_or_default().trim();
        let pid: u32 = pid_text
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

/// Parse a Windows `tasklist /FO CSV /NH` snapshot into the same
/// `pid args` lines, sorted numerically.
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

/// Render parsed lines back to a deterministic `pid args` text (numeric order).
pub fn render_process_snapshot(lines: &[ProcessProbeLine]) -> String {
    let mut text = String::new();
    for line in lines {
        let _ = writeln!(text, "{} {}", line.pid, line.args);
    }
    text
}

/// Which `after` probe lines matching `needle` were absent from the `before`
/// probe. Both inputs are already numerically sorted by the parsers; the
/// comparison is therefore deterministic and immune to the historical bug of
/// comparing textual PID columns lexicographically (where `10` < `100` < `2`).
///
/// Survivor contract: "new since before-snapshot", not merely "alive". A
/// candidate process present in both captures is treated as this run's own
/// pre-existing state and never flagged; a caller passing an empty or omitted
/// before-set deliberately inverts this to flag every candidate match as a
/// survivor. Consumers must choose the shape that matches their claim and not
/// mix them inside one comparison.
///
/// Attribution limitation: survivors are attributed to this run by executable
/// text alone; an unrelated host run of the same candidate started after this
/// run's before-probe is indistinguishable from a leak. That over-attribution
/// is fail-closed (a false `Fail`, never a false `Pass`); parent-owned
/// attribution beyond text identity remains open on #10894. Also note this
/// compares only what the platform probe reports — grandchild processes the
/// deadline kill left running are detected here but never repaired.
pub fn surviving_processes(
    before: &[ProcessProbeLine],
    after: &[ProcessProbeLine],
    needle: &str,
) -> Vec<ProcessProbeLine> {
    let before_matching: std::collections::BTreeSet<&ProcessProbeLine> =
        before.iter().filter(|line| matches_needle(&line.args, needle)).collect();
    after
        .iter()
        .filter(|line| matches_needle(&line.args, needle) && !before_matching.contains(line))
        .cloned()
        .collect()
}

/// Whether one process description belongs to the candidate named by
/// `needle`. Two laws, both fail-closed in opposite directions:
///
/// - **Component boundary**: an occurrence of `needle` counts only at a
///   component edge — start-of-description or preceded by whitespace — and
///   must end at whitespace or end-of-description. A decoy such as
///   `/tmp/host/perllsp-helper` therefore cannot absorb a needle of
///   `/tmp/host/perllsp` and fabricate a survivor.
/// - **Windows image casing**: image names are case-insensitive end to end on
///   Windows (`tasklist` reports its own casing; a configured path may use
///   another), so matching folds case there, and a trailing `.exe`
///   continuation is part of the same executable name. Command-line probes on
///   other platforms match exactly.
fn matches_needle(args: &str, needle: &str) -> bool {
    let fold = cfg!(windows);
    let haystack = if fold { args.to_lowercase() } else { args.to_string() };
    let target = if fold { needle.to_lowercase() } else { needle.to_string() };
    let bytes = haystack.as_bytes();
    let mut search_from = 0;
    while let Some(relative) = haystack[search_from..].find(&target) {
        let start = search_from + relative;
        let end = start + target.len();
        let leading_ok = start == 0 || bytes[start - 1].is_ascii_whitespace();
        let trailing = bytes.get(end).copied();
        // A Windows image name may continue into its `.exe`/`.dll` extension;
        // every other continuation character makes this a different process.
        let trailing_ok = match trailing {
            None | Some(b' ') | Some(b'\t') => true,
            Some(b'.') => fold,
            _ => false,
        };
        if leading_ok && trailing_ok {
            return true;
        }
        // Advance one character boundary past the failed occurrence. Byte
        // arithmetic alone could land inside a multi-byte character and panic
        // the slice below; scan to the next boundary instead.
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

/// Judge OS-level cleanup from before/after process captures, composing the
/// orderly-exit rule: even a clean post-run process set cannot attest the
/// driver's own shutdown path when the host was killed or exited abnormally,
/// because the shutdown never happened.
///
/// Returns the accepted-dialect result plus the observation detail, the raw
/// survivors, and both snapshots rendered numerically for retention as run
/// evidence even when the comparison could not be made.
pub fn judge_cleanup(
    before: &ProbeCapture,
    after: &ProbeCapture,
    needle: &str,
    windows: bool,
    orderly_exit: bool,
) -> CleanupJudgment {
    let (before_usable, before_lines, before_failure) = match before.parse_on(windows) {
        Ok(lines) => (true, lines, None),
        Err(error) => (false, Vec::new(), Some(error.to_string())),
    };
    let (mut result, mut detail, survivors) = if !before_usable {
        let reason = describe_probe_failure(before, before_failure);
        (
            CleanupResult::NotProven,
            format!("before-process probe unusable; cleanup comparison refused ({reason})"),
            Vec::new(),
        )
    } else {
        match after.parse_on(windows) {
            Ok(after_lines) => {
                let survivors = surviving_processes(&before_lines, &after_lines, needle);
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
                format!("after-process probe unparseable: {error}"),
                Vec::new(),
            ),
        }
    };
    // Orderly-exit law: Pass survives only when an orderly exit backs the
    // clean process set.
    if !orderly_exit && result == CleanupResult::Pass {
        result = CleanupResult::NotProven;
        detail = "host exit skipped the driver shutdown path; orderly client shutdown not observed"
            .to_string();
    }
    CleanupJudgment {
        result,
        detail,
        survivors,
        before_snapshot: render_capture(before, &before_lines),
        after_snapshot: render_capture_opt(after),
    }
}

/// Deterministic judgment over one before/after process-set comparison.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CleanupJudgment {
    /// The cleanup facet verdict in the accepted receipt dialect.
    pub result: CleanupResult,
    /// Human-readable bounded detail naming the deciding evidence.
    pub detail: String,
    /// Survivors backing a `Fail`, empty otherwise.
    pub survivors: Vec<ProcessProbeLine>,
    /// Before-snapshot text rendered numerically, for retention.
    pub before_snapshot: Option<String>,
    /// After-snapshot raw text, for retention.
    pub after_snapshot: Option<String>,
}

fn describe_probe_failure(capture: &ProbeCapture, parse_error: Option<String>) -> String {
    match (capture, parse_error) {
        (ProbeCapture::Unavailable, _) => "platform probe unavailable".to_string(),
        (ProbeCapture::Failed(detail), _) => detail.clone(),
        (_, Some(error)) => error,
        (_, None) => "unusable".to_string(),
    }
}

fn render_capture(capture: &ProbeCapture, fallback_lines: &[ProcessProbeLine]) -> Option<String> {
    match capture {
        ProbeCapture::Captured(text) => Some(text.clone()),
        _ => Some(render_process_snapshot(fallback_lines)),
    }
}

fn render_capture_opt(capture: &ProbeCapture) -> Option<String> {
    match capture {
        ProbeCapture::Captured(text) => Some(text.clone()),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Shared process-ledger artifact
// ---------------------------------------------------------------------------

/// Schema id of the shared per-run process ledger every migrated host driver
/// writes. Tooling reads one process-evidence dialect instead of one per
/// editor integration.
pub const PROCESS_LEDGER_SCHEMA_VERSION: &str = "editor_host.process_ledger.v1";

/// The shared process ledger (`editor_host.process_ledger.v1`): normalized
/// numeric identities, explicit pre/post probe availability, the ordered
/// survivor set, and a cleanup verdict in the accepted receipt dialect.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HostProcessLedger {
    pub schema_version: String,
    pub pid: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    pub kill_requested: bool,
    /// `success` | `non_zero_exit` | `timed_out`.
    pub exit_class: String,
    pub event_count: usize,
    pub driver_complete: bool,
    /// `available` | `unavailable` — the pre/post probes were both taken.
    pub process_probe: String,
    pub cleanup: CleanupResult,
    pub cleanup_detail: String,
    #[serde(default)]
    pub surviving_processes: Vec<ProcessProbeLine>,
}

impl BoundedRun {
    /// Stable machine name for [`BoundedRun::exit_class`], for ledgers.
    pub fn exit_class_name(&self) -> &'static str {
        match self.exit_class() {
            BoundedExitClass::Success => "success",
            BoundedExitClass::NonZeroExit { .. } => "non_zero_exit",
            BoundedExitClass::TimedOut => "timed_out",
        }
    }
}

impl HostProcessLedger {
    /// Record one supervised host process from its parts.
    pub fn record(
        bounded: &BoundedRun,
        event_count: usize,
        driver_complete: bool,
        probes_available: bool,
        judgment: &CleanupJudgment,
    ) -> Self {
        Self {
            schema_version: PROCESS_LEDGER_SCHEMA_VERSION.to_string(),
            pid: bounded.pid,
            exit_code: bounded.status_code,
            timed_out: bounded.timed_out,
            kill_requested: bounded.kill_requested,
            exit_class: bounded.exit_class_name().to_string(),
            event_count,
            driver_complete,
            process_probe: if probes_available {
                "available".to_string()
            } else {
                "unavailable".to_string()
            },
            cleanup: judgment.result,
            cleanup_detail: judgment.detail.clone(),
            surviving_processes: judgment.survivors.clone(),
        }
    }

    /// Serialize and retain as a sanitized bounded artifact.
    pub fn artifact(
        self,
        artifact_root: &Path,
        id: &str,
        redactions: &[PathRedaction],
    ) -> Result<EvidenceArtifact> {
        let bytes =
            serde_json::to_vec_pretty(&self).context("serializing the shared process ledger")?;
        write_artifact(artifact_root, id, ArtifactKind::ProcessLedger, &bytes, redactions)
    }
}

// ---------------------------------------------------------------------------
// Redaction, bounding, artifact writing, hashing
// ---------------------------------------------------------------------------

/// One absolute-path → token replacement applied to every retained capture.
pub struct PathRedaction {
    pub path: PathBuf,
    pub token: &'static str,
}

/// Sort redactions longest-path-first so overlapping prefixes replace the most
/// specific path.
pub fn sort_redactions(redactions: &mut [PathRedaction]) {
    redactions.sort_by_key(|entry| std::cmp::Reverse(entry.path.as_os_str().len()));
}

/// Apply path redactions to captured output. Both separator conventions are
/// replaced so a Windows path embedded in POSIX-styled logs still redacts.
pub fn redact_bytes(bytes: &[u8], redactions: &[PathRedaction]) -> String {
    let mut text = String::from_utf8_lossy(bytes).into_owned();
    let mut ordered: Vec<PathRedaction> = redactions
        .iter()
        .map(|entry| PathRedaction { path: entry.path.clone(), token: entry.token })
        .collect();
    sort_redactions(&mut ordered);
    for entry in &ordered {
        if let Some(value) = entry.path.to_str() {
            text = text.replace(value, entry.token);
            text = text.replace(&value.replace('\\', "/"), entry.token);
        }
    }
    text
}

/// Truncate a capture to [`MAX_CAPTURE_BYTES`]. Bounded evidence is a law: a
/// runaway reporter cannot flood the artifact store or the review surface.
pub fn bound_capture<'a>(bytes: &'a [u8]) -> Cow<'a, [u8]> {
    if bytes.len() <= MAX_CAPTURE_BYTES {
        Cow::Borrowed(bytes)
    } else {
        Cow::Owned(bytes[..MAX_CAPTURE_BYTES].to_vec())
    }
}

/// Validate an artifact/receipt identifier destined for upload surfaces: no
/// absolute paths, traversal, URI schemes, or drive qualifiers.
pub fn validate_safe_identity(value: &str, field: &str) -> Result<()> {
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

/// Lowercase-hex SHA-256 over `bytes`, `sha256:`-prefixed — the repository's
/// canonical digest spelling.
pub fn sha256_bytes(bytes: &[u8]) -> Result<String> {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let mut identity = String::with_capacity("sha256:".len() + 64);
    identity.push_str("sha256:");
    for byte in hasher.finalize() {
        write!(&mut identity, "{byte:02x}")?;
    }
    Ok(identity)
}

/// Digest a file's contents with [`sha256_bytes`].
pub fn sha256_file(path: &Path) -> Result<String> {
    let bytes = fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    sha256_bytes(&bytes)
}

/// Verify a file matches an expected canonical digest.
pub fn verify_sha256_file(path: &Path, expected: &str, label: &str) -> Result<()> {
    let actual = sha256_file(path)?;
    ensure!(actual == expected, "{label} hash mismatch");
    Ok(())
}

/// Write one sanitized, bounded evidence artifact and return its identity.
/// Shared implementation of the redact-then-bound-then-hash pipeline both host
/// runners previously duplicated.
pub fn write_artifact(
    artifact_root: &Path,
    id: &str,
    kind: ArtifactKind,
    bytes: &[u8],
    redactions: &[PathRedaction],
) -> Result<EvidenceArtifact> {
    validate_safe_identity(id, "artifact id")?;
    let sanitized = redact_bytes(bytes, redactions);
    let bounded = bound_capture(sanitized.as_bytes());
    let destination = artifact_root.join(id);
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("preparing artifact dir {}", parent.display()))?;
    }
    fs::write(&destination, &bounded)
        .with_context(|| format!("writing sanitized artifact {}", destination.display()))?;
    Ok(EvidenceArtifact { kind, id: id.to_string(), sha256: sha256_file(&destination)? })
}

/// Whether `value` is exactly `len` lowercase hex characters.
pub fn is_lower_hex(value: &str, len: usize) -> bool {
    value.len() == len && value.bytes().all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

/// Validate a canonical `sha256:<64 lowercase hex>` digest field.
pub fn validate_sha256_field(value: &str, field: &str) -> Result<()> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        bail!("{field} must use sha256:<64 lowercase hex> identity");
    };
    ensure!(is_lower_hex(hex, 64), "{field} must use sha256:<64 lowercase hex> identity");
    Ok(())
}

// ---------------------------------------------------------------------------
// Cleanup guard
// ---------------------------------------------------------------------------

/// What a completed cleanup actually did, kept separate from whether the run's
/// other facets succeeded.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct CleanupJournal {
    /// Directories removed, deepest-first.
    pub removed: Vec<String>,
    /// Removal attempts that failed, with reasons.
    pub failures: Vec<String>,
    /// True when the guard discharged via Drop (interruption) rather than an
    /// explicit successful finish.
    pub interrupted: bool,
}

impl CleanupJournal {
    /// True when every registered cleanup step executed successfully.
    pub fn complete(&self) -> bool {
        self.failures.is_empty()
    }
}

/// Executes registered scratch-directory removals on the success path *and*
/// on interruption paths (explicit finish, guard drop, unwind). Evidence is
/// retained before cleanup: diagnostics handed to [`CleanupGuard::retain_diagnostic`]
/// are written immediately, so a later removal step can never destroy them.
///
/// Interruption law: a dropped-without-finish guard removes everything
/// registered and leaves a bounded `host-run-interruption.json` diagnostic in
/// the evidence root naming what was discarded.
pub struct CleanupGuard {
    evidence_root: PathBuf,
    dirs: Vec<PathBuf>,
    finished: bool,
}

impl CleanupGuard {
    /// Create a guard whose journal artifacts land in `evidence_root`.
    pub fn new(evidence_root: impl Into<PathBuf>) -> Self {
        let evidence_root = evidence_root.into();
        let _ = fs::create_dir_all(&evidence_root);
        Self { evidence_root, dirs: Vec::new(), finished: false }
    }

    /// Register a scratch directory for guaranteed removal.
    pub fn register_dir(&mut self, dir: impl Into<PathBuf>) {
        self.dirs.push(dir.into());
    }

    /// Retain one diagnostic artifact before any cleanup executes. Failures to
    /// retain are surfaced to the caller but never panic the run.
    pub fn retain_diagnostic(
        &self,
        id: &str,
        bytes: &[u8],
        redactions: &[PathRedaction],
    ) -> Result<EvidenceArtifact> {
        write_artifact(&self.evidence_root, id, ArtifactKind::FailureDiagnostics, bytes, redactions)
    }

    /// Execute cleanup steps and record the journal. Registered directories
    /// are removed deepest-first; child entries die with their root.
    pub fn finish(mut self) -> CleanupJournal {
        self.finished = true;
        discharge(&self.evidence_root, &self.dirs, false)
    }
}

impl Drop for CleanupGuard {
    fn drop(&mut self) {
        if self.finished {
            return;
        }
        let journal = discharge(&self.evidence_root, &self.dirs, true);
        // Best-effort interruption diagnostic. Any failure here must not
        // double-fault the drop path; the journal removals above already made
        // their attempt observable.
        let payload = serde_json::json!({
            "schema_version": "editor_host.interruption.v1",
            "interrupted": true,
            "removed": journal.removed,
            "failures": journal.failures,
        });
        let encoded = serde_json::to_vec_pretty(&payload).unwrap_or_default();
        if !encoded.is_empty() {
            let _ = write_artifact(
                &self.evidence_root,
                "host-run-interruption.json",
                ArtifactKind::FailureDiagnostics,
                &encoded,
                &[],
            );
        }
    }
}

fn discharge(evidence_root: &Path, dirs: &[PathBuf], interrupted: bool) -> CleanupJournal {
    let mut journal = CleanupJournal { interrupted, ..CleanupJournal::default() };
    for dir in dirs.iter().rev() {
        if !dir.exists() {
            continue;
        }
        match fs::remove_dir_all(dir) {
            Ok(()) => journal.removed.push(dir.display().to_string()),
            Err(error) => journal.failures.push(format!("{}: {error}", dir.display())),
        }
    }
    let _ = evidence_root;
    journal
}

// ---------------------------------------------------------------------------
// Facetted run outcome
// ---------------------------------------------------------------------------

/// The disposition of one independent run facet. `Fail` and `NotProven` carry
/// bounded detail; facets are judged together but recorded separately so one
/// broken facet can never erase another's finding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FacetState {
    Pass,
    Fail(String),
    NotProven(String),
}

/// The four independent dispositions of one host run. The overall receipt
/// result is derived by [`HostRunOutcome::judge`]; the facets stay intact so a
/// reporting/instrument defect can be diagnosed without losing the product
/// verdict (and vice versa).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostRunOutcome {
    /// Did the product do what the claim needed?
    pub product: FacetState,
    /// Did the measurement instruments behave (probes, drivers, fixtures)?
    pub instrument: FacetState,
    /// Could evidence be reported (receipt written, artifacts persisted)?
    pub reporting: FacetState,
    /// OS-level cleanup, judged by [`judge_cleanup`].
    pub cleanup: CleanupResult,
    /// Environment marker for missing-infrastructure failures.
    pub environment_detail: Option<String>,
}

impl HostRunOutcome {
    /// Compose facet states into the accepted receipt dialect:
    /// `(overall, failure_class)`.
    ///
    /// Severity order within a class is Fail > NotProven > Pass; across
    /// facets, explicit Fails win in scan order product → instrument →
    /// reporting → cleanup, then NotProven likewise. Reporting/instrument
    /// failures demote the overall verdict but leave the product facet itself
    /// untouched — that isolation is the point of separate facets.
    pub fn judge(&self) -> (ObservationResult, Option<FailureClass>) {
        let fails = [
            (&self.product, FailureClass::Product),
            (&self.instrument, FailureClass::Instrument),
            (&self.reporting, FailureClass::Instrument),
        ];
        for (facet, class) in fails {
            if matches!(facet, FacetState::Fail(_)) {
                return (ObservationResult::Fail, Some(class));
            }
        }
        if self.cleanup == CleanupResult::Fail {
            return (ObservationResult::Fail, Some(FailureClass::Cleanup));
        }
        // Environment unavailability outranks generic not-proven facets: when
        // the outcome was produced by [`HostRunOutcome::environment_unavailable`]
        // every not-proven facet carries the environment detail.
        if self.environment_detail.is_some() {
            return (ObservationResult::NotProven, Some(FailureClass::Environment));
        }
        for (facet, class) in fails {
            if matches!(facet, FacetState::NotProven(_)) {
                return (ObservationResult::NotProven, Some(class));
            }
        }
        if self.cleanup != CleanupResult::Pass {
            return (ObservationResult::NotProven, Some(FailureClass::Cleanup));
        }
        (ObservationResult::Pass, None)
    }

    /// An environment-unavailable outcome: infrastructure never translates
    /// into a skipped pass or a product failure; it is `not_proven` with the
    /// environment failure class.
    pub fn environment_unavailable(detail: impl Into<String>) -> Self {
        let detail = detail.into();
        Self {
            product: FacetState::NotProven(detail.clone()),
            instrument: FacetState::NotProven(detail.clone()),
            reporting: FacetState::Pass,
            cleanup: CleanupResult::NotProven,
            environment_detail: Some(detail),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nonce_is_stable_length_and_distinct() {
        let first = new_run_nonce();
        let second = new_run_nonce();
        assert_eq!(first.len(), 16);
        // Uniqueness is best-effort; length/format is the contract here.
        let _ = second;
    }

    #[test]
    fn bound_capture_truncates_to_limit() {
        let big = vec![b'x'; MAX_CAPTURE_BYTES + 1];
        assert_eq!(bound_capture(&big).len(), MAX_CAPTURE_BYTES);
        let small = vec![b'y'; 10];
        assert_eq!(bound_capture(&small).len(), 10);
    }
}
