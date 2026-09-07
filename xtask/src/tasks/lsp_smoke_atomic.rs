//! Atomic `lsp_smoke` child harness (#8063).
//!
//! The pre-#8063 `lsp_smoke` gate was one composite shell command
//! (`cargo build && cargo test … && cargo test …`) under a single 300s
//! watchdog. Any compile overrun, assertion, request hang, or teardown hang
//! produced exactly one receipt fact — `lsp_smoke = timeout` — erasing every
//! independent child verdict, which is what made the #8053 product defect and
//! the #9601 fleet timeouts hard to classify.
//!
//! This module turns the gate into a bounded collection of independently
//! terminal child facts:
//!
//! - **atomic** — every required child (setup builds, per-target compiles,
//!   each behavioral case) has a stable identity and its own terminal status;
//! - **bounded** — every child carries its own timeout, enforced by the same
//!   watchdog machinery the gate runner uses
//!   ([`run_shell_command_with_timeout`]);
//! - **independently terminal** — one child's assertion, hang, process exit,
//!   or teardown hang cannot erase another child's evidence: the suite
//!   continues while prerequisites remain usable, and the child receipt is
//!   persisted after every child so even an outer-watchdog kill retains
//!   completed children and marks the remaining/running children explicitly.
//!
//! Compile/setup failures make the dependent behavior children
//! `NOT_PROVEN` (blocked by prerequisite) — never assertion failures. The
//! in-process API child has no server-setup dependency. Behavior children
//! that do use the product server run against prebuilt binaries, so their
//! watchdog timeout is classified as a request/teardown timeout rather than
//! retried away.
//!
//! The gate receipt (`gates` runner) stays the aggregate authority; the child
//! receipt written here is gate telemetry for #8053/#4789-style consumers and
//! deliberately does not use the `test_results` envelope that feeds Test
//! Analytics (`scripts/ci/receipts-to-junit.py` classifies unknown shapes as
//! non-tests, so no aggregate JUnit pseudo-test is manufactured).

use std::collections::BTreeMap;
use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Instant;

use chrono::Utc;
use color_eyre::eyre::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use tempfile::NamedTempFile;

use crate::tasks::gates::run_shell_command_with_timeout_in;
use crate::tasks::git_context::git_stdout_with_worktree_fallback;
use crate::utils::project_root;

/// Child receipt envelope version. Bump on any breaking shape change.
const RECEIPT_SCHEMA_VERSION: u32 = 1;

/// Per-child timeout for the deterministic single-test behavior cases.
///
/// A behavior child runs one prebuilt test binary; the whole 16-test suite
/// completes in about a second once `perllsp` is prebuilt (#9677), so 120s is
/// a generous bound whose only purpose is to make a hang terminal.
const BEHAVIOR_TIMEOUT_SECONDS: u64 = 120;

/// Per-child timeout for setup/compile children.
///
/// Cold-cache compile is the observed budget consumer (#10023 race family,
/// #9779 Cargo.lock cold starts). 300s is the old composite's whole-gate
/// budget — the incident compile alone measured ~219s under load, and sibling
/// unit gates complete in 173-243s *including* their compiles — so each
/// compile child carrying the old gate's entire budget is a generous bound.
/// Setup/compile children additionally use default cargo build parallelism
/// (the old composite forced `CARGO_BUILD_JOBS=1`, which is a determinism
/// lever for test execution, not for compiling artifacts), which cuts the
/// cold-compile wall time the single-job build was suffering.
const SETUP_COMPILE_TIMEOUT_SECONDS: u64 = 300;

/// Setup/compile children are the only children whose watchdog timeout may be
/// retried once, mirroring the #10023 runner policy: their timeout is a
/// compile-overrun remedy, while a behavior child runs prebuilt binaries and
/// its timeout is genuine test evidence that must stay visible.
const SETUP_COMPILE_RETRIES: u32 = 1;

/// Cap the retained per-child failure summary.
const MAX_FAILURE_SUMMARY_CHARS: usize = 1600;

/// Working directory (and `CARGO_MANIFEST_DIR`) cargo would have given the
/// test process: the owning package's directory, not the workspace root.
/// Direct binary execution must stay environment-equivalent to
/// `cargo test -p perl-lsp-rs` — running from the workspace root instead let
/// the semantic analyzer resolve `Foo::bar()` into an unrelated workspace
/// fixture (observed live, PR #12091 run 32684241909), turning an
/// environment delta into a product-looking failure.
const BEHAVIOR_WORKING_DIR: &str = "crates/perl-lsp-rs";

// =============================================================================
// Child specifications
// =============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChildKind {
    /// Prebuild of the `perllsp` server binary (#9677).
    Setup,
    /// `cargo test --no-run` for one integration target.
    Compile,
    /// One bounded behavioral case against a prebuilt binary.
    Behavior,
}

/// One required child of the `lsp_smoke` gate with a stable identity.
#[derive(Debug, Clone)]
pub struct ChildSpec {
    /// Stable identity, e.g. `semantic_definition/scalar_variable`.
    pub id: &'static str,
    pub kind: ChildKind,
    /// Whether the behavior child needs the prebuilt product server.
    pub requires_setup: bool,
    /// Cargo integration target (`--test <target>`); the package is implied
    /// (the binary package for [`ChildKind::Setup`]).
    pub target: &'static str,
    /// Exact test path or module filter for behavior children.
    pub test_filter: Option<&'static str>,
    /// Whether `test_filter` is an exact test path (`-- --exact`).
    pub exact: bool,
    pub timeout_seconds: u64,
}

const SEMANTIC_TARGET: &str = "semantic_definition";
const API_TARGET: &str = "lsp_api_contracts";
const REGISTRY_FILTER: &str = "client_support_registry";

/// The required child set, in deterministic execution order.
///
/// The four named semantic cases and the textDocumentSync camelCase contract
/// are the #8063 minimum child set. The client-support registry suite is
/// included in this target since the registry became merge-blocking; its
/// thirteen deterministic registry-validation cases run as one grouped child
/// because they share one failure surface (registry/evidence drift) and the
/// group cannot suppress or conflate any sibling child's independent verdict.
///
/// The in-process API lane, semantic compile, and registry lane run before the
/// retryable product-server setup so an expensive setup timeout cannot erase
/// their independent evidence.
pub fn child_specs() -> Vec<ChildSpec> {
    let semantic = |id: &'static str, test: &'static str| ChildSpec {
        id,
        kind: ChildKind::Behavior,
        requires_setup: true,
        target: SEMANTIC_TARGET,
        test_filter: Some(test),
        exact: true,
        timeout_seconds: BEHAVIOR_TIMEOUT_SECONDS,
    };
    vec![
        ChildSpec {
            id: "compile/lsp_api_contracts",
            kind: ChildKind::Compile,
            requires_setup: false,
            target: API_TARGET,
            test_filter: None,
            exact: false,
            timeout_seconds: SETUP_COMPILE_TIMEOUT_SECONDS,
        },
        ChildSpec {
            id: "lsp_api_contracts/textdocument_sync_camel_case",
            kind: ChildKind::Behavior,
            requires_setup: false,
            target: API_TARGET,
            test_filter: Some("test_text_document_sync_option_keys_use_lsp_camel_case"),
            exact: true,
            timeout_seconds: BEHAVIOR_TIMEOUT_SECONDS,
        },
        ChildSpec {
            id: "compile/semantic_definition",
            kind: ChildKind::Compile,
            requires_setup: false,
            target: SEMANTIC_TARGET,
            test_filter: None,
            exact: false,
            timeout_seconds: SETUP_COMPILE_TIMEOUT_SECONDS,
        },
        ChildSpec {
            id: "semantic_definition/client_support_registry",
            kind: ChildKind::Behavior,
            requires_setup: false,
            target: SEMANTIC_TARGET,
            test_filter: Some(REGISTRY_FILTER),
            exact: false,
            timeout_seconds: BEHAVIOR_TIMEOUT_SECONDS,
        },
        ChildSpec {
            id: "setup/build_perllsp",
            kind: ChildKind::Setup,
            requires_setup: false,
            target: "perllsp",
            test_filter: None,
            exact: false,
            timeout_seconds: SETUP_COMPILE_TIMEOUT_SECONDS,
        },
        semantic(
            "semantic_definition/scalar_variable",
            "semantic_definition_tests::definition_finds_scalar_variable_declaration",
        ),
        semantic(
            "semantic_definition/subroutine",
            "semantic_definition_tests::definition_finds_subroutine_declaration",
        ),
        semantic(
            "semantic_definition/scoped_variable",
            "semantic_definition_tests::definition_resolves_scoped_variables",
        ),
        semantic(
            "semantic_definition/package_qualified_call",
            "semantic_definition_tests::definition_handles_package_qualified_calls",
        ),
    ]
}

/// The exact command one child executes (also recorded in the receipt).
///
/// Behavior children run the prebuilt test binary **directly** — the path
/// `cargo test --no-run` printed for their target — never through cargo
/// again. The first hosted run of the cargo-per-child shape (PR #12091, run
/// 32681631693) showed why: each `cargo test` re-entry rebuilt units the
/// previous child had already built, the first rebuild invalidated `perllsp`
/// (a dependency of the test's in-process pre-build), the pre-build overran
/// the child budget, and every watchdog kill left cargo fingerprints
/// half-written so each later child rebuilt more (1 → 7 → 2 → 4 units) —
/// four consecutive REQUEST_TIMEOUTs with zero test output. A bare test
/// binary spawn has no cargo to rebuild, lock, or corrupt; the executable
/// filename embeds the binary's content hash, so the receipt records the
/// exact artifact identity each child ran.
pub fn command_line(spec: &ChildSpec, executable: Option<&str>) -> String {
    match spec.kind {
        ChildKind::Setup => format!("cargo build -p {} --locked", spec.target),
        ChildKind::Compile => {
            format!("cargo test -p perl-lsp-rs --test {} --locked --no-run", spec.target)
        }
        ChildKind::Behavior => {
            let filter = spec.test_filter.unwrap_or_default();
            let executable = executable.unwrap_or("<unresolved-prebuilt-test-binary>");
            let exact = if spec.exact { " --exact" } else { "" };
            format!("\"{executable}\" {filter}{exact} --test-threads=1")
        }
    }
}

/// Parse the prebuilt test-binary path a compile child's `--no-run` output
/// names: `Executable tests/<target>.rs (target/debug/deps/<target>-<hash>)`.
///
/// Handles ANSI-colored output (`CARGO_TERM_COLOR=always` is set gate-wide)
/// by stripping escape sequences before matching. Returns the path relative
/// to the workspace root.
pub fn parse_executable_path(compile_stdout: &str, target: &str) -> Option<std::path::PathBuf> {
    let expected = format!("tests/{target}.rs");
    for line in compile_stdout.lines() {
        // Cargo uses native separators in the executable diagnostic on
        // Windows. Normalize only for matching/parsing; the resulting path
        // remains valid when joined to the workspace root on every platform.
        let plain = strip_ansi(line).replace('\\', "/");
        if !plain.contains("Executable") || !plain.contains(&expected) {
            continue;
        }
        let start = plain.find('(')?;
        let end = plain[start + 1..].find(')')? + start + 1;
        let path = plain[start + 1..end].trim();
        if path.is_empty() {
            return None;
        }
        return Some(std::path::PathBuf::from(path));
    }
    None
}

/// Strip ANSI escape sequences (color codes) from one line.
fn strip_ansi(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut chars = line.chars();
    while let Some(ch) = chars.next() {
        if ch != '\u{1b}' {
            out.push(ch);
            continue;
        }
        // ESC [ ... final-byte (simplified CSI/OSC handling is enough for
        // cargo's SGR color codes).
        match chars.next() {
            Some('[') => {
                for inner in chars.by_ref() {
                    if inner.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
            Some(']') => {
                for inner in chars.by_ref() {
                    if inner == '\u{7}' {
                        break;
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// Per-child log file name under `target/receipts/logs/`.
fn log_file_name(spec: &ChildSpec) -> String {
    format!("lsp_smoke-{}.log", spec.id.replace('/', "-"))
}

// =============================================================================
// Typed child vocabulary (#8063 §4; consumed by #4789)
// =============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ChildStatus {
    Pass,
    /// Deterministic test assertion failure.
    AssertionFailure,
    /// Watchdog fired before the test emitted a terminal `test result:` line.
    RequestTimeout,
    /// Watchdog fired after a terminal `test result:` line (results complete,
    /// the process never exited) — response and teardown stay separable.
    TeardownTimeout,
    /// Non-zero, non-assertion exit (server/runtime died, signal, cargo error).
    ProcessExit,
    /// Setup/compile child failed or timed out.
    CompileFailure,
    /// The harness itself could not produce trustworthy evidence.
    InstrumentFailure,
    /// Suite ended before the child executed or while it ran.
    Cancelled,
    /// Prerequisite (setup/compile) failed; the child is blocked, not failed.
    NotProven,
}

impl ChildStatus {
    fn as_str(self) -> &'static str {
        match self {
            ChildStatus::Pass => "PASS",
            ChildStatus::AssertionFailure => "ASSERTION_FAILURE",
            ChildStatus::RequestTimeout => "REQUEST_TIMEOUT",
            ChildStatus::TeardownTimeout => "TEARDOWN_TIMEOUT",
            ChildStatus::ProcessExit => "PROCESS_EXIT",
            ChildStatus::CompileFailure => "COMPILE_FAILURE",
            ChildStatus::InstrumentFailure => "INSTRUMENT_FAILURE",
            ChildStatus::Cancelled => "CANCELLED",
            ChildStatus::NotProven => "NOT_PROVEN",
        }
    }
}

// =============================================================================
// Raw outcomes and classification
// =============================================================================

/// Raw executor outcome for one child, including its attempt history.
#[derive(Debug, Clone)]
pub struct RawOutcome {
    pub exit_code: i32,
    pub timed_out: bool,
    pub stdout: String,
    /// The command could not be spawned/instrumented at all.
    pub spawn_failed: bool,
    /// Attempts actually run (≥1 when spawn did not fail).
    pub attempts: u32,
    /// Retained per-attempt outcomes, e.g. `attempt 1/2: watchdog timeout`.
    pub history: Vec<String>,
}

/// libtest's terminal summary line, e.g. `test result: ok. 1 passed; ...`.
fn has_terminal_result_line(stdout: &str) -> bool {
    stdout.contains("test result:")
}

/// libtest failure/panic signature.
fn has_assertion_signature(stdout: &str) -> bool {
    stdout.contains("test result: FAILED") || stdout.contains("panicked at")
}

/// Count tests executed from the libtest summary line, if present.
///
/// Fail-closed seam: a behavior child whose filter matches zero tests exits 0
/// with `test result: ok. 0 passed` — a false pass. Children that executed no
/// tests are classified [`ChildStatus::InstrumentFailure`], never
/// [`ChildStatus::Pass`].
fn tests_executed(stdout: &str) -> Option<u64> {
    let line = stdout.lines().rev().find(|line| line.contains("test result:"))?;
    let before_passed = line.split(" passed").next()?;
    let digits: String = before_passed
        .chars()
        .rev()
        .take_while(char::is_ascii_digit)
        .collect::<String>()
        .chars()
        .rev()
        .collect();
    digits.parse::<u64>().ok()
}

/// Classify one executed child (pure; #8063 §4 closed vocabulary).
pub fn classify(kind: ChildKind, outcome: &RawOutcome) -> ChildStatus {
    if outcome.spawn_failed {
        return ChildStatus::InstrumentFailure;
    }
    match kind {
        ChildKind::Setup | ChildKind::Compile => {
            if outcome.exit_code == 0 && !outcome.timed_out {
                ChildStatus::Pass
            } else {
                ChildStatus::CompileFailure
            }
        }
        ChildKind::Behavior => {
            if outcome.timed_out {
                if has_terminal_result_line(&outcome.stdout) {
                    ChildStatus::TeardownTimeout
                } else {
                    ChildStatus::RequestTimeout
                }
            } else if outcome.exit_code == 0 {
                match tests_executed(&outcome.stdout) {
                    Some(count) if count > 0 => ChildStatus::Pass,
                    // Zero tests matched the filter (or no summary line):
                    // spec/target drift, not a product verdict.
                    Some(_) | None => ChildStatus::InstrumentFailure,
                }
            } else if has_assertion_signature(&outcome.stdout) {
                ChildStatus::AssertionFailure
            } else {
                ChildStatus::ProcessExit
            }
        }
    }
}

/// Bounded first-failure summary retained for every non-pass child.
pub fn summarize_failure(outcome: &RawOutcome) -> Option<String> {
    if outcome.spawn_failed {
        return Some("child command could not be spawned (instrument failure)".to_string());
    }
    if outcome.timed_out && !has_terminal_result_line(&outcome.stdout) {
        return Some(format!(
            "watchdog timeout after {} attempt(s) before any terminal test output",
            outcome.attempts
        ));
    }
    let stdout = outcome.stdout.as_str();
    let markers = ["panicked at", "test result: FAILED", "error: could not compile", "error["];
    let start = stdout.lines().position(|line| markers.iter().any(|marker| line.contains(marker)));
    let Some(start) = start else {
        return if outcome.timed_out {
            Some("watchdog timeout after a terminal test result line (teardown hang)".to_string())
        } else {
            stdout.lines().map(str::trim).find(|line| !line.is_empty()).map(str::to_string)
        };
    };
    let summary: Vec<&str> = stdout.lines().skip(start).take(8).collect();
    let mut text = summary.join("\n");
    if text.len() > MAX_FAILURE_SUMMARY_CHARS {
        text.truncate(MAX_FAILURE_SUMMARY_CHARS);
        text.push('…');
    }
    Some(text)
}

// =============================================================================
// Receipt shape
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChildEntry {
    pub id: String,
    pub kind: ChildKind,
    pub target: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub test: Option<String>,
    pub status: ChildStatus,
    /// `pending` | `running` | `final` — distinguishes the persisted snapshot
    /// of a not-yet-terminal child from its terminal verdict.
    pub execution_mark: String,
    /// Attempt that produced the recorded outcome; `attempts` retains history.
    pub attempt: u32,
    pub attempts: u32,
    pub timeout_seconds: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    /// Resolved executable for a direct-binary behavior child.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub executable: Option<String>,
    pub env_set: BTreeMap<String, String>,
    pub env_unset: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ended_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure_summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub log_path: Option<String>,
    /// Working directory for direct-binary behavior children (cargo
    /// equivalence; `None` for children spawned from the workspace root).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub working_dir: Option<String>,
    /// `exited` | `unobserved after timeout` | `never spawned` |
    /// `unobserved`
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cleanup: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attempt_history: Vec<String>,
    pub subject_sha: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AggregateDoc {
    pub status: String,
    pub passed: usize,
    pub total: usize,
    /// Full non-success child set, in deterministic spec order.
    pub non_success: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ChildReceiptDoc {
    pub schema_version: u32,
    pub gate: String,
    pub subject_sha: String,
    /// `running` snapshots persist after every child; `complete` only when the
    /// suite itself reached a verdict. An outer-watchdog kill leaves the last
    /// `running` snapshot on disk with completed children retained.
    pub suite_state: String,
    pub children: Vec<ChildEntry>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aggregate: Option<AggregateDoc>,
}

/// Aggregate from child evidence (#8063 §6): fail when any required child is
/// not `PASS`; report the full non-success set in spec order; a missing child
/// row — or an empty child table — can never produce pass.
pub fn aggregate(entries: &[ChildEntry], expected: &[ChildSpec]) -> AggregateDoc {
    let mut counts = BTreeMap::<&str, usize>::new();
    for entry in entries {
        *counts.entry(entry.id.as_str()).or_default() += 1;
    }

    let mut non_success = expected
        .iter()
        .filter_map(|spec| {
            let count = counts.get(spec.id).copied().unwrap_or(0);
            let entry = entries.iter().find(|entry| entry.id == spec.id);
            (count != 1 || entry.is_none_or(|entry| entry.status != ChildStatus::Pass))
                .then_some(spec.id.to_string())
        })
        .collect::<Vec<_>>();
    // An unexpected row is also evidence that the required table was
    // substituted or drifted. Keep it visible rather than allowing a
    // same-sized table with the wrong IDs to pass.
    for entry in entries {
        if !expected.iter().any(|spec| spec.id == entry.id) {
            non_success.push(entry.id.clone());
        }
    }
    let passed = expected
        .iter()
        .filter(|spec| {
            counts.get(spec.id) == Some(&1)
                && entries
                    .iter()
                    .find(|entry| entry.id == spec.id)
                    .is_some_and(|entry| entry.status == ChildStatus::Pass)
        })
        .count();
    let status =
        if non_success.is_empty() && !expected.is_empty() && entries.len() == expected.len() {
            "pass"
        } else {
            "fail"
        };
    AggregateDoc { status: status.to_string(), passed, total: entries.len(), non_success }
}

// =============================================================================
// Execution
// =============================================================================

/// Executor seam: real runs shell out through the gate watchdog machinery;
/// tests inject outcomes.
///
/// `executable` carries the prebuilt test-binary path (resolved from the
/// target's compile child) for behavior children; setup/compile children
/// ignore it.
pub trait ChildExecutor {
    fn execute(
        &mut self,
        spec: &ChildSpec,
        executable: Option<&Path>,
        working_dir: Option<&Path>,
        log_path: &Path,
    ) -> Result<RawOutcome>;
}

/// Real executor: one shell command per child through
/// [`run_shell_command_with_timeout`], with the #10023 timeout-only retry
/// policy applied to setup/compile children only. Behavior children run the
/// prebuilt test binary directly — no cargo in the behavior path.
pub struct CargoChildExecutor;

impl ChildExecutor for CargoChildExecutor {
    fn execute(
        &mut self,
        spec: &ChildSpec,
        executable: Option<&Path>,
        working_dir: Option<&Path>,
        log_path: &Path,
    ) -> Result<RawOutcome> {
        apply_child_environment(spec.kind, working_dir);
        let executable = executable.map(|path| path.display().to_string());
        let command = command_line(spec, executable.as_deref());
        let retries = match spec.kind {
            ChildKind::Setup | ChildKind::Compile => SETUP_COMPILE_RETRIES,
            ChildKind::Behavior => 0,
        };
        let total_attempts = 1 + retries;
        let mut attempt = 1_u32;
        let mut history = Vec::new();
        loop {
            let execution = run_shell_command_with_timeout_in(
                &command,
                log_path,
                spec.timeout_seconds,
                working_dir,
            );
            let mut execution = match execution {
                Ok(execution) => execution,
                // Spawn/instrument failure is harness evidence, not a product
                // verdict; retrying would only duplicate it.
                Err(error) => {
                    return Ok(RawOutcome {
                        exit_code: -1,
                        timed_out: false,
                        stdout: format!("failed to spawn child command: {error:#}"),
                        spawn_failed: true,
                        attempts: attempt,
                        history,
                    });
                }
            };
            if execution.timed_out {
                let note = format!("attempt {attempt}/{total_attempts}: watchdog timeout");
                history.push(note.clone());
                execution.stdout.push_str(&format!("\n==== {note} ====\n"));
                if attempt < total_attempts {
                    eprintln!(
                        "lsp_smoke child {} timed out after {}s on attempt {attempt}; \
                         retrying ({}/{total_attempts})",
                        spec.id,
                        spec.timeout_seconds,
                        attempt + 1
                    );
                    attempt += 1;
                    continue;
                }
            }
            return Ok(RawOutcome {
                exit_code: execution.exit_code,
                timed_out: execution.timed_out,
                stdout: execution.stdout,
                spawn_failed: false,
                attempts: attempt,
                history,
            });
        }
    }
}

/// Child environment: drop the compile cache wrapper everywhere (parity with
/// the old composite), force single-threaded test execution for determinism,
/// and let every cargo invocation use default build parallelism — the old
/// composite's `CARGO_BUILD_JOBS=1` is a determinism lever for test
/// execution, not compilation, and only slowed builds (including the test
/// harness's own once-per-process `perllsp` pre-build freshness check).
///
/// SAFETY: the xtask runner is single-threaded when a gate executes, the same
/// pattern `gates::run_single_gate` uses for policy environment variables.
fn apply_child_environment(kind: ChildKind, behavior_dir: Option<&Path>) {
    unsafe {
        std::env::remove_var("RUSTC_WRAPPER");
        std::env::remove_var("CARGO_BUILD_JOBS");
        std::env::remove_var("CARGO_MANIFEST_DIR");
    }
    match kind {
        ChildKind::Setup | ChildKind::Compile => unsafe {
            std::env::remove_var("RUST_TEST_THREADS");
        },
        ChildKind::Behavior => {
            let Some(dir) = behavior_dir else {
                return;
            };
            unsafe {
                std::env::set_var("RUST_TEST_THREADS", "1");
                // cargo sets the ABSOLUTE package path for test processes;
                // the harness's binary resolution reads it at runtime to
                // locate the workspace root for its perllsp freshness
                // pre-build. A relative value here made the pre-build run
                // cargo from a nonexistent directory (run 32686346867).
                std::env::set_var("CARGO_MANIFEST_DIR", dir);
            }
        }
    }
}

fn child_env_record(
    kind: ChildKind,
    behavior_dir: Option<&Path>,
) -> (BTreeMap<String, String>, Vec<String>) {
    let mut set = BTreeMap::new();
    let mut unset = vec!["RUSTC_WRAPPER".to_string(), "CARGO_BUILD_JOBS".to_string()];
    match kind {
        ChildKind::Setup | ChildKind::Compile => {
            unset.push("CARGO_MANIFEST_DIR".to_string());
            unset.push("RUST_TEST_THREADS".to_string());
        }
        ChildKind::Behavior => {
            set.insert("RUST_TEST_THREADS".to_string(), "1".to_string());
            if let Some(dir) = behavior_dir {
                set.insert("CARGO_MANIFEST_DIR".to_string(), dir.display().to_string());
            }
        }
    }
    (set, unset)
}

/// Placeholder entry for a child that has not reached a terminal verdict in
/// the persisted snapshot: `CANCELLED` + execution mark, never a pass.
fn placeholder(spec: &ChildSpec, mark: &str, sha: &str) -> ChildEntry {
    let (env_set, env_unset) = child_env_record(spec.kind, None);
    ChildEntry {
        id: spec.id.to_string(),
        kind: spec.kind,
        target: spec.target.to_string(),
        test: spec.test_filter.map(str::to_string),
        status: ChildStatus::Cancelled,
        execution_mark: mark.to_string(),
        attempt: 0,
        attempts: 0,
        timeout_seconds: spec.timeout_seconds,
        command: Some(command_line(spec, None)),
        executable: None,
        env_set,
        env_unset,
        started_at: None,
        ended_at: None,
        duration_ms: None,
        exit_code: None,
        timed_out: false,
        failure_summary: None,
        log_path: Some(format!("logs/{}", log_file_name(spec))),
        working_dir: None,
        cleanup: None,
        attempt_history: Vec::new(),
        subject_sha: sha.to_string(),
    }
}

pub struct SuiteOutcome {
    pub aggregate: AggregateDoc,
}

/// How a persist call should mark the suite.
enum PersistState<'a> {
    /// Mid-suite: `running` snapshot; `Running(entry)` marks that child with
    /// the exact execution context resolved before spawn.
    Running(Option<&'a ChildEntry>),
    /// Suite finished: `complete` + aggregate.
    Final(&'a AggregateDoc),
}

/// Run the full child suite, persisting the receipt after every child.
pub fn run_suite(receipt_path: &Path, executor: &mut dyn ChildExecutor) -> Result<SuiteOutcome> {
    let root = project_root()?;
    // Resolve relative receipt paths against the project root so a direct
    // `cargo run -p xtask -- lsp-smoke-atomic` from a workspace subdirectory
    // still writes where the gate policy declares it.
    let receipt_path = if receipt_path.is_absolute() {
        receipt_path.to_path_buf()
    } else {
        root.join(receipt_path)
    };
    let log_dir = root.join("target/receipts/logs");
    fs::create_dir_all(&log_dir).context("failed to create lsp_smoke child log directory")?;
    if let Some(parent) = receipt_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create receipt dir {}", parent.display()))?;
    }
    let sha = git_stdout_with_worktree_fallback(&root, &["rev-parse", "HEAD"]).unwrap_or_default();

    let specs = child_specs();
    let mut entries: Vec<ChildEntry> = Vec::with_capacity(specs.len());
    let mut setup_ok = false;
    // Per-target compile verdicts: a semantic child is blocked only by the
    // semantic compile, not by an API-target compile failure.
    let mut compile_ok_by_target: HashMap<&'static str, bool> = HashMap::new();
    // Prebuilt test-binary path per target, parsed from each compile child's
    // `--no-run` output. Behavior children execute these directly.
    let mut executable_by_target: HashMap<&'static str, Option<PathBuf>> = HashMap::new();

    for spec in &specs {
        let behavior = spec.kind == ChildKind::Behavior;
        let behavior_working_dir = behavior.then(|| root.join(BEHAVIOR_WORKING_DIR));
        let working_dir = behavior_working_dir.as_deref();
        let executable = if behavior {
            executable_by_target.get(spec.target).and_then(Option::as_deref)
        } else {
            None
        };
        let blocked = match spec.kind {
            ChildKind::Setup | ChildKind::Compile => false,
            ChildKind::Behavior => {
                (spec.requires_setup && !setup_ok)
                    || compile_ok_by_target.get(spec.target) != Some(&true)
            }
        };
        if blocked {
            let mut entry = placeholder(spec, "final", &sha);
            entry.status = ChildStatus::NotProven;
            entry.failure_summary =
                Some("blocked by prerequisite: setup/compile child did not pass".to_string());
            println!("lsp_smoke child {} status=NOT_PROVEN (blocked by prerequisite)", spec.id);
            entries.push(entry);
            persist(&receipt_path, &sha, &entries, &specs, PersistState::Running(None))?;
            continue;
        }
        // Fail-closed: a behavior child whose compile passed but whose
        // prebuilt binary path could not be resolved never executes — and an
        // unresolvable path can never produce a pass.
        let unresolved_binary = behavior && executable_by_target.get(spec.target) == Some(&None);
        if unresolved_binary {
            let mut entry = placeholder(spec, "final", &sha);
            entry.status = ChildStatus::InstrumentFailure;
            entry.failure_summary = Some(
                "compile child passed but its --no-run output named no prebuilt \
                 test binary for this target; refusing to re-enter cargo from the \
                 behavior path"
                    .to_string(),
            );
            println!(
                "lsp_smoke child {} status=INSTRUMENT_FAILURE (no prebuilt binary resolved)",
                spec.id
            );
            entries.push(entry);
            persist(&receipt_path, &sha, &entries, &specs, PersistState::Running(None))?;
            continue;
        }

        let log_path = log_dir.join(log_file_name(spec));
        let started_at = Utc::now();
        let start = Instant::now();
        // Resolve and persist the complete execution context before spawn, so
        // an outer-watchdog kill leaves the actual executable, cwd,
        // environment, and cleanup uncertainty rather than a generic
        // placeholder (#8063 negative control 9).
        let running_entry = running_entry(spec, &sha, executable, working_dir);
        persist(
            &receipt_path,
            &sha,
            &entries,
            &specs,
            PersistState::Running(Some(&running_entry)),
        )?;
        let outcome = executor.execute(spec, executable, working_dir, &log_path)?;
        let duration_ms = start.elapsed().as_millis() as u64;
        let status = classify(spec.kind, &outcome);

        let (env_set, env_unset) = child_env_record(spec.kind, working_dir);
        let entry = ChildEntry {
            id: spec.id.to_string(),
            kind: spec.kind,
            target: spec.target.to_string(),
            test: spec.test_filter.map(str::to_string),
            status,
            execution_mark: "final".to_string(),
            attempt: outcome.attempts,
            attempts: outcome.attempts,
            timeout_seconds: spec.timeout_seconds,
            command: Some(command_line(
                spec,
                executable.map(|path| path.display().to_string()).as_deref(),
            )),
            executable: executable.map(|path| path.display().to_string()),
            env_set,
            env_unset,
            started_at: Some(started_at.to_rfc3339()),
            ended_at: Some(Utc::now().to_rfc3339()),
            duration_ms: Some(duration_ms),
            exit_code: if outcome.spawn_failed { None } else { Some(outcome.exit_code) },
            timed_out: outcome.timed_out,
            failure_summary: if status == ChildStatus::Pass {
                None
            } else {
                summarize_failure(&outcome)
            },
            log_path: Some(format!("logs/{}", log_file_name(spec))),
            working_dir: working_dir.map(|dir| dir.display().to_string()),
            cleanup: Some(if outcome.timed_out {
                "unobserved after timeout".to_string()
            } else if outcome.spawn_failed {
                "never spawned".to_string()
            } else {
                "exited".to_string()
            }),
            attempt_history: outcome.history.clone(),
            subject_sha: sha.clone(),
        };
        println!(
            "lsp_smoke child {} status={} exit={} duration_ms={} timeout_seconds={} \
             attempts={} log={}",
            spec.id,
            status.as_str(),
            entry.exit_code.map(|code| code.to_string()).unwrap_or_else(|| "signal".to_string()),
            duration_ms,
            spec.timeout_seconds,
            outcome.attempts,
            log_file_name(spec),
        );
        match spec.kind {
            ChildKind::Setup => setup_ok = status == ChildStatus::Pass,
            ChildKind::Compile => {
                compile_ok_by_target.insert(spec.target, status == ChildStatus::Pass);
                // Resolve the prebuilt test binary this compile child named —
                // behavior children execute it directly instead of re-entering
                // cargo (see `command_line`).
                let resolved = if status == ChildStatus::Pass {
                    parse_executable_path(&outcome.stdout, spec.target)
                        .map(|relative| root.join(relative))
                } else {
                    None
                };
                executable_by_target.insert(spec.target, resolved);
            }
            ChildKind::Behavior => {}
        }
        entries.push(entry);
        persist(&receipt_path, &sha, &entries, &specs, PersistState::Running(None))?;
    }

    let final_aggregate = aggregate(&entries, &specs);
    persist(&receipt_path, &sha, &entries, &specs, PersistState::Final(&final_aggregate))?;
    Ok(SuiteOutcome { aggregate: final_aggregate })
}

fn persist(
    receipt_path: &Path,
    sha: &str,
    executed: &[ChildEntry],
    specs: &[ChildSpec],
    state: PersistState<'_>,
) -> Result<()> {
    let doc = build_doc(sha, executed, specs, &state)?;
    let json = serde_json::to_string_pretty(&doc).context("failed to encode child receipt")?;
    let parent = receipt_path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let mut temporary = NamedTempFile::new_in(parent).with_context(|| {
        format!("failed to create temporary child receipt in {}", parent.display())
    })?;
    temporary.write_all(format!("{json}\n").as_bytes()).with_context(|| {
        format!("failed to write temporary child receipt {}", receipt_path.display())
    })?;
    temporary.flush().with_context(|| {
        format!("failed to flush temporary child receipt {}", receipt_path.display())
    })?;
    temporary.as_file().sync_all().with_context(|| {
        format!("failed to sync temporary child receipt {}", receipt_path.display())
    })?;
    temporary.persist(receipt_path).map_err(|error| {
        color_eyre::eyre::eyre!(
            "failed to atomically replace child receipt {}: {}",
            receipt_path.display(),
            error.error
        )
    })?;
    Ok(())
}

fn build_doc(
    sha: &str,
    executed: &[ChildEntry],
    specs: &[ChildSpec],
    state: &PersistState<'_>,
) -> Result<ChildReceiptDoc> {
    let mut children: Vec<ChildEntry> = Vec::with_capacity(specs.len());
    for spec in specs {
        if let Some(entry) = executed.iter().find(|entry| entry.id == spec.id) {
            children.push(entry.clone());
            continue;
        }
        if let PersistState::Running(Some(running)) = state
            && running.id == spec.id
        {
            children.push((*running).clone());
            continue;
        }
        children.push(placeholder(spec, "pending", sha));
    }
    let (suite_state, aggregate) = match state {
        PersistState::Running(_) => ("running", None),
        PersistState::Final(agg) => ("complete", Some((*agg).clone())),
    };
    Ok(ChildReceiptDoc {
        schema_version: RECEIPT_SCHEMA_VERSION,
        gate: "lsp_smoke".to_string(),
        subject_sha: sha.to_string(),
        suite_state: suite_state.to_string(),
        children,
        aggregate,
    })
}

/// Build the in-flight receipt row from the exact context that will be passed
/// to the executor. Cleanup is deliberately `unobserved` until the child
/// returns, because an outer watchdog can interrupt the suite between spawn
/// and the next persistence point.
fn running_entry(
    spec: &ChildSpec,
    sha: &str,
    executable: Option<&Path>,
    working_dir: Option<&Path>,
) -> ChildEntry {
    let (env_set, env_unset) = child_env_record(spec.kind, working_dir);
    ChildEntry {
        id: spec.id.to_string(),
        kind: spec.kind,
        target: spec.target.to_string(),
        test: spec.test_filter.map(str::to_string),
        status: ChildStatus::Cancelled,
        execution_mark: "running".to_string(),
        attempt: 0,
        attempts: 0,
        timeout_seconds: spec.timeout_seconds,
        command: Some(command_line(
            spec,
            executable.map(|path| path.display().to_string()).as_deref(),
        )),
        executable: executable.map(|path| path.display().to_string()),
        env_set,
        env_unset,
        started_at: Some(Utc::now().to_rfc3339()),
        ended_at: None,
        duration_ms: None,
        exit_code: None,
        timed_out: false,
        failure_summary: None,
        log_path: Some(format!("logs/{}", log_file_name(spec))),
        working_dir: working_dir.map(|dir| dir.display().to_string()),
        cleanup: Some("unobserved".to_string()),
        attempt_history: Vec::new(),
        subject_sha: sha.to_string(),
    }
}

/// CLI entry: run the suite, print the verdict, fail closed.
pub fn run_cli(receipt_path: &Path) -> Result<()> {
    let mut executor = CargoChildExecutor;
    let outcome = run_suite(receipt_path, &mut executor)?;
    let agg = outcome.aggregate;
    println!("lsp_smoke aggregate: {} ({}/{} children pass)", agg.status, agg.passed, agg.total);
    if agg.status != "pass" {
        bail!("lsp_smoke aggregate FAIL — non-success children: {}", agg.non_success.join(", "));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use perl_tdd_support::{must_some_with, must_with};
    use std::path::PathBuf;

    // ---------------------------------------------------------------------
    // Scripted executor
    // ---------------------------------------------------------------------

    struct ScriptedExecutor {
        outcomes: HashMap<&'static str, RawOutcome>,
        executed: Vec<&'static str>,
    }

    impl ScriptedExecutor {
        fn new() -> Self {
            Self { outcomes: HashMap::new(), executed: Vec::new() }
        }

        fn pass_child(&mut self, id: &'static str) {
            self.outcomes.insert(id, passing_outcome());
        }

        fn set(&mut self, id: &'static str, outcome: RawOutcome) {
            self.outcomes.insert(id, outcome);
        }
    }

    fn passed_output(n: u64) -> String {
        format!("running {n} test\ntest result: ok. {n} passed; 0 failed; 0 ignored\n")
    }

    fn passing_outcome() -> RawOutcome {
        RawOutcome {
            exit_code: 0,
            timed_out: false,
            stdout: passed_output(1),
            spawn_failed: false,
            attempts: 1,
            history: Vec::new(),
        }
    }

    impl ChildExecutor for ScriptedExecutor {
        fn execute(
            &mut self,
            spec: &ChildSpec,
            _executable: Option<&Path>,
            _working_dir: Option<&Path>,
            _log_path: &Path,
        ) -> Result<RawOutcome> {
            self.executed.push(spec.id);
            Ok(self.outcomes.get(spec.id).cloned().unwrap_or_else(passing_outcome))
        }
    }

    fn temp_receipt_path(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("plsw-8063-tests-{}-{name}", std::process::id()));
        let _ = fs::create_dir_all(&dir);
        dir.join("lsp_smoke_children.json")
    }

    fn cleanup(path: &Path) {
        if let Some(parent) = path.parent() {
            let _ = fs::remove_dir_all(parent);
        }
    }

    fn all_pass_executor() -> ScriptedExecutor {
        let mut executor = ScriptedExecutor::new();
        for spec in child_specs() {
            match spec.kind {
                // Compile children must name their prebuilt test binary so
                // behavior children have a resolvable executable to run.
                ChildKind::Compile => executor.set(
                    spec.id,
                    behavior_outcome(
                        0,
                        false,
                        &format!(
                            "   Finished `test` profile in 0.1s
   Executable tests/{}.rs                              (target/debug/deps/{}-e6b16757b69b565f)",
                            spec.target, spec.target
                        ),
                    ),
                ),
                ChildKind::Setup | ChildKind::Behavior => executor.pass_child(spec.id),
            }
        }
        executor
    }

    fn read_receipt(path: &Path) -> ChildReceiptDoc {
        let raw = must_with(fs::read_to_string(path), "read receipt");
        must_with(serde_json::from_str(&raw), "parse receipt")
    }

    fn run_suite_ok(path: &Path, executor: &mut ScriptedExecutor) -> SuiteOutcome {
        must_with(run_suite(path, executor), "suite should run")
    }

    fn pinned_child_position(specs: &[ChildSpec], id: &str) -> usize {
        must_some_with(
            specs.iter().position(|spec| spec.id == id),
            format_args!("pinned child {id} must exist"),
        )
    }

    fn pinned_spec(id: &str) -> ChildSpec {
        must_some_with(
            child_specs().into_iter().find(|spec| spec.id == id),
            format_args!("pinned child {id} must exist"),
        )
    }

    fn first_behavior_spec() -> ChildSpec {
        must_some_with(
            child_specs().into_iter().find(|spec| spec.kind == ChildKind::Behavior),
            "behavior child",
        )
    }

    fn required_child<'a>(doc: &'a ChildReceiptDoc, id: &str) -> &'a ChildEntry {
        must_some_with(
            doc.children.iter().find(|child| child.id == id),
            format_args!("child {id} retained"),
        )
    }

    fn required_failure_summary(outcome: &RawOutcome, context: &str) -> String {
        must_some_with(summarize_failure(outcome), context)
    }

    fn build_doc_ok(
        sha: &str,
        executed: &[ChildEntry],
        specs: &[ChildSpec],
        state: &PersistState<'_>,
    ) -> ChildReceiptDoc {
        must_with(build_doc(sha, executed, specs, state), "doc should build")
    }

    fn last_pinned_spec(specs: &[ChildSpec]) -> &ChildSpec {
        must_some_with(specs.last(), "pinned child set is non-empty")
    }

    // ---------------------------------------------------------------------
    // Child spec invariants
    // ---------------------------------------------------------------------

    #[test]
    fn child_set_is_pinned_and_ordered() {
        let specs = child_specs();
        let ids: Vec<&str> = specs.iter().map(|spec| spec.id).collect();
        assert_eq!(
            ids,
            vec![
                "compile/lsp_api_contracts",
                "lsp_api_contracts/textdocument_sync_camel_case",
                "compile/semantic_definition",
                "semantic_definition/client_support_registry",
                "setup/build_perllsp",
                "semantic_definition/scalar_variable",
                "semantic_definition/subroutine",
                "semantic_definition/scoped_variable",
                "semantic_definition/package_qualified_call",
            ],
            "the required child set is the #8063 minimum plus the registry group; \
             drift must be deliberate"
        );
        let position = |id: &str| pinned_child_position(&specs, id);
        assert!(
            position("compile/lsp_api_contracts")
                < position("lsp_api_contracts/textdocument_sync_camel_case"),
            "the API behavior must follow its own compile"
        );
        assert!(
            position("compile/semantic_definition")
                < position("semantic_definition/client_support_registry"),
            "the registry behavior must follow its own target compile"
        );
        assert!(
            position("semantic_definition/client_support_registry")
                < position("setup/build_perllsp"),
            "setup-independent registry evidence must precede retryable product-server setup"
        );
        for spec in specs.iter().filter(|spec| spec.requires_setup) {
            assert!(
                position("setup/build_perllsp") < position(spec.id)
                    && position("compile/semantic_definition") < position(spec.id),
                "server-dependent behavior must follow setup and its target compile: {}",
                spec.id
            );
        }
    }

    #[test]
    fn behavior_command_passes_libtest_options_directly() {
        let spec = pinned_spec("semantic_definition/scalar_variable");
        assert_eq!(
            command_line(&spec, Some("target/debug/deps/semantic_definition-test")),
            "\"target/debug/deps/semantic_definition-test\" \
semantic_definition_tests::definition_finds_scalar_variable_declaration --exact \
--test-threads=1"
        );
    }

    #[test]
    fn commands_are_atomic_no_composite_operators() {
        for spec in child_specs() {
            let command = command_line(&spec, None);
            assert!(!command.contains("&&"), "child commands must be atomic: {command}");
            match spec.kind {
                ChildKind::Setup => assert!(command.starts_with("cargo build -p perllsp")),
                ChildKind::Compile => assert!(command.contains("--no-run")),
                // Behavior children must execute a prebuilt binary directly:
                // re-entering cargo from the behavior path is the hosted
                // rebuild-cascade mechanism observed in run 32681631693.
                ChildKind::Behavior => {
                    assert!(
                        !command.starts_with("cargo"),
                        "behavior children must not invoke cargo: {command}"
                    );
                    assert!(command.contains("--test-threads=1"));
                    assert!(
                        command.contains("<unresolved-prebuilt-test-binary>"),
                        "an unresolvable path must be visible in the recorded command, not hidden"
                    );
                }
            }
            let resolved =
                command_line(&spec, Some("target/debug/deps/semantic_definition-deadbeef"));
            if spec.kind == ChildKind::Behavior {
                assert!(resolved.contains("semantic_definition-deadbeef"));
            }
        }
    }

    #[test]
    fn parse_executable_path_extracts_target_binary_from_colored_output() {
        // Exact shape captured from the hosted lsp shard run 32681631693,
        // including the ANSI SGR codes CARGO_TERM_COLOR=always emits.
        let colored = "\u{1b}[1m\u{1b}[92m  Executable\u{1b}[0m \
tests/semantic_definition.rs (target/debug/deps/semantic_definition-e6b16757b69b565f)";
        let parsed = parse_executable_path(colored, "semantic_definition");
        assert_eq!(
            parsed.as_deref(),
            Some(std::path::Path::new("target/debug/deps/semantic_definition-e6b16757b69b565f"))
        );

        let plain = "   Executable tests/lsp_api_contracts.rs (target/debug/deps/lsp_api_contracts-0123abcd)";
        assert_eq!(
            parse_executable_path(plain, "lsp_api_contracts").as_deref(),
            Some(std::path::Path::new("target/debug/deps/lsp_api_contracts-0123abcd"))
        );
        let windows = r"   Executable tests\semantic_definition.rs (target\debug\deps\semantic_definition-e6b16757b69b565f)";
        assert_eq!(
            parse_executable_path(windows, "semantic_definition").as_deref(),
            Some(std::path::Path::new("target/debug/deps/semantic_definition-e6b16757b69b565f"))
        );

        // Wrong target's Executable line must not satisfy another target.
        assert_eq!(parse_executable_path(colored, "lsp_api_contracts"), None);
        // A compile failure output names no executable at all.
        assert_eq!(parse_executable_path("error: could not compile", "semantic_definition"), None);
    }

    // Fail-closed: a compile child that passed without naming a prebuilt
    // binary must never let behavior children execute (or pass).
    #[test]
    fn suite_fails_closed_when_compiled_binary_path_is_unresolvable() {
        let path = temp_receipt_path("unresolved");
        let mut executor = all_pass_executor();
        // Compile "passes" but its output names no Executable — spec/target
        // drift in cargo's output format, or an unexpected locale.
        executor.set(
            "compile/semantic_definition",
            behavior_outcome(0, false, "   Finished `test` profile in 0.1s"),
        );
        let outcome = run_suite_ok(&path, &mut executor);
        assert_eq!(outcome.aggregate.status, "fail");
        assert!(
            outcome
                .aggregate
                .non_success
                .contains(&"semantic_definition/scalar_variable".to_string()),
            "children depending on an unresolvable binary are non-success: {:?}",
            outcome.aggregate.non_success
        );
        assert!(
            !executor.executed.contains(&"semantic_definition/scalar_variable"),
            "the behavior child must not execute without a resolved binary"
        );
        let doc = read_receipt(&path);
        let child = required_child(&doc, "semantic_definition/scalar_variable");
        assert_eq!(child.status, ChildStatus::InstrumentFailure);
        assert!(child.failure_summary.as_deref().is_some_and(|s| s.contains("no prebuilt")));
        // The API lane still executes: its compile resolved normally.
        assert!(executor.executed.contains(&"lsp_api_contracts/textdocument_sync_camel_case"));
        cleanup(&path);
    }

    // ---------------------------------------------------------------------
    // Classification matrix (#8063 §4 / negative controls 2, 4, 5)
    // ---------------------------------------------------------------------

    fn behavior_outcome(exit_code: i32, timed_out: bool, stdout: &str) -> RawOutcome {
        RawOutcome {
            exit_code,
            timed_out,
            stdout: stdout.to_string(),
            spawn_failed: false,
            attempts: 1,
            history: Vec::new(),
        }
    }

    #[test]
    fn classify_behavior_pass_requires_executed_tests() {
        let pass = classify(ChildKind::Behavior, &behavior_outcome(0, false, &passed_output(1)));
        assert_eq!(pass, ChildStatus::Pass);

        // Fail-closed: filter matched zero tests, cargo exits 0.
        let zero = classify(
            ChildKind::Behavior,
            &behavior_outcome(0, false, "running 0 tests\ntest result: ok. 0 passed; 0 failed\n"),
        );
        assert_eq!(zero, ChildStatus::InstrumentFailure, "zero-test runs must not pass");
    }

    #[test]
    fn classify_distinguishes_request_timeout_from_teardown_timeout() {
        let request = classify(
            ChildKind::Behavior,
            &behavior_outcome(124, true, "running 1 test\ntest x ... "),
        );
        assert_eq!(request, ChildStatus::RequestTimeout);

        let teardown = classify(
            ChildKind::Behavior,
            &behavior_outcome(124, true, "test x ... ok\ntest result: ok. 1 passed; 0 failed\n"),
        );
        assert_eq!(teardown, ChildStatus::TeardownTimeout);
    }

    #[test]
    fn classify_distinguishes_assertion_failure_from_process_exit() {
        let assertion = classify(
            ChildKind::Behavior,
            &behavior_outcome(
                101,
                false,
                "test x ... FAILED\nthread 'x' panicked at src/lib.rs:4:\nassertion failed\n\
                 test result: FAILED. 0 passed; 1 failed\n",
            ),
        );
        assert_eq!(assertion, ChildStatus::AssertionFailure);

        let crash = classify(
            ChildKind::Behavior,
            &behavior_outcome(1, false, "server process exited unexpectedly\n"),
        );
        assert_eq!(crash, ChildStatus::ProcessExit);
    }

    #[test]
    fn classify_compile_and_instrument_classes() {
        let compile_ok = classify(ChildKind::Compile, &behavior_outcome(0, false, "Finished"));
        assert_eq!(compile_ok, ChildStatus::Pass);
        let compile_fail = classify(
            ChildKind::Setup,
            &behavior_outcome(101, false, "error: could not compile `perllsp`"),
        );
        assert_eq!(compile_fail, ChildStatus::CompileFailure);
        let compile_timeout = classify(ChildKind::Compile, &behavior_outcome(124, true, ""));
        assert_eq!(compile_timeout, ChildStatus::CompileFailure);

        let mut spawn_failed = behavior_outcome(0, false, "");
        spawn_failed.spawn_failed = true;
        assert_eq!(classify(ChildKind::Behavior, &spawn_failed), ChildStatus::InstrumentFailure);
    }

    #[test]
    fn failure_summaries_are_bounded_and_retained() {
        let outcome = behavior_outcome(
            101,
            false,
            "test x ... FAILED\nthread panicked at a.rs:1:\nboom\ntest result: FAILED.\n",
        );
        let summary = required_failure_summary(&outcome, "summary for assertion failure");
        assert!(summary.contains("panicked at"), "summary retains the failure site");

        let mut long_outcome = behavior_outcome(0, false, &String::new());
        let mut long = String::from("error: could not compile\n");
        long.push_str(&"x".repeat(10_000));
        long_outcome.stdout = long;
        let summary = required_failure_summary(&long_outcome, "summary for compile failure");
        assert!(summary.len() <= MAX_FAILURE_SUMMARY_CHARS + 8);
    }

    // ---------------------------------------------------------------------
    // Suite semantics (#8063 negative controls 1, 2, 3, 6, 7, 8, 9)
    // ---------------------------------------------------------------------

    #[test]
    fn suite_all_pass_passes_and_writes_complete_receipt() {
        let path = temp_receipt_path("all-pass");
        let mut executor = all_pass_executor();
        let outcome = run_suite_ok(&path, &mut executor);
        assert_eq!(outcome.aggregate.status, "pass");
        assert_eq!(outcome.aggregate.total, child_specs().len());

        let doc = read_receipt(&path);
        assert_eq!(doc.suite_state, "complete");
        assert!(doc.aggregate.as_ref().is_some_and(|agg| agg.status == "pass"));
        assert!(doc.children.iter().all(|child| child.status == ChildStatus::Pass));
        // Behavior children record the cargo-equivalent working directory
        // and the ABSOLUTE CARGO_MANIFEST_DIR cargo itself would set.
        for child in &doc.children {
            match child.kind {
                ChildKind::Behavior => {
                    assert!(
                        child
                            .working_dir
                            .as_deref()
                            .is_some_and(|dir| dir.ends_with(BEHAVIOR_WORKING_DIR)),
                        "behavior children must run from the package directory cargo uses: {:?}",
                        child.working_dir
                    );
                    let manifest_dir = child.env_set.get("CARGO_MANIFEST_DIR");
                    assert!(
                        manifest_dir.is_some_and(|dir| dir.ends_with(BEHAVIOR_WORKING_DIR)
                            && Path::new(dir).is_absolute()),
                        "CARGO_MANIFEST_DIR must be cargo's absolute package path: {manifest_dir:?}"
                    );
                }
                ChildKind::Setup | ChildKind::Compile => {
                    assert!(child.working_dir.is_none())
                }
            }
        }
        // Retained fields the issue requires for every child.
        for child in &doc.children {
            assert!(!child.subject_sha.is_empty(), "child carries subject SHA");
            assert!(child.command.as_deref().is_some_and(|c| !c.is_empty()));
            assert!(child.log_path.as_deref().is_some_and(|l| !l.is_empty()));
            assert!(child.timeout_seconds > 0);
        }
        cleanup(&path);
    }

    // Negative control 1: scalar assertion fails; siblings still execute.
    #[test]
    fn suite_assertion_failure_does_not_erase_siblings() {
        let path = temp_receipt_path("assert");
        let mut executor = all_pass_executor();
        executor.set(
            "semantic_definition/scalar_variable",
            behavior_outcome(
                101,
                false,
                "test ... FAILED\npanicked at tests/semantic_definition.rs:47:\n\
                 assertion failed\ntest result: FAILED. 0 passed; 1 failed\n",
            ),
        );
        let outcome = run_suite_ok(&path, &mut executor);
        assert_eq!(outcome.aggregate.status, "fail");
        assert_eq!(outcome.aggregate.non_success, vec!["semantic_definition/scalar_variable"]);
        assert_eq!(
            executor.executed.len(),
            child_specs().len(),
            "all children must execute despite one assertion failure"
        );
        cleanup(&path);
    }

    // Negative control 2: a hanging child is bounded and the API child runs.
    #[test]
    fn suite_hanging_child_is_bounded_and_api_child_still_executes() {
        let path = temp_receipt_path("hang");
        let mut executor = all_pass_executor();
        executor.set(
            "semantic_definition/package_qualified_call",
            behavior_outcome(124, true, "running 1 test\ntest pkg ... "),
        );
        let outcome = run_suite_ok(&path, &mut executor);
        assert_eq!(outcome.aggregate.status, "fail");
        assert!(
            outcome
                .aggregate
                .non_success
                .contains(&"semantic_definition/package_qualified_call".to_string())
        );
        assert!(
            executor.executed.contains(&"lsp_api_contracts/textdocument_sync_camel_case"),
            "API-contract child must execute regardless of semantic hang outcomes"
        );
        let doc = read_receipt(&path);
        let hung = required_child(&doc, "semantic_definition/package_qualified_call");
        assert_eq!(hung.status, ChildStatus::RequestTimeout);
        assert_eq!(hung.cleanup.as_deref(), Some("unobserved after timeout"));
        assert!(hung.failure_summary.as_deref().is_some_and(|s| !s.is_empty()));
        cleanup(&path);
    }

    // Negative control 3: compile failure blocks dependents as NOT_PROVEN,
    // not assertion failures; independent targets still execute.
    #[test]
    fn suite_compile_failure_blocks_dependents_as_not_proven() {
        let path = temp_receipt_path("compile-fail");
        let mut executor = all_pass_executor();
        executor.set(
            "compile/semantic_definition",
            behavior_outcome(101, false, "error: could not compile `perl-lsp-rs`"),
        );
        let outcome = run_suite_ok(&path, &mut executor);
        assert_eq!(outcome.aggregate.status, "fail");
        assert_eq!(
            outcome.aggregate.non_success,
            vec![
                "compile/semantic_definition",
                "semantic_definition/client_support_registry",
                "semantic_definition/scalar_variable",
                "semantic_definition/subroutine",
                "semantic_definition/scoped_variable",
                "semantic_definition/package_qualified_call",
            ],
            "semantic dependents are blocked; the API lane still executes"
        );
        assert!(!executor.executed.contains(&"semantic_definition/scalar_variable"));
        assert!(executor.executed.contains(&"lsp_api_contracts/textdocument_sync_camel_case"));
        let doc = read_receipt(&path);
        let blocked = required_child(&doc, "semantic_definition/scalar_variable");
        assert_eq!(blocked.status, ChildStatus::NotProven);
        assert_eq!(blocked.execution_mark, "final");
        cleanup(&path);
    }

    // Prerequisite independence is per target: an API compile failure must
    // not block semantic children.
    #[test]
    fn suite_api_compile_failure_does_not_block_semantic_children() {
        let path = temp_receipt_path("api-compile-fail");
        let mut executor = all_pass_executor();
        executor.set(
            "compile/lsp_api_contracts",
            behavior_outcome(101, false, "error: could not compile `perl-lsp-rs`"),
        );
        let outcome = run_suite_ok(&path, &mut executor);
        assert_eq!(
            outcome.aggregate.non_success,
            vec!["compile/lsp_api_contracts", "lsp_api_contracts/textdocument_sync_camel_case",],
            "only the API lane is blocked by an API compile failure"
        );
        assert!(executor.executed.contains(&"semantic_definition/scalar_variable"));
        assert!(!executor.executed.contains(&"lsp_api_contracts/textdocument_sync_camel_case"));
        cleanup(&path);
    }

    // A setup (perllsp prebuild) timeout, including its retry, blocks only
    // server-dependent semantic behavior; the in-process API contract remains
    // independently provable.
    #[test]
    fn suite_setup_failure_does_not_block_in_process_api_child() {
        let path = temp_receipt_path("setup-fail");
        let mut executor = all_pass_executor();
        executor.set("setup/build_perllsp", behavior_outcome(124, true, "building perllsp"));
        let outcome = run_suite_ok(&path, &mut executor);
        assert!(outcome.aggregate.status == "fail");
        assert_eq!(
            outcome.aggregate.non_success,
            vec![
                "setup/build_perllsp",
                "semantic_definition/scalar_variable",
                "semantic_definition/subroutine",
                "semantic_definition/scoped_variable",
                "semantic_definition/package_qualified_call",
            ],
            "a setup timeout blocks server-dependent behavior; the in-process API lane still runs"
        );
        let doc = read_receipt(&path);
        let blocked = required_child(&doc, "semantic_definition/scalar_variable");
        assert_eq!(blocked.status, ChildStatus::NotProven);
        assert!(
            executor.executed.contains(&"lsp_api_contracts/textdocument_sync_camel_case"),
            "the in-process API child must execute despite product-server setup failure"
        );
        assert!(
            executor.executed.contains(&"semantic_definition/client_support_registry"),
            "the setup-independent registry child must execute before product-server setup"
        );
        cleanup(&path);
    }

    // Negative control 6: API failure while semantic children pass.
    #[test]
    fn suite_api_failure_identifies_that_child_only() {
        let path = temp_receipt_path("api-fail");
        let mut executor = all_pass_executor();
        executor.set(
            "lsp_api_contracts/textdocument_sync_camel_case",
            behavior_outcome(
                101,
                false,
                "test ... FAILED\npanicked at tests/lsp_api_contracts.rs:104:\n\
                 camelCase\ntest result: FAILED.\n",
            ),
        );
        let outcome = run_suite_ok(&path, &mut executor);
        assert_eq!(
            outcome.aggregate.non_success,
            vec!["lsp_api_contracts/textdocument_sync_camel_case"]
        );
        cleanup(&path);
    }

    // Negative control 7 (no-retry half): behavior outcomes are single-attempt.
    #[test]
    fn suite_records_attempt_identity() {
        let path = temp_receipt_path("attempts");
        let mut executor = all_pass_executor();
        let timeout = behavior_outcome(124, true, "running 1 test\ntest x ... ");
        executor.set("lsp_api_contracts/textdocument_sync_camel_case", timeout);
        let outcome = run_suite_ok(&path, &mut executor);
        assert_eq!(outcome.aggregate.status, "fail");
        let doc = read_receipt(&path);
        let child = required_child(&doc, "lsp_api_contracts/textdocument_sync_camel_case");
        assert_eq!(child.attempt, 1);
        assert_eq!(child.attempts, 1, "behavior timeouts are never retried");
        assert!(child.attempt_history.is_empty());
        cleanup(&path);
    }

    // Negative controls 8/9 (snapshot half): unexecuted children persist as
    // CANCELLED with explicit marks; cancellation cannot create pass.
    #[test]
    fn snapshot_marks_running_and_pending_children_as_cancelled() {
        let sha = "deadbeef";
        let specs = child_specs();
        let executed: Vec<ChildEntry> = vec![placeholder(&specs[0], "final", sha)];
        let running = running_entry(&specs[1], sha, None, None);
        let doc = build_doc_ok(sha, &executed, &specs, &PersistState::Running(Some(&running)));
        assert_eq!(doc.suite_state, "running");
        assert!(doc.aggregate.is_none(), "no aggregate before the suite completes");
        let running = required_child(&doc, specs[1].id);
        assert_eq!(running.status, ChildStatus::Cancelled);
        assert_eq!(running.execution_mark, "running");
        assert_eq!(running.cleanup.as_deref(), Some("unobserved"));
        let pending = required_child(&doc, specs[2].id);
        assert_eq!(pending.status, ChildStatus::Cancelled);
        assert_eq!(pending.execution_mark, "pending");
        // Fail-closed: an incomplete table aggregates to fail.
        let agg = aggregate(&doc.children, &specs);
        assert_eq!(agg.status, "fail");
        assert!(agg.non_success.contains(&specs[1].id.to_string()));
    }

    #[test]
    fn aggregate_rejects_duplicate_substitution_for_missing_child() {
        let path = temp_receipt_path("duplicate-aggregate");
        let mut executor = all_pass_executor();
        let _ = run_suite_ok(&path, &mut executor);
        let doc = read_receipt(&path);
        let specs = child_specs();
        let missing_id = last_pinned_spec(&specs).id;
        let mut entries = doc.children;
        entries.pop();
        entries.push(entries[0].clone());

        let aggregate_doc = aggregate(&entries, &specs);
        assert_eq!(aggregate_doc.status, "fail");
        assert!(
            aggregate_doc.non_success.contains(&missing_id.to_string()),
            "missing required child must be named: {:?}",
            aggregate_doc.non_success
        );
        assert_eq!(aggregate_doc.passed, specs.len() - 2);
        assert_eq!(aggregate_doc.total, specs.len());

        let mut unexpected_entries = read_receipt(&path).children;
        unexpected_entries.pop();
        let mut unexpected = unexpected_entries[0].clone();
        unexpected.id = "unexpected/pass".to_string();
        unexpected_entries.push(unexpected);
        let aggregate_doc = aggregate(&unexpected_entries, &specs);
        assert_eq!(aggregate_doc.status, "fail");
        assert!(aggregate_doc.non_success.contains(&"unexpected/pass".to_string()));
        assert_eq!(aggregate_doc.passed, specs.len() - 1);
        cleanup(&path);
    }

    #[test]
    fn running_snapshot_retains_resolved_execution_context() {
        let spec = first_behavior_spec();
        let executable = Path::new("target/debug/deps/semantic_definition-deadbeef");
        let working_dir = Path::new("crates/perl-lsp-rs");
        let entry = running_entry(&spec, "deadbeef", Some(executable), Some(working_dir));

        assert_eq!(entry.execution_mark, "running");
        assert_eq!(entry.status, ChildStatus::Cancelled);
        assert_eq!(entry.executable.as_deref(), Some(executable.to_string_lossy().as_ref()));
        assert_eq!(entry.working_dir.as_deref(), Some(working_dir.to_string_lossy().as_ref()));
        assert_eq!(entry.env_set.get("RUST_TEST_THREADS"), Some(&"1".to_string()));
        assert_eq!(
            entry.env_set.get("CARGO_MANIFEST_DIR"),
            Some(&working_dir.to_string_lossy().to_string())
        );
        assert_eq!(entry.cleanup.as_deref(), Some("unobserved"));
        assert!(entry.command.as_deref().is_some_and(|command| command.contains("deadbeef")));
    }

    // Fail-closed negative control: instrument failure everywhere still
    // aggregates to fail, and an empty child table can never pass.
    #[test]
    fn suite_fails_closed_on_instrument_failure() {
        assert_eq!(aggregate(&[], &[]).status, "fail");
        assert_eq!(aggregate(&[], &child_specs()).status, "fail");

        let path = temp_receipt_path("instrument");
        let mut executor = ScriptedExecutor::new();
        for spec in child_specs() {
            let mut outcome = behavior_outcome(0, false, "");
            outcome.spawn_failed = true;
            executor.set(spec.id, outcome);
        }
        let outcome = run_suite_ok(&path, &mut executor);
        assert_eq!(outcome.aggregate.status, "fail");
        assert_eq!(outcome.aggregate.non_success.len(), child_specs().len());
        let doc = read_receipt(&path);
        assert!(doc.children.iter().all(|child| child.status == ChildStatus::InstrumentFailure
            || child.status == ChildStatus::NotProven));
        cleanup(&path);
    }

    // Negative control 5: teardown timeout keeps response evidence separate.
    #[test]
    fn suite_teardown_timeout_is_distinct_from_request_timeout() {
        let path = temp_receipt_path("teardown");
        let mut executor = all_pass_executor();
        executor.set(
            "semantic_definition/scoped_variable",
            behavior_outcome(
                124,
                true,
                "test scoped ... ok\ntest result: ok. 1 passed; 0 failed\n",
            ),
        );
        let outcome = run_suite_ok(&path, &mut executor);
        assert_eq!(outcome.aggregate.status, "fail");
        let doc = read_receipt(&path);
        let child = required_child(&doc, "semantic_definition/scoped_variable");
        assert_eq!(child.status, ChildStatus::TeardownTimeout);
        assert!(child.failure_summary.as_deref().is_some_and(|s| s.contains("teardown")));
        cleanup(&path);
    }

    // -------------------------------------------------------------------
    // Conversion falsifiers: helpers still fail with named context
    // -------------------------------------------------------------------

    #[test]
    #[should_panic(expected = "must: read receipt:")]
    fn lsp_smoke_converted_result_assertion_still_fails_when_receipt_is_unreadable() {
        let path = temp_receipt_path("missing-receipt-file");
        let _ = read_receipt(&path);
    }

    #[test]
    #[should_panic(expected = "must_some: child absent/child retained:")]
    fn lsp_smoke_converted_option_assertion_still_fails_when_child_is_absent() {
        let doc = ChildReceiptDoc {
            schema_version: RECEIPT_SCHEMA_VERSION,
            gate: "lsp_smoke".to_string(),
            subject_sha: "deadbeef".to_string(),
            suite_state: "complete".to_string(),
            children: Vec::new(),
            aggregate: None,
        };
        let _ = required_child(&doc, "absent/child");
    }

    #[test]
    #[should_panic(expected = "must_some: summary for assertion failure:")]
    fn lsp_smoke_converted_option_assertion_still_fails_when_failure_summary_is_absent() {
        let outcome = behavior_outcome(0, false, "");
        let _ = required_failure_summary(&outcome, "summary for assertion failure");
    }
}
