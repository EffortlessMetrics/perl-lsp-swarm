//! Raw-RPC latency receipts for the post-cutover Neovim lean-mode lane.
//!
//! Five scenarios that exercise the e2e runtime path against the real LSP
//! binary. They prove that:
//!
//! 1. `open -> completion` returns a useful answer (any non-error response).
//! 2. `open -> hover` returns a useful answer.
//! 3. `edit -> parse-error diagnostic` surfaces a parse error.
//! 4. `edit -> diagnostic clear` clears diagnostics when the parse cleans.
//! 5. `rapid typing -> latest completion wins` - under a burst of `didChange`
//!    notifications, the last completion request still returns successfully.
//! 6. Dynamic `textDocument/inlineCompletion` returns deterministic items while
//!    watcher registration stays off in the lean profile.
//! 7. Document symbols, workspace symbols, and code actions complete over the
//!    same real-process e2e path.
//!
//! These are intentionally "does it work end-to-end" tests, not numeric
//! latency assertions. CI machine variance makes wallclock budgets
//! brittle; the receipt is "we drove the e2e config and the answer
//! arrived." Wallclock measurements belong on dedicated benchmark
//! hardware, not in `cargo test`.
//!
//! Run with:
//!
//!     PERL_LSP_E2E=1 \
//!     PERL_LSP_DIAGNOSTIC_DEBOUNCE_MS=0 \
//!     PERL_LSP_DIAGNOSTIC_MODE=syntax-only \
//!     PERL_LSP_EAGER_WORKSPACE_INDEXING=false \
//!     PERL_LSP_FILE_WATCHERS=false \
//!     cargo test -p perl-lsp-ux-tests --test ux_latency_raw_rpc \
//!         -- --test-threads=1 --nocapture

use anyhow::{Context, Result, bail};
use perl_lsp_ux_tests::observation::WaitEnd;
use perl_lsp_ux_tests::{LspEvent, ScenarioConfig, UxHarness, binary_available};
use serde_json::{Value, json};
use std::io::Write;
use std::time::{Duration, Instant};

const SHORT_SOURCE: &str = r#"use strict;
use warnings;

my $value = 42;
my $other = $val
"#;

const PARSE_ERROR_SOURCE: &str = r#"use strict;
use warnings;

sub broken {
"#;

const CLEAN_SOURCE: &str = r#"use strict;
use warnings;

sub broken {}
"#;

const SYMBOL_SOURCE: &str = r#"package Latency::Symbols;
use strict;
use warnings;

sub alpha {
    return 1;
}

sub beta {
    return alpha();
}

1;
"#;

/// Build an e2e harness config: syntax-only diagnostics, zero debounce, no
/// eager workspace indexing, no file watchers. These values mirror
/// `perllsp --runtime-mode e2e`, but are set explicitly so the receipt
/// documents the exact lean runtime path it exercises.
fn e2e_config(timeout: Duration) -> ScenarioConfig {
    ScenarioConfig {
        timeout,
        path_restriction: None,
        echo_stderr: false,
        extra_env: vec![
            ("PERL_LSP_E2E".to_string(), Some("1".to_string())),
            ("PERL_LSP_DIAGNOSTIC_DEBOUNCE_MS".to_string(), Some("0".to_string())),
            ("PERL_LSP_DIAGNOSTIC_MODE".to_string(), Some("syntax-only".to_string())),
            ("PERL_LSP_EAGER_WORKSPACE_INDEXING".to_string(), Some("false".to_string())),
            ("PERL_LSP_FILE_WATCHERS".to_string(), Some("false".to_string())),
            // Quiet the startup banner so test output is uncluttered.
            ("PERL_LSP_QUIET".to_string(), Some("1".to_string())),
        ],
        workspace_files: Vec::new(),
        workspace_folders: Vec::new(),
        client_capability_overrides: json!({
            "workspace": {
                "didChangeWatchedFiles": {
                    "dynamicRegistration": true
                }
            },
            "textDocument": {
                "inlineCompletion": {
                    "dynamicRegistration": true
                }
            }
        }),
        initialization_options: Value::Null,
    }
}

/// Budget for one e2e scenario and for individual request roundtrips.
///
/// These receipts assert arrival, not speed (see the module header): the
/// claim is "we drove the e2e config and the answer arrived," so the bound
/// exists only to fail fast on a wedged binary. A tight budget turns the
/// verdict into a function of the runner: #16278 measured the two UX
/// workflows disagreeing on 11 of 30 identical heads because a loaded CI
/// moment pushed healthy roundtrips past fixed 8s/5s budgets (one observed
/// expiry: 7999ms with the stream still live). 60s keeps the wedge
/// detector while leaving runner-speed noise outside the claim; the happy
/// path still completes in well under a second.
fn timeout() -> Duration {
    Duration::from_secs(60)
}

/// Budget for "wait until the observation arrives" helpers. Same contract
/// as [`timeout`]: arrival is the assertion; the bound only detects a
/// wedged binary.
const ARRIVAL_BUDGET: Duration = Duration::from_secs(30);

fn symbol_tree_contains_name(symbols: &[Value], expected_name: &str) -> bool {
    let mut pending: Vec<&Value> = symbols.iter().collect();
    while let Some(symbol) = pending.pop() {
        if symbol.get("name").and_then(Value::as_str) == Some(expected_name) {
            return true;
        }
        if let Some(children) = symbol.get("children").and_then(Value::as_array) {
            pending.extend(children.iter());
        }
    }
    false
}

#[derive(Debug)]
struct WorkspaceSymbolObservation {
    symbols: Vec<Value>,
    elapsed: Duration,
    budget: Duration,
}

// Local runs use a print macro; CI needs descriptor IO to bypass libtest capture.
// Keep both paths explicit so the source-policy exception is operative.
#[expect(
    clippy::print_stderr,
    reason = "policy:allow-ux-ws-symbol-receipt-15988: one path-free success receipt must survive libtest capture in CI logs"
)]
fn emit_workspace_symbol_probe_receipt(receipt: &Value) -> Result<()> {
    if std::env::var_os("GITHUB_ACTIONS").is_some() {
        let stderr = std::io::stderr();
        let mut output = stderr.lock();
        serde_json::to_writer(&mut output, receipt)?;
        output.write_all(b"\n")?;
    } else {
        eprintln!("{receipt}");
    }
    Ok(())
}

fn workspace_symbols_with_budget(
    harness: &UxHarness,
    deadline: Instant,
    phase: &str,
) -> Result<WorkspaceSymbolObservation> {
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        bail!("workspace/symbol {phase}: overall deadline expired before RPC");
    }

    let request_started = Instant::now();
    match harness.workspace_symbols_with_timeout("alpha", remaining) {
        Ok(symbols) => Ok(WorkspaceSymbolObservation {
            symbols,
            elapsed: request_started.elapsed(),
            budget: remaining,
        }),
        Err(error) => {
            let kind = if format!("{error:#}").contains("deadline expired after") {
                "RPC timeout"
            } else {
                "RPC failure"
            };
            bail!(
                "workspace/symbol {phase}: {kind} after {}ms (budget {}ms): {error:#}",
                request_started.elapsed().as_millis(),
                remaining.as_millis()
            )
        }
    }
}

fn observe_immediate_workspace_symbols(
    request: impl FnOnce() -> Result<WorkspaceSymbolObservation>,
    readiness_after_rpc: impl FnOnce() -> std::result::Result<(), WaitEnd>,
    ready_before_query: bool,
) -> Result<WorkspaceSymbolObservation> {
    match request() {
        Ok(observation) => Ok(observation),
        Err(error) => {
            // The first RPC can consume the whole scenario budget. Preserve the
            // exact-document readiness state already buffered when it ends.
            let readiness = readiness_after_rpc();
            bail!(
                "workspace/symbol immediate request failed; ready_before_query={ready_before_query}; active_document_readiness_after_rpc={readiness:?}: {error:#}"
            )
        }
    }
}

fn registration_seen(events: &[LspEvent], method_name: &str) -> bool {
    events.iter().any(|event| {
        let LspEvent::Other { method, params } = event else {
            return false;
        };
        method == "client/registerCapability"
            && params.get("registrations").and_then(Value::as_array).into_iter().flatten().any(
                |registration| {
                    registration.get("method").and_then(Value::as_str) == Some(method_name)
                },
            )
    })
}

fn wait_for_registration(harness: &UxHarness, method_name: &str, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if registration_seen(&harness.client.peek_events(), method_name) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    false
}

#[test]
fn ux_latency_open_then_completion() -> Result<()> {
    if !binary_available() {
        eprintln!("SKIP ux_latency_open_then_completion: perl-lsp binary not found");
        return Ok(());
    }

    let harness = UxHarness::new(e2e_config(timeout()))?;
    harness.open_file("latency.pl", SHORT_SOURCE)?;

    // Completion just after `my $other = $val` (cursor at end of partial var name).
    let items = harness
        .completion("latency.pl", 4, 16)
        .map_err(|e| anyhow::anyhow!("textDocument/completion errored under e2e config: {e}"))?;

    // E2E receipt: completion responded under e2e mode. Empty list is
    // acceptable: the receipt is "the request completed cleanly", not
    // "completion is high-quality" (that's the job of the dedicated
    // scenario_19 tests).
    let _ = items;
    harness.assert_no_crash();
    Ok(())
}

#[test]
fn ux_latency_open_then_hover() -> Result<()> {
    if !binary_available() {
        eprintln!("SKIP ux_latency_open_then_hover: perl-lsp binary not found");
        return Ok(());
    }

    let harness = UxHarness::new(e2e_config(timeout()))?;
    harness.open_file("latency.pl", SHORT_SOURCE)?;

    // Hover on `$value` (line 3 `my $value = 42;`, cursor inside the name).
    let _hover = harness
        .hover("latency.pl", 3, 5)
        .map_err(|e| anyhow::anyhow!("textDocument/hover errored under e2e config: {e}"))?;

    harness.assert_no_crash();
    Ok(())
}

#[test]
fn ux_latency_edit_publishes_parse_error_diagnostic() -> Result<()> {
    if !binary_available() {
        eprintln!(
            "SKIP ux_latency_edit_publishes_parse_error_diagnostic: perl-lsp binary not found"
        );
        return Ok(());
    }

    let harness = UxHarness::new(e2e_config(timeout()))?;
    harness.open_file("broken.pl", PARSE_ERROR_SOURCE)?;

    // Under syntax-only + zero debounce, a parse error must arrive promptly.
    let diags = perl_lsp_ux_tests::wait_with_subject(
        &format!("diagnostics for {}", "broken.pl"),
        harness.wait_for_diagnostics("broken.pl", ARRIVAL_BUDGET),
    )?;
    assert!(
        !diags.is_empty(),
        "syntax-only e2e mode must surface parse errors; got empty diagnostics list"
    );

    // The diagnostic must be parser-sourced; syntax-only mode strips
    // critic / dead-code / module-resolution noise.
    let saw_parser =
        diags.iter().any(|d| d.get("source").and_then(|v| v.as_str()) == Some("perl-lsp"));
    assert!(
        saw_parser,
        "expected at least one perl-parser diagnostic under syntax-only mode; got {diags:?}"
    );

    harness.assert_no_crash();
    Ok(())
}

#[test]
fn ux_latency_edit_clears_diagnostics_when_parse_recovers() -> Result<()> {
    if !binary_available() {
        eprintln!(
            "SKIP ux_latency_edit_clears_diagnostics_when_parse_recovers: perl-lsp binary not found"
        );
        return Ok(());
    }

    let harness = UxHarness::new(e2e_config(timeout()))?;
    harness.open_file("recovers.pl", PARSE_ERROR_SOURCE)?;

    let bad = perl_lsp_ux_tests::wait_with_subject(
        &format!("diagnostics for {}", "recovers.pl"),
        harness.wait_for_diagnostics("recovers.pl", ARRIVAL_BUDGET),
    )?;
    assert!(!bad.is_empty(), "broken parse must report at least one diagnostic; got {bad:?}");

    // Apply the fix and expect the latest publish for this URI to be empty.
    harness.change_file_full("recovers.pl", CLEAN_SOURCE)?;
    let cleared = harness.wait_for_no_diagnostics("recovers.pl", ARRIVAL_BUDGET);
    assert!(
        cleared.is_ok(),
        "syntax-only mode must publish an empty diagnostic list after the parse recovers: {cleared:?}"
    );

    harness.assert_no_crash();
    Ok(())
}

#[test]
fn ux_latency_rapid_typing_latest_request_returns() -> Result<()> {
    if !binary_available() {
        eprintln!("SKIP ux_latency_rapid_typing_latest_request_returns: perl-lsp binary not found");
        return Ok(());
    }

    let harness = UxHarness::new(e2e_config(timeout()))?;
    harness.open_file("typing.pl", SHORT_SOURCE)?;

    // Simulate a short edit burst: each version replaces the file with a
    // longer variable name, growing one character at a time. This is the
    // typing-storm shape; every edit bumps the document generation, which
    // PR 4's generation-aware cancellation hooks onto.
    let burst = ["$va", "$val", "$valu", "$value"];
    for (i, partial) in burst.iter().enumerate() {
        let updated =
            format!("use strict;\nuse warnings;\n\nmy $value = 42;\nmy $other = {partial}\n");
        harness.change_file_full("typing.pl", &updated)?;
        // Throttle each edit just enough to give the scheduler real bursts
        // to deduplicate; on a wedged server this loop would time out.
        let _ = i;
        std::thread::sleep(Duration::from_millis(5));
    }

    // After the burst, the *latest* completion at the final cursor must
    // still return successfully. With PR 4, older queued completions are
    // cancelled; without PR 4, they may all run but the final answer is
    // what matters for the latency receipt.
    let _items: Vec<Value> = harness.completion("typing.pl", 4, 17)?;
    harness.assert_no_crash();
    Ok(())
}

#[test]
fn ux_latency_inline_completion_dynamic_path_returns_deterministic_items() -> Result<()> {
    if !binary_available() {
        eprintln!(
            "SKIP ux_latency_inline_completion_dynamic_path_returns_deterministic_items: perl-lsp binary not found"
        );
        return Ok(());
    }

    let harness = UxHarness::new(e2e_config(timeout()))?;
    let inline_registered =
        wait_for_registration(&harness, "textDocument/inlineCompletion", ARRIVAL_BUDGET);
    assert!(
        inline_registered,
        "lean LSP4IJ-shaped client must receive dynamic inline-completion registration"
    );
    assert!(
        !registration_seen(&harness.client.peek_events(), "workspace/didChangeWatchedFiles"),
        "lean profile must not register file watchers even when the client supports dynamic watchers"
    );

    harness.open_file("inline.pl", "use ")?;
    let items = harness.inline_completion("inline.pl", 0, 4)?;
    assert!(!items.is_empty(), "inline completion must return deterministic items for `use `");
    assert!(
        items.iter().any(|item| item.get("insertText").and_then(Value::as_str) == Some("strict;")),
        "inline completion must include deterministic strict; suggestion, got {items:?}"
    );

    harness.assert_no_crash();
    Ok(())
}

#[test]
fn ux_latency_document_symbols_returns_real_process_shape() -> Result<()> {
    if !binary_available() {
        return Ok(());
    }

    let harness = UxHarness::new(e2e_config(timeout()))?;
    harness.open_file("lib/Latency/Symbols.pm", SYMBOL_SOURCE)?;

    let symbols = harness.document_symbols("lib/Latency/Symbols.pm")?;
    assert!(
        symbol_tree_contains_name(&symbols, "alpha"),
        "documentSymbol must expose the alpha subroutine over the e2e path; got {symbols:?}"
    );

    harness.assert_no_crash();
    Ok(())
}

#[test]
fn ux_latency_workspace_symbols_sees_open_document_symbols() -> Result<()> {
    if !binary_available() {
        return Ok(());
    }

    let harness = UxHarness::new(e2e_config(timeout()))?;
    harness.open_file("lib/Latency/Symbols.pm", SYMBOL_SOURCE)?;
    let opened_at = Instant::now();
    let deadline = opened_at + ARRIVAL_BUDGET;
    let uri = harness.workspace.uri("lib/Latency/Symbols.pm");
    let ready_before_query = harness.wait_for_active_document_ready(&uri, Duration::ZERO);
    let first = observe_immediate_workspace_symbols(
        || workspace_symbols_with_budget(&harness, deadline, "immediate after didOpen"),
        || harness.wait_for_active_document_ready_result(&uri, Duration::ZERO),
        ready_before_query.is_ok(),
    )
    .with_context(|| {
        format!(
            "immediate workspace symbols for {uri}; readiness before query: {ready_before_query:?}"
        )
    })?;
    let first_has_alpha = first.symbols.iter().any(|symbol| symbol["name"] == "alpha");
    let ready_by_response = harness.wait_for_active_document_ready(&uri, Duration::ZERO);

    let ready_budget = deadline.saturating_duration_since(Instant::now());
    match harness.wait_for_active_document_ready_result(&uri, ready_budget) {
        Ok(()) => {}
        Err(WaitEnd::Deadline { .. }) => {
            bail!(
                "active-document readiness timeout after {}ms with stream live for {uri}; ready_before_query={ready_before_query:?}, ready_by_response={ready_by_response:?}, immediate={first:?}",
                opened_at.elapsed().as_millis()
            );
        }
        Err(end) => {
            bail!(
                "active-document readiness stream ended after {}ms for {uri}: {end:?}; ready_before_query={ready_before_query:?}, ready_by_response={ready_by_response:?}, immediate={first:?}",
                opened_at.elapsed().as_millis()
            );
        }
    }
    // The event may have arrived while the first RPC was in flight. This is
    // the latest time by which readiness is confirmed, not its arrival time.
    let readiness_confirmed_by = opened_at.elapsed();

    let after_ready = workspace_symbols_with_budget(&harness, deadline, "after active-document-ready")
        .with_context(|| {
            format!(
                "active-document-ready confirmed by {}ms for {uri}; ready_before_query={ready_before_query:?}, ready_by_response={ready_by_response:?}, immediate={first:?}",
                readiness_confirmed_by.as_millis()
            )
        })?;
    let after_ready_has_alpha = after_ready.symbols.iter().any(|symbol| symbol["name"] == "alpha");
    if !after_ready_has_alpha {
        bail!(
            "workspace/symbol empty or missing alpha after active-document-ready (confirmed by {}ms) for {uri}; ready_before_query={ready_before_query:?}, ready_by_response={ready_by_response:?}, immediate={first:?}, after_ready={after_ready:?}",
            readiness_confirmed_by.as_millis()
        );
    }
    harness.assert_no_crash();

    // One reviewed stderr exception keeps the passing timing visible in CI.
    let receipt = json!({
        "kind": "workspace_symbol_readiness_probe",
        "test": "ux_latency_workspace_symbols_sees_open_document_symbols",
        "result": "pass",
        "immediate_rpc_ms": first.elapsed.as_millis(),
        "immediate_budget_ms": first.budget.as_millis(),
        "immediate_alpha": first_has_alpha,
        "ready_before_query": ready_before_query.is_ok(),
        "ready_by_response": ready_by_response.is_ok(),
        "readiness_confirmed_by_ms": readiness_confirmed_by.as_millis(),
        "after_ready_rpc_ms": after_ready.elapsed.as_millis(),
        "after_ready_budget_ms": after_ready.budget.as_millis(),
        "after_ready_alpha": after_ready_has_alpha,
    });
    emit_workspace_symbol_probe_receipt(&receipt)?;

    Ok(())
}

#[test]
fn symbol_tree_contains_name_searches_nested_children() -> Result<()> {
    let symbols = vec![
        json!({
            "name": "Latency::Symbols",
            "children": [
                {
                    "name": "alpha",
                    "children": []
                }
            ]
        }),
        json!({
            "name": "beta"
        }),
    ];

    assert!(symbol_tree_contains_name(&symbols, "alpha"));
    assert!(symbol_tree_contains_name(&symbols, "beta"));
    assert!(!symbol_tree_contains_name(&symbols, "gamma"));
    Ok(())
}

#[test]
fn stalled_immediate_rpc_reports_readiness_that_arrived_during_request() -> Result<()> {
    let ready = std::cell::Cell::new(false);
    let error = observe_immediate_workspace_symbols(
        || {
            ready.set(true);
            Err(anyhow::anyhow!("deadline expired after 30s"))
        },
        || ready.get().then_some(()).ok_or(WaitEnd::Deadline { timeout: Duration::ZERO }),
        false,
    )
    .err()
    .ok_or_else(|| anyhow::anyhow!("simulated stalled request unexpectedly succeeded"))?;
    let message = format!("{error:#}");
    anyhow::ensure!(message.contains("active_document_readiness_after_rpc=Ok(())"), "{message}");
    anyhow::ensure!(message.contains("deadline expired after 30s"), "{message}");

    let not_ready = observe_immediate_workspace_symbols(
        || Err(anyhow::anyhow!("deadline expired after 30s")),
        || Err(WaitEnd::Deadline { timeout: Duration::ZERO }),
        false,
    )
    .err()
    .ok_or_else(|| anyhow::anyhow!("simulated stalled request unexpectedly succeeded"))?;
    anyhow::ensure!(
        format!("{not_ready:#}").contains("active_document_readiness_after_rpc=Err(Deadline"),
        "{not_ready:#}"
    );
    Ok(())
}

#[test]
fn ux_latency_code_action_returns_without_error_for_parse_diagnostic() -> Result<()> {
    if !binary_available() {
        return Ok(());
    }

    let harness = UxHarness::new(e2e_config(timeout()))?;
    harness.open_file("action.pl", PARSE_ERROR_SOURCE)?;
    let diagnostics = perl_lsp_ux_tests::wait_with_subject(
        &format!("diagnostics for {}", "action.pl"),
        harness.wait_for_diagnostics("action.pl", ARRIVAL_BUDGET),
    )?;
    assert!(
        !diagnostics.is_empty(),
        "code action e2e receipt needs a real diagnostic to act on; got {diagnostics:?}"
    );

    let uri = harness.workspace.uri("action.pl");
    let response = harness.client.request(
        "textDocument/codeAction",
        json!({
            "textDocument": { "uri": uri },
            "range": {
                "start": { "line": 2, "character": 0 },
                "end": { "line": 3, "character": 0 }
            },
            "context": {
                "diagnostics": diagnostics,
                "only": ["quickfix", "source"]
            }
        }),
        timeout(),
    )?;

    assert!(
        response.get("error").is_none(),
        "textDocument/codeAction must not return a JSON-RPC error under e2e mode: {response:?}"
    );
    assert!(
        response.get("result").and_then(Value::as_array).is_some() || response["result"].is_null(),
        "textDocument/codeAction must return an action array or null; got {response:?}"
    );

    harness.assert_no_crash();
    Ok(())
}
