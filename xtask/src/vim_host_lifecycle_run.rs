//! #11401 host-reopen lifecycle scenario for the hermetic Vim + vim-lsp host
//! runner.
//!
//! This module is the host-reopen execution consumer of the #10944/#12545
//! substrate, the #12589 scenario pattern, and the #12660 restart mechanics:
//! it proves, through the pinned actual Vim + vim-lsp + perllsp subject, the
//! eight #11387 lifecycle cells — buffer close/reopen, full host exit and
//! replacement launch, workspace/session replacement disposition, pending
//! action cancellation, late-result rejection, finite repeated sessions,
//! normal terminal cleanup, and forced-failure cleanup.
//!
//! ## Journey shape (canonical)
//!
//! The journey is a finite sequence of hermetic host sessions, each a new
//! exact subject/run instance launched through the same governed toolchain
//! roles (`bind_host_run_plan`), sharing one on-disk fixture so the disk
//! handoff between sessions is real:
//!
//! | host | role | proves |
//! | --- | --- | --- |
//! | 1 | `full_lifecycle_session` | defect state, identity-bound cancellation (`$/cancelRequest` by request id), buffer wipe/reopen with a changed document instance on an unchanged server generation, late old-document result rejection, one pending action left in flight at the user-equivalent exit |
//! | 2 | `replacement_host_session` | full host replacement: new process/host/document generations through the complete initialize sequence, disk-current (not stale) opening state, its own edit-cycle product result, and no response to host 1's in-flight request identity |
//! | 3 | `assertion_failure_session` | a typed forced assertion failure with evidence preserved before the nonzero exit |
//! | 4 | `timeout_interruption_session` | a deliberate indefinite hang bounded by the supervisor's hard deadline kill |
//!
//! The repeated-session denominator is therefore 4 independently bound
//! iterations over 3 changed host instances (>= the #11387 minimum of 2).
//!
//! ## Honest dispositions
//!
//! - **workspace/session replacement: `client_not_exposed` for this subject.**
//!   The pinned vim-lsp's own initialize request carries
//!   `workspace.workspaceFolders: false` (read from each run's wire, never a
//!   registration token), and the only workspace-folder mutation path in the
//!   pinned bytes is the private `s:workspace_add_folder` gated behind
//!   `g:lsp_experimental_workspace_folders` (default off, not enabled by this
//!   harness). No stable public concept exists to exercise, so the cell stays
//!   `unsupported` with a `client_not_exposed` limitation and is never
//!   relabeled; a client that someday exposes the surface changes this row
//!   honestly (the cell fails instead of silently passing).
//! - **server restart and buffer reopen are required false subjects** for the
//!   host-reopen cell: the judgment requires a changed host instance (new
//!   supervisor-observed process, fresh initialize sequence on the
//!   replacement's own wire). The `server_restart_relabel` negative control
//!   types exactly this absence.
//!
//! ## Fail-closed laws beyond the substrate's
//!
//! - every state observation is the client's own diagnostics state through
//!   the deterministic barrier, and every semantic claim is cross-bound to
//!   the client's own wire record (batches ordered against didOpen/didClose);
//! - a cancelled pending action proves zero admissions (the event contract
//!   itself rejects a nonzero admission count);
//! - the late old-document result must have completed (the response is mined
//!   from the client's own log) while the replacement instance stayed
//!   unchanged across a bounded window;
//! - the replacement host's opening state must equal the disk generation the
//!   supervisor itself wrote — a prior session's in-memory state appearing as
//!   the new session's opening state fails the run (stale-state falsifier);
//! - every iteration binds its own initialize chain, its own product result,
//!   and its own process ledger; a prior session's output cannot satisfy it;
//! - forced-failure cleanup is judged from observed settled process probes
//!   with retained snapshots and diagnostics — a missing probe is
//!   `not_proven`, never zero, and surviving owned processes fail the cell.

use anyhow::{Context, Result, bail, ensure};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::editor_client_compat::{
    CapabilityBasis, CleanupResult, EvidenceArtifact, JourneyCell, ObservationResult,
};
use crate::vim_host_run::vim_host_runner;
use crate::vim_host_run::{BoundHostPlan, VimHostRunInputs, bind_host_run_plan};
use vim_host_runner::{
    DriverEventKind, HermeticVimLayout, ProcessObservation, ProcessProbeLine, VimHostRunPlan,
    WireEvidence, build_vim_command_with_extras, parse_process_snapshot, probe_process_table,
    run_owned_process, validate_receipt_binding,
};

pub const LIFECYCLE_JOURNEY_SELECTOR: &str = "vim_vim_lsp_host_reopen_lifecycle.v1";
pub const LIFECYCLE_FIXTURE_ID: &str = "vim_vim_lsp_host_reopen_lifecycle_v1";

// ---------------------------------------------------------------------------
// Rust-authored fixture expectations
// ---------------------------------------------------------------------------

/// The governed fixture's stable layout, relative to the materialized fixture
/// root. Authored here, never derived from run output. The same fixture root
/// is shared by every host session of one journey so the disk handoff between
/// sessions is real; each session's plan binds the digest of the disk state it
/// actually observed at bind time.
pub const GOVERNED_ROOT_REL: &str = "workspace/project";
pub const OPENED_FILE_REL: &str = "workspace/project/main.pl";
/// The governed root marker. `cpanfile` is on the #7762 authority list.
pub const ROOT_MARKER: &str = "cpanfile";
/// Wire file-name token of the governed document (publishDiagnostics `uri`
/// tail) — the only token the judgment accepts evidence from.
pub const MAIN_TOKEN: &str = "main.pl";

/// The governed defective generation: the trailing semicolon is missing (the
/// #10946 governed defect shape). The fixture ships this generation so the
/// buffer-reopen chain observes a real old-state → disk-truth transition.
pub const MUTATION_LINE: usize = 4;
pub const DEFECT_LINE_TEXT: &str = "my $value = My::Widget::answer()";
pub const CLEAN_LINE_TEXT: &str = "my $value = My::Widget::answer();";

pub const SOURCE_LINES: [&str; 5] =
    ["use strict;", "use warnings;", "use My::Widget;", DEFECT_LINE_TEXT, "print \"$value\\n\";"];

/// The bounded observation window for late-result rejection and cancelled
/// holds (the substrate's minimum honest absence window applies).
pub const LATE_WINDOW_MS: u64 = 3000;
/// The supervisor deadline for the timeout/interruption session: short by
/// design so the forced-kill path is bounded without burning the journey
/// budget.
pub const TIMEOUT_SESSION_TIMEOUT_MS: u64 = 45_000;
/// The bounded settle window for the post-kill process probe: the forced kill
/// closes the host's channel and the owned server child settles on stdin EOF;
/// cleanup is judged from the settled probe, never from the transient.
pub const SETTLE_PROBE_WINDOW_MS: u64 = 10_000;
/// The finite repeated-session denominator of the canonical journey (>= the
/// #11387 minimum of 2 changed host instances).
pub const CANONICAL_HOST_COUNT: usize = 4;

/// The #11387 catalog cell ids this journey evidences. The catalog owns
/// registration; this scenario only cites.
pub const CELL_BUFFER_REOPEN: &str = "vim.vim_lsp.lifecycle.buffer_reopen";
pub const CELL_HOST_REOPEN: &str = "vim.vim_lsp.lifecycle.host_reopen";
pub const CELL_WORKSPACE_REOPEN: &str = "vim.vim_lsp.lifecycle.workspace_or_session_reopen";
pub const CELL_CANCELLATION: &str = "vim.vim_lsp.lifecycle.cancellation";
pub const CELL_LATE_RESULT: &str = "vim.vim_lsp.lifecycle.late_result_rejected";
pub const CELL_REPEATED_SESSIONS: &str = "vim.vim_lsp.lifecycle.repeated_sessions";
pub const CELL_NORMAL_CLEANUP: &str = "vim.vim_lsp.lifecycle.normal_cleanup";
pub const CELL_FAILURE_CLEANUP: &str = "vim.vim_lsp.lifecycle.failure_cleanup";

// ---------------------------------------------------------------------------
// Fixture variants
// ---------------------------------------------------------------------------

/// One scenario fixture variant. `Canonical` must pass; the relabel control
/// must fail with its typed judgment reason (the red-first control for the
/// server-restart false subject).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LifecycleFixtureVariant {
    Canonical,
    /// The journey attempts to satisfy host replacement through the client's
    /// own server restart inside one host: the judgment must reject it with
    /// the typed reason `host_replacement_absent` (a server restart is a
    /// required false subject, never a full host reopen).
    ServerRestartRelabel,
}

impl LifecycleFixtureVariant {
    pub fn from_id(id: &str) -> Result<Self> {
        match id {
            "canonical" => Ok(Self::Canonical),
            "server_restart_relabel" => Ok(Self::ServerRestartRelabel),
            other => bail!(
                "unknown lifecycle fixture variant {other}: known variants are canonical, \
                 server_restart_relabel"
            ),
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            Self::Canonical => "canonical",
            Self::ServerRestartRelabel => "server_restart_relabel",
        }
    }

    /// The typed judgment failure reason this variant must produce; `None`
    /// for the canonical variant, which must pass.
    pub fn expected_negative_reason(self) -> Option<&'static str> {
        match self {
            Self::Canonical => None,
            Self::ServerRestartRelabel => Some("host_replacement_absent"),
        }
    }
}

/// Materialize the #11401 governed fixture under `root`:
///
/// ```text
/// workspace/
///   project/                      <- the governed #7762 root
///     cpanfile                    <- the governed root marker
///     main.pl                     <- the governed source (defective generation)
///     lib/My/Widget.pm            <- resolvable through the registration channel
/// ```
///
/// The initial bytes are the defective generation: the buffer-reopen chain
/// establishes the old document-owned state (error present), externally
/// replaces the disk with the clean generation, and proves the reopened
/// instance reflects disk truth — not the old instance's state.
pub fn materialize_lifecycle_fixture(root: &Path) -> Result<PathBuf> {
    ensure!(root.is_absolute(), "fixture root must be absolute");
    let project = root.join("workspace/project");
    fs::create_dir_all(project.join("lib/My"))
        .with_context(|| format!("creating {}", project.join("lib/My").display()))?;
    write_lines(&project.join("main.pl"), &SOURCE_LINES)?;
    fs::write(
        project.join("lib/My/Widget.pm"),
        "package My::Widget;\nuse strict;\nuse warnings;\nsub answer { 42 }\n1;\n",
    )?;
    fs::write(
        project.join(ROOT_MARKER),
        "# vim/vim-lsp #11401 governed root marker (cpanfile per #7762)\n",
    )?;
    Ok(root.to_path_buf())
}

fn write_lines(path: &Path, lines: &[&str]) -> Result<()> {
    let mut text = String::new();
    for line in lines {
        text.push_str(line);
        text.push('\n');
    }
    fs::write(path, text).with_context(|| format!("writing {}", path.display()))
}

/// The clean generation text (the external replacement oracle), authored here
/// and delivered to the driver through the environment.
pub fn clean_source_text() -> String {
    let mut lines: Vec<String> = SOURCE_LINES.iter().map(ToString::to_string).collect();
    lines[MUTATION_LINE - 1] = CLEAN_LINE_TEXT.to_string();
    lines.join("\n")
}

/// The defective generation text as shipped on disk.
pub fn defect_source_text() -> String {
    SOURCE_LINES.join("\n")
}

/// The scenario's environment contract beyond the substrate's: the
/// Rust-authored expectations and the session role, delivered to the driver
/// (never re-derived in Vimscript).
pub fn lifecycle_env(
    role: &str,
    variant: LifecycleFixtureVariant,
) -> Vec<(std::ffi::OsString, std::ffi::OsString)> {
    let pairs = [
        ("PERLLSP_VIM_HOST_LIFECYCLE_ROLE", role.to_string()),
        ("PERLLSP_VIM_HOST_LIFECYCLE_VARIANT", variant.id().to_string()),
        ("PERLLSP_VIM_HOST_OPENED_FILE_REL", OPENED_FILE_REL.to_string()),
        ("PERLLSP_VIM_HOST_EXPECTED_ROOT_REL", GOVERNED_ROOT_REL.to_string()),
        ("PERLLSP_VIM_HOST_MUTATION_LINE", MUTATION_LINE.to_string()),
        ("PERLLSP_VIM_HOST_CLEAN_SOURCE_TEXT", clean_source_text()),
        ("PERLLSP_VIM_HOST_DEFECT_SOURCE_TEXT", defect_source_text()),
        // The single-line generation texts for the one-line buffer edit path.
        // The whole-document texts above feed whole-file replacement only; a
        // one-line `setline()` must never receive a multiline payload (it
        // would corrupt the buffer line with embedded NULs).
        ("PERLLSP_VIM_HOST_CLEAN_LINE_TEXT", CLEAN_LINE_TEXT.to_string()),
        ("PERLLSP_VIM_HOST_DEFECT_LINE_TEXT", DEFECT_LINE_TEXT.to_string()),
        ("PERLLSP_VIM_HOST_LATE_WINDOW_MS", LATE_WINDOW_MS.to_string()),
    ];
    pairs
        .into_iter()
        .map(|(key, value)| (std::ffi::OsString::from(key), std::ffi::OsString::from(value)))
        .collect()
}

// ---------------------------------------------------------------------------
// Scenario-local lifecycle wire mining
// ---------------------------------------------------------------------------

/// One mined response envelope for a pending request: the client's own log
/// line carrying the server's answer (id) for the echoed request method.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingResponse {
    pub line_index: usize,
    pub request_id: u64,
}

/// The lifecycle facts mined from the vim-lsp client log of one host session:
/// initialize count (process generations), didClose positions for the
/// governed token, response envelopes for `textDocument/documentSymbol`
/// (pending identities), and `$/cancelRequest` sends (cancellation
/// identities). Direction-aware: response envelopes embed the echoed request,
/// so client-originated facts are counted from outgoing send lines only.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LifecycleWire {
    pub initialize_count: usize,
    /// Ordered (line, token) pairs for every outgoing `textDocument/didClose`.
    pub did_close_lines: Vec<(usize, String)>,
    /// Every response envelope whose echoed request method is
    /// `textDocument/documentSymbol`, in wire order.
    pub document_symbol_responses: Vec<PendingResponse>,
    /// The request ids carried by every outgoing `$/cancelRequest`.
    pub cancel_request_ids: Vec<u64>,
}

/// Extract the lifecycle wire facts from one host session's vim-lsp client
/// log bytes. Envelope arrays carry the direction marker first (`--->`
/// client-to-server, `<---` server-to-client) with the payload as the fourth
/// element; unenveloped payloads are walked tolerantly like the substrate's
/// mining.
pub fn extract_lifecycle_wire(log: &[u8]) -> LifecycleWire {
    let text = String::from_utf8_lossy(log);
    let mut wire = LifecycleWire::default();
    for (index, line) in text.lines().enumerate() {
        let Some(value) = first_json_value(line) else { continue };
        if let serde_json::Value::Array(items) = &value
            && let Some(serde_json::Value::String(direction)) = items.first()
            && (direction == "--->" || direction == "<---")
            && let Some(payload) = items.get(3)
        {
            walk_lifecycle_value(payload, index, direction == "--->", &mut wire);
        }
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

fn walk_lifecycle_value(
    value: &serde_json::Value,
    line_index: usize,
    outgoing: bool,
    wire: &mut LifecycleWire,
) {
    match value {
        serde_json::Value::Object(map) => {
            if let Some(serde_json::Value::String(method)) = map.get("method") {
                match method.as_str() {
                    "initialize" if outgoing => wire.initialize_count += 1,
                    "textDocument/didClose" if outgoing => {
                        let token = map
                            .get("params")
                            .and_then(|params| params.get("textDocument"))
                            .and_then(|document| document.get("uri"))
                            .and_then(serde_json::Value::as_str)
                            .map(|uri| uri.rsplit('/').next().unwrap_or("").to_string());
                        if let Some(token) = token
                            && !token.is_empty()
                            && !token.contains('\\')
                        {
                            wire.did_close_lines.push((line_index, token));
                        }
                    }
                    "$/cancelRequest" if outgoing => {
                        if let Some(id) = map
                            .get("params")
                            .and_then(|params| params.get("id"))
                            .and_then(serde_json::Value::as_u64)
                        {
                            wire.cancel_request_ids.push(id);
                        }
                    }
                    _ => {}
                }
            }
            // Response envelopes: the payload carries `response` (the server's
            // answer) and `request` (the echoed original request).
            if let (Some(response), Some(request)) = (map.get("response"), map.get("request")) {
                let method =
                    request.get("method").and_then(serde_json::Value::as_str).unwrap_or("");
                if method == "textDocument/documentSymbol"
                    && let Some(id) = response.get("id").and_then(serde_json::Value::as_u64)
                {
                    wire.document_symbol_responses
                        .push(PendingResponse { line_index, request_id: id });
                }
            }
            for child in map.values() {
                walk_lifecycle_value(child, line_index, outgoing, wire);
            }
        }
        serde_json::Value::Array(items) => {
            for child in items {
                walk_lifecycle_value(child, line_index, outgoing, wire);
            }
        }
        _ => {}
    }
}

impl LifecycleWire {
    /// The response envelope line for one pending request id, if the client's
    /// own log carries the server's answer.
    pub fn response_line_of(&self, request_id: u64) -> Option<usize> {
        self.document_symbol_responses
            .iter()
            .find(|response| response.request_id == request_id)
            .map(|response| response.line_index)
    }

    /// The first didClose line for the governed token.
    pub fn first_close_line(&self, token: &str) -> Option<usize> {
        self.did_close_lines.iter().find(|(_, file)| file == token).map(|(line, _)| *line)
    }
}

// ---------------------------------------------------------------------------
// Run
// ---------------------------------------------------------------------------

/// The typed outcome of one lifecycle journey run.
pub struct LifecycleRunOutcome {
    pub receipt_path: PathBuf,
    pub result: ObservationResult,
    pub process_cleanup: CleanupResult,
    pub driver_complete: bool,
    /// The typed failure reason when the journey failed: the driver's own
    /// typed reason when the driver failed, otherwise the judgment's typed
    /// detection (for example `host_replacement_absent` on the relabel
    /// control).
    pub failure_reason: Option<String>,
}

/// One observed host session of the journey.
pub struct HostSessionRecord {
    pub index: usize,
    pub role: String,
    pub observation: ProcessObservation,
    pub wire: WireEvidence,
    pub lifecycle_wire: LifecycleWire,
    pub plan: VimHostRunPlan,
    /// The parsed `vim/process-ledger.json` artifact (supervisor-owned
    /// process identity and cleanup facts).
    pub ledger: Option<serde_json::Value>,
    /// Whether the bounded settle probe observed zero owned candidate
    /// processes after the session's terminal path.
    pub settled_probe_clean: Option<bool>,
    /// The session's initialize capability snapshot path (raw layout).
    pub capability_snapshot: PathBuf,
}

impl HostSessionRecord {
    pub fn pid(&self) -> Option<u64> {
        self.ledger.as_ref()?.get("pid").and_then(serde_json::Value::as_u64)
    }

    pub fn ledger_survivors(&self) -> Option<usize> {
        ledger_survivor_count(self.ledger.as_ref()?)
    }

    pub fn ledger_probe_available(&self) -> bool {
        self.ledger
            .as_ref()
            .and_then(|ledger| ledger.get("process_probe"))
            .and_then(serde_json::Value::as_str)
            .is_some_and(|probe| probe == "available")
    }
}

fn ledger_survivor_count(ledger: &serde_json::Value) -> Option<usize> {
    ledger.get("surviving_processes")?.as_array().map(Vec::len)
}

/// Execute one #11401 host-reopen lifecycle journey against the exact pinned
/// subject and write its canonical receipt. `variant` selects the journey;
/// only `canonical` may pass.
pub fn host_lifecycle_run(
    repo_root: &Path,
    run: &VimHostRunInputs,
    variant: LifecycleFixtureVariant,
) -> Result<LifecycleRunOutcome> {
    crate::vim_host_run::ensure_fresh_output_root(&run.out_root)?;
    fs::create_dir_all(&run.out_root)
        .with_context(|| format!("creating output root {}", run.out_root.display()))?;

    let driver = repo_root.join("scripts/test/vim-host-lifecycle-driver.vim");
    let fixture_root = materialize_lifecycle_fixture(&run.out_root.join("fixture"))?;

    // The ambient process baseline captured before the first host spawns. The
    // settle probe diffs every late snapshot against it (never an empty
    // baseline): the supervisor's own `--candidate <abs-path>` argument would
    // otherwise present as a perpetual survivor (CI run 33036254312), and any
    // needle-matching live process at settle time that was already running
    // before this journey must not be attributed to it either. A missing or
    // unparseable baseline is an instrument gap: forced-shape settles stay
    // honestly `not_proven`, never zero.
    let ambient_baseline: Option<Vec<ProcessProbeLine>> = match probe_process_table() {
        Some(Ok(text)) => parse_process_snapshot(&text).ok(),
        _ => None,
    };

    let sessions = match variant {
        LifecycleFixtureVariant::Canonical => {
            let roles = [
                "full_lifecycle_session",
                "replacement_host_session",
                "assertion_failure_session",
                "timeout_interruption_session",
            ];
            let mut sessions = Vec::with_capacity(roles.len());
            for (index, role) in roles.iter().enumerate() {
                sessions.push(run_host_session(
                    repo_root,
                    run,
                    &driver,
                    &fixture_root,
                    index + 1,
                    role,
                    variant,
                    ambient_baseline.as_deref(),
                )?);
            }
            sessions
        }
        LifecycleFixtureVariant::ServerRestartRelabel => vec![run_host_session(
            repo_root,
            run,
            &driver,
            &fixture_root,
            1,
            "server_restart_relabel_session",
            variant,
            ambient_baseline.as_deref(),
        )?],
    };

    let judgment = evaluate_lifecycle_observation(&sessions, variant);

    // The journey receipt is bound to the first session's plan: every
    // binding-checked identity (vim build, vim-lsp commit, candidate artifact,
    // driver, adapter, fixture bytes at bind time) is shared by construction,
    // and each session's own plan and ledger are retained under its host
    // root.
    let first_session = &sessions[0];
    let first_plan = first_session.plan.clone();
    let journey_observation = aggregate_journey_observation(&sessions);

    let snapshot = first_session.capability_snapshot.clone();
    let snapshot_sha256 =
        if snapshot.is_file() { Some(vim_host_runner::file_sha256(&snapshot)?) } else { None };
    let capabilities =
        vim_host_runner::capabilities_from_wire_evidence(&first_session.wire, snapshot_sha256)?;
    let diagnostics = vim_host_runner::diagnostics_from_wire_evidence(&first_session.wire);

    let mut limitations = lifecycle_limitations(&sessions, &judgment, variant);
    if first_plan.identity.platform.os == "windows" {
        limitations.push(
            "windows is a local probe platform for this harness; the maintained CI host row is \
             linux (vim availability and process probes are best-effort on windows)"
                .to_string(),
        );
    }

    let receipt = vim_host_runner::build_receipt(
        &first_plan,
        &journey_observation,
        capabilities,
        diagnostics,
        lifecycle_journey(&sessions, &judgment),
        judgment.result,
        judgment.failure_class,
        limitations,
        format!(
            "#11401 {LIFECYCLE_JOURNEY_SELECTOR}: full host reopen, cancellation, repeated \
             sessions, and terminal cleanup for the exact pinned subject only; the lifecycle \
             cells are owned by the #11387 catalog"
        ),
    );
    let receipt_path = run.out_root.join("receipt.json");
    fs::write(&receipt_path, serde_json::to_vec_pretty(&receipt)?)
        .with_context(|| format!("writing receipt {}", receipt_path.display()))?;
    validate_receipt_binding(&receipt, &first_plan)
        .context("the emitted receipt failed its own lifecycle binding")?;
    Ok(LifecycleRunOutcome {
        receipt_path,
        result: judgment.result,
        process_cleanup: journey_observation.cleanup,
        driver_complete: judgment.all_hosts_as_designed,
        failure_reason: judgment.failure_reason,
    })
}

/// Launch and observe one hermetic host session of the journey.
fn run_host_session(
    repo_root: &Path,
    journey_run: &VimHostRunInputs,
    driver: &Path,
    fixture_root: &Path,
    index: usize,
    role: &str,
    variant: LifecycleFixtureVariant,
    ambient_baseline: Option<&[ProcessProbeLine]>,
) -> Result<HostSessionRecord> {
    let host_root = journey_run.out_root.join(format!("host-{index}"));
    crate::vim_host_run::ensure_fresh_output_root(&host_root)?;
    fs::create_dir_all(&host_root)
        .with_context(|| format!("creating host root {}", host_root.display()))?;
    let host_run = VimHostRunInputs {
        vim_executable: journey_run.vim_executable.clone(),
        vim_lsp_checkout: journey_run.vim_lsp_checkout.clone(),
        candidate_executable: journey_run.candidate_executable.clone(),
        out_root: host_root.clone(),
        timeout_ms: if role == "timeout_interruption_session" {
            TIMEOUT_SESSION_TIMEOUT_MS
        } else {
            journey_run.timeout_ms
        },
    };
    let BoundHostPlan { plan, server_name, root_markers } = bind_host_run_plan(
        repo_root,
        &host_run,
        driver,
        fixture_root,
        LIFECYCLE_JOURNEY_SELECTOR,
        LIFECYCLE_FIXTURE_ID,
    )?;
    let layout = HermeticVimLayout::prepare(&host_root.join("hermetic"))?;
    let mut command = build_vim_command_with_extras(
        &plan,
        &layout,
        &server_name,
        &root_markers,
        &lifecycle_env(role, variant),
    )?;
    let observation = run_owned_process(&mut command, &plan, &layout)?;

    let client_log_bytes = fs::read(layout.client_log()).unwrap_or_default();
    let wire = vim_host_runner::extract_wire_evidence(&client_log_bytes);
    let lifecycle_wire = extract_lifecycle_wire(&client_log_bytes);

    // The bounded settle probe: after a forced or abnormal terminal path the
    // owned server child settles on stdin EOF; cleanup is judged from the
    // settled process set, never from the transient immediately after the
    // kill. Retained next to the substrate's own snapshots.
    let settled_probe_clean = settle_probe(&plan, &layout, ambient_baseline)?;
    let ledger = fs::read_to_string(layout.artifact_directory.join("vim/process-ledger.json"))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok());

    Ok(HostSessionRecord {
        index,
        role: role.to_string(),
        observation,
        wire,
        lifecycle_wire,
        plan,
        ledger,
        settled_probe_clean,
        capability_snapshot: layout.capability_snapshot(),
    })
}

/// Probe the process table until the exact candidate needle disappears or the
/// bounded settle window closes. Returns `None` when the probe is
/// unavailable on this platform (honest not-proven), `Some(false)` when owned
/// candidate processes survive the settled window.
fn settle_probe(
    plan: &VimHostRunPlan,
    layout: &HermeticVimLayout,
    ambient_baseline: Option<&[ProcessProbeLine]>,
) -> Result<Option<bool>> {
    // A missing or unparseable journey-level baseline is an instrument gap:
    // the survivor diff below cannot attribute ownership, so the settle stays
    // `not_proven` instead of silently comparing against an empty baseline.
    let Some(ambient_baseline) = ambient_baseline else { return Ok(None) };
    let needle = if cfg!(windows) {
        plan.paths
            .candidate_executable
            .file_name()
            .and_then(std::ffi::OsStr::to_str)
            .unwrap_or("perllsp")
            .to_string()
    } else {
        vim_host_runner::vim_path(&plan.paths.candidate_executable).to_string_lossy().into_owned()
    };
    let Some(probe) = probe_process_table() else { return Ok(None) };
    let Ok(text) = probe else { return Ok(None) };
    let mut settled = text.clone();
    let deadline = Instant::now() + Duration::from_millis(SETTLE_PROBE_WINDOW_MS);
    loop {
        let parsed = if cfg!(windows) {
            vim_host_runner::parse_windows_process_snapshot(&settled)
        } else {
            vim_host_runner::parse_process_snapshot(&settled)
        };
        let survivors = match parsed {
            // Same law as the substrate's comparison: a late snapshot line is
            // a survivor only when it matches the candidate needle AND was
            // not already running in the ambient baseline. This keeps the
            // supervisor's own argument vector (which contains the absolute
            // candidate path) from masquerading as an owned leak.
            Ok(lines) => vim_host_runner::surviving_processes(ambient_baseline, &lines, &needle),
            Err(_) => return Ok(None),
        };
        if survivors.is_empty() {
            let destination = layout.raw_directory.join("processes-settled.txt");
            fs::write(&destination, &settled)
                .with_context(|| format!("writing {}", destination.display()))?;
            return Ok(Some(true));
        }
        if Instant::now() >= deadline {
            let destination = layout.raw_directory.join("processes-settled.txt");
            fs::write(&destination, &settled)
                .with_context(|| format!("writing {}", destination.display()))?;
            return Ok(Some(false));
        }
        std::thread::sleep(Duration::from_millis(500));
        if let Some(Ok(next)) = probe_process_table() {
            settled = next;
        }
    }
}

/// Compose the journey-level observation the receipt is built from: the
/// aggregate cleanup (worst of), the prefixed per-host artifacts, and the
/// designed-boundary driver-completeness.
///
/// The aggregate is role-aware. Orderly-shape roles are judged through the
/// substrate's own shutdown-path comparison as-is. Forced-shape roles
/// (`assertion_failure_session`, `timeout_interruption_session`) can never
/// produce a substrate `pass`: a nonzero exit or supervisor kill skips the
/// driver shutdown path by design (#10944 degradation), so their owned-resource
/// claim rests on the dedicated bounded settle probe and retained ledger. A
/// clean settlement maps to `pass`, an observed survivor to `fail`, and an
/// unavailable probe to `not_proven` — never silently zero. Raw per-session
/// truths stay visible in the receipt limitations either way.
pub fn forced_shape_settled(session: &HostSessionRecord) -> Option<(CleanupResult, &'static str)> {
    match session.settled_probe_clean {
        Some(true) => Some((CleanupResult::Pass, "forced shape settled to zero owned processes")),
        Some(false) => Some((
            CleanupResult::Fail,
            "forced shape retained owned processes through the settle window",
        )),
        None => None,
    }
}

pub fn aggregate_journey_observation(sessions: &[HostSessionRecord]) -> ProcessObservation {
    let mut cleanup = CleanupResult::Pass;
    let mut details = Vec::new();
    for session in sessions {
        let contributed = match session.role.as_str() {
            "assertion_failure_session" | "timeout_interruption_session" => {
                forced_shape_settled(session)
            }
            _ => None,
        };
        let (session_cleanup, session_detail) = match contributed {
            Some((result, detail)) => (result, detail.to_string()),
            None => (session.observation.cleanup, session.observation.cleanup_detail.clone()),
        };
        if session_cleanup == CleanupResult::Fail || cleanup == CleanupResult::Fail {
            cleanup = CleanupResult::Fail;
        } else if session_cleanup == CleanupResult::NotProven || cleanup == CleanupResult::NotProven
        {
            cleanup = CleanupResult::NotProven;
        }
        details.push(format!(
            "host-{} ({}): {} ({})",
            session.index,
            session.role,
            match session_cleanup {
                CleanupResult::Pass => "pass",
                CleanupResult::Fail => "fail",
                CleanupResult::NotProven => "not_proven",
            },
            session_detail,
        ));
    }
    let artifacts: Vec<EvidenceArtifact> = sessions
        .iter()
        .flat_map(|session| {
            session.observation.artifacts.iter().map(move |artifact| EvidenceArtifact {
                kind: artifact.kind,
                id: format!("host-{}/{}", session.index, artifact.id),
                sha256: artifact.sha256.clone(),
            })
        })
        .collect();
    // Status/timeline fields carry no journey claim here: the per-host
    // ledgers retain each session's real process boundary.
    ProcessObservation {
        status_code: None,
        timed_out: false,
        kill_requested: false,
        cleanup,
        cleanup_detail: details.join("; "),
        events: Vec::new(),
        driver_complete: sessions.iter().all(|session| session.observation.driver_complete),
        artifacts,
    }
}

fn lifecycle_limitations(
    sessions: &[HostSessionRecord],
    judgment: &LifecycleJudgment,
    variant: LifecycleFixtureVariant,
) -> Vec<String> {
    let mut limitations = vec![
        "headless silent-ex Vim (-es): GUI-only client surfaces are not exercised by this harness"
            .to_string(),
        format!(
            "journey variant {}: {} host sessions over a shared fixture; every session binds \
             its own initialize chain, product result, and process ledger; the disk generation \
             each session opened was written by the supervisor, never inherited from a prior \
             session's memory or receipt",
            variant.id(),
            sessions.len()
        ),
        "the pending observation is the client's public request path (lsp#request_with_context \
         over textDocument/documentSymbol): cancellation is identity-bound through the client's \
         own lsp#cancel_request ($/cancelRequest by request id) and admission is observed \
         through the subscription the client itself delivers to"
            .to_string(),
        "workspace/session replacement stays client_not_exposed for this subject: the pinned \
         client's own initialize request carries workspace.workspaceFolders=false and its only \
         mutation path is the private experimental s:workspace_add_folder behind a default-off \
         flag this harness does not enable"
            .to_string(),
        "late-result rejection at the document boundary records the response's wire ordering \
         relative to the didClose honestly; the replacement instance is proven unaffected \
         through its own didOpen-settled state and content, never by ignoring the delivery"
            .to_string(),
        "the timeout session's cleanup is judged from a bounded settled process probe after the \
         supervisor kill (the owned server child settles on stdin EOF); the substrate's \
         immediate post-kill comparison is retained alongside it"
            .to_string(),
        "restart/crash recovery, save/format/activation, navigation, and maintained/public \
         replay cells are separate leaves and are not claimed here"
            .to_string(),
    ];
    if let Some(reason) = &judgment.failure_reason {
        limitations.push(format!("journey failed: {reason}"));
    }
    if judgment.cells.get(CELL_FAILURE_CLEANUP).is_some_and(|result| {
        matches!(result, ObservationResult::Fail | ObservationResult::NotProven)
    }) {
        // A failing forced-failure cell must name its failing clause: an
        // opaque fail would hide whether the defect is a leak (product), a
        // missed settle (instrument), or a lost ledger (reporting).
        let facts = failure_cleanup_facts(sessions);
        let settles = sessions
            .iter()
            .filter(|session| {
                matches!(
                    session.role.as_str(),
                    "assertion_failure_session" | "timeout_interruption_session"
                )
            })
            .map(|session| {
                format!(
                    "host-{}={}",
                    session.index,
                    match session.settled_probe_clean {
                        Some(true) => "settled",
                        Some(false) => "survivors",
                        None => "probe_unavailable",
                    }
                )
            })
            .collect::<Vec<_>>()
            .join(" ");
        let settled_zero_text = if facts.probe_missing {
            "false (probe unavailable)".to_string()
        } else {
            facts.settled_zero.to_string()
        };
        limitations.push(format!(
            "failure_cleanup facts: assertion_typed={} timeout_bounded={} ledgers_retained={} \
             settled_zero={} [{}]",
            facts.assertion_typed,
            facts.timeout_bounded,
            facts.ledgers_retained,
            settled_zero_text,
            if settles.is_empty() { "no forced sessions" } else { &settles },
        ));
    }
    for session in sessions {
        if session.observation.cleanup != CleanupResult::Pass {
            limitations.push(format!(
                "host-{} cleanup {} ({})",
                session.index,
                match session.observation.cleanup {
                    CleanupResult::Pass => "pass",
                    CleanupResult::Fail => "fail",
                    CleanupResult::NotProven => "not_proven",
                },
                session.observation.cleanup_detail
            ));
        }
    }
    limitations.push(format!(
        "wire generations observed: {}",
        sessions
            .iter()
            .map(|session| format!(
                "host-{} initialize={} didClose={} symbolResponses={}",
                session.index,
                session.lifecycle_wire.initialize_count,
                session.lifecycle_wire.did_close_lines.len(),
                session.lifecycle_wire.document_symbol_responses.len()
            ))
            .collect::<Vec<_>>()
            .join(", ")
    ));
    limitations
}

// ---------------------------------------------------------------------------
// Judgment
// ---------------------------------------------------------------------------

/// The eight-cell judgment over one observed lifecycle journey.
pub struct LifecycleJudgment {
    pub result: ObservationResult,
    pub failure_class: Option<crate::editor_client_compat::FailureClass>,
    /// The typed failure reason: the driver's own typed reason when a driver
    /// failed, otherwise the judgment's detection.
    pub failure_reason: Option<String>,
    /// Whether every host session ended through its designed terminal path.
    pub all_hosts_as_designed: bool,
    /// The client's own offered workspace-folders capability value read from
    /// the first session's initialize request (`None` when absent).
    pub workspace_folders_offered: Option<bool>,
    /// Per-cell results for the receipt journey, keyed by catalog cell id.
    pub cells: BTreeMap<String, ObservationResult>,
}

fn detail<'a>(
    events: &'a [vim_host_runner::DriverEvent],
    kind: DriverEventKind,
    key: &str,
) -> Option<&'a str> {
    events
        .iter()
        .find(|event| event.kind == kind)
        .and_then(|event| event.details.get(key))
        .map(String::as_str)
}

fn events_of_kind(
    events: &[vim_host_runner::DriverEvent],
    kind: DriverEventKind,
) -> Vec<&vim_host_runner::DriverEvent> {
    events.iter().filter(|event| event.kind == kind).collect()
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

/// Whether a host session's root observation selected the governed root.
fn session_root_ok(session: &HostSessionRecord) -> bool {
    let events = &session.observation.events;
    detail(events, DriverEventKind::RootSelected, "observed_root") == Some(GOVERNED_ROOT_REL)
        && detail(events, DriverEventKind::RootSelected, "root_source")
            == Some("activation_root_marker")
}

/// Whether the session completed the substrate attach barriers: registration
/// bound to the planned candidate, initialize/initialized on its own wire,
/// native buffer attachment.
fn session_attach_ok(session: &HostSessionRecord) -> bool {
    let events = &session.observation.events;
    detail(events, DriverEventKind::RegistrationSelected, "candidate_sha256")
        == Some(session.plan.identity.candidate_artifact_sha256.as_str())
        && session.wire.saw_initialize
        && session.wire.saw_initialized
        && events.iter().any(|event| {
            event.kind == DriverEventKind::BufferEnabled
                && event.details.get("detection") == Some(&"native_vim".to_string())
        })
}

/// Judge one observed journey against the scenario's Rust-authored
/// expectations. The canonical positive path requires every actionable cell
/// to pass through its exact evidence chain and the workspace cell to carry
/// its honest not-exposed disposition; the relabel control must fail with
/// exactly `host_replacement_absent`.
#[allow(clippy::too_many_lines)]
/// The named law inputs of the `failure_cleanup` cell. Both the judgment and
/// the receipt limitations read these, so a failing cell always carries its
/// exact failing clause — an opaque `fail` would be undiagnosable evidence.
struct FailureCleanupFacts {
    /// A forced-failure session ran at all (else the shape is unobserved).
    observed: bool,
    /// The assertion session exited typed: status 2, a `driver_failed` event
    /// carrying its reason, and retained event evidence.
    assertion_typed: bool,
    /// The timeout session was bounded by the supervisor kill.
    timeout_bounded: bool,
    /// Every settle probe reported zero owned survivors.
    settled_zero: bool,
    /// Every forced-shape ledger was retained for judgment.
    ledgers_retained: bool,
    /// Any settle probe was unavailable: an instrument gap is `not_proven`,
    /// never zero.
    probe_missing: bool,
}

fn failure_cleanup_facts(sessions: &[HostSessionRecord]) -> FailureCleanupFacts {
    let forced: Vec<&HostSessionRecord> = sessions
        .iter()
        .filter(|session| {
            matches!(
                session.role.as_str(),
                "assertion_failure_session" | "timeout_interruption_session"
            )
        })
        .collect();
    let assertion_typed = forced.iter().any(|session| {
        session.role == "assertion_failure_session"
            && session.observation.status_code == Some(2)
            && session.observation.events.iter().any(|event| {
                event.kind == DriverEventKind::DriverFailed
                    && event.details.get("reason") == Some(&"forced_assertion_failure".to_string())
            })
            && !session.observation.events.is_empty()
    });
    let timeout_bounded = forced.iter().any(|session| {
        session.role == "timeout_interruption_session"
            && session.observation.timed_out
            && session.observation.kill_requested
    });
    let ledgers_retained = forced.iter().all(|session| session.ledger.is_some());
    let probe_missing = forced.iter().any(|session| session.settled_probe_clean.is_none());
    FailureCleanupFacts {
        observed: !forced.is_empty(),
        assertion_typed,
        timeout_bounded,
        settled_zero: !probe_missing
            && forced.iter().all(|session| session.settled_probe_clean == Some(true)),
        ledgers_retained,
        probe_missing,
    }
}

pub fn evaluate_lifecycle_observation(
    sessions: &[HostSessionRecord],
    variant: LifecycleFixtureVariant,
) -> LifecycleJudgment {
    let mut cells = BTreeMap::new();
    if sessions.is_empty() {
        for cell in [
            CELL_BUFFER_REOPEN,
            CELL_HOST_REOPEN,
            CELL_WORKSPACE_REOPEN,
            CELL_CANCELLATION,
            CELL_LATE_RESULT,
            CELL_REPEATED_SESSIONS,
            CELL_NORMAL_CLEANUP,
            CELL_FAILURE_CLEANUP,
        ] {
            cells.insert(cell.to_string(), ObservationResult::NotProven);
        }
        return LifecycleJudgment {
            result: ObservationResult::NotProven,
            failure_class: Some(crate::editor_client_compat::FailureClass::Instrument),
            failure_reason: None,
            all_hosts_as_designed: false,
            workspace_folders_offered: None,
            cells,
        };
    }
    let first = sessions.first();
    // The journey's typed failure reason: a driver failure outside the
    // designed forced-failure sessions (whose typed failures are the
    // failure-cleanup evidence, not journey failures), or the judgment's own
    // detection on the relabel control.
    let failure_reason = sessions
        .iter()
        .filter(|session| session.role != "assertion_failure_session")
        .find_map(|session| {
            session
                .observation
                .events
                .iter()
                .find(|event| event.kind == DriverEventKind::DriverFailed)
                .and_then(|event| event.details.get("reason").cloned())
        });

    // --- workspace disposition: read from the first session's own wire.
    let workspace_folders_offered = first.and_then(|session| {
        session.wire.client_capabilities.as_ref().and_then(|capabilities| {
            capabilities
                .get("workspace")
                .and_then(|workspace| workspace.get("workspaceFolders"))
                .and_then(serde_json::Value::as_bool)
        })
    });
    let workspace_probe_observed = first.is_some_and(|session| session.wire.saw_initialize);
    let workspace_not_exposed = workspace_folders_offered.is_none_or(|offered| !offered);
    cells.insert(
        CELL_WORKSPACE_REOPEN.to_string(),
        if workspace_probe_observed && workspace_not_exposed {
            ObservationResult::Unsupported
        } else if workspace_probe_observed {
            // A client that exposes the surface is a different route row:
            // relabeling it as not-exposed (or passing it through synthetic
            // traffic) is forbidden.
            ObservationResult::Fail
        } else {
            ObservationResult::NotProven
        },
    );

    // --- buffer close/reopen (host 1): old defect state observed, didClose
    // on the wire, a changed document instance on the same server generation,
    // and the reopened instance settled clean through its own push.
    let host1 = first;
    let empty_events: Vec<vim_host_runner::DriverEvent> = Vec::new();
    let buffer_events = host1
        .map(|session| session.observation.events.as_slice())
        .unwrap_or(empty_events.as_slice());
    let defect_observed = events_of_kind(buffer_events, DriverEventKind::GenerationCurrentObserved)
        .iter()
        .any(|event| {
            event.details.get("generation") == Some(&"defect_present".to_string())
                && event
                    .details
                    .get("errors")
                    .and_then(|value| value.parse::<u32>().ok())
                    .is_some_and(|errors| errors >= 1)
        });
    let wipe = events_of_kind(buffer_events, DriverEventKind::BufferWiped).first().copied();
    let reopen = events_of_kind(buffer_events, DriverEventKind::BufferReopened).first().copied();
    let host1_init_count = host1.map(|session| session.lifecycle_wire.initialize_count);
    let reopen_server_unchanged = reopen.is_some_and(|event| {
        event.details.get("server_init_count").map(String::as_str) == Some("1")
    }) && host1_init_count == Some(1);
    let instance2_clean = events_of_kind(buffer_events, DriverEventKind::GenerationCurrentObserved)
        .iter()
        .any(|event| {
            event.details.get("generation") == Some(&"instance2_clean".to_string())
                && event.details.get("errors") == Some(&"0".to_string())
                && event.details.get("warnings") == Some(&"0".to_string())
        });
    let host1_close_line =
        host1.and_then(|session| session.lifecycle_wire.first_close_line(MAIN_TOKEN));
    let didclose_on_wire = host1_close_line.is_some();
    let buffer_reopen_observed = defect_observed && (wipe.is_some() || reopen.is_some());
    let buffer_reopen_ok = defect_observed
        && wipe.is_some()
        && reopen.is_some()
        && didclose_on_wire
        && reopen_server_unchanged
        && instance2_clean;
    cells.insert(
        CELL_BUFFER_REOPEN.to_string(),
        cell_result(buffer_reopen_observed, buffer_reopen_ok),
    );

    // --- cancellation (host 1): pending started with identity, cancelled by
    // identity on the wire, zero admissions.
    let pending_cancel =
        events_of_kind(buffer_events, DriverEventKind::PendingActionCancelled).first().copied();
    let pending1_started = events_of_kind(buffer_events, DriverEventKind::PendingActionStarted)
        .iter()
        .any(|event| event.details.get("pending_index") == Some(&"1".to_string()));
    let cancel_identity_match = pending_cancel.zip(host1).is_some_and(|(event, session)| {
        let started_id =
            events_of_kind(&session.observation.events, DriverEventKind::PendingActionStarted)
                .iter()
                .find(|event| event.details.get("pending_index") == Some(&"1".to_string()))
                .and_then(|event| event.details.get("request_id").cloned());
        let started_id_echo = started_id.clone();
        started_id.is_some_and(|id| event.details.get("request_id") == Some(&id))
            && session
                .lifecycle_wire
                .cancel_request_ids
                .iter()
                .any(|cancelled| Some(cancelled.to_string()) == started_id_echo)
    });
    let cancellation_observed = pending1_started || pending_cancel.is_some();
    let cancellation_ok = pending1_started && pending_cancel.is_some() && cancel_identity_match;
    cells
        .insert(CELL_CANCELLATION.to_string(), cell_result(cancellation_observed, cancellation_ok));

    // --- late-result rejection: document route (host 1) plus host route
    // (host 1 pending at exit, host 2 fresh chain).
    let late_event =
        events_of_kind(buffer_events, DriverEventKind::LateResultRejected).first().copied();
    let pending2_id = host1.and_then(|session| {
        events_of_kind(&session.observation.events, DriverEventKind::PendingActionStarted)
            .iter()
            .find(|event| event.details.get("pending_index") == Some(&"2".to_string()))
            .and_then(|event| event.details.get("request_id").cloned())
            .and_then(|id| id.parse::<u64>().ok())
    });
    // The document route: the old operation's response must be mined from the
    // wire AND must arrive strictly after the governed `didClose` — a response
    // that precedes the document invalidation is not a late result.
    let response_mined_after_close = pending2_id.zip(host1).is_some_and(|(id, session)| {
        host1_close_line.is_some_and(|close_line| {
            session
                .lifecycle_wire
                .response_line_of(id)
                .is_some_and(|response_line| response_line > close_line)
        })
    });
    let host2 = sessions.get(1);
    // The host route: pending action 3 must be bound to its wire request
    // identity and that identity must still be UNANSWERED when host 1 exits —
    // a fast response between the event and shutdown means no work was ever
    // in flight across the host boundary.
    let pending3_id = host1.and_then(|session| {
        events_of_kind(&session.observation.events, DriverEventKind::PendingActionStarted)
            .iter()
            .find(|event| event.details.get("pending_index") == Some(&"3".to_string()))
            .and_then(|event| event.details.get("request_id").cloned())
            .and_then(|id| id.parse::<u64>().ok())
    });
    let pending3_unresolved_at_exit = pending3_id
        .zip(host1)
        .is_some_and(|(id, session)| session.lifecycle_wire.response_line_of(id).is_none());
    let replacement_chain_own = host2.is_some_and(|session| {
        session_attach_ok(session)
            && session.lifecycle_wire.initialize_count == 1
            && events_of_kind(
                &session.observation.events,
                DriverEventKind::GenerationCurrentObserved,
            )
            .iter()
            .any(|event| {
                event.details.get("generation") == Some(&"replacement_open_clean".to_string())
                    && event.details.get("errors") == Some(&"0".to_string())
            })
    });
    let late_result_observed = late_event.is_some() || pending3_unresolved_at_exit;
    let late_result_ok = late_event.is_some()
        && response_mined_after_close
        && pending3_unresolved_at_exit
        && replacement_chain_own;
    cells.insert(CELL_LATE_RESULT.to_string(), cell_result(late_result_observed, late_result_ok));

    // --- full host reopen: an orderly user-equivalent exit of host 1 plus a
    // replacement host with a changed process identity and the complete
    // initialize sequence on its own wire.
    let host1_exit_ok = host1.is_some_and(|session| {
        session.observation.status_code == Some(0)
            && session.observation.cleanup == CleanupResult::Pass
            && buffer_events.iter().any(|event| event.kind == DriverEventKind::HostExitInitiated)
    });
    let host_identity_changed =
        match (host1.and_then(HostSessionRecord::pid), host2.and_then(HostSessionRecord::pid)) {
            (Some(first_pid), Some(second_pid)) => first_pid != second_pid,
            _ => false,
        };
    let host_reopen_observed = host1_exit_ok || host2.is_some();
    let host_reopen_ok = host1_exit_ok
        && host_identity_changed
        && replacement_chain_own
        && host1.is_some_and(session_attach_ok);
    cells.insert(CELL_HOST_REOPEN.to_string(), cell_result(host_reopen_observed, host_reopen_ok));

    // --- repeated sessions: finite denominator over changed host instances,
    // per-iteration fresh results, per-iteration ledgers, no stale opening
    // state. Every consecutive session pair must be a changed host instance
    // (the sequence is a chain of replacements, not a re-run of one host),
    // and the denominator carries at least two replacement transitions.
    let iterations = sessions.len();
    let transitions = sessions.len().saturating_sub(1);
    let unchanged_pairs = sessions
        .windows(2)
        .filter(|pair| {
            matches!((pair[0].pid(), pair[1].pid()), (Some(first_pid), Some(second_pid)) if first_pid == second_pid)
        })
        .count();
    let all_hosts_distinct = transitions > 0
        && unchanged_pairs == 0
        && sessions.iter().all(|session| session.pid().is_some());
    let per_iteration_result = sessions.iter().all(|session| {
        session_attach_ok(session)
            && session_root_ok(session)
            && events_of_kind(&session.observation.events, DriverEventKind::SessionIterationSettled)
                .iter()
                .any(|event| event.details.contains_key("product_result"))
            && events_of_kind(
                &session.observation.events,
                DriverEventKind::GenerationCurrentObserved,
            )
            .iter()
            .any(|event| event.details.get("state_source") == Some(&"client_state".to_string()))
    });
    let ledgers_retained = sessions.iter().all(|session| session.ledger.is_some());
    let no_stale_opening = host2.is_none_or(|session| {
        events_of_kind(&session.observation.events, DriverEventKind::GenerationCurrentObserved)
            .iter()
            .any(|event| {
                event.details.get("generation") == Some(&"replacement_open_clean".to_string())
                    && event.details.get("errors") == Some(&"0".to_string())
            })
    });
    let repeated_observed = iterations >= 2;
    let repeated_ok = iterations >= CANONICAL_HOST_COUNT.min(2)
        && transitions >= 2
        && all_hosts_distinct
        && per_iteration_result
        && ledgers_retained
        && no_stale_opening;
    cells.insert(CELL_REPEATED_SESSIONS.to_string(), cell_result(repeated_observed, repeated_ok));

    // --- normal cleanup: the orderly sessions exited through the
    // user-equivalent path with observed clean process comparisons and
    // retained snapshots. A missing retained ledger is an instrument gap
    // (not_proven), never zero.
    let normal_sessions: Vec<&HostSessionRecord> = sessions
        .iter()
        .filter(|session| {
            matches!(session.role.as_str(), "full_lifecycle_session" | "replacement_host_session")
        })
        .collect();
    let normal_cleanup_observed = !normal_sessions.is_empty();
    let normal_ledger_missing = normal_sessions.iter().any(|session| session.ledger.is_none());
    let normal_cleanup_ok = !normal_sessions.is_empty()
        && normal_sessions.iter().all(|session| {
            session.observation.status_code == Some(0)
                && session.observation.cleanup == CleanupResult::Pass
                && session.observation.driver_complete
                && session
                    .observation
                    .events
                    .iter()
                    .any(|event| event.kind == DriverEventKind::HostExitInitiated)
                && session.ledger_probe_available()
                && session.ledger_survivors() == Some(0)
        });
    let normal_cleanup_result = if !normal_cleanup_observed || normal_ledger_missing {
        ObservationResult::NotProven
    } else if normal_cleanup_ok {
        ObservationResult::Pass
    } else {
        ObservationResult::Fail
    };
    cells.insert(CELL_NORMAL_CLEANUP.to_string(), normal_cleanup_result);

    // --- forced-failure cleanup: the assertion-failure session failed typed
    // with evidence preserved, and the timeout session was bounded by the
    // supervisor kill; both settled to zero owned processes through observed
    // probes.
    let facts = failure_cleanup_facts(sessions);
    let failure_cleanup_result = if !facts.observed || facts.probe_missing {
        ObservationResult::NotProven
    } else if facts.assertion_typed
        && facts.timeout_bounded
        && facts.settled_zero
        && facts.ledgers_retained
    {
        ObservationResult::Pass
    } else {
        ObservationResult::Fail
    };
    cells.insert(CELL_FAILURE_CLEANUP.to_string(), failure_cleanup_result);

    // --- journey-level result.
    let actionable_cells = [
        CELL_BUFFER_REOPEN,
        CELL_HOST_REOPEN,
        CELL_CANCELLATION,
        CELL_LATE_RESULT,
        CELL_REPEATED_SESSIONS,
        CELL_NORMAL_CLEANUP,
        CELL_FAILURE_CLEANUP,
    ];
    let all_actionable_ok =
        actionable_cells.iter().all(|cell| cells.get(*cell) == Some(&ObservationResult::Pass));
    let workspace_ok = cells.get(CELL_WORKSPACE_REOPEN) == Some(&ObservationResult::Unsupported);
    let designed_boundaries = sessions.iter().all(|session| match session.role.as_str() {
        "full_lifecycle_session" | "replacement_host_session" => {
            session.observation.status_code == Some(0) && session.observation.driver_complete
        }
        "assertion_failure_session" => session.observation.status_code == Some(2),
        "timeout_interruption_session" => {
            session.observation.timed_out && session.observation.kill_requested
        }
        _ => true,
    });
    let any_leak =
        sessions.iter().any(|session| session.observation.cleanup == CleanupResult::Fail);
    // Positive relabel evidence: the control's own host observed a second
    // outgoing `initialize` on its wire — the attempted in-host server
    // restart. Without it the control never exercised its designed false
    // subject (a Vim timeout or pre-initialization exit proves nothing), and
    // assigning the typed failure would assert detection instead of
    // observing it. A negative control can still never report Pass: the
    // un-exercised path falls through the same fail-closed ladder as any
    // other run and lands on NotProven.
    let relabel_exercised = sessions.iter().any(|session| {
        session.role == "server_restart_relabel_session"
            && session.lifecycle_wire.initialize_count > 1
    });
    let result = if variant.expected_negative_reason().is_some() && relabel_exercised {
        // A negative control whose designed relabel path was exercised must
        // fail on every reachable path: reaching an all-ok state through
        // designed defect injection would itself be an oracle violation, and
        // reporting it as a pass would hide it.
        ObservationResult::Fail
    } else if all_actionable_ok && workspace_ok && designed_boundaries && !any_leak {
        // Unreachable for a negative variant (its designed relabel makes the
        // host-reopen cell fail), but kept explicit so no ladder rewrite can
        // ever produce a passing negative control.
        ObservationResult::Pass
    } else if any_leak
        || !designed_boundaries
        || actionable_cells.iter().any(|cell| cells.get(*cell) == Some(&ObservationResult::Fail))
    {
        ObservationResult::Fail
    } else {
        ObservationResult::NotProven
    };
    let failure_class = if result == ObservationResult::Pass {
        None
    } else if any_leak {
        Some(crate::editor_client_compat::FailureClass::Cleanup)
    } else {
        Some(crate::editor_client_compat::FailureClass::HostClient)
    };
    // The relabel control's typed detection: the judgment itself observed the
    // absent host replacement (the server restart the run offered in its
    // place, bound by `relabel_exercised`), so the typed reason is derived
    // from the evidence rather than asserted from the variant.
    let failure_reason = failure_reason.or_else(|| {
        if variant.expected_negative_reason().is_some()
            && relabel_exercised
            && cells.get(CELL_HOST_REOPEN) != Some(&ObservationResult::Pass)
        {
            Some("host_replacement_absent".to_string())
        } else {
            None
        }
    });
    LifecycleJudgment {
        result,
        failure_class,
        failure_reason,
        all_hosts_as_designed: designed_boundaries,
        workspace_folders_offered,
        cells,
    }
}

// ---------------------------------------------------------------------------
// Receipt journey
// ---------------------------------------------------------------------------

/// Compose the receipt journey: the per-host substrate barrier cells (the
/// #10944 surface, prefixed per host — each session's own barriers, judged
/// against that session's real mined wire) plus the eight #11387 catalog
/// cells this journey evidences. The designed-failure sessions contribute
/// their evidence to the `failure_cleanup` cell instead of barrier cells:
/// their sessions deliberately never reach the orderly shutdown barriers, so
/// their absence is the designed shape, not an unproven claim, and a passing
/// receipt never carries not-proven barrier cells for them.
pub fn lifecycle_journey(
    sessions: &[HostSessionRecord],
    judgment: &LifecycleJudgment,
) -> Vec<JourneyCell> {
    let mut cells = Vec::new();
    for session in sessions.iter().filter(|session| {
        matches!(
            session.role.as_str(),
            "full_lifecycle_session"
                | "replacement_host_session"
                | "server_restart_relabel_session"
        )
    }) {
        for mut cell in crate::vim_host_run::outcome_journey(&session.observation, &session.wire) {
            cell.id = format!("host{}_{}", session.index, cell.id);
            cell.evidence = vec![format!("host-{}/vim/driver-events.jsonl", session.index)];
            cells.push(cell);
        }
    }
    let catalog_limitations: BTreeMap<&str, &str> = BTreeMap::from([
        (
            CELL_BUFFER_REOPEN,
            "same-host close/wipe+reopen only: a changed document instance on an unchanged server \
             generation; a full host replacement or a server restart never satisfies it",
        ),
        (
            CELL_HOST_REOPEN,
            "full Vim exit plus replacement launch with a changed host instance through the \
             complete initialize sequence; the replacement's opening state equals the disk \
             generation the supervisor wrote, never a prior session's memory",
        ),
        (
            CELL_WORKSPACE_REOPEN,
            "client_not_exposed: the pinned client's own initialize request carries \
             workspace.workspaceFolders=false and the only mutation path is private experimental \
             surface behind a default-off flag; no stable public concept exists to exercise",
        ),
        (
            CELL_CANCELLATION,
            "cancellation is identity-bound ($/cancelRequest by the client's own request id) and \
             admission is observed through the client's own delivery; zero admissions after \
             invalidation",
        ),
        (
            CELL_LATE_RESULT,
            "the late old result completed on the wire and the replacement instance stayed \
             unchanged through its own didOpen-settled state; the in-flight request at host exit \
             cannot reach the replacement host's fresh channel",
        ),
        (
            CELL_REPEATED_SESSIONS,
            "finite denominator with per-iteration subject/result/cleanup binding; one passing \
             run is not repeated use and stale prior state never satisfies a new iteration",
        ),
        (
            CELL_NORMAL_CLEANUP,
            "normal-exit terminal cleanup settles observably through the deterministic \
             process-set comparison; a client exit event alone is not clean cleanup",
        ),
        (
            CELL_FAILURE_CLEANUP,
            "forced-failure/timeout cleanup judged from observed settled probes with diagnostics \
             preserved before cleanup; missing observation is not_proven, never zero",
        ),
    ]);
    for (cell_id, result) in &judgment.cells {
        cells.push(JourneyCell {
            id: cell_id.clone(),
            capability_basis: CapabilityBasis::NotApplicable,
            observed: *result != ObservationResult::NotProven
                && *result != ObservationResult::Unsupported,
            result: *result,
            evidence: catalog_evidence(cell_id, sessions),
            limitation: match result {
                ObservationResult::Pass => {
                    catalog_limitations.get(cell_id.as_str()).map(|text| text.to_string())
                }
                ObservationResult::Unsupported => Some(
                    "client_not_exposed for this exact subject; the disposition is read from the \
                     run's own initialize wire and is never relabeled"
                        .to_string(),
                ),
                _ => Some(format!("{cell_id} was not proven for this exact subject")),
            },
        });
    }
    cells
}

fn catalog_evidence(cell_id: &str, sessions: &[HostSessionRecord]) -> Vec<String> {
    let host_ids = |indexes: &[usize]| -> Vec<String> {
        indexes
            .iter()
            .filter_map(|index| sessions.iter().find(|session| session.index == *index))
            .flat_map(|session| {
                [
                    format!("host-{}/vim/driver-events.jsonl", session.index),
                    format!("host-{}/vim/vim-lsp-client.log", session.index),
                    format!("host-{}/vim/process-ledger.json", session.index),
                ]
                .into_iter()
                .collect::<Vec<_>>()
            })
            .collect()
    };
    match cell_id {
        CELL_BUFFER_REOPEN | CELL_CANCELLATION | CELL_LATE_RESULT => host_ids(&[1]),
        CELL_HOST_REOPEN | CELL_REPEATED_SESSIONS => host_ids(&[1, 2]),
        CELL_WORKSPACE_REOPEN => host_ids(&[1]),
        CELL_NORMAL_CLEANUP => host_ids(&[1, 2]),
        CELL_FAILURE_CLEANUP => host_ids(&[3, 4]),
        _ => host_ids(&[1]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn malformed_survivor_ledger_is_not_coerced_to_zero() {
        for value in [
            serde_json::json!({}),
            serde_json::json!({"surviving_processes": null}),
            serde_json::json!({"surviving_processes": true}),
            serde_json::json!({"surviving_processes": 0}),
            serde_json::json!({"surviving_processes": {}}),
        ] {
            assert_eq!(ledger_survivor_count(&value), None);
        }
        assert_eq!(ledger_survivor_count(&serde_json::json!({"surviving_processes": []})), Some(0));
        assert_eq!(
            ledger_survivor_count(&serde_json::json!({"surviving_processes": [1, 2]})),
            Some(2)
        );
    }

    #[test]
    fn fixture_variants_parse_and_carry_typed_negative_reasons() {
        assert!(matches!(
            LifecycleFixtureVariant::from_id("canonical"),
            Ok(LifecycleFixtureVariant::Canonical)
        ));
        assert!(matches!(
            LifecycleFixtureVariant::from_id("server_restart_relabel"),
            Ok(LifecycleFixtureVariant::ServerRestartRelabel)
        ));
        assert!(LifecycleFixtureVariant::from_id("other").is_err());
        assert_eq!(LifecycleFixtureVariant::Canonical.expected_negative_reason(), None);
        assert_eq!(
            LifecycleFixtureVariant::ServerRestartRelabel.expected_negative_reason(),
            Some("host_replacement_absent")
        );
    }

    #[test]
    fn clean_text_changes_exactly_the_mutation_line() {
        let defect = defect_source_text();
        let clean = clean_source_text();
        assert_ne!(defect, clean);
        let defect_lines: Vec<&str> = defect.lines().collect();
        let clean_lines: Vec<&str> = clean.lines().collect();
        assert_eq!(defect_lines.len(), clean_lines.len());
        for index in 0..defect_lines.len() {
            if index + 1 == MUTATION_LINE {
                assert_eq!(defect_lines[index], DEFECT_LINE_TEXT);
                assert_eq!(clean_lines[index], CLEAN_LINE_TEXT);
            } else {
                assert_eq!(defect_lines[index], clean_lines[index]);
            }
        }
    }

    #[test]
    fn lifecycle_wire_mines_cancels_responses_and_closes() {
        let log = concat!(
            "[\"--->\",1,\"perllsp\",{\"method\":\"initialize\",\"params\":{}}]\n",
            "[\"--->\",2,\"perllsp\",{\"method\":\"textDocument/documentSymbol\",\"params\":{}}]\n",
            "[\"--->\",3,\"perllsp\",{\"method\":\"$/cancelRequest\",\"params\":{\"id\":2}}]\n",
            "[\"<---\",3,\"perllsp\",{\"response\":{\"id\":2,\"result\":[]},\"request\":{\"id\":2,\"method\":\"textDocument/documentSymbol\"}}]\n",
            "[\"--->\",4,\"perllsp\",{\"method\":\"textDocument/didClose\",\"params\":{\"textDocument\":{\"uri\":\"file:///w/main.pl\"}}}]\n"
        );
        let wire = extract_lifecycle_wire(log.as_bytes());
        assert_eq!(wire.initialize_count, 1);
        assert_eq!(wire.cancel_request_ids, vec![2]);
        assert_eq!(wire.document_symbol_responses.len(), 1);
        assert_eq!(wire.response_line_of(2), Some(3));
        assert_eq!(wire.first_close_line(MAIN_TOKEN), Some(4));
        assert_eq!(wire.response_line_of(9), None);
    }
}
