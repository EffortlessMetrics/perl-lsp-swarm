//! Memory-ceiling + startup-to-ready receipts for the real perl-lsp binary.
//!
//! Two lanes over the committed `test_corpus/real_projects/mojolicious_skeleton`
//! fixture, covering issue #17159 matrix rows 3-4 (`server_startup_to_ready`,
//! `server_rss_memory`); the memory lane also tracks #10015:
//!
//! 1. `server_startup_to_ready` — 15 cold spawns. Each sample spawns a fresh
//!    server process against the pre-seeded fixture workspace, records the
//!    handshake (`spawn` → `initialize` response, the `initialized`
//!    notification is inside [`UxClient::spawn`]), opens
//!    `lib/Mojolicious.pm`, and waits for the FIRST
//!    `textDocument/publishDiagnostics` for that URI — the observable
//!    completion of the open, the same ready-point definition as the
//!    #17179 did-open lane.
//! 2. `server_rss_memory` — one warm session. RSS checkpoints after the
//!    handshake, after opening the whole fixture + index readiness, and after
//!    the first `workspace/symbol` query; then a 600-message soak in a fixed
//!    six-message cycle with RSS sampled after every 50th completed message.
//!
//! # Timing policy
//!
//! These are intentionally "does it work end-to-end" receipts, not numeric
//! latency assertions. CI machine variance makes wallclock budgets brittle;
//! the receipt is "we drove the e2e config and the answer arrived."
//! Wallclock measurements belong on dedicated benchmark hardware, not in
//! `cargo test`. Arrival is the assertion: the 30s diagnostics budget and
//! the 60s request timeout below exist only to fail fast on a wedged or
//! broken binary, never to claim the server is fast. The entire numeric
//! lane sits behind `#[ignore]`, so no wallclock or memory number can leak
//! into the default `cargo test` lane (#17159 acceptance criterion 2); the
//! three pure unit tests below (percentile, slope, cycle composition) are
//! the only members of this binary that run by default, and none of them
//! spawns a server.
//!
//! # Run
//!
//! ```bash
//! just ux-bench-memory-startup
//! # or directly:
//! cargo build --release -p perllsp
//! PERL_LSP_BIN="$PWD/target/release/perllsp" PERL_LSP_UX_REQUIRE_BINARY=1 \
//!     cargo test -p perl-lsp-ux-tests --test lsp_memory_startup \
//!     -- --test-threads=1 --ignored --nocapture
//! ```
//!
//! The receipt is written once, at the end, to `.ci/metrics/lsp_memory_startup.json`.

use anyhow::{Context, Result, anyhow, bail};
use perl_lsp_ux_tests::{
    LspEvent, UxClient, UxHarness, binary_available, fixture_content, fixture_scenario_config,
    iso8601_now, load_mojolicious_fixture_files, open_all_fixture_files, resolve_binary,
    workspace_root,
};
use serde_json::{Value, json};
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

// ---- Lane configuration ---------------------------------------------------------

/// Cold spawn samples for the startup-to-ready lane.
const STARTUP_SAMPLES: usize = 15;

/// Bound for one full request round trip. Arrival is the assertion; this
/// bound only fails fast on a wedged binary (see the module timing policy).
const REQUEST_TIMEOUT: Duration = Duration::from_mins(1);

/// Bound for waiting on the first diagnostics publish after a `didOpen`,
/// and for the per-change diagnostics gate inside the soak.
const DIAGNOSTICS_ARRIVAL_BUDGET: Duration = Duration::from_secs(30);

/// Bound for the post-open workspace index readiness wait.
const INDEX_READY_BUDGET: Duration = Duration::from_secs(30);

/// Total messages driven through the warm memory-lane session.
const SOAK_MESSAGE_COUNT: usize = 600;

/// Messages in one mixed soak cycle (6 ops × 100 cycles = 600).
const MIXED_CYCLE: [&str; 6] =
    ["hover", "completion", "definition", "documentSymbol", "workspace_symbol", "did_change_full"];

/// RSS is sampled after every Nth completed soak message (12 soak samples).
const RSS_SAMPLE_EVERY_MESSAGES: usize = 50;

/// Receipt path relative to the workspace root.
const RECEIPT_PATH: &str = ".ci/metrics/lsp_memory_startup.json";

// ---- Fixture coordinates (mirror the #17179 round-trip bench) -------------------

const ENTRY_FILE: &str = "lib/Mojolicious.pm";
const HOVER_LINE: u32 = 10;
const HOVER_COL: u32 = 6;
const COMPLETION_LINE: u32 = 14;
const COMPLETION_COL: u32 = 10;
const DEFINITION_LINE: u32 = 5;
const DEFINITION_COL: u32 = 4;
const WORKSPACE_SYMBOL_QUERY: &str = "new";

// ---- Pure statistics helpers ----------------------------------------------------

/// Nearest-rank percentile over a sample slice in any order.
///
/// Nearest-rank definition: the smallest value `v` such that at least `pct`%
/// of the samples are `<= v`; for `n` sorted samples the rank is
/// `ceil(pct/100 * n)` and the answer is `sorted[rank - 1]`. An empty sample
/// set yields 0 (mirrors the `real_project_latency` precedent); the lanes
/// below only ever pass compile-time-constant non-empty sample sets.
fn nearest_rank_percentile(samples: &[u64], pct: u8) -> u64 {
    if samples.is_empty() {
        return 0;
    }
    let mut sorted = samples.to_vec();
    sorted.sort_unstable();
    let rank = (u64::from(pct) * sorted.len() as u64).div_ceil(100);
    let rank = rank.clamp(1, sorted.len() as u64);
    sorted[(rank - 1) as usize]
}

/// Summarise one lane's samples into the shared `p50/p95/p99 + samples` shape.
fn summarise_samples(samples: &[u64]) -> Value {
    json!({
        "p50_ms": nearest_rank_percentile(samples, 50),
        "p95_ms": nearest_rank_percentile(samples, 95),
        "p99_ms": nearest_rank_percentile(samples, 99),
        "samples": samples.len(),
    })
}

/// Least-squares slope (KB per message) of `(message_index, rss_kb)` samples.
///
/// `None` when a slope is undefined: fewer than two samples, or all samples
/// taken at the same message index (zero x variance).
fn least_squares_slope_kb_per_message(samples: &[(u64, u64)]) -> Option<f64> {
    let count = samples.len();
    if count < 2 {
        return None;
    }
    let count_f = count as f64;
    let mean_x = samples.iter().map(|(message, _)| *message as f64).sum::<f64>() / count_f;
    let mean_y = samples.iter().map(|(_, rss)| *rss as f64).sum::<f64>() / count_f;
    let mut covariance = 0.0;
    let mut variance = 0.0;
    for (message, rss) in samples {
        let dx = *message as f64 - mean_x;
        covariance += dx * (*rss as f64 - mean_y);
        variance += dx * dx;
    }
    if variance == 0.0 {
        return None;
    }
    Some(covariance / variance)
}

/// The mixed-cycle op that drives soak message `message_index`.
///
/// Deterministic by construction: message `i` is always
/// `MIXED_CYCLE[i % MIXED_CYCLE.len()]`, so the composition recorded in the
/// receipt's `sampling.mixed_cycle` is the composition that ran.
fn mixed_op_kind(message_index: usize) -> &'static str {
    MIXED_CYCLE[message_index % MIXED_CYCLE.len()]
}

// ---- Pure unit tests (default `cargo test`; no server spawned) ------------------

#[test]
fn nearest_rank_percentile_matches_expected_ranks() {
    // Nearest-rank on n=10: p50 → ceil(5.0)=5th value, p95/p99 → 10th value.
    let one_to_ten: Vec<u64> = (1..=10).collect();
    assert_eq!(nearest_rank_percentile(&one_to_ten, 50), 5);
    assert_eq!(nearest_rank_percentile(&one_to_ten, 95), 10);
    assert_eq!(nearest_rank_percentile(&one_to_ten, 99), 10);

    // Unsorted input on purpose: the helper owns sorting. n=4: p50 → 2nd,
    // p75 → 3rd, p100 → 4th sorted value.
    let quartet = [40, 10, 30, 20];
    assert_eq!(nearest_rank_percentile(&quartet, 50), 20);
    assert_eq!(nearest_rank_percentile(&quartet, 75), 30);
    assert_eq!(nearest_rank_percentile(&quartet, 100), 40);

    assert_eq!(nearest_rank_percentile(&[42], 50), 42);
    assert_eq!(nearest_rank_percentile(&[], 50), 0, "empty samples are documented as 0");
}

#[test]
fn least_squares_slope_matches_hand_computed_fit() {
    // Perfect line y = 10 + 2x.
    let perfect = [(0, 10), (1, 12), (2, 14)];
    assert!(
        least_squares_slope_kb_per_message(&perfect)
            .is_some_and(|slope| (slope - 2.0).abs() < 1e-9),
        "perfect line (0,10),(1,12),(2,14) must fit slope 2.0"
    );

    // Hand-computed: mean_x=100, mean_y=430/3, cov=4000, var=5000 → 0.8.
    let noisy = [(50, 100), (100, 150), (150, 180)];
    assert!(
        least_squares_slope_kb_per_message(&noisy).is_some_and(|slope| (slope - 0.8).abs() < 1e-9),
        "hand-computed fit of {noisy:?} must be 0.8 KB/message"
    );

    assert!(least_squares_slope_kb_per_message(&[]).is_none());
    assert!(least_squares_slope_kb_per_message(&[(5, 100)]).is_none());
    assert!(
        least_squares_slope_kb_per_message(&[(5, 100), (5, 120)]).is_none(),
        "all samples at one message index have zero x variance"
    );
}

/// Lock the soak composition the receipt records: 6 ops per cycle, 100
/// cycles, exactly 600 messages, every index deterministic, 100 of each op.
#[test]
fn mixed_request_cycle_covers_600_messages_deterministically() -> Result<()> {
    anyhow::ensure!(
        MIXED_CYCLE.len() == 6,
        "the mixed cycle must stay six messages, got {}: {MIXED_CYCLE:?}",
        MIXED_CYCLE.len()
    );
    anyhow::ensure!(
        SOAK_MESSAGE_COUNT == 100 * MIXED_CYCLE.len(),
        "soak must be 100 whole cycles (600 messages), got {SOAK_MESSAGE_COUNT}"
    );
    anyhow::ensure!(
        SOAK_MESSAGE_COUNT.is_multiple_of(RSS_SAMPLE_EVERY_MESSAGES),
        "RSS sampling every {RSS_SAMPLE_EVERY_MESSAGES} must divide {SOAK_MESSAGE_COUNT} evenly"
    );

    let first_cycle: Vec<&str> = (0..MIXED_CYCLE.len()).map(mixed_op_kind).collect();
    anyhow::ensure!(
        first_cycle.as_slice() == MIXED_CYCLE,
        "first cycle must reproduce the recorded composition in order: {first_cycle:?}"
    );

    for message_index in 0..SOAK_MESSAGE_COUNT {
        let expected = MIXED_CYCLE[message_index % MIXED_CYCLE.len()];
        anyhow::ensure!(
            mixed_op_kind(message_index) == expected,
            "message {message_index} must deterministically map to {expected}"
        );
    }

    for op in MIXED_CYCLE {
        let count = (0..SOAK_MESSAGE_COUNT).filter(|index| mixed_op_kind(*index) == op).count();
        anyhow::ensure!(
            count == SOAK_MESSAGE_COUNT / MIXED_CYCLE.len(),
            "op {op} must appear exactly {} times, got {count}",
            SOAK_MESSAGE_COUNT / MIXED_CYCLE.len()
        );
    }
    Ok(())
}

// ---- Best-effort RSS sampling (mirrors real_project_latency.rs) -----------------

#[cfg(target_os = "linux")]
fn best_effort_process_rss_kb(pid: u32) -> Option<u64> {
    let status = std::fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    let line = status.lines().find(|line| line.starts_with("VmRSS:"))?;
    line.split_whitespace().nth(1).and_then(|raw| raw.parse::<u64>().ok())
}

#[cfg(target_os = "windows")]
fn best_effort_process_rss_kb(pid: u32) -> Option<u64> {
    let output = Command::new("powershell")
        .args(["-NoProfile", "-Command", &format!("(Get-Process -Id {pid}).WorkingSet64")])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8(output.stdout).ok()?;
    let bytes = stdout.trim().parse::<u64>().ok()?;
    Some(bytes / 1024)
}

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
fn best_effort_process_rss_kb(_pid: u32) -> Option<u64> {
    None
}

// ---- Receipt helpers ------------------------------------------------------------

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

fn binary_profile(binary: &str) -> &'static str {
    let normalized = binary.replace('\\', "/");
    for profile in ["release", "agent", "debug"] {
        if normalized.contains(&format!("/target/{profile}/")) {
            return profile;
        }
    }
    "unknown"
}

// ---- The e2e receipt lane -------------------------------------------------------

/// One mixed soak message against the warm memory-lane harness.
///
/// Request ops use the #17179 fixture coordinates; the `did_change_full` op
/// alternates broken/clean entry-file content per cycle and waits for the
/// diagnostics publish caused by THAT change (event-driven, gated on the
/// publish count observed before the change) so the soak measures steady
/// state rather than a queue flood.
fn run_mixed_soak_message(
    harness: &UxHarness,
    message_index: usize,
    broken_content: &str,
    clean_content: &str,
) -> Result<()> {
    match mixed_op_kind(message_index) {
        "hover" => {
            harness.hover(ENTRY_FILE, HOVER_LINE, HOVER_COL)?;
        }
        "completion" => {
            harness.completion(ENTRY_FILE, COMPLETION_LINE, COMPLETION_COL)?;
        }
        "definition" => {
            harness.definition(ENTRY_FILE, DEFINITION_LINE, DEFINITION_COL)?;
        }
        "documentSymbol" => {
            harness.document_symbols(ENTRY_FILE)?;
        }
        "workspace_symbol" => {
            harness.workspace_symbols(WORKSPACE_SYMBOL_QUERY)?;
        }
        "did_change_full" => {
            let cycle = message_index / MIXED_CYCLE.len();
            let content = if cycle.is_multiple_of(2) { broken_content } else { clean_content };
            let seen_before = harness.diagnostics_event_count(ENTRY_FILE);
            harness.change_file_full(ENTRY_FILE, content)?;
            harness
                .wait_for_diagnostics_after_count(
                    ENTRY_FILE,
                    seen_before,
                    DIAGNOSTICS_ARRIVAL_BUDGET,
                )
                .map_err(|end| {
                    anyhow!(
                        "soak message {message_index}: diagnostics publish after \
                         did_change_full did not arrive within \
                         {DIAGNOSTICS_ARRIVAL_BUDGET:?}: {end}"
                    )
                })?;
        }
        other => bail!("unmapped mixed-cycle op {other} at message {message_index}"),
    }
    Ok(())
}

/// Memory ceiling + startup-to-ready receipt (#17159 rows 3-4, memory #10015).
///
/// Both lanes run in this single test so the receipt has exactly one writer.
#[test]
#[ignore = "stress: nightly numeric receipt lane for #17159 (matrix rows 3-4: \
            server_startup_to_ready + server_rss_memory); run via cargo test \
            -p perl-lsp-ux-tests --test lsp_memory_startup -- --ignored; also \
            tracks the memory lane #10015"]
#[expect(
    clippy::print_stderr,
    reason = "policy: the memory/startup receipt and its SKIP breadcrumb must \
              survive libtest capture in CI logs (memory-receipt precedent \
              real_project_latency.rs)"
)]
fn lsp_memory_startup_receipt() -> Result<()> {
    if !binary_available() {
        eprintln!("SKIP lsp_memory_startup_receipt: perl-lsp binary not found");
        return Ok(());
    }

    let files = load_mojolicious_fixture_files()
        .context("failed to load mojolicious_skeleton fixture files")?;
    anyhow::ensure!(
        !files.is_empty(),
        "mojolicious_skeleton fixture contains no Perl files; the memory/startup bench needs its corpus"
    );
    let file_count = files.len();
    let source_line_count: usize = files.iter().map(|file| file.content.lines().count()).sum();
    let entry_content =
        fixture_content(&files, ENTRY_FILE).context("fixture entry file missing")?.to_string();

    let mut config = fixture_scenario_config(&files);
    config.timeout = REQUEST_TIMEOUT;

    let binary = resolve_binary().context("resolving the perl-lsp binary for the bench")?;

    // ── Lane 1: server_startup_to_ready — 15 cold spawns ─────────────────────
    let startup_harness = UxHarness::new(config.clone())
        .context("failed to spawn perl-lsp for the startup-lane workspace")?;
    let entry_uri = startup_harness.workspace.uri(ENTRY_FILE);

    let mut spawn_to_initialize_samples: Vec<u64> = Vec::new();
    let mut initialize_to_diagnostics_samples: Vec<u64> = Vec::new();
    let mut spawn_to_ready_samples: Vec<u64> = Vec::new();
    for sample in 0..STARTUP_SAMPLES {
        let spawn_started = Instant::now();
        let client = UxClient::spawn(&binary, &startup_harness.workspace, &config)
            .with_context(|| format!("startup sample {sample}: spawn + initialize failed"))?;
        let handshake_elapsed = spawn_started.elapsed();

        client
            .did_open(&entry_uri, &entry_content)
            .with_context(|| format!("startup sample {sample}: did_open {ENTRY_FILE} failed"))?;

        client
            .wait_for_events(DIAGNOSTICS_ARRIVAL_BUDGET, |events| {
                events.iter().find_map(|event| match event {
                    LspEvent::Diagnostics { uri, .. } if uri == &entry_uri => Some(()),
                    _ => None,
                })
            })
            .map_err(|end| {
                anyhow!(
                    "startup sample {sample}: first publishDiagnostics for {entry_uri} did not \
                     arrive within {DIAGNOSTICS_ARRIVAL_BUDGET:?}: {end}"
                )
            })?;
        let ready_elapsed = spawn_started.elapsed();

        spawn_to_initialize_samples.push(handshake_elapsed.as_millis() as u64);
        initialize_to_diagnostics_samples
            .push((ready_elapsed - handshake_elapsed).as_millis() as u64);
        spawn_to_ready_samples.push(ready_elapsed.as_millis() as u64);

        // Bounded best-effort drop (SHUTDOWN_GRACE) before the next cold spawn.
        drop(client);
    }

    // ── Lane 2: server_rss_memory — one warm session ─────────────────────────
    let memory_harness = UxHarness::new(config.clone())
        .context("failed to spawn perl-lsp for the memory-lane session")?;
    let pid = memory_harness
        .client
        .process_id()
        .context("memory lane could not read the server pid from UxClient")?;

    let rss_after_initialize_kb = best_effort_process_rss_kb(pid);

    open_all_fixture_files(&memory_harness, &files)
        .context("memory lane: open_all_fixture_files failed")?;
    memory_harness.wait_for_index_ready(INDEX_READY_BUDGET).map_err(|end| {
        anyhow!("memory lane: index-ready did not arrive within {INDEX_READY_BUDGET:?}: {end}")
    })?;
    memory_harness.client.drain_events();
    let rss_after_index_ready_kb = best_effort_process_rss_kb(pid);

    memory_harness
        .workspace_symbols(WORKSPACE_SYMBOL_QUERY)
        .context("memory lane: workspace/symbol query failed")?;
    memory_harness.client.drain_events();
    let rss_after_workspace_symbols_kb = best_effort_process_rss_kb(pid);

    // Soak: 600 mixed messages in fixed 6-message cycles; RSS after every 50th.
    let broken_content = format!("{entry_content}\nsub broken_bench_soak {{\n");
    let mut soak_samples: Vec<(u64, u64)> = Vec::new();
    let mut missed_rss_samples = 0usize;
    for message_index in 0..SOAK_MESSAGE_COUNT {
        run_mixed_soak_message(
            &memory_harness,
            message_index,
            &broken_content,
            entry_content.as_str(),
        )
        .with_context(|| format!("soak message {message_index} failed"))?;
        let completed = message_index + 1;
        if completed % RSS_SAMPLE_EVERY_MESSAGES == 0
            && let Some(rss_kb) = best_effort_process_rss_kb(pid)
        {
            soak_samples.push((completed as u64, rss_kb));
        } else if completed % RSS_SAMPLE_EVERY_MESSAGES == 0 {
            missed_rss_samples += 1;
        }
    }
    memory_harness.client.drain_events();
    memory_harness.assert_no_crash();
    drop(memory_harness);

    // ── Receipt ──────────────────────────────────────────────────────────────
    let checkpoint_values: Vec<Option<u64>> =
        vec![rss_after_initialize_kb, rss_after_index_ready_kb, rss_after_workspace_symbols_kb];
    let mut all_rss_values: Vec<u64> = checkpoint_values.iter().flatten().copied().collect();
    all_rss_values.extend(soak_samples.iter().map(|(_, rss)| *rss));
    let peak_rss_kb = all_rss_values.iter().max().copied();
    let availability = if peak_rss_kb.is_some() { "measured" } else { "unavailable_on_host" };

    let receipt = json!({
        "schema_version": 1,
        "kind": "lsp_memory_startup",
        "commit": current_commit(),
        "measured_at": iso8601_now(),
        "corpus": {
            "fixture": "mojolicious_skeleton",
            "entry_file": ENTRY_FILE,
            "file_count": file_count,
            "source_line_count": source_line_count,
        },
        "runtime": {
            "binary": binary,
            "binary_profile": binary_profile(&binary),
            "index_ready": true,
            "workspace_env": "PERL_LSP_WORKSPACE=1 (fixture_scenario_config)",
            "rss_metric": "child process RSS/working set, best-effort KB \
                           (VmRSS on Linux, Get-Process WorkingSet64 on Windows)",
        },
        "lanes": {
            "server_startup_to_ready": {
                "samples": STARTUP_SAMPLES,
                "spawn_to_initialize_response_ms":
                    summarise_samples(&spawn_to_initialize_samples),
                "initialize_to_first_diagnostics_ms":
                    summarise_samples(&initialize_to_diagnostics_samples),
                "spawn_to_ready_ms": summarise_samples(&spawn_to_ready_samples),
            },
            "server_rss_memory": {
                "availability": availability,
                "checkpoints_kb": {
                    "after_initialize": rss_after_initialize_kb,
                    "after_index_ready": rss_after_index_ready_kb,
                    "after_workspace_symbols": rss_after_workspace_symbols_kb,
                },
                "peak_rss_kb": peak_rss_kb,
                "soak": {
                    "messages": SOAK_MESSAGE_COUNT,
                    "rss_samples": soak_samples.len(),
                    "rss_samples_missed": missed_rss_samples,
                    "sample_every_messages": RSS_SAMPLE_EVERY_MESSAGES,
                    "first_sample_kb": soak_samples.first().map(|(_, rss)| *rss),
                    "last_sample_kb": soak_samples.last().map(|(_, rss)| *rss),
                    "growth_slope_kb_per_message":
                        least_squares_slope_kb_per_message(&soak_samples),
                },
            },
        },
        "sampling": {
            "startup_spawn_samples": STARTUP_SAMPLES,
            "soak_messages": SOAK_MESSAGE_COUNT,
            "mixed_cycle": MIXED_CYCLE,
            "cycle_count": SOAK_MESSAGE_COUNT / MIXED_CYCLE.len(),
            "rss_sample_every_messages": RSS_SAMPLE_EVERY_MESSAGES,
        },
        "timing_policy": "arrival is the assertion; the 30s diagnostics budget and 60s request \
                          timeout are wedge detectors only; numeric claims live only in this \
                          ignored nightly receipt and wallclock belongs on benchmark hardware \
                          (ux_latency_raw_rpc.rs timing policy, #17159)",
    });

    let receipt_path: PathBuf = workspace_root()?.join(RECEIPT_PATH);
    std::fs::write(&receipt_path, format!("{}\n", serde_json::to_string_pretty(&receipt)?))
        .with_context(|| format!("writing receipt to {}", receipt_path.display()))?;

    // One stderr receipt keeps the passing numbers visible in CI logs
    // (memory-receipt precedent: real_project_latency.rs).
    eprintln!("{}", serde_json::to_string_pretty(&receipt)?);
    eprintln!("receipt written to {}", receipt_path.display());
    Ok(())
}
