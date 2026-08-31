//! #11398 server-generation recovery scenario for the hermetic Vim + vim-lsp
//! host runner.
//!
//! This module is the recovery execution consumer of the #10944/#12545
//! substrate and the #12589/#12660 scenario pattern: it proves, through the
//! pinned actual Vim + vim-lsp + perllsp subject, the #11386 recovery cells —
//! explicit restart, unexpected exit disposition, new-generation
//! initialize/readiness, open-document/root/config replay, current
//! post-recovery result, old-generation rejection, retry/manual disposition,
//! and shutdown-during-recovery cleanup — using only the routes the exact
//! subject genuinely supports.
//!
//! Source-backed client facts (re-proven on every run's own wire and events):
//!
//! - **no restart command is exposed.** The pinned vim-lsp has no
//!   `:LspRestartServer`; the ordinary restart route (#11369) is the public
//!   `lsp#stop_server` stop plus the next document open, whose FileType
//!   triggers the client's lazy start — exactly the route #12660 proved for
//!   the freshness restarts. A private/raw process launch is never used.
//! - **unexpected exit does not auto-restart.** The pinned client's
//!   `s:on_exit` clears the server state (lsp_id, buffers, init result,
//!   workspace folders), emits `User lsp_server_exit`, and nothing retries:
//!   there is no timer, no retry loop, no bounded automatic recovery while
//!   the host idles. The honest disposition for this subject is
//!   `manual_restart_required`: recovery happens when the user next opens a
//!   document, which fires the client's own lazy start. The manual route is
//!   then proven through the complete new-generation chain — a new PID alone
//!   is never counted as recovery.
//! - **config push is registration-scoped.** `s:ensure_conf` pushes
//!   `workspace/didChangeConfiguration` once per registration
//!   (`_workspace_config_sent` lives on the registration's server_info and
//!   survives restarts), so replacement generations in one session receive no
//!   client config re-push; their configuration is their own initialize-time
//!   load. The governed fixture therefore has no include-path dependencies:
//!   the current-result oracle stays independent of the config channel, and
//!   the replay cell records the directly observed no-repush fact instead of
//!   inventing a replay.
//!
//! Ownership split (consumed, never duplicated):
//!
//! - `vim_host_run::vim_host_runner` (#10944) owns hermetic launch,
//!   supervision, process ledgers, cleanup comparison, generic wire mining,
//!   and receipt composition. This module owns the recovery fixture
//!   variants, the Rust-side crash stimulus watcher, the scenario-local
//!   recovery wire mining (direction-aware initialize/initialized
//!   generation counting, per-generation publish windows), the eight-cell
//!   judgment, and the scenario receipt.
//! - `vim_lsp_cell_catalog::recovery` (#11386) owns cell registration; this
//!   module cites catalog cell ids in its receipt journey and never edits a
//!   catalog.
//! - The crash stimulus is Rust-owned: the driver writes a typed marker file
//!   (a journey action, like #12660's external mutations) and waits for the
//!   client's own exit evidence; a watcher thread in this module finds the
//!   exact serving process by full command-line match on the exact candidate
//!   path and terminates it PID-precisely. Vimscript never spawns, kills, or
//!   addresses a process.
//!
//! Fail-closed laws beyond the substrate's:
//!
//! - `vim.vim_lsp.recovery.unexpected_exit` never reports `pass` (#11386
//!   family law): the canonical journey's honest top-line is `partial`, with
//!   the adverse-exit cell carrying the directly observed
//!   `manual_restart_required` disposition and every affirming cell passing;
//! - a replacement generation counts only through the complete chain —
//!   initialize and initialized on the wire, the client's `lsp_server_init`
//!   and `lsp_buffer_enabled` events, the governed document's didOpen after
//!   the new initialize, the same governed root, and a settled current
//!   result recomputed by the new generation. A bare new PID, a process
//!   spawn event, or a clean first launch satisfies nothing;
//! - the old-generation rejection is wire-bounded: after the replacement
//!   generation's initialize, no publishDiagnostics batch for the governed
//!   document may carry the old defect signature, and the settled client
//!   state must be the new generation's authored expectation;
//! - every terminate stimulus must have a watcher kill record (marker,
//!   PIDs, time); a stimulus event without a landed kill cannot pass any
//!   recovery cell;
//! - negative fixture variants (`wrong_root_decoy`, `auto_recovery_claimed`,
//!   `replay_skipped_claimed`) must fail with their typed reasons; a pass on
//!   a negative variant is an oracle violation, never a green run.

use anyhow::{Context, Result, bail, ensure};
use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::Mutex;

use crate::editor_client_compat::{
    ArtifactKind, CapabilityBasis, CleanupResult, JourneyCell, ObservationResult,
};
use crate::vim_host_run::vim_host_runner;
use crate::vim_host_run::{BoundHostPlan, VimHostRunInputs, bind_host_run_plan};
use vim_host_runner::{
    DriverEvent, DriverEventKind, HermeticVimLayout, ProcessObservation, VimHostRunPlan,
    WireEvidence, build_vim_command_with_extras, run_owned_process, validate_receipt_binding,
};

pub const RECOVERY_JOURNEY_SELECTOR: &str = "vim_vim_lsp_recovery_generations.v1";
pub const RECOVERY_FIXTURE_ID: &str = "vim_vim_lsp_recovery_generations_v1";

// ---------------------------------------------------------------------------
// Rust-authored fixture expectations
// ---------------------------------------------------------------------------

/// The governed fixture's stable layout, relative to the materialized fixture
/// root. Authored here, never derived from run output.
pub const GOVERNED_ROOT_REL: &str = "workspace/project";
pub const DECOY_ROOT_REL: &str = "workspace";
pub const OPENED_FILE_REL: &str = "workspace/project/main.pl";
pub const DECOY_FILE_REL: &str = "workspace/main.pl";
/// The governed root marker (#7762 authority list).
pub const ROOT_MARKER: &str = "cpanfile";

/// Wire file-name tokens (publishDiagnostics/didOpen `uri` tails) — the only
/// tokens the judgment accepts evidence from.
pub const MAIN_TOKEN: &str = "main.pl";

/// The old generation's governed source: the #10946 governed defect — the
/// trailing semicolon of line 4 is missing — an error-severity parser defect
/// with no include-path dependency, so the current-result oracle never
/// depends on the client's registration-scoped config channel.
pub const DEFECT_LINE_TEXT: &str = "my $value = scheduled_maintenance()";
pub const CLEAN_LINE_TEXT: &str = "my $value = scheduled_maintenance();";
pub const MUTATION_LINE: usize = 4;

pub const DEFECT_SOURCE_LINES: [&str; 5] = [
    "use strict;",
    "use warnings;",
    "sub scheduled_maintenance { return 7 }",
    DEFECT_LINE_TEXT,
    "print \"$value\\n\";",
];

/// The decoy same-named file at the outer root: clean, and never a source of
/// recovery evidence.
pub const DECOY_CLEAN_LINES: [&str; 3] =
    ["use strict;", "use warnings;", "print \"outer decoy\\n\";"];

/// The bounded absence-observation window for the no-automatic-retry
/// disposition (and the post-replacement quiet window).
pub const STALE_WINDOW_MS: u64 = 5000;

/// The #11386 catalog cell ids this journey evidences. The catalog owns
/// registration; this scenario only cites.
pub const CELL_EXPLICIT_RESTART: &str = "vim.vim_lsp.recovery.explicit_restart";
pub const CELL_UNEXPECTED_EXIT: &str = "vim.vim_lsp.recovery.unexpected_exit";
pub const CELL_INITIALIZED_NEW_GENERATION: &str = "vim.vim_lsp.recovery.initialized_new_generation";
pub const CELL_DOCUMENT_REPLAY: &str = "vim.vim_lsp.recovery.document_replay";
pub const CELL_CURRENT_RESULT: &str = "vim.vim_lsp.recovery.current_result";
pub const CELL_OLD_GENERATION_REJECTED: &str = "vim.vim_lsp.recovery.old_generation_rejected";
pub const CELL_RETRY_OR_MANUAL: &str = "vim.vim_lsp.recovery.retry_or_manual_disposition";
pub const CELL_SHUTDOWN_CLEANUP: &str = "vim.vim_lsp.recovery.shutdown_cleanup";

/// Every #11386 cell id, in denominator order.
pub const RECOVERY_CELLS: [&str; 8] = [
    CELL_EXPLICIT_RESTART,
    CELL_UNEXPECTED_EXIT,
    CELL_INITIALIZED_NEW_GENERATION,
    CELL_DOCUMENT_REPLAY,
    CELL_CURRENT_RESULT,
    CELL_OLD_GENERATION_REJECTED,
    CELL_RETRY_OR_MANUAL,
    CELL_SHUTDOWN_CLEANUP,
];

// ---------------------------------------------------------------------------
// Fixture variants
// ---------------------------------------------------------------------------

/// One scenario fixture variant. `Canonical` must reach the honest
/// `partial` top-line (seven affirming cells pass, the adverse-exit cell
/// carries the directly observed `manual_restart_required` disposition); the
/// three negative variants must fail with their typed reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryFixtureVariant {
    Canonical,
    /// The #7762 marker moves to the outer workspace: native resolution
    /// selects the decoy root and the journey must reject it.
    WrongRootDecoy,
    /// The journey claims the client recovers from the unexpected exit
    /// automatically (a new generation without any user action): the pinned
    /// client has no automatic recovery, so the claim must fail typed.
    AutoRecoveryClaimed,
    /// The journey claims an explicit restart replaces the generation
    /// without replaying the governed document: no didOpen ever follows the
    /// new initialize, so the claim must fail typed.
    ReplaySkippedClaimed,
}

impl RecoveryFixtureVariant {
    pub fn from_id(id: &str) -> Result<Self> {
        match id {
            "canonical" => Ok(Self::Canonical),
            "wrong_root_decoy" => Ok(Self::WrongRootDecoy),
            "auto_recovery_claimed" => Ok(Self::AutoRecoveryClaimed),
            "replay_skipped_claimed" => Ok(Self::ReplaySkippedClaimed),
            other => bail!(
                "unknown recovery fixture variant {other}: known variants are canonical, \
                 wrong_root_decoy, auto_recovery_claimed, replay_skipped_claimed"
            ),
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            Self::Canonical => "canonical",
            Self::WrongRootDecoy => "wrong_root_decoy",
            Self::AutoRecoveryClaimed => "auto_recovery_claimed",
            Self::ReplaySkippedClaimed => "replay_skipped_claimed",
        }
    }

    /// The typed driver-failure reason this variant must produce; `None` for
    /// the canonical variant, which must reach its honest disposition.
    pub fn expected_negative_reason(self) -> Option<&'static str> {
        match self {
            Self::Canonical => None,
            Self::WrongRootDecoy => Some("root_mismatch"),
            Self::AutoRecoveryClaimed => Some("automatic_recovery_absent"),
            Self::ReplaySkippedClaimed => Some("document_replay_absent"),
        }
    }
}

/// The materialized governed fixture for one variant.
pub struct RecoveryFixture {
    pub root: PathBuf,
    pub variant: RecoveryFixtureVariant,
}

/// Materialize the #11398 governed fixture under `root`:
///
/// ```text
/// workspace/                      <- outer decoy root (no marker, canonical)
///   main.pl                       <- same-named decoy file (clean)
///   cpanfile                      <- marker ONLY in the wrong_root_decoy variant
///   project/                      <- the governed #7762 root
///     cpanfile                    <- the governed root marker (all but decoy)
///     main.pl                     <- the governed source (defective old generation)
/// ```
///
/// No `.perl-lsp.toml` and no include-path dependency exist: the recovery
/// oracle is independent of the client's registration-scoped config channel.
/// The fixture digest recorded in the run plan pins exactly this initial
/// state; the one later source mutation (the authored clean generation) is a
/// typed journey action, never silent fixture drift.
pub fn materialize_recovery_fixture(
    root: &Path,
    variant: RecoveryFixtureVariant,
) -> Result<RecoveryFixture> {
    ensure!(root.is_absolute(), "fixture root must be absolute");
    let workspace = root.join("workspace");
    let project = workspace.join("project");
    fs::create_dir_all(&project).with_context(|| format!("creating {}", project.display()))?;
    write_lines(&project.join("main.pl"), &DEFECT_SOURCE_LINES)?;
    write_lines(&workspace.join("main.pl"), &DECOY_CLEAN_LINES)?;
    let marker = "# vim/vim-lsp #11398 governed root marker (cpanfile per #7762)\n";
    match variant {
        RecoveryFixtureVariant::WrongRootDecoy => {
            fs::write(workspace.join(ROOT_MARKER), marker)?;
        }
        _ => {
            fs::write(project.join(ROOT_MARKER), marker)?;
        }
    }
    Ok(RecoveryFixture { root: root.to_path_buf(), variant })
}

fn write_lines(path: &Path, lines: &[&str]) -> Result<()> {
    let mut text = String::new();
    for line in lines {
        text.push_str(line);
        text.push('\n');
    }
    fs::write(path, text).with_context(|| format!("writing {}", path.display()))
}

/// The full old/new source texts delivered to the driver (the mutation
/// oracle: authored here, applied verbatim by the driver).
pub fn defect_source_text() -> String {
    DEFECT_SOURCE_LINES.join("\n")
}

pub fn clean_source_text() -> String {
    let mut lines: Vec<String> = DEFECT_SOURCE_LINES.iter().map(ToString::to_string).collect();
    lines[MUTATION_LINE - 1] = CLEAN_LINE_TEXT.to_string();
    lines.join("\n")
}

/// The scenario's environment contract beyond the substrate's: the
/// Rust-authored expectations and the crash-stimulus marker channel delivered
/// to the driver (never re-derived in Vimscript).
pub fn recovery_env(
    _fixture_root: &Path,
    stimulus_dir: &Path,
    variant: RecoveryFixtureVariant,
) -> Vec<(std::ffi::OsString, std::ffi::OsString)> {
    let pairs = [
        ("PERLLSP_VIM_HOST_RECOVERY_VARIANT", variant.id().to_string()),
        ("PERLLSP_VIM_HOST_OPENED_FILE_REL", OPENED_FILE_REL.to_string()),
        ("PERLLSP_VIM_HOST_EXPECTED_ROOT_REL", GOVERNED_ROOT_REL.to_string()),
        ("PERLLSP_VIM_HOST_DECOY_ROOT_REL", DECOY_ROOT_REL.to_string()),
        ("PERLLSP_VIM_HOST_DECOY_FILE_REL", DECOY_FILE_REL.to_string()),
        ("PERLLSP_VIM_HOST_DEFECT_SOURCE_TEXT", defect_source_text()),
        ("PERLLSP_VIM_HOST_CLEAN_SOURCE_TEXT", clean_source_text()),
        ("PERLLSP_VIM_HOST_STALE_WINDOW_MS", STALE_WINDOW_MS.to_string()),
        ("PERLLSP_VIM_HOST_STIMULUS_DIR", stimulus_dir.to_string_lossy().replace('\\', "/")),
    ];
    pairs
        .into_iter()
        .map(|(key, value)| (std::ffi::OsString::from(key), std::ffi::OsString::from(value)))
        .collect()
}

// ---------------------------------------------------------------------------
// Scenario-local recovery wire mining
// ---------------------------------------------------------------------------

/// One mined `textDocument/publishDiagnostics` batch with the
/// recovery-relevant discriminators: line index, governed file token, and
/// error/warning severity counts (the old generation's defect signature is an
/// error-severity publish for the governed token).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveryBatch {
    pub line_index: usize,
    pub uri_file: String,
    pub error_severity_count: usize,
    pub warning_severity_count: usize,
}

/// The recovery facts mined from the vim-lsp client log: ordered line indexes
/// of every outgoing `initialize` request and `initialized` notification (one
/// pair per server-process generation), ordered didOpen/didClose positions
/// per governed file token, `workspace/didChangeConfiguration` positions
/// (the registration-scoped config push), and every publishDiagnostics batch
/// in wire order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RecoveryWire {
    /// Line indexes of outgoing `initialize` requests (generation starts).
    pub initialize_lines: Vec<usize>,
    /// Line indexes of outgoing `initialized` notifications.
    pub initialized_lines: Vec<usize>,
    /// Ordered (line, token) pairs for every outgoing `textDocument/didOpen`.
    pub did_open_lines: Vec<(usize, String)>,
    /// Ordered (line, token) pairs for every outgoing `textDocument/didClose`.
    pub did_close_lines: Vec<(usize, String)>,
    /// Ordered line indexes of outgoing `workspace/didChangeConfiguration`.
    pub did_change_configuration_lines: Vec<usize>,
    pub batches: Vec<RecoveryBatch>,
}

impl RecoveryWire {
    pub fn opens_of(&self, token: &str) -> Vec<usize> {
        self.did_open_lines
            .iter()
            .filter(|(_, file)| file == token)
            .map(|(line, _)| *line)
            .collect()
    }

    pub fn closes_of(&self, token: &str) -> Vec<usize> {
        self.did_close_lines
            .iter()
            .filter(|(_, file)| file == token)
            .map(|(line, _)| *line)
            .collect()
    }

    pub fn batches_of(&self, token: &str) -> Vec<&RecoveryBatch> {
        self.batches.iter().filter(|batch| batch.uri_file == token).collect()
    }

    /// The line index of the `initialize` request that started the given
    /// 1-based process generation, if the wire carried it.
    pub fn initialize_line_of(&self, generation: usize) -> Option<usize> {
        generation.checked_sub(1).and_then(|index| self.initialize_lines.get(index).copied())
    }

    /// Every publishDiagnostics batch for `token` that landed after the given
    /// wire line (the replacement generation's publish window).
    pub fn batches_after(&self, token: &str, line: usize) -> Vec<&RecoveryBatch> {
        self.batches_of(token).iter().copied().filter(|batch| batch.line_index > line).collect()
    }
}

/// Extract the recovery wire facts from the vim-lsp client log bytes. Each
/// vim-lsp log line carries its JSON payload inside an envelope array whose
/// first element is the direction marker (`--->` client-to-server, `<---`
/// server-to-client) followed by the payload — and response lines embed the
/// original request, so a method can appear on both its send line and its
/// response echo. Client-originated lifecycle facts (initialize,
/// initialized, didOpen/didClose, configuration pushes) are counted from
/// outgoing send lines only; server pushes (publishDiagnostics) are mined
/// from incoming lines.
pub fn extract_recovery_wire(log: &[u8]) -> RecoveryWire {
    let text = String::from_utf8_lossy(log);
    let mut wire = RecoveryWire::default();
    for (index, line) in text.lines().enumerate() {
        let Some(value) = first_json_value(line) else { continue };
        if let serde_json::Value::Array(items) = &value
            && let Some(serde_json::Value::String(direction)) = items.first()
            && (direction == "--->" || direction == "<---")
        {
            if let Some(payload) = items.get(3) {
                walk_recovery_value(payload, index, direction == "--->", &mut wire);
            }
            continue;
        }
        walk_recovery_value(&value, index, true, &mut wire);
    }
    wire
}

fn first_json_value(line: &str) -> Option<serde_json::Value> {
    for (index, byte) in line.bytes().enumerate() {
        if (byte == b'[' || byte == b'{')
            && let Ok(value) = serde_json::from_str::<serde_json::Value>(&line[index..])
        {
            return Some(value);
        }
    }
    None
}

fn walk_recovery_value(
    value: &serde_json::Value,
    line_index: usize,
    outgoing: bool,
    wire: &mut RecoveryWire,
) {
    match value {
        serde_json::Value::Object(map) => {
            if let Some(serde_json::Value::String(method)) = map.get("method") {
                // Only top-level protocol objects count: nested echoes inside
                // other params would double-count generations.
                if map.contains_key("params")
                    || map.contains_key("id")
                    || map.contains_key("result")
                {
                    match method.as_str() {
                        "initialize" if outgoing => wire.initialize_lines.push(line_index),
                        "initialized" if outgoing => wire.initialized_lines.push(line_index),
                        "textDocument/didOpen" if outgoing => {
                            if let Some(token) = document_token(map) {
                                wire.did_open_lines.push((line_index, token));
                            }
                        }
                        "textDocument/didClose" if outgoing => {
                            if let Some(token) = document_token(map) {
                                wire.did_close_lines.push((line_index, token));
                            }
                        }
                        "workspace/didChangeConfiguration" if outgoing => {
                            wire.did_change_configuration_lines.push(line_index);
                        }
                        "textDocument/publishDiagnostics" => {
                            if let Some(batch) = mine_recovery_batch(map, line_index) {
                                wire.batches.push(batch);
                            }
                        }
                        _ => {}
                    }
                }
            }
            for child in map.values() {
                walk_recovery_value(child, line_index, outgoing, wire);
            }
        }
        serde_json::Value::Array(items) => {
            for child in items {
                walk_recovery_value(child, line_index, outgoing, wire);
            }
        }
        _ => {}
    }
}

fn document_token(map: &serde_json::Map<String, serde_json::Value>) -> Option<String> {
    let uri = map.get("params")?.get("textDocument")?.get("uri")?.as_str()?;
    let token = uri.rsplit('/').next().unwrap_or("").to_string();
    if token.is_empty() || token.contains('\\') { None } else { Some(token) }
}

fn mine_recovery_batch(
    map: &serde_json::Map<String, serde_json::Value>,
    line_index: usize,
) -> Option<RecoveryBatch> {
    let params = map.get("params")?;
    let uri = params.get("uri")?.as_str()?;
    let uri_file = uri.rsplit('/').next().unwrap_or("").to_string();
    if uri_file.is_empty() || uri_file.contains('\\') {
        return None;
    }
    let diagnostics = params.get("diagnostics")?.as_array()?;
    let mut batch =
        RecoveryBatch { line_index, uri_file, error_severity_count: 0, warning_severity_count: 0 };
    for diagnostic in diagnostics {
        match diagnostic.get("severity").and_then(serde_json::Value::as_i64) {
            Some(1) => batch.error_severity_count += 1,
            Some(2) => batch.warning_severity_count += 1,
            _ => {}
        }
    }
    Some(batch)
}

// ---------------------------------------------------------------------------
// Rust-owned crash stimulus watcher
// ---------------------------------------------------------------------------

/// One landed crash stimulus: the marker the driver wrote, the exact
/// candidate processes the watcher terminated (full command-line match on the
/// exact candidate path), and when.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct StimulusRecord {
    pub marker: String,
    pub pids: Vec<u32>,
    pub killed_at: String,
    pub outcome: String,
}

#[derive(Debug, Default)]
struct StimulusWatchState {
    stop: bool,
    handled: BTreeSet<String>,
    records: Vec<StimulusRecord>,
}

/// The exact candidate needle: the path exactly as the registration delivers
/// it (the vim-normalized absolute form), so a decoy `perllsp` on PATH, an
/// ambient installation, or another checkout's candidate can never match.
fn candidate_needle(candidate: &Path) -> String {
    vim_host_runner::vim_path(candidate).to_string_lossy().into_owned()
}

/// Whether one observed `ps`-style command line is the exact serving server
/// process: its argv[0] IS the exact candidate path (as the registration
/// delivers it) and its argv carries the canonical `--stdio` transport
/// argument. A substring match is not enough here — the supervising
/// `cargo run ... --candidate <path>` and the `xtask` harness itself carry
/// the same path in their own command lines, and killing the supervisor
/// would abort the run instead of stimulating the server.
fn normalize_process_path(path: &str) -> String {
    let normalized = path.trim_matches(['"', '\'']).to_lowercase().replace('\\', "/");
    normalized.strip_suffix(".exe").unwrap_or(&normalized).to_string()
}

fn shell_words(args: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut quote = None;
    let mut escaped = false;
    for character in args.chars() {
        if escaped {
            word.push(character);
            escaped = false;
            continue;
        }
        match (quote, character) {
            (Some('"'), '\\') => escaped = true,
            (Some(active), c) if c == active => quote = None,
            (None, '"' | '\'') => quote = Some(c),
            (None, c) if c.is_whitespace() => {
                if !word.is_empty() {
                    words.push(std::mem::take(&mut word));
                }
            }
            (_, c) => word.push(c),
        }
    }
    if escaped {
        word.push('\\');
    }
    if !word.is_empty() {
        words.push(word);
    }
    words
}

pub fn unix_args_match_serving_server(args: &str, normalized_needle: &str) -> bool {
    let tokens = shell_words(args);
    match tokens.first() {
        Some(argv0) if normalize_process_path(argv0) == normalize_process_path(normalized_needle) => {
            tokens.iter().any(|token| token == "--stdio")
        }
        _ => false,
    }
}

/// Find every running process that is the exact serving server: argv[0]
/// equal to the exact candidate path plus the canonical `--stdio`
/// transport. On unix the `ps` probe reports full command lines; on Windows
/// the process set is first name-filtered to `perllsp.exe` (tasklist exposes
/// only image names) and then the same serving-command binding applies.
fn find_candidate_pids(needle: &str) -> Result<Vec<u32>> {
    let normalized_needle = needle.to_lowercase().replace('\\', "/");
    if cfg!(windows) {
        let script = "Get-CimInstance Win32_Process -Filter \"Name='perllsp.exe'\" | ForEach-Object { \
             \"$($_.ProcessId)|$($_.CommandLine)\" }";
        let output = std::process::Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", script])
            .stdin(std::process::Stdio::null())
            .output()
            .context("powershell process probe failed")?;
        ensure!(output.status.success(), "powershell process probe exited with {}", output.status);
        let text = String::from_utf8_lossy(&output.stdout).into_owned();
        let mut pids = Vec::new();
        for line in text.lines() {
            let Some((pid, command)) = line.split_once('|') else { continue };
            if unix_args_match_serving_server(command, &normalized_needle)
                && let Ok(pid) = pid.trim().parse::<u32>()
            {
                pids.push(pid);
            }
        }
        pids.sort_unstable();
        pids.dedup();
        Ok(pids)
    } else {
        let output = std::process::Command::new("ps")
            .args(["-eo", "pid=,args="])
            .stdin(std::process::Stdio::null())
            .output()
            .context("ps process probe failed")?;
        ensure!(output.status.success(), "ps process probe exited with {}", output.status);
        let text = String::from_utf8_lossy(&output.stdout).into_owned();
        let mut pids = Vec::new();
        for line in text.lines() {
            let trimmed = line.trim_start();
            let Some((pid, args)) = trimmed.split_once(char::is_whitespace) else { continue };
            if unix_args_match_serving_server(args.trim(), &normalized_needle)
                && let Ok(pid) = pid.parse::<u32>()
            {
                pids.push(pid);
            }
        }
        pids.sort_unstable();
        pids.dedup();
        Ok(pids)
    }
}

/// Terminate one exact PID (an adverse crash stimulus, never a graceful
/// server shutdown request).
fn terminate_pid(pid: u32) -> Result<()> {
    let output = if cfg!(windows) {
        std::process::Command::new("taskkill")
            .args(["/F", "/PID", &pid.to_string()])
            .stdin(std::process::Stdio::null())
            .output()
    } else {
        std::process::Command::new("kill")
            .args(["-9", &pid.to_string()])
            .stdin(std::process::Stdio::null())
            .output()
    }
    .with_context(|| format!("terminating stimulus pid {pid}"))?;
    ensure!(
        output.status.success(),
        "terminating stimulus pid {pid} exited with {}",
        output.status
    );
    Ok(())
}

fn watcher_cycle(
    markers_dir: &Path,
    needle: &str,
    state: &mut StimulusWatchState,
) -> Vec<StimulusRecord> {
    let mut landed = Vec::new();
    let Ok(entries) = fs::read_dir(markers_dir) else { return landed };
    let mut markers: Vec<String> = entries
        .flatten()
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| name.starts_with("kill-") && name.ends_with(".req"))
        .collect();
    markers.sort();
    for marker in markers {
        if state.handled.contains(&marker) {
            continue;
        }
        state.handled.insert(marker.clone());
        let record = match find_candidate_pids(needle) {
            Ok(pids) if !pids.is_empty() => {
                let mut killed = Vec::new();
                let mut failures = Vec::new();
                for pid in &pids {
                    match terminate_pid(*pid) {
                        Ok(()) => killed.push(*pid),
                        Err(error) => failures.push(format!("{pid}: {error}")),
                    }
                }
                let outcome = if failures.is_empty() {
                    format!("terminated {} exact candidate process(es)", killed.len())
                } else {
                    format!("partial termination: {}", failures.join("; "))
                };
                StimulusRecord {
                    marker,
                    pids: killed,
                    killed_at: chrono::Utc::now().to_rfc3339(),
                    outcome,
                }
            }
            Ok(_) => StimulusRecord {
                marker,
                pids: Vec::new(),
                killed_at: chrono::Utc::now().to_rfc3339(),
                outcome: "no exact candidate process found".to_string(),
            },
            Err(error) => StimulusRecord {
                marker,
                pids: Vec::new(),
                killed_at: chrono::Utc::now().to_rfc3339(),
                outcome: format!("process probe failed: {error}"),
            },
        };
        landed.push(record.clone());
        state.records.push(record);
    }
    landed
}

/// Spawn the crash-stimulus watcher for one run. The watcher polls the
/// marker channel until [`stop_stimulus_watcher`] is called, terminating the
/// exact serving candidate process for every marker the driver writes. The
/// returned state is read after the host run to assemble the stimulus
/// ledger; a marker with no killed PID is an honest stimulus failure.
fn spawn_stimulus_watcher(
    markers_dir: &Path,
    candidate: &Path,
) -> (Arc<Mutex<StimulusWatchState>>, std::thread::JoinHandle<()>) {
    let state = Arc::new(Mutex::new(StimulusWatchState::default()));
    let thread_state = Arc::clone(&state);
    let markers_dir = markers_dir.to_path_buf();
    let needle = candidate_needle(candidate);
    std::thread::spawn(move || {
        loop {
            let stop = thread_state.lock().map(|state| state.stop).unwrap_or(true);
            if stop {
                break;
            }
            if let Ok(mut state) = thread_state.lock() {
                watcher_cycle(&markers_dir, &needle, &mut state);
            }
            std::thread::sleep(std::time::Duration::from_millis(150));
        }
    });
    let handle = std::thread::spawn(move || {
        loop {
            let stop = thread_state.lock().map(|state| state.stop).unwrap_or(true);
            if stop {
                break;
            }
            if let Ok(mut state) = thread_state.lock() {
                watcher_cycle(&markers_dir, &needle, &mut state);
            }
            std::thread::sleep(std::time::Duration::from_millis(150));
        }
    });
    (state, handle)
}

fn stop_stimulus_watcher(
    state: &Arc<Mutex<StimulusWatchState>>,
    handle: std::thread::JoinHandle<()>,
) -> Vec<StimulusRecord> {
    // Set the stop flag, then join before draining records. This preserves a
    // watcher cycle that is already in progress when the host exits.
    if let Ok(mut state) = state.lock() {
        state.stop = true;
    }
    let _ = handle.join();
    state
        .lock()
        .map(|mut state| std::mem::take(&mut state.records))
        .unwrap_or_default()
}

/// Whether every stimulus event marker has a watcher record that actually
/// terminated at least one exact candidate process.
pub fn stimulus_ledger_is_complete(
    events: &[DriverEvent],
    records: &[StimulusRecord],
) -> Result<bool> {
    let markers: BTreeSet<String> = events
        .iter()
        .filter(|event| event.kind == DriverEventKind::RecoveryStimulusApplied)
        .filter_map(|event| event.details.get("marker").cloned())
        .collect();
    for marker in &markers {
        let Some(record) = records.iter().find(|record| &record.marker == marker) else {
            return Ok(false);
        };
        if record.pids.is_empty() {
            return Ok(false);
        }
    }
    Ok(!markers.is_empty())
}

// ---------------------------------------------------------------------------
// Run
// ---------------------------------------------------------------------------

/// The typed outcome of one recovery host run.
pub struct RecoveryRunOutcome {
    pub receipt_path: PathBuf,
    pub result: ObservationResult,
    pub process_cleanup: CleanupResult,
    pub driver_complete: bool,
    /// The typed driver-failure reason when the driver failed; the negative
    /// variants' expected reason lands here.
    pub driver_failure_reason: Option<String>,
}

/// Execute one #11398 server-generation recovery host run against the exact
/// pinned subject and write its canonical receipt. `variant` selects the
/// fixture; only `canonical` may reach its honest partial disposition.
pub fn host_recovery_run(
    repo_root: &Path,
    run: &VimHostRunInputs,
    variant: RecoveryFixtureVariant,
) -> Result<RecoveryRunOutcome> {
    crate::vim_host_run::ensure_fresh_output_root(&run.out_root)?;
    fs::create_dir_all(&run.out_root)
        .with_context(|| format!("creating output root {}", run.out_root.display()))?;

    let driver = repo_root.join("scripts/test/vim-host-recovery-driver.vim");
    let fixture = materialize_recovery_fixture(&run.out_root.join("fixture"), variant)?;
    let stimulus_dir = run.out_root.join("stimulus");
    fs::create_dir_all(&stimulus_dir)
        .with_context(|| format!("creating stimulus channel {}", stimulus_dir.display()))?;

    let BoundHostPlan { plan, server_name, root_markers } = bind_host_run_plan(
        repo_root,
        run,
        &driver,
        &fixture.root,
        RECOVERY_JOURNEY_SELECTOR,
        RECOVERY_FIXTURE_ID,
    )?;
    let layout = HermeticVimLayout::prepare(&run.out_root.join("hermetic"))?;
    let mut command = build_vim_command_with_extras(
        &plan,
        &layout,
        &server_name,
        &root_markers,
        &recovery_env(&fixture.root, &stimulus_dir, variant),
    )?;

    // The crash-stimulus watcher starts before the host: a marker the driver
    // writes mid-journey is terminated PID-precisely by this side, while the
    // driver observes only the client's own exit evidence.
    let (watcher, watcher_handle) =
        spawn_stimulus_watcher(&stimulus_dir, &plan.paths.candidate_executable);
    let mut observation = run_owned_process(&mut command, &plan, &layout)?;
    let stimulus_records = stop_stimulus_watcher(&watcher, watcher_handle);

    let (client_log_bytes, client_log_error) = match fs::read(layout.client_log()) {
        Ok(bytes) => (bytes, None),
        Err(error) => (Vec::new(), Some(error.to_string())),
    };
    let wire = vim_host_runner::extract_wire_evidence(&client_log_bytes);
    let recovery_wire = extract_recovery_wire(&client_log_bytes);
    observation
        .artifacts
        .extend(vim_host_runner::retain_wire_evidence_artifacts(&plan, &layout, &wire)?);
    observation.artifacts.push(vim_host_runner::write_sanitized_artifact(
        &layout.artifact_directory,
        "vim/recovery-stimulus-ledger.json",
        ArtifactKind::ProcessLedger,
        &serde_json::to_vec_pretty(&stimulus_records)
            .context("serializing the recovery stimulus ledger")?,
        &plan,
        &layout,
    )?);

    let judgment = evaluate_recovery_observation(
        &plan,
        &observation,
        &wire,
        &recovery_wire,
        &stimulus_records,
        variant,
    );

    let snapshot = layout.capability_snapshot();
    let snapshot_sha256 =
        if snapshot.is_file() { Some(vim_host_runner::file_sha256(&snapshot)?) } else { None };
    let capabilities = vim_host_runner::capabilities_from_wire_evidence(&wire, snapshot_sha256)?;
    let diagnostics = vim_host_runner::diagnostics_from_wire_evidence(&wire);

    let mut limitations =
        recovery_limitations(&observation, &judgment, &recovery_wire, &stimulus_records, variant);
    if let Some(error) = client_log_error {
        limitations.push(format!(
            "client log could not be read: {error}; wire evidence is unavailable, not empty"
        ));
    }
    if plan.identity.platform.os == "windows" {
        limitations.push(
            "windows is a local probe platform for this harness; the maintained CI host row is \
             linux (vim availability and process probes are best-effort on windows)"
                .to_string(),
        );
    }

    let receipt = vim_host_runner::build_receipt(
        &plan,
        &observation,
        capabilities,
        diagnostics,
        recovery_journey(&observation, &judgment, &wire),
        judgment.result,
        judgment.failure_class,
        limitations,
        format!(
            "#11398 {RECOVERY_JOURNEY_SELECTOR}: explicit restart, unexpected-exit disposition, \
             new-generation initialize/readiness, document replay, current result, \
             old-generation rejection, retry/manual disposition, and shutdown-during-recovery \
             cleanup for the exact pinned subject only"
        ),
    );
    let receipt_path = run.out_root.join("receipt.json");
    fs::write(&receipt_path, serde_json::to_vec_pretty(&receipt)?)
        .with_context(|| format!("writing receipt {}", receipt_path.display()))?;
    validate_receipt_binding(&receipt, &plan)
        .context("the emitted receipt failed its own recovery binding")?;
    Ok(RecoveryRunOutcome {
        receipt_path,
        result: judgment.result,
        process_cleanup: observation.cleanup,
        driver_complete: observation.driver_complete,
        driver_failure_reason: judgment.driver_failure_reason,
    })
}

fn recovery_limitations(
    observation: &ProcessObservation,
    judgment: &RecoveryJudgment,
    recovery_wire: &RecoveryWire,
    stimulus_records: &[StimulusRecord],
    variant: RecoveryFixtureVariant,
) -> Vec<String> {
    let mut limitations = vec![
        "headless silent-ex Vim (-es): GUI-only client surfaces are not exercised by this harness"
            .to_string(),
        format!(
            "fixture variant {}: the governed defect/clean source generations and all expectations \
             are Rust-authored, never derived from run output; the fixture digest pins the initial \
             state and the one source mutation is a typed journey event",
            variant.id()
        ),
        "restart route: the pinned client exposes no restart command; the ordinary route is the \
         public lsp#stop_server stop plus the next document open (the client's lazy start), and \
         no private process launch is ever used"
            .to_string(),
        "unexpected-exit disposition: manual_restart_required — the pinned client clears server \
         state on exit and performs no automatic retry; recovery happens through the next \
         document open, which is then proven through the complete new-generation chain"
            .to_string(),
        "config replay: the pinned client's workspace/didChangeConfiguration push is \
         registration-scoped (once per session), so replacement generations receive no client \
         config re-push; the governed source has no include-path dependency, and document and \
         root replay are the proven replay surface"
            .to_string(),
        "the top-line disposition of the canonical journey is partial by law: an unexpected exit \
         is never a passing recovery observation (#11386), so the adverse-exit cell carries the \
         honest manual_restart_required disposition while the affirming cells pass"
            .to_string(),
        "full host reopen/repeated sessions, save/activation, and maintained/public replay cells \
         are separate leaves and are not claimed here"
            .to_string(),
    ];
    if variant.expected_negative_reason().is_some() {
        limitations.push(format!(
            "negative control variant: the run must fail with the typed reason {}; a pass would \
             be an oracle violation",
            variant.expected_negative_reason().unwrap_or("unknown")
        ));
    }
    if let Some(reason) = &judgment.driver_failure_reason {
        limitations.push(format!("driver failed: {reason}"));
    }
    if judgment.wrong_initialize_root {
        limitations.push(
            "the initialize request's rootUri disagreed with the driver-observed root; the run \
             cannot claim a consistent root identity"
                .to_string(),
        );
    }
    if !stimulus_records.iter().all(|record| !record.pids.is_empty()) {
        limitations.push(format!(
            "crash stimulus ledger incomplete: {}",
            stimulus_records
                .iter()
                .filter(|record| record.pids.is_empty())
                .map(|record| record.outcome.clone())
                .collect::<Vec<_>>()
                .join("; ")
        ));
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
    limitations.push(format!(
        "wire process generations observed: {} initialize requests, {} initialized notifications, \
         {} didOpen events, {} workspace/didChangeConfiguration pushes; crash stimuli landed: {}",
        recovery_wire.initialize_lines.len(),
        recovery_wire.initialized_lines.len(),
        recovery_wire.did_open_lines.len(),
        recovery_wire.did_change_configuration_lines.len(),
        stimulus_records.iter().filter(|record| !record.pids.is_empty()).count(),
    ));
    limitations
}

// ---------------------------------------------------------------------------
// Judgment
// ---------------------------------------------------------------------------

/// The eight-cell judgment over one observed recovery run.
pub struct RecoveryJudgment {
    /// The honest top-line: `Partial` for a fully proven canonical journey
    /// (the adverse-exit cell never passes), `Fail` for typed failures and
    /// boundary violations, `NotProven` when the evidence is missing.
    pub result: ObservationResult,
    pub failure_class: Option<crate::editor_client_compat::FailureClass>,
    pub driver_failure_reason: Option<String>,
    /// The initialize request's rootUri tail disagreed with the expected
    /// governed root (typed inconsistency; cannot pass).
    pub wrong_initialize_root: bool,
    /// Per-cell results for the receipt journey, keyed by catalog cell id.
    pub cells: BTreeMap<String, ObservationResult>,
}

fn detail<'a>(events: &'a [DriverEvent], kind: DriverEventKind, key: &str) -> Option<&'a str> {
    events
        .iter()
        .find(|event| event.kind == kind)
        .and_then(|event| event.details.get(key))
        .map(String::as_str)
}

fn indexed_events(events: &[DriverEvent], kind: DriverEventKind) -> Vec<&DriverEvent> {
    let mut found: Vec<&DriverEvent> = events.iter().filter(|event| event.kind == kind).collect();
    found.sort_by_key(|event| {
        event
            .details
            .get(match kind {
                DriverEventKind::ServerRestartApplied => "restart_index",
                DriverEventKind::RecoveryStimulusApplied => "stimulus_index",
                DriverEventKind::RecoveryDispositionObserved => "disposition_index",
                DriverEventKind::GenerationReplayObserved => "replay_index",
                DriverEventKind::OldGenerationRejected => "rejection_index",
                DriverEventKind::GenerationCurrentObserved => "generation_index",
                _ => "sequence",
            })
            .and_then(|value| value.parse::<u32>().ok())
            .unwrap_or(u32::MAX)
    });
    found
}

fn cell_result(observed: bool, ok: bool) -> ObservationResult {
    if ok {
        ObservationResult::Pass
    } else if observed {
        ObservationResult::Fail
    } else {
        ObservationResult::NotProven
    }
}

/// Whether a `file://` URI ends with the expected relative directory segment,
/// on every host's path spelling (mirrors the #10946 helper).
fn uri_ends_with_segment(uri: &str, segment: &str) -> bool {
    let normalized = uri.replace('\\', "/");
    normalized.trim_end_matches('/').ends_with(&format!("/{segment}"))
        || normalized.trim_end_matches('/') == format!("file:///{segment}")
        || normalized.ends_with(segment)
}

/// The expected honest per-generation result of the governed document: the
/// old generation (and the unchanged-file restarts of it) recomputes the
/// authored defect; the post-replacement generation recomputes the authored
/// clean source.
fn generation_expects_errors(generation: &str) -> Option<bool> {
    match generation {
        "g1_defect_current" | "g2_recomputed_defect" | "g3_manual_recovery_defect" => Some(true),
        "g4_clean_current" => Some(false),
        _ => None,
    }
}

/// Judge one observed run against the scenario's Rust-authored expectations.
///
/// Canonical positive path: registration bound to the planned candidate
/// digest, native governed root with the decoy distinct, one public-route
/// explicit restart and two manual-route recoveries each proven through the
/// complete new-generation chain (initialize and initialized on the wire,
/// the client's init/buffer-enabled events, exact didOpen replay, the same
/// governed root, a settled recomputed current result), the directly
/// observed `manual_restart_required` disposition with a bounded
/// zero-retry window per exit stimulus, the wire-bounded old-generation
/// rejection, and the shutdown-during-pending observation under a clean
/// process boundary.
#[allow(clippy::too_many_lines)]
pub fn evaluate_recovery_observation(
    plan: &VimHostRunPlan,
    observation: &ProcessObservation,
    wire: &WireEvidence,
    recovery_wire: &RecoveryWire,
    stimulus_records: &[StimulusRecord],
    variant: RecoveryFixtureVariant,
) -> RecoveryJudgment {
    let mut cells = BTreeMap::new();
    let events = &observation.events;

    // --- substrate prerequisites: registration, attach, root.
    let registration_digest_match =
        detail(events, DriverEventKind::RegistrationSelected, "candidate_sha256")
            == Some(plan.identity.candidate_artifact_sha256.as_str());
    let attach_identity_observed = wire.saw_initialize && wire.saw_initialized;
    let root_event = events.iter().find(|event| event.kind == DriverEventKind::RootSelected);
    let observed_root = detail(events, DriverEventKind::RootSelected, "observed_root");
    let decoy_reported = detail(events, DriverEventKind::RootSelected, "decoy_root");
    let root_observed = root_event.is_some();
    let initialize_root_ok = wire
        .initialize_request
        .as_ref()
        .and_then(|request| request.get("params"))
        .and_then(|params| params.get("rootUri"))
        .and_then(|uri| uri.as_str())
        .is_some_and(|uri| uri_ends_with_segment(uri, GOVERNED_ROOT_REL));
    let wrong_initialize_root = root_observed && !initialize_root_ok;
    let root_ok = root_observed
        && observed_root == Some(GOVERNED_ROOT_REL)
        && decoy_reported == Some(DECOY_ROOT_REL)
        && initialize_root_ok;

    let restarts = indexed_events(events, DriverEventKind::ServerRestartApplied);
    let stimuli = indexed_events(events, DriverEventKind::RecoveryStimulusApplied);
    let dispositions = indexed_events(events, DriverEventKind::RecoveryDispositionObserved);
    let replays = indexed_events(events, DriverEventKind::GenerationReplayObserved);
    let rejections = indexed_events(events, DriverEventKind::OldGenerationRejected);
    let currents = indexed_events(events, DriverEventKind::GenerationCurrentObserved);
    let pending =
        events.iter().find(|event| event.kind == DriverEventKind::ShutdownDuringPendingObserved);

    let stimulus_ledger_ok = stimulus_ledger_is_complete(events, stimulus_records).unwrap_or(false);

    // --- new-generation initialize/readiness chain, per replacement
    // generation: initialize and initialized on the wire (outgoing only),
    // the client's own lsp_server_init event count, and its
    // lsp_buffer_enabled re-fire count, both bound by the replay
    // observation of that generation. A bare new PID satisfies nothing here
    // (the ServerInitialized/BufferEnabled barrier events are #10944
    // singletons of the first attach; replacement readiness rides the
    // adapter's own counters).
    let generation_count = recovery_wire.initialize_lines.len();
    let chain_ok = |generation: usize| -> bool {
        let Some(init_line) = recovery_wire.initialize_line_of(generation) else { return false };
        if !recovery_wire.initialized_lines.iter().any(|line| *line > init_line) {
            return false;
        }
        replays.iter().any(|event| {
            event.details.get("initialize_generation").and_then(|v| v.parse::<u32>().ok())
                == Some(generation as u32)
                && event
                    .details
                    .get("client_init_events")
                    .and_then(|v| v.parse::<u32>().ok())
                    .is_some_and(|count| count >= generation as u32)
                && event
                    .details
                    .get("buffer_enabled_events")
                    .and_then(|v| v.parse::<u32>().ok())
                    .is_some_and(|count| count >= generation as u32)
        })
    };
    let wire_generations_expected = restarts.len() + 1;
    let chains_ok = generation_count == wire_generations_expected
        && generation_count >= 2
        && (2..=generation_count).all(chain_ok);

    // --- explicit restart cell: one public-route stop+reopen replacement
    // with the old generation's termination bound, the complete chain, and
    // the exact-subject prerequisites (registration digest, attach identity,
    // governed root with the decoy distinct).
    let public_restarts = restarts
        .iter()
        .any(|event| event.details.get("route") == Some(&"public_stop_reopen".to_string()));
    let restart_observed = !restarts.is_empty();
    let restart_ok = public_restarts
        && chains_ok
        && registration_digest_match
        && attach_identity_observed
        && root_ok
        && replays.len() == restarts.len()
        && recovery_wire.initialize_lines.len() >= 2;
    cells.insert(CELL_EXPLICIT_RESTART.to_string(), cell_result(restart_observed, restart_ok));

    // --- unexpected exit cell: the adverse-exit disposition, directly
    // observed and honestly classified. This cell NEVER passes (#11386 law):
    // the honest affirming outcome is Partial — the disposition observed
    // (manual restart required, no automatic recovery), every stimulus
    // landed PID-precisely, and the manual route then proven completely.
    let exit_disposition_observed = !stimuli.is_empty() && !dispositions.is_empty();
    let exit_disposition_honest = exit_disposition_observed
        && stimulus_ledger_ok
        && dispositions.iter().all(|event| {
            event.details.get("disposition") == Some(&"manual_restart_required".to_string())
                && event.details.get("retry_count") == Some(&"0".to_string())
                && event.details.get("exit_observed") == Some(&"1".to_string())
        })
        && restarts.iter().any(|event| {
            event.details.get("route") == Some(&"manual_reopen_after_exit".to_string())
        })
        && chains_ok;
    let unexpected_exit_result = if exit_disposition_observed {
        if exit_disposition_honest { ObservationResult::Partial } else { ObservationResult::Fail }
    } else {
        ObservationResult::NotProven
    };
    cells.insert(CELL_UNEXPECTED_EXIT.to_string(), unexpected_exit_result);

    // --- initialized new generation cell: every replacement generation
    // completed the initialize/initialized/buffer-enabled readiness chain.
    let initialized_observed = recovery_wire.initialize_lines.len() >= 2;
    let initialized_ok = chains_ok;
    cells.insert(
        CELL_INITIALIZED_NEW_GENERATION.to_string(),
        cell_result(initialized_observed, initialized_ok),
    );

    // --- document replay cell: per replacement generation, exactly one
    // didOpen of the governed document after that generation's initialize,
    // the same governed root re-selected, and the directly observed config
    // fact (no client config re-push inside the session).
    let replay_observed = !replays.is_empty();
    let replay_ok = replay_ok(&replays, recovery_wire, restarts.len());
    cells.insert(CELL_DOCUMENT_REPLAY.to_string(), cell_result(replay_observed, replay_ok));

    // --- current result cell: every current observation matches its
    // generation's Rust-authored expectation in the client's own state, and
    // the settled wire batch of that generation's window agrees. A
    // clean-launch answer is not a recovery result (#11386 claim ceiling):
    // only a post-replacement current observation opens the cell.
    let post_replacement_currents = currents.iter().any(|event| {
        event.details.get("generation").map(String::as_str) != Some("g1_defect_current")
    });
    let current_observed = post_replacement_currents;
    let expected_generation_count = restarts.len() + 1;
    let current_ok = currents.len() == expected_generation_count
        && currents.iter().all(|event| {
            let generation = event.details.get("generation").map(String::as_str).unwrap_or("");
            let Some(expect_errors) = generation_expects_errors(generation) else { return false };
            let errors = event.details.get("errors").and_then(|v| v.parse::<u32>().ok());
            let warnings = event.details.get("warnings").and_then(|v| v.parse::<u32>().ok());
            match (errors, warnings) {
                (Some(errors), Some(warnings)) => {
                    warnings == 0 && (if expect_errors { errors >= 1 } else { errors == 0 })
                }
                _ => false,
            }
        })
        && current_wire_agrees(recovery_wire, expected_generation_count);
    cells.insert(CELL_CURRENT_RESULT.to_string(), cell_result(current_observed, current_ok));

    // --- old generation rejection cell: after the replacement generation's
    // initialize, no publishDiagnostics batch for the governed document
    // carries the old defect signature, and the driver observed the settled
    // quiet window.
    let rejection_observed = !rejections.is_empty();
    let replacement_initialize = recovery_wire.initialize_line_of(4);
    let old_signature_clean = replacement_initialize.is_some_and(|line| {
        recovery_wire
            .batches_after(MAIN_TOKEN, line)
            .iter()
            .all(|batch| batch.error_severity_count == 0 && batch.warning_severity_count == 0)
    });
    let rejection_ok = rejection_observed
        && old_signature_clean
        && rejections.iter().all(|event| {
            event.details.get("held_generation") == Some(&"g3_manual_recovery_defect".to_string())
                && event.details.get("released_after_generation")
                    == Some(&"g4_clean_current".to_string())
                && event.details.get("old_signature_settled") == Some(&"0".to_string())
        });
    cells.insert(
        CELL_OLD_GENERATION_REJECTED.to_string(),
        cell_result(rejection_observed, rejection_ok),
    );

    // --- retry/manual disposition cell: the bounded zero-retry windows with
    // the manual disposition, and the manual route then proven — a manual
    // restart is never relabeled automatic recovery.
    let disposition_observed = !dispositions.is_empty();
    let disposition_ok = exit_disposition_honest;
    cells.insert(
        CELL_RETRY_OR_MANUAL.to_string(),
        cell_result(disposition_observed, disposition_ok),
    );

    // --- shutdown cleanup cell: the shutdown-during-pending observation
    // under an orderly host exit and a clean deterministic process boundary.
    let shutdown_pending_observed = pending.is_some();
    let shutdown_ok =
        shutdown_pending_observed && observation.passed_process_boundary() && stimulus_ledger_ok;
    cells.insert(
        CELL_SHUTDOWN_CLEANUP.to_string(),
        cell_result(shutdown_pending_observed, shutdown_ok),
    );

    let driver_failed_event =
        events.iter().find(|event| event.kind == DriverEventKind::DriverFailed);
    let driver_failure_reason =
        driver_failed_event.and_then(|event| event.details.get("reason")).cloned();
    let leaked = observation.cleanup == CleanupResult::Fail;
    let any_cell_failed = cells.values().any(|result| *result == ObservationResult::Fail);
    let affirming_ok = [
        CELL_EXPLICIT_RESTART,
        CELL_INITIALIZED_NEW_GENERATION,
        CELL_DOCUMENT_REPLAY,
        CELL_CURRENT_RESULT,
        CELL_OLD_GENERATION_REJECTED,
        CELL_RETRY_OR_MANUAL,
        CELL_SHUTDOWN_CLEANUP,
    ]
    .iter()
    .all(|cell| cells.get(*cell) == Some(&ObservationResult::Pass));
    let adverse_honest = cells.get(CELL_UNEXPECTED_EXIT) == Some(&ObservationResult::Partial);
    let result = if observation.passed_process_boundary() && affirming_ok && adverse_honest {
        // The canonical journey's honest top-line: partial by #11386 law.
        // A negative variant that reaches it is an oracle violation.
        if variant.expected_negative_reason().is_some() {
            ObservationResult::Fail
        } else {
            ObservationResult::Partial
        }
    } else if driver_failed_event.is_some()
        || observation.timed_out
        || leaked
        || any_cell_failed
        || observation.status_code.is_some_and(|code| code != 0)
    {
        // A contradicted cell (an admitted old-generation result, a restart
        // without its wire generations, a stimulus that never landed) is a
        // failing recovery even under a clean process boundary.
        ObservationResult::Fail
    } else {
        ObservationResult::NotProven
    };
    let failure_class = if matches!(result, ObservationResult::Pass | ObservationResult::Partial) {
        None
    } else if leaked {
        Some(crate::editor_client_compat::FailureClass::Cleanup)
    } else if observation.timed_out {
        Some(crate::editor_client_compat::FailureClass::Instrument)
    } else if driver_failed_event.is_some() || observation.status_code.is_some_and(|code| code != 0)
    {
        Some(crate::editor_client_compat::FailureClass::HostClient)
    } else {
        Some(crate::editor_client_compat::FailureClass::Instrument)
    };
    RecoveryJudgment { result, failure_class, driver_failure_reason, wrong_initialize_root, cells }
}

/// The document replay law: per replacement generation, exactly one didOpen
/// of the governed token inside that generation's window (after its
/// initialize, before the next initialize), and the same governed root named
/// by every replay observation.
fn replay_ok(replays: &[&DriverEvent], recovery_wire: &RecoveryWire, restart_count: usize) -> bool {
    if replays.len() != restart_count || replays.is_empty() {
        return false;
    }
    if !replays.iter().all(|event| {
        event.details.get("document") == Some(&MAIN_TOKEN.to_string())
            && event.details.get("root") == Some(&GOVERNED_ROOT_REL.to_string())
            && event.details.get("did_open_replayed") == Some(&"1".to_string())
    }) {
        return false;
    }
    (2..=restart_count + 1).all(|generation| {
        let Some(init_line) = recovery_wire.initialize_line_of(generation) else { return false };
        let next_init = recovery_wire.initialize_line_of(generation + 1).unwrap_or(usize::MAX);
        let opens: Vec<usize> = recovery_wire
            .opens_of(MAIN_TOKEN)
            .into_iter()
            .filter(|line| *line > init_line && *line < next_init)
            .collect();
        opens.len() == 1
    })
}

/// The wire side of the current-result law: the settled batch of each
/// generation's window carries that generation's authored expectation (the
/// defect recomputed for the unchanged-file generations; the clean batch for
/// the post-replacement generation). Windows are close-bounded (#12660
/// finding: the server's on-close clearing publish is a transport artifact,
/// not a state claim) — the settled generation is the last batch before the
/// document's first didClose inside the window, or the last batch of the
/// window when no close intervenes.
fn current_wire_agrees(recovery_wire: &RecoveryWire, generation_count: usize) -> bool {
    (1..=generation_count).all(|generation| {
        let Some(init_line) = recovery_wire.initialize_line_of(generation) else { return false };
        let next_init = recovery_wire.initialize_line_of(generation + 1).unwrap_or(usize::MAX);
        let window_end = recovery_wire
            .closes_of(MAIN_TOKEN)
            .into_iter()
            .find(|close| *close > init_line && *close < next_init)
            .unwrap_or(next_init);
        let batches: Vec<&RecoveryBatch> = recovery_wire
            .batches_after(MAIN_TOKEN, init_line)
            .into_iter()
            .filter(|batch| batch.line_index < window_end)
            .collect();
        let Some(last) = batches.last() else { return false };
        let expect_errors = generation < generation_count;
        if expect_errors {
            last.error_severity_count >= 1 && last.warning_severity_count == 0
        } else {
            last.error_severity_count == 0 && last.warning_severity_count == 0
        }
    })
}

// ---------------------------------------------------------------------------
// Receipt journey
// ---------------------------------------------------------------------------

/// Compose the receipt journey: the lifecycle barrier cells (the #10944
/// surface, judged against the run's real mined wire) plus the eight #11386
/// catalog cells this scenario evidences.
pub fn recovery_journey(
    observation: &ProcessObservation,
    judgment: &RecoveryJudgment,
    wire: &WireEvidence,
) -> Vec<JourneyCell> {
    let mut cells = crate::vim_host_run::outcome_journey(observation, wire);
    let catalog_limitations: BTreeMap<&str, &str> = BTreeMap::from([
        (
            CELL_EXPLICIT_RESTART,
            "the user-initiated public-route stop+start of the exact server through the pinned \
             client's own lifecycle; a first launch, a host reopen, and another client's restart \
             can never satisfy it",
        ),
        (
            CELL_UNEXPECTED_EXIT,
            "disposition: manual_restart_required — the adverse exit was directly observed \
             (client exit evidence plus a PID-precise external stimulus), no automatic recovery \
             occurred within the bounded windows, and the manual route was then proven; an \
             unexpected exit is never a passing recovery observation",
        ),
        (
            CELL_INITIALIZED_NEW_GENERATION,
            "every replacement generation completed initialize/initialized on the wire plus the \
             client's own lsp_server_init and lsp_buffer_enabled events; a bare new PID or \
             process-start event is not initialize",
        ),
        (
            CELL_DOCUMENT_REPLAY,
            "exactly one governed-document didOpen per replacement generation window with the \
             same governed root; the pinned client pushes workspace/didChangeConfiguration once \
             per registration (no replacement re-push in-session), which is recorded as the \
             directly observed config fact",
        ),
        (
            CELL_CURRENT_RESULT,
            "every post-replacement current observation was recomputed by the then-current \
             generation — the defect returned for unchanged-file replacements and the authored \
             clean generation returned after the source mutation",
        ),
        (
            CELL_OLD_GENERATION_REJECTED,
            "after the replacement generation's initialize, no publishDiagnostics batch for the \
             governed document carries the old defect signature and the settled client state is \
             the new generation's",
        ),
        (
            CELL_RETRY_OR_MANUAL,
            "retry count zero inside every bounded observation window; the pinned client has no \
             automatic recovery, and the manual reopen route — never relabeled automatic — \
             recovered the generation",
        ),
        (
            CELL_SHUTDOWN_CLEANUP,
            "the host exited while a recovery was pending (old generation dead, replacement not \
             started) and the deterministic before/after process-set comparison proved no owned \
             perllsp process survives",
        ),
    ]);
    for (cell_id, result) in &judgment.cells {
        cells.push(JourneyCell {
            id: cell_id.clone(),
            capability_basis: CapabilityBasis::NotApplicable,
            observed: *result != ObservationResult::NotProven,
            result: *result,
            evidence: catalog_evidence(cell_id),
            limitation: if *result == ObservationResult::Pass
                || *result == ObservationResult::Partial
            {
                catalog_limitations.get(cell_id.as_str()).map(|text| text.to_string())
            } else {
                Some(format!("{cell_id} was not proven for this exact subject"))
            },
        });
    }
    cells
}

fn catalog_evidence(cell_id: &str) -> Vec<String> {
    match cell_id {
        CELL_UNEXPECTED_EXIT | CELL_RETRY_OR_MANUAL => vec![
            "vim/driver-events.jsonl".to_string(),
            "vim/vim-lsp-client.log".to_string(),
            "vim/recovery-stimulus-ledger.json".to_string(),
            "vim/process-ledger.json".to_string(),
        ],
        CELL_SHUTDOWN_CLEANUP => vec![
            "vim/driver-events.jsonl".to_string(),
            "vim/process-ledger.json".to_string(),
            "vim/recovery-stimulus-ledger.json".to_string(),
        ],
        CELL_DOCUMENT_REPLAY | CELL_CURRENT_RESULT | CELL_OLD_GENERATION_REJECTED => {
            vec!["vim/driver-events.jsonl".to_string(), "vim/vim-lsp-client.log".to_string()]
        }
        _ => vec![
            "vim/driver-events.jsonl".to_string(),
            "vim/initialize-request.json".to_string(),
            "vim/process-ledger.json".to_string(),
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recovery_variants_parse_and_carry_typed_negative_reasons() {
        assert!(matches!(
            RecoveryFixtureVariant::from_id("canonical"),
            Ok(RecoveryFixtureVariant::Canonical)
        ));
        assert!(RecoveryFixtureVariant::from_id("other").is_err());
        assert_eq!(
            RecoveryFixtureVariant::WrongRootDecoy.expected_negative_reason(),
            Some("root_mismatch")
        );
        assert_eq!(
            RecoveryFixtureVariant::AutoRecoveryClaimed.expected_negative_reason(),
            Some("automatic_recovery_absent")
        );
        assert_eq!(
            RecoveryFixtureVariant::ReplaySkippedClaimed.expected_negative_reason(),
            Some("document_replay_absent")
        );
        assert_eq!(RecoveryFixtureVariant::Canonical.expected_negative_reason(), None);
    }

    #[test]
    fn clean_text_repairs_exactly_the_mutation_line() {
        let defect = defect_source_text();
        let clean = clean_source_text();
        assert_ne!(clean, defect);
        let defect_lines: Vec<&str> = defect.lines().collect();
        let clean_lines: Vec<&str> = clean.lines().collect();
        assert_eq!(defect_lines.len(), clean_lines.len());
        for index in 0..defect_lines.len() {
            if index + 1 == MUTATION_LINE {
                assert_eq!(clean_lines[index], CLEAN_LINE_TEXT);
                assert_ne!(clean_lines[index], defect_lines[index]);
            } else {
                assert_eq!(clean_lines[index], defect_lines[index]);
            }
        }
    }

    #[test]
    fn recovery_wire_counts_generations_direction_aware() {
        let log = concat!(
            "{\"method\":\"initialize\",\"params\":{}}\n",
            "{\"method\":\"initialized\",\"params\":{}}\n",
            "{\"method\":\"textDocument/didOpen\",\"params\":{\"textDocument\":{\"uri\":\"file:///w/project/main.pl\"}}}\n",
            "{\"method\":\"textDocument/publishDiagnostics\",\"params\":{\"uri\":\"file:///w/project/main.pl\",\"diagnostics\":[{\"severity\":1}]}}\n",
            "{\"method\":\"workspace/didChangeConfiguration\",\"params\":{}}\n",
            "{\"method\":\"textDocument/didClose\",\"params\":{\"textDocument\":{\"uri\":\"file:///w/project/main.pl\"}}}\n",
            "{\"method\":\"initialize\",\"params\":{}}\n",
            "{\"method\":\"initialize\",\"params\":{}}\n"
        );
        let wire = extract_recovery_wire(log.as_bytes());
        assert_eq!(wire.initialize_lines, vec![0, 6, 7]);
        assert_eq!(wire.initialized_lines, vec![1]);
        assert_eq!(wire.opens_of("main.pl"), vec![2]);
        assert_eq!(wire.closes_of("main.pl"), vec![5]);
        assert_eq!(wire.did_change_configuration_lines, vec![4]);
        assert_eq!(wire.initialize_line_of(2), Some(6));
        let batches = wire.batches_after("main.pl", 6);
        assert!(batches.is_empty());
        assert_eq!(wire.batches_of("main.pl")[0].error_severity_count, 1);
    }
}
