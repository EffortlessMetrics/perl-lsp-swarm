//! LSP-over-stdio round-trip latency benchmark (#17159, first slice).
//!
//! Measures client-side wallclock for the first interaction sequence of an
//! editor session against the real `perllsp` binary on stdio, using the
//! committed `test_corpus/real_projects/mojolicious_skeleton` fixture:
//!
//! 1. `initialize` — process spawn → `initialize` response (+ `initialized`
//!    notification), over cold spawns against the fixture workspace.
//! 2. `did_open_to_diagnostics` — `textDocument/didOpen` → first
//!    `textDocument/publishDiagnostics` for that URI. `didOpen` is a
//!    notification with no response of its own, so the first diagnostics
//!    publish is the observable completion of the open.
//! 3. Warm request lanes — `textDocument/hover`, `textDocument/completion`,
//!    `textDocument/definition`, `workspace/symbol`, each sampled after a
//!    warmup burst on a post-initialize/index session.
//! 4. Completion sub-lanes — `textDocument/completion` at three coordinated
//!    positions on the entry file (member after `$self->`, package member
//!    after `Mojolicious::`, lexical after `$`), each probe-validated then
//!    sampled at a higher request count than the generic lanes (#17159).
//!    The probe-selected coordinates (fallback included) are resolved once
//!    and shared by the receipt pass and the criterion groups, so a lane
//!    name times one subject in both passes (#17731).
//! 5. `diagnostics_after_edit` — `textDocument/didChange` on a dedicated
//!    generated document, waiting for the version-matched
//!    `textDocument/publishDiagnostics` for that exact edit. Duplicate
//!    publications are swept once at the end of the lane so a duplicate for
//!    an earlier version arriving late is still attributed (#17732).
//!
//! Each lane reports p50/p95/p99 over raw wallclock samples and writes a
//! receipt to `.ci/metrics/lsp_roundtrip.json` (created relative to the
//! workspace root). After the receipt pass, the same warm session is driven
//! through criterion regression groups so steady-state round trips are
//! tracked by the usual `target/criterion` history.
//!
//! # Timing policy
//!
//! No wallclock assertion in this file runs under plain `cargo test`; this is
//! a bench lane driven explicitly, following the receipt policy documented in
//! `crates/perl-lsp-ux-tests/tests/ux_latency_raw_rpc.rs` (arrival is the
//! assertion; the bounds below only fail fast on a wedged binary). Thresholds
//! and nightly ratchets are calibrated on pinned hardware after several
//! receipt runs — the numbers recorded here are baselines, not gates.
//!
//! # Running
//!
//! ```bash
//! # Build the server binary the bench will spawn (release profile so the
//! # receipt measures an optimized server, and so `resolve_binary` finds it
//! # next to the bench binary in target/release/deps).
//! cargo build --release -p perllsp
//!
//! # Full receipt pass + criterion groups
//! cargo bench -p perl-lsp-rs --bench lsp_stdio_roundtrip_benchmark --locked
//!
//! # Criterion groups only (receipt pass still runs; filter after `--`)
//! cargo bench -p perl-lsp-rs --bench lsp_stdio_roundtrip_benchmark -- hover
//! ```
//!
//! `cargo test -p perl-lsp-rs --benches` (and `--all-targets`) runs this binary
//! without `--bench`: the receipt pass is skipped so a plain test run never
//! overwrites the committed baseline, and only the criterion groups execute
//! (once each, in Criterion's test mode).
//!
//! Request coordinates mirror `MOJOLICIOUS_FIXTURE` in
//! `tests/real_project_latency.rs` so this receipt and the real-project
//! receipt stay comparable.

// Bench report channel: the receipt pass and criterion groups print the
// baseline summary and the resolved binary to stderr/stdout; this is not the
// server's stdio transport, so the workspace print_stderr/print_stdout deny
// does not apply the way it does to production code.
#![allow(clippy::print_stdout, clippy::print_stderr)]

use anyhow::{Context, Result, anyhow, bail};
use criterion::Criterion;
use perl_lsp_ux_tests::{
    CompletionProbeOutcome, DiagnosticsTracker, LspEvent, REQUIRE_BINARY_ENV, ScenarioConfig,
    UxClient, UxHarness, classify_completion_probe,
    project_fixture::{fixture_content, fixture_scenario_config, load_mojolicious_fixture_files},
    resolve_binary, validate_completion_response,
};
use serde_json::{Value, json};
use std::cell::Cell;
use std::hint::black_box;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

// ---- Lane configuration ---------------------------------------------------------

/// Warm request samples per lane. The filed plan (#17159) asks for ≥50 sampled
/// requests per method before any percentile claim.
const REQUEST_SAMPLES: usize = 50;

/// Unrecorded warmup requests per lane before sampling begins ("warm" means
/// past the first-call parsing/indexing effects, not cold).
const REQUEST_WARMUP: usize = 5;

/// Cold `initialize` spawn samples. Process spawns are the expensive part of
/// this lane, so the first slice records 10 rather than 50; the dedicated
/// startup-to-ready metric (#17159 item 3) widens this separately.
const INIT_SAMPLES: usize = 10;

/// `didOpen → first publishDiagnostics` samples, one per distinct generated
/// file (a URI is opened at most once, per LSP lifecycle).
const DID_OPEN_SAMPLES: usize = 12;

/// Timed requests per completion sub-lane. The filed plan (#17159) asks for
/// N=200 per position kind before any percentile claim.
const COMPLETION_SUBLANE_SAMPLES: usize = 200;

/// Unrecorded warmup requests per completion sub-lane. The first warmup
/// response doubles as the coordinate probe (see `measure_completion_sublane`).
const COMPLETION_SUBLANE_WARMUP: usize = 10;

/// `didChange → version-matched publishDiagnostics` samples, one edit per
/// sample on the dedicated generated document.
const DIDCHANGE_EDIT_SAMPLES: usize = 50;

/// Settling window between the last sampled edit's arrival and the
/// end-of-lane duplicate sweep. The arrival wait observes only the first
/// push per version; the window lets a duplicate for the final version (and
/// any in-flight straggler) land inside the sweep's horizon. It bounds the
/// attribution window; it cannot extend it indefinitely (#17732).
const DUPLICATE_SWEEP_SETTLE: Duration = Duration::from_millis(500);

/// Bound for one warm request round trip. Arrival is the assertion; this
/// bound only fails fast on a wedged binary (see the module timing policy).
const REQUEST_TIMEOUT: Duration = Duration::from_mins(1);

/// Bound for waiting on the first diagnostics publish after a `didOpen`.
const DIAGNOSTICS_ARRIVAL_BUDGET: Duration = Duration::from_secs(30);

/// Bound for the post-open workspace index readiness wait.
const INDEX_READY_BUDGET: Duration = Duration::from_secs(30);

/// Receipt path relative to the workspace root.
const RECEIPT_PATH: &str = ".ci/metrics/lsp_roundtrip.json";

// ---- Fixture coordinates (mirror tests/real_project_latency.rs) ------------------

const ENTRY_FILE: &str = "lib/Mojolicious.pm";
const HOVER_LINE: u32 = 10;
const HOVER_COL: u32 = 6;
const COMPLETION_LINE: u32 = 14;
const COMPLETION_COL: u32 = 10;
const DEFINITION_LINE: u32 = 5;
const DEFINITION_COL: u32 = 4;
const WORKSPACE_SYMBOL_QUERY: &str = "new";

// ---- Completion sub-lane coordinates ----------------------------------------------

// All coordinates are 0-indexed positions on the already-opened fixture entry
// file `lib/Mojolicious.pm` (the same file the lanes above mirror
// `tests/real_project_latency.rs` on; these sub-lane positions are new in
// #17159 and deliberately reuse that opened document). Each caret sits
// immediately after an advertised trigger character
// (`completion_trigger_characters` in perl-lsp-rs-core/src/protocol/
// capabilities.rs advertises `$ @ % - > : . / \ " '`):
//
// - method position: file line 35 `    $self->startup;`, caret after the `>`
//   of `$self->` (member completion);
// - package prefix: file line 14 `use Mojolicious::Routes;`, caret after the
//   second `:` of `Mojolicious::` (package-member completion);
// - lexical: file line 40 `    my $self = shift;`, caret after the `$`
//   (lexical-variable completion).
//
// Fallback when a warmup probe finds zero items: also open
// `lib/Mojolicious/Controller.pm` and use the method position after the `>`
// of `$self->` on file line 19
// `    return $self->app->routes->continue($self);` (`has app` is declared in
// the same package at file line 11).
//
// Probe guard order is load-bearing (#17731): a JSON-RPC error is a failed
// probe and surfaces as an error before any emptiness check, so the fallback
// only ever follows a genuinely zero-item answer — never a failed request.
// Whichever coordinate the probe selects, the criterion group for the lane
// times the exact same request params (#17731).
const COMPLETION_METHOD_LINE: u32 = 34;
const COMPLETION_METHOD_COL: u32 = 11;
const COMPLETION_PACKAGE_LINE: u32 = 13;
const COMPLETION_PACKAGE_COL: u32 = 17;
const COMPLETION_LEXICAL_LINE: u32 = 39;
const COMPLETION_LEXICAL_COL: u32 = 8;
const COMPLETION_FALLBACK_FILE: &str = "lib/Mojolicious/Controller.pm";
const COMPLETION_FALLBACK_LINE: u32 = 18;
const COMPLETION_FALLBACK_COL: u32 = 18;

/// Workspace-relative path of the dedicated `diagnostics_after_edit` document.
const DIDCHANGE_TARGET_PATH: &str = "bench_didchange_target.pl";

/// Criterion round-trip requests that ended in a JSON-RPC error or timeout.
/// Criterion closures cannot fail, so failures are counted and reported after
/// the groups finish instead of silently flattering the numbers.
static CRITERION_ROUND_TRIP_FAILURES: AtomicU64 = AtomicU64::new(0);

// ---- Session ---------------------------------------------------------------------

/// One warm LSP session over the mojolicious fixture, plus everything the
/// receipt needs to describe how it was measured.
struct RoundTripSession {
    harness: UxHarness,
    /// Configuration the warm session was spawned with; reused verbatim for
    /// the cold `initialize` spawn samples so both lanes measure the same
    /// capabilities and environment.
    config: ScenarioConfig,
    binary_path: String,
    index_ready: bool,
    file_count: usize,
    source_line_count: usize,
}

/// Resolve the session or report why the bench cannot run.
///
/// A missing server binary follows the UX-suite skip contract: silent skip by
/// default, hard failure when `PERL_LSP_UX_REQUIRE_BINARY` is truthy.
fn create_session(binary: &str) -> Result<RoundTripSession> {
    let files = load_mojolicious_fixture_files()
        .context("failed to load mojolicious_skeleton fixture files")?;
    anyhow::ensure!(
        !files.is_empty(),
        "mojolicious_skeleton fixture contains no Perl files; the round-trip bench needs its corpus"
    );
    let file_count = files.len();
    let source_line_count: usize = files.iter().map(|file| file.content.lines().count()).sum();

    let mut config = fixture_scenario_config(&files);
    for index in 0..DID_OPEN_SAMPLES {
        config = config.with_file(did_open_relative_path(index), did_open_source(index));
    }
    config = config.with_file(DIDCHANGE_TARGET_PATH, didchange_target_source());

    let harness = UxHarness::new(config.clone())
        .context("failed to spawn the perl-lsp server for the warm round-trip session")?;

    let entry_content =
        fixture_content(&files, ENTRY_FILE).context("fixture entry file missing")?.to_string();
    harness
        .open_file(ENTRY_FILE, &entry_content)
        .context("failed to open the fixture entry file on the warm session")?;
    let index_ready = harness.wait_for_index_ready(INDEX_READY_BUDGET).is_ok();

    Ok(RoundTripSession {
        harness,
        config,
        binary_path: binary.to_string(),
        index_ready,
        file_count,
        source_line_count,
    })
}

// ---- Lanes -----------------------------------------------------------------------

fn measure_initialize(session: &RoundTripSession) -> Result<Vec<u64>> {
    let mut samples = Vec::with_capacity(INIT_SAMPLES);
    let mut client: Option<UxClient> = None;
    for _ in 0..INIT_SAMPLES {
        if let Some(previous) = client.take() {
            // Orderly drop (bounded shutdown grace) before the next cold
            // spawn sample so consecutive spawns do not contend.
            drop(previous);
        }
        let start = Instant::now();
        let spawned =
            UxClient::spawn(&session.binary_path, &session.harness.workspace, &session.config)
                .context("initialize lane: spawn + handshake failed")?;
        samples.push(millis_since(start));
        client = Some(spawned);
    }
    drop(client);
    Ok(samples)
}

fn measure_did_open_to_diagnostics(session: &RoundTripSession) -> Result<Vec<u64>> {
    let mut samples = Vec::with_capacity(DID_OPEN_SAMPLES);
    for index in 0..DID_OPEN_SAMPLES {
        let relative_path = did_open_relative_path(index);
        let uri = session.harness.workspace.uri(&relative_path);
        let content = did_open_source(index);

        let start = Instant::now();
        session
            .harness
            .client
            .did_open(&uri, &content)
            .with_context(|| format!("didOpen lane: failed to send didOpen for {relative_path}"))?;

        // Event-driven wait on the buffered observation substrate: the first
        // diagnostics publish for this fresh URI is the open's completion.
        let arrival =
            session.harness.client.wait_for_events(DIAGNOSTICS_ARRIVAL_BUDGET, |events| {
                events.iter().find_map(|event| match event {
                    LspEvent::Diagnostics { uri: published, .. } if published == &uri => Some(()),
                    _ => None,
                })
            });
        match arrival {
            Ok(()) => samples.push(millis_since(start)),
            Err(end) => bail!(
                "didOpen lane: no publishDiagnostics arrived for {relative_path}: {}",
                end.describe()
            ),
        }
    }
    Ok(samples)
}

/// Run `request` through a warmup burst and then `REQUEST_SAMPLES` timed
/// round trips.
fn timed_requests(lane: &'static str, mut request: impl FnMut() -> Result<()>) -> Result<Vec<u64>> {
    for _ in 0..REQUEST_WARMUP {
        request().with_context(|| format!("{lane} lane: warmup request failed"))?;
    }
    let mut samples = Vec::with_capacity(REQUEST_SAMPLES);
    for _ in 0..REQUEST_SAMPLES {
        let start = Instant::now();
        request().with_context(|| format!("{lane} lane: sampled request failed"))?;
        samples.push(millis_since(start));
    }
    Ok(samples)
}

// ---- Completion sub-lanes ----------------------------------------------------------

/// One completion sub-lane description: a coordinated position on the entry
/// file plus its advertised trigger character.
struct CompletionSublane {
    lane: &'static str,
    line: u32,
    character: u32,
    trigger: &'static str,
}

/// The three sub-lane descriptors, shared by the probe resolution, the
/// receipt pass, and the criterion groups so every pass names and times the
/// same lane subject (#17731).
const COMPLETION_METHOD_SUBLANE: CompletionSublane = CompletionSublane {
    lane: "completion_method_position",
    line: COMPLETION_METHOD_LINE,
    character: COMPLETION_METHOD_COL,
    trigger: ">",
};
const COMPLETION_PACKAGE_SUBLANE: CompletionSublane = CompletionSublane {
    lane: "completion_package_prefix",
    line: COMPLETION_PACKAGE_LINE,
    character: COMPLETION_PACKAGE_COL,
    trigger: ":",
};
const COMPLETION_LEXICAL_SUBLANE: CompletionSublane = CompletionSublane {
    lane: "completion_lexical",
    line: COMPLETION_LEXICAL_LINE,
    character: COMPLETION_LEXICAL_COL,
    trigger: "$",
};

/// The `textDocument/completion` request shape shared by the sub-lanes (the
/// generic `completion` lane keeps its own inline copy untouched).
fn completion_request_params(uri: &str, line: u32, character: u32, trigger: &str) -> Value {
    json!({
        "textDocument": { "uri": uri },
        "position": { "line": line, "character": character },
        "context": { "triggerKind": 2, "triggerCharacter": trigger }
    })
}

/// Probe-resolved request params for the three completion sub-lanes.
///
/// The probe selection runs once, before the mode split, and the resolved
/// params are shared by the receipt pass and the criterion groups so a lane
/// name times one subject in both passes (#17731). In bench mode a failed
/// probe fails the receipt lane at its measurement point; in test mode the
/// matching criterion group is skipped and counted as a round-trip failure.
struct CompletionSublaneResolutions {
    method_position: Result<Value>,
    package_prefix: Result<Value>,
    lexical: Result<Value>,
}

/// Borrow one probe resolution for sampling; a failed probe fails the
/// receipt lane at its measurement point (bench mode; test mode skips the
/// group instead).
fn resolution_params(resolution: &Result<Value>) -> Result<&Value> {
    resolution.as_ref().map_err(|error| anyhow!("{error:#}"))
}

/// Resolve the request params for one completion sub-lane by probing the
/// primary coordinate.
///
/// Guard order is load-bearing (#17731): the probe response is error-guarded
/// first, so a JSON-RPC error surfaces as a lane failure instead of parsing
/// as an empty list and silently switching the lane's subject; only a
/// genuinely zero-item answer falls back to the proven method position on
/// `lib/Mojolicious/Controller.pm`, which must itself answer with a
/// non-empty list. The resolved params are returned so the criterion group
/// for the lane times the exact subject the receipt pass samples.
fn resolve_completion_sublane_params(
    session: &RoundTripSession,
    sublane: &CompletionSublane,
) -> Result<Value> {
    let entry_uri = session.harness.workspace.uri(ENTRY_FILE);
    let params =
        completion_request_params(&entry_uri, sublane.line, sublane.character, sublane.trigger);

    let probe = session
        .harness
        .client
        .request("textDocument/completion", params.clone(), REQUEST_TIMEOUT)
        .with_context(|| format!("{} lane: probe request failed", sublane.lane))?;
    if matches!(
        classify_completion_probe(&probe)
            .with_context(|| format!("{} lane: probe", sublane.lane))?,
        CompletionProbeOutcome::Verified
    ) {
        return Ok(params);
    }

    eprintln!(
        "WARNING: {} lane probe at (line {}, char {}) returned zero items; \
         falling back to the {COMPLETION_FALLBACK_FILE} method position",
        sublane.lane, sublane.line, sublane.character,
    );
    session
        .harness
        .open_fixture(COMPLETION_FALLBACK_FILE)
        .with_context(|| format!("{} lane: failed to open the fallback file", sublane.lane))?;
    let fallback_uri = session.harness.workspace.uri(COMPLETION_FALLBACK_FILE);
    let fallback_params = completion_request_params(
        &fallback_uri,
        COMPLETION_FALLBACK_LINE,
        COMPLETION_FALLBACK_COL,
        ">",
    );
    let fallback_probe = session
        .harness
        .client
        .request("textDocument/completion", fallback_params.clone(), REQUEST_TIMEOUT)
        .with_context(|| format!("{} lane: fallback probe request failed", sublane.lane))?;
    anyhow::ensure!(
        matches!(
            classify_completion_probe(&fallback_probe)
                .with_context(|| format!("{} lane: fallback probe", sublane.lane))?,
            CompletionProbeOutcome::Verified
        ),
        "{} lane: probe at (line {}, char {}) and fallback probe at \
         (line {COMPLETION_FALLBACK_LINE}, char {COMPLETION_FALLBACK_COL}) both returned \
         zero completion items — the coordinates are wrong; \
         fix the coordinate, never drop the probe",
        sublane.lane,
        sublane.line,
        sublane.character,
    );
    Ok(fallback_params)
}

/// Sample one completion sub-lane on already-resolved request params: a
/// warmup burst, then `COMPLETION_SUBLANE_SAMPLES` timed round trips.
///
/// `params` comes from `resolve_completion_sublane_params`, whose probe
/// already verified the coordinate, so every sampled response must pass the
/// same combined guard as the criterion groups — a JSON-RPC error or a
/// zero-item list is refused, never recorded as a successful receipt sample
/// (#17731).
fn measure_completion_sublane(
    session: &RoundTripSession,
    sublane: &CompletionSublane,
    params: &Value,
) -> Result<Vec<u64>> {
    for _ in 1..COMPLETION_SUBLANE_WARMUP {
        let response = session
            .harness
            .client
            .request("textDocument/completion", params.clone(), REQUEST_TIMEOUT)
            .with_context(|| format!("{} lane: warmup request failed", sublane.lane))?;
        validate_completion_response(&response, sublane.lane)
            .with_context(|| format!("{} lane: warmup request", sublane.lane))?;
    }

    let mut samples = Vec::with_capacity(COMPLETION_SUBLANE_SAMPLES);
    for _ in 0..COMPLETION_SUBLANE_SAMPLES {
        let start = Instant::now();
        let response = session
            .harness
            .client
            .request("textDocument/completion", params.clone(), REQUEST_TIMEOUT)
            .with_context(|| format!("{} lane: sampled request failed", sublane.lane))?;
        validate_completion_response(&response, sublane.lane)
            .with_context(|| format!("{} lane: sampled request", sublane.lane))?;
        samples.push(millis_since(start));
    }
    Ok(samples)
}

// ---- Diagnostics after edit ----------------------------------------------------------

/// Shared edit cursor for the dedicated `bench_didchange_target.pl` document.
///
/// The LSP document version must increase monotonically across
/// `textDocument/didChange` notifications for one open document, and the
/// receipt-pass lane and the criterion pass drive the same warm session — so
/// both passes share one cursor and the counter persists across criterion
/// iterations.
struct DiagTargetEditor {
    /// Current LSP document version; 1 is the `didOpen` version.
    version: Cell<i32>,
    /// Whether the document has been opened on this session (the
    /// diagnostics lane opens it first; criterion test mode opens it lazily).
    opened: Cell<bool>,
}

impl DiagTargetEditor {
    fn new() -> Self {
        Self { version: Cell::new(1), opened: Cell::new(false) }
    }

    /// Produce the next edit: the new document version and the full-document
    /// replacement carrying the matching sentinel revision.
    fn next_edit(&self) -> (i32, String) {
        let version = self.version.get() + 1;
        self.version.set(version);
        (version, didchange_target_source_with_rev(i64::from(version)))
    }
}

/// Open the didChange target on the warm session, untimed.
///
/// The `didOpen` establishes document version 1; the first diagnostics
/// publish for the fresh URI is drained here so no timed sample ever
/// includes it. Idempotent: the diagnostics lane calls this before its own
/// samples, and the criterion pass calls it again so test mode (which skips
/// the receipt pass) still measures an opened, warm document.
fn ensure_didchange_target_open(
    session: &RoundTripSession,
    editor: &DiagTargetEditor,
) -> Result<()> {
    if editor.opened.get() {
        return Ok(());
    }
    session
        .harness
        .open_file(DIDCHANGE_TARGET_PATH, &didchange_target_source())
        .context("failed to open the didChange target document")?;
    let uri = session.harness.workspace.uri(DIDCHANGE_TARGET_PATH);
    let arrival = session.harness.client.wait_for_events(DIAGNOSTICS_ARRIVAL_BUDGET, |events| {
        events.iter().find_map(|event| match event {
            LspEvent::Diagnostics { uri: published, .. } if published == &uri => Some(()),
            _ => None,
        })
    });
    arrival.map_err(|end| {
        anyhow!(
            "didChange target: initial publishDiagnostics did not arrive for \
             {DIDCHANGE_TARGET_PATH}: {}",
            end.describe()
        )
    })?;
    editor.opened.set(true);
    Ok(())
}

/// Run the `diagnostics_after_edit` lane: `DIDCHANGE_EDIT_SAMPLES` samples,
/// each one edit followed by the version-matched diagnostics publish.
///
/// One sample = client-side wallclock from immediately before the framed
/// `textDocument/didChange` is written until the `publishDiagnostics` for
/// that exact document version arrives. A late publication for a previous
/// version does not satisfy the wait, and a timeout is a lane failure, never
/// a sample. Also reports the duplicate-push count, swept once at the end of
/// the lane over the whole retained event buffer: every publication beyond
/// the first for a sampled version counts, including a duplicate for an
/// earlier version that arrived while later edits were being awaited
/// (#17732 — a per-version scan taken once at each arrival undercounted
/// those late arrivals).
fn measure_didchange_to_diagnostics(
    session: &RoundTripSession,
    editor: &DiagTargetEditor,
) -> Result<(Vec<u64>, usize)> {
    ensure_didchange_target_open(session, editor)?;
    let uri = session.harness.workspace.uri(DIDCHANGE_TARGET_PATH);
    let mut samples = Vec::with_capacity(DIDCHANGE_EDIT_SAMPLES);
    let mut sampled_versions = Vec::with_capacity(DIDCHANGE_EDIT_SAMPLES);
    for _ in 0..DIDCHANGE_EDIT_SAMPLES {
        let (version, source) = editor.next_edit();

        let start = Instant::now();
        session
            .harness
            .client
            .did_change_full(&uri, version, &source)
            .with_context(|| format!("didChange lane: failed to send version {version}"))?;
        let arrival =
            session.harness.client.wait_for_events(DIAGNOSTICS_ARRIVAL_BUDGET, |events| {
                events.iter().find_map(|event| match event {
                    LspEvent::Diagnostics {
                        uri: published,
                        version: Some(published_version),
                        ..
                    } if published == &uri && *published_version == i64::from(version) => Some(()),
                    _ => None,
                })
            });
        match arrival {
            Ok(()) => {
                samples.push(millis_since(start));
                sampled_versions.push(i64::from(version));
            }
            Err(end) => bail!(
                "didChange lane: no version-{version} publishDiagnostics arrived for \
                 {DIDCHANGE_TARGET_PATH}: {}",
                end.describe()
            ),
        }
    }

    // Duplicate attribution at the publisher boundary (#17732): one sweep
    // over the retained buffer after the last arrival wait counts every push
    // for every sampled version, so a duplicate for version N that lands
    // after N's own arrival wait is still counted and the receipt cannot
    // report `duplicate_pushes: 0` despite observed duplicates.
    std::thread::sleep(DUPLICATE_SWEEP_SETTLE);
    let duplicate_pushes = DiagnosticsTracker::duplicate_pushes_for_versions(
        &session.harness.client.peek_events(),
        &uri,
        &sampled_versions,
    );
    Ok((samples, duplicate_pushes))
}

// ---- Receipt ---------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
struct LatencySummary {
    p50_ms: u64,
    p95_ms: u64,
    p99_ms: u64,
    samples: usize,
}

impl LatencySummary {
    fn to_json(self) -> Value {
        json!({
            "p50_ms": self.p50_ms,
            "p95_ms": self.p95_ms,
            "p99_ms": self.p99_ms,
            "samples": self.samples,
        })
    }
}

/// Percentile convention shared with `tests/real_project_latency.rs`.
fn percentile(sorted: &[u64], pct: usize) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    let idx = ((f64::from(pct as u8) / 100.0) * (sorted.len() - 1) as f64).round() as usize;
    sorted[idx.min(sorted.len() - 1)]
}

fn summarise(mut samples: Vec<u64>) -> LatencySummary {
    let count = samples.len();
    samples.sort_unstable();
    LatencySummary {
        p50_ms: percentile(&samples, 50),
        p95_ms: percentile(&samples, 95),
        p99_ms: percentile(&samples, 99),
        samples: count,
    }
}

fn millis_since(start: Instant) -> u64 {
    u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX)
}

fn summary_to_value(samples: Result<Vec<u64>>, lane: &'static str) -> Result<Value> {
    Ok(summarise(samples.context(lane)?).to_json())
}

fn did_open_relative_path(index: usize) -> String {
    format!("bench_didopen_{index:02}.pl")
}

fn did_open_source(index: usize) -> String {
    format!(
        "package Bench::DidOpen::Sample{index};\n\
         use strict;\n\
         use warnings;\n\
         \n\
         sub record{index} {{\n\
         \x20   my ($self, $value) = @_;\n\
         \x20   return $value // 0;\n\
         }}\n\
         \n\
         1;\n"
    )
}

/// The initial `bench_didchange_target.pl` document (sentinel revision 0).
fn didchange_target_source() -> String {
    didchange_target_source_with_rev(0)
}

/// The didChange target regenerated for one sentinel revision.
///
/// Deliberately generated rather than a committed fixture: every edit is a
/// full-document replacement that changes only the `our $BENCH_REV` sentinel,
/// so successive edits stay parseable, differ byte-wise, and keep the
/// document identity stable across the whole warm session.
fn didchange_target_source_with_rev(rev: i64) -> String {
    let mut source = String::new();
    source.push_str("package Bench::DiagTarget;\n");
    source.push_str("use strict;\n");
    source.push_str("use warnings;\n");
    source.push('\n');
    source.push_str(&format!("our $BENCH_REV = {rev};\n"));
    source.push('\n');
    for index in 0..12 {
        source.push_str(&format!("sub record_{index} {{\n"));
        source.push_str("    my ($self, $value) = @_;\n");
        source.push_str(&format!("    return ($value // 0) + {index} + $BENCH_REV;\n"));
        source.push_str("}\n");
        source.push('\n');
    }
    source.push_str("1;\n");
    source
}

fn binary_profile_hint(binary_path: &str) -> &'static str {
    let normalized = binary_path.replace('\\', "/");
    for profile in ["release", "agent", "debug"] {
        if normalized.contains(&format!("/target/{profile}/")) {
            return profile;
        }
    }
    "unknown"
}

fn current_commit() -> String {
    Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|commit| commit.trim().to_string())
        .filter(|commit| !commit.is_empty())
        .unwrap_or_else(|| "unknown".to_string())
}

/// Build and write `.ci/metrics/lsp_roundtrip.json`; returns the receipt path.
fn write_receipt(session: &RoundTripSession, lanes: &[(&'static str, Value)]) -> Result<PathBuf> {
    let mut lanes_map = serde_json::Map::new();
    for (lane, summary) in lanes {
        lanes_map.insert((*lane).to_string(), summary.clone());
    }

    let receipt = json!({
        "schema_version": 1,
        "kind": "lsp_stdio_roundtrip",
        "measured_at": chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        "commit": current_commit(),
        "corpus": {
            "fixture": "mojolicious_skeleton",
            "entry_file": ENTRY_FILE,
            "file_count": session.file_count,
            "source_line_count": session.source_line_count,
        },
        "runtime": {
            "binary": session.binary_path,
            "binary_profile": binary_profile_hint(&session.binary_path),
            "workspace_env": "PERL_LSP_WORKSPACE=1 (fixture_scenario_config)",
            "index_ready": session.index_ready,
        },
        "sampling": {
            "request_samples_per_lane": REQUEST_SAMPLES,
            "request_warmup_per_lane": REQUEST_WARMUP,
            "initialize_samples": INIT_SAMPLES,
            "did_open_samples": DID_OPEN_SAMPLES,
            "completion_sublane_samples": COMPLETION_SUBLANE_SAMPLES,
            "completion_sublane_warmup": COMPLETION_SUBLANE_WARMUP,
            "didchange_edit_samples": DIDCHANGE_EDIT_SAMPLES,
        },
        "lanes": Value::Object(lanes_map),
        "timing_policy": "client-side wallclock send→response over stdio on a warm session; \
          advisory baseline only — no wallclock assertion runs in cargo test \
          (ux_latency_raw_rpc.rs timing policy, #17159)",
    });

    let output_path = bench_workspace_root().join(RECEIPT_PATH);
    if let Some(parent) = output_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    std::fs::write(
        &output_path,
        serde_json::to_string_pretty(&receipt).context("failed to serialise the receipt")?,
    )
    .with_context(|| format!("failed to write {}", output_path.display()))?;
    Ok(output_path)
}

fn bench_workspace_root() -> PathBuf {
    let manifest = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".to_string());
    let mut dir = PathBuf::from(manifest.clone());
    while !dir.join("Cargo.lock").exists() {
        match dir.parent() {
            Some(parent) => dir = parent.to_path_buf(),
            None => return PathBuf::from(manifest),
        }
    }
    dir
}

fn print_lane_summary(lane: &'static str, summary: Value) {
    println!(
        "{lane:<26} p50={:>5}ms  p95={:>5}ms  p99={:>5}ms  samples={}",
        summary["p50_ms"].as_u64().unwrap_or_default(),
        summary["p95_ms"].as_u64().unwrap_or_default(),
        summary["p99_ms"].as_u64().unwrap_or_default(),
        summary["samples"].as_u64().unwrap_or_default(),
    );
}

// ---- Criterion regression groups ---------------------------------------------------

fn run_criterion_pass(
    session: &RoundTripSession,
    editor: &DiagTargetEditor,
    completion_resolutions: &CompletionSublaneResolutions,
) {
    let mut criterion = Criterion::default().configure_from_args();
    let mut group = criterion.benchmark_group("lsp_stdio_roundtrip");
    group.measurement_time(Duration::from_secs(10));

    let completion_uri = session.harness.workspace.uri(ENTRY_FILE);

    group.bench_function("hover", |b| {
        b.iter(|| {
            let start = Instant::now();
            let outcome = session.harness.hover(ENTRY_FILE, HOVER_LINE, HOVER_COL);
            let elapsed = millis_since(start);
            record_criterion_outcome("hover", &outcome.map(|_| ()));
            black_box(elapsed);
        })
    });

    group.bench_function("completion", |b| {
        b.iter(|| {
            let start = Instant::now();
            // `client.request` returns `Ok` for a JSON-RPC error response
            // (only the transport can fail it), so mirror the receipt pass and
            // the harness request helpers by mapping an `error` field to `Err`;
            // otherwise the round-trip failure counter misses the failure.
            let outcome = session
                .harness
                .client
                .request(
                    "textDocument/completion",
                    json!({
                        "textDocument": { "uri": completion_uri },
                        "position": { "line": COMPLETION_LINE, "character": COMPLETION_COL },
                        "context": { "triggerKind": 2, "triggerCharacter": ">" }
                    }),
                    REQUEST_TIMEOUT,
                )
                .and_then(|response| {
                    if response.get("error").is_some() {
                        Err(anyhow!("completion returned a JSON-RPC error: {response}"))
                    } else {
                        Ok(())
                    }
                });
            let elapsed = millis_since(start);
            record_criterion_outcome("completion", &outcome);
            black_box(elapsed);
        })
    });

    group.bench_function("definition", |b| {
        b.iter(|| {
            let start = Instant::now();
            let outcome = session.harness.definition(ENTRY_FILE, DEFINITION_LINE, DEFINITION_COL);
            let elapsed = millis_since(start);
            record_criterion_outcome("definition", &outcome.map(|_| ()));
            black_box(elapsed);
        })
    });

    group.bench_function("workspace_symbol", |b| {
        b.iter(|| {
            let start = Instant::now();
            let outcome = session.harness.workspace_symbols(WORKSPACE_SYMBOL_QUERY);
            let elapsed = millis_since(start);
            record_criterion_outcome("workspace_symbol", &outcome.map(|_| ()));
            black_box(elapsed);
        })
    });

    // Completion sub-lanes: same triggerKind-2 request shape as the generic
    // `completion` group above, at the exact request params the receipt pass
    // probe-resolved for each lane (fallback included), so a lane name times
    // one subject across both passes (#17731). The shared guard rejects
    // JSON-RPC errors and zero-item responses alike: a wrong coordinate or a
    // failed request would otherwise record fast empty round trips and
    // flatter the numbers. In test mode a failed probe skips the group;
    // bench mode fails the receipt lane instead.
    for (resolution, lane) in [
        (&completion_resolutions.method_position, COMPLETION_METHOD_SUBLANE.lane),
        (&completion_resolutions.package_prefix, COMPLETION_PACKAGE_SUBLANE.lane),
        (&completion_resolutions.lexical, COMPLETION_LEXICAL_SUBLANE.lane),
    ] {
        match resolution {
            Ok(params) => {
                group.bench_function(lane, |b| {
                    b.iter(criterion_completion_body(session, params, lane))
                });
            }
            Err(error) => {
                eprintln!("SKIP {lane} criterion group: {error:#}");
                CRITERION_ROUND_TRIP_FAILURES.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    // The receipt pass opens the didChange target untimed before its own
    // lane; criterion test mode (no receipt pass) opens it here so the
    // iterations still measure an opened, warm document. The version counter
    // persists across criterion iterations via the shared editor cursor.
    match ensure_didchange_target_open(session, editor) {
        Ok(()) => {
            let uri = session.harness.workspace.uri(DIDCHANGE_TARGET_PATH);
            group.bench_function("didchange_to_diagnostics", |b| {
                b.iter(|| {
                    let (version, source) = editor.next_edit();
                    let start = Instant::now();
                    let outcome =
                        session.harness.client.did_change_full(&uri, version, &source).and_then(
                            |()| {
                                session
                                    .harness
                                    .client
                                    .wait_for_events(DIAGNOSTICS_ARRIVAL_BUDGET, |events| {
                                        events.iter().find_map(|event| match event {
                                            LspEvent::Diagnostics {
                                                uri: published,
                                                version: Some(published_version),
                                                ..
                                            } if published == &uri
                                                && *published_version == i64::from(version) =>
                                            {
                                                Some(())
                                            }
                                            _ => None,
                                        })
                                    })
                                    .map_err(|end| {
                                        anyhow!(
                                            "no version-{version} publishDiagnostics arrived: {}",
                                            end.describe()
                                        )
                                    })
                            },
                        );
                    let elapsed = millis_since(start);
                    record_criterion_outcome("didchange_to_diagnostics", &outcome);
                    black_box(elapsed);
                })
            });
        }
        Err(error) => {
            eprintln!("SKIP didchange_to_diagnostics criterion group: {error:#}");
            CRITERION_ROUND_TRIP_FAILURES.fetch_add(1, Ordering::Relaxed);
        }
    }

    group.finish();
    criterion.final_summary();

    let failures = CRITERION_ROUND_TRIP_FAILURES.load(Ordering::Relaxed);
    if failures > 0 {
        eprintln!(
            "WARNING: {failures} criterion round trips returned a JSON-RPC error or timed out; \
             the criterion numbers above include those failures"
        );
    }
}

fn record_criterion_outcome<T>(lane: &'static str, outcome: &Result<T>) {
    if outcome.is_err() {
        CRITERION_ROUND_TRIP_FAILURES.fetch_add(1, Ordering::Relaxed);
        eprintln!("criterion round trip failed in the {lane} lane");
    }
}

/// The per-iteration body shared by the criterion completion sub-lanes:
/// raw request timed from write to decoded response, then the combined
/// response guard shared with the receipt pass — a JSON-RPC error or a
/// zero-item list counts as a round-trip failure rather than silently
/// flattering the numbers (#17731).
fn criterion_completion_body<'a>(
    session: &'a RoundTripSession,
    params: &'a Value,
    lane: &'static str,
) -> impl FnMut() + 'a {
    move || {
        let start = Instant::now();
        let outcome = session
            .harness
            .client
            .request("textDocument/completion", params.clone(), REQUEST_TIMEOUT)
            .and_then(|response| validate_completion_response(&response, lane));
        let elapsed = millis_since(start);
        record_criterion_outcome(lane, &outcome);
        black_box(elapsed);
    }
}

// ---- Main -------------------------------------------------------------------------

enum SessionStart {
    /// Bench skipped (no server binary; strict mode off).
    Skipped,
    Running(Box<RoundTripSession>),
}

fn resolve_session() -> Result<SessionStart> {
    let binary = match resolve_binary() {
        Ok(binary) => binary,
        Err(error) => {
            let strict = std::env::var(REQUIRE_BINARY_ENV)
                .is_ok_and(|value| value == "1" || value.eq_ignore_ascii_case("true"));
            assert!(
                !strict,
                "{REQUIRE_BINARY_ENV} is truthy, which forbids silently skipping the \
                 round-trip bench — perl-lsp binary not found. \
                 Run `cargo build --release -p perllsp` first. Resolution error: {error:#}"
            );
            return Ok(SessionStart::Skipped);
        }
    };
    create_session(&binary).map(|session| SessionStart::Running(Box::new(session)))
}

fn run() -> Result<()> {
    // `cargo bench` passes `--bench` to this harness-free binary; plain
    // `cargo test --benches`/`--all-targets` runs it without one. The receipt
    // pass measures a real session and overwrites the committed baseline, so
    // it must only run in bench mode — never from an unrelated test run.
    let bench_mode = std::env::args().any(|arg| arg == "--bench");
    let session = match resolve_session()? {
        SessionStart::Skipped => {
            eprintln!(
                "SKIP lsp_stdio_roundtrip_benchmark: perl-lsp binary not found; \
                 run `cargo build --release -p perllsp` (or set PERL_LSP_BIN)"
            );
            return Ok(());
        }
        SessionStart::Running(session) => session,
    };

    // Shared didChange edit cursor: created before the mode split so test
    // mode (criterion groups only) and bench mode drive the same monotonic
    // document versioning.
    let editor = DiagTargetEditor::new();

    // Probe-resolve each completion sub-lane's request subject once, before
    // the mode split: the receipt pass and the criterion groups must time the
    // same coordinates (fallback included), so one resolution is shared by
    // both passes instead of each pass rebuilding entry-file params (#17731).
    let completion_resolutions = CompletionSublaneResolutions {
        method_position: resolve_completion_sublane_params(&session, &COMPLETION_METHOD_SUBLANE),
        package_prefix: resolve_completion_sublane_params(&session, &COMPLETION_PACKAGE_SUBLANE),
        lexical: resolve_completion_sublane_params(&session, &COMPLETION_LEXICAL_SUBLANE),
    };

    if !bench_mode {
        eprintln!(
            "SKIP receipt pass lsp_stdio_roundtrip_benchmark: no --bench (test mode); \
             criterion groups still run once in test mode"
        );
        run_criterion_pass(&session, &editor, &completion_resolutions);
        return Ok(());
    }

    if !session.index_ready {
        eprintln!(
            "WARNING: workspace index-ready event not observed within {INDEX_READY_BUDGET:?}; \
             request lanes ran without a confirmed warm index"
        );
    }
    eprintln!(
        "receipt pass: binary={} profile={} files={} lines={}",
        session.binary_path,
        binary_profile_hint(&session.binary_path),
        session.file_count,
        session.source_line_count
    );

    let initialize = measure_initialize(&session);
    let did_open = measure_did_open_to_diagnostics(&session);

    let completion_uri = session.harness.workspace.uri(ENTRY_FILE);
    let hover = timed_requests("hover", || {
        session
            .harness
            .hover(ENTRY_FILE, HOVER_LINE, HOVER_COL)
            .map(|_| ())
            .context("hover request errored")
    });
    let completion = timed_requests("completion", || -> Result<()> {
        let response = session
            .harness
            .client
            .request(
                "textDocument/completion",
                json!({
                    "textDocument": { "uri": completion_uri },
                    "position": { "line": COMPLETION_LINE, "character": COMPLETION_COL },
                    "context": { "triggerKind": 2, "triggerCharacter": ">" }
                }),
                REQUEST_TIMEOUT,
            )
            .context("completion request errored")?;
        if response.get("error").is_some() {
            bail!("completion returned a JSON-RPC error: {response}");
        }
        Ok(())
    });
    let definition = timed_requests("definition", || {
        session
            .harness
            .definition(ENTRY_FILE, DEFINITION_LINE, DEFINITION_COL)
            .map(|_| ())
            .context("definition request errored")
    });
    let workspace_symbol = timed_requests("workspace_symbol", || {
        session
            .harness
            .workspace_symbols(WORKSPACE_SYMBOL_QUERY)
            .map(|_| ())
            .context("workspace/symbol request errored")
    });

    // Completion sub-lanes: higher sample counts at coordinated positions
    // (see the coordinate provenance block at the top of this file). Each
    // lane samples the exact params its probe resolved above, and the
    // criterion group for the same lane name reuses them (#17731). The
    // generic `completion` lane above stays untouched for comparability with
    // the committed receipt.
    let method_position = measure_completion_sublane(
        &session,
        &COMPLETION_METHOD_SUBLANE,
        resolution_params(&completion_resolutions.method_position)?,
    );
    let package_prefix = measure_completion_sublane(
        &session,
        &COMPLETION_PACKAGE_SUBLANE,
        resolution_params(&completion_resolutions.package_prefix)?,
    );
    let lexical = measure_completion_sublane(
        &session,
        &COMPLETION_LEXICAL_SUBLANE,
        resolution_params(&completion_resolutions.lexical)?,
    );

    // The didChange editor cursor (created above, before the mode split) is
    // shared with the criterion pass so the document version increases
    // monotonically for the whole session.
    let didchange = measure_didchange_to_diagnostics(&session, &editor);

    let didchange_lane = match didchange {
        Ok((samples, duplicate_pushes)) => {
            let mut value = summarise(samples).to_json();
            value["duplicate_pushes"] = json!(duplicate_pushes);
            Ok(("diagnostics_after_edit", value))
        }
        Err(error) => Err(error.context("diagnostics_after_edit lane")),
    };

    let lanes = vec![
        ("initialize", summary_to_value(initialize, "initialize lane")?),
        ("did_open_to_diagnostics", summary_to_value(did_open, "did_open_to_diagnostics lane")?),
        ("hover", summary_to_value(hover, "hover lane")?),
        ("completion", summary_to_value(completion, "completion lane")?),
        ("definition", summary_to_value(definition, "definition lane")?),
        ("workspace_symbol", summary_to_value(workspace_symbol, "workspace_symbol lane")?),
        (
            "completion_method_position",
            summary_to_value(method_position, "completion_method_position lane")?,
        ),
        (
            "completion_package_prefix",
            summary_to_value(package_prefix, "completion_package_prefix lane")?,
        ),
        ("completion_lexical", summary_to_value(lexical, "completion_lexical lane")?),
        didchange_lane?,
    ];

    println!("lsp_stdio_roundtrip baseline (mojolicious_skeleton, warm session):");
    for (lane, summary) in &lanes {
        print_lane_summary(lane, summary.clone());
    }
    let receipt_path = write_receipt(&session, &lanes)?;
    eprintln!("receipt written to {}", receipt_path.display());

    run_criterion_pass(&session, &editor, &completion_resolutions);
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("lsp_stdio_roundtrip_benchmark failed: {error:#}");
        std::process::exit(1);
    }
}
