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
    LspEvent, REQUIRE_BINARY_ENV, ScenarioConfig, UxClient, UxHarness,
    project_fixture::{fixture_content, fixture_scenario_config, load_mojolicious_fixture_files},
    resolve_binary,
};
use serde_json::{Value, json};
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

fn run_criterion_pass(session: &RoundTripSession) {
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

    if !bench_mode {
        eprintln!(
            "SKIP receipt pass lsp_stdio_roundtrip_benchmark: no --bench (test mode); \
             criterion groups still run once in test mode"
        );
        run_criterion_pass(&session);
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

    let lanes = vec![
        ("initialize", summary_to_value(initialize, "initialize lane")?),
        ("did_open_to_diagnostics", summary_to_value(did_open, "did_open_to_diagnostics lane")?),
        ("hover", summary_to_value(hover, "hover lane")?),
        ("completion", summary_to_value(completion, "completion lane")?),
        ("definition", summary_to_value(definition, "definition lane")?),
        ("workspace_symbol", summary_to_value(workspace_symbol, "workspace_symbol lane")?),
    ];

    println!("lsp_stdio_roundtrip baseline (mojolicious_skeleton, warm session):");
    for (lane, summary) in &lanes {
        print_lane_summary(lane, summary.clone());
    }
    let receipt_path = write_receipt(&session, &lanes)?;
    eprintln!("receipt written to {}", receipt_path.display());

    run_criterion_pass(&session);
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("lsp_stdio_roundtrip_benchmark failed: {error:#}");
        std::process::exit(1);
    }
}
