//! Startup trace receipt for the Neovim lean profile.
//!
//! This is an e2e wiring receipt, not a hard latency budget. It records the
//! observed lean startup path and asserts that the no-eager-indexing and
//! no-file-watcher dials are active.
//!
//! # Disposition policy (#15613, #16977)
//!
//! Both print findings in this target are classified rather than silenced:
//!
//! - The missing-binary case is **local non-execution**. It is modelled by
//!   [`TraceStart::NotExecuted`], which publishes a `not_executed` receipt and
//!   can never be read as a product pass.
//! - The structured trace is **scenario evidence**. It goes through the one
//!   sanctioned emitter, [`emit_lean_startup_trace_receipt`], which writes to
//!   the descriptor directly under CI so the receipt survives libtest capture.
//!
//! Neither finding buys a file-level lint allowance; the workspace
//! `print_stderr`/`print_stdout` denial stays in force for every other line.

use anyhow::Result;
use perl_lsp_ux_tests::{
    LspEvent, ScenarioConfig, UxHarness, binary_available, missing_binary_skip,
};
use serde_json::{Value, json};
use std::io::Write;
use std::time::{Duration, Instant};

const TRACE_SOURCE: &str = r#"use strict;
use warnings;

my $value = 42;
my $other = $val
sub broken {
"#;

/// The startup trace is "an e2e wiring receipt, not a hard latency budget"
/// (module header): the assertion is that the lean dials fire and answers
/// arrive, not that they arrive quickly. These bounds exist only to fail
/// fast on a wedged binary. Tight budgets made the verdict a function of
/// the runner — #16278 measured the two UX workflows disagreeing on 11 of
/// 30 identical heads. The happy path still completes in well under a
/// second.
const SCENARIO_TIMEOUT: Duration = Duration::from_mins(1);
const ARRIVAL_BUDGET: Duration = Duration::from_secs(30);

/// Why the scenario did or did not reach a runnable `perl-lsp`.
#[derive(Debug, Clone, PartialEq, Eq)]
enum TraceStart {
    /// A runnable binary exists, so the scenario may publish a trace.
    Run,
    /// Local non-execution. Carries the package's typed infra skip reason and
    /// is never a product pass.
    NotExecuted {
        /// Human-readable reason taken from [`missing_binary_skip`].
        reason: String,
    },
}

impl TraceStart {
    /// Only a run that actually reached a binary is product evidence.
    fn is_product_evidence(&self) -> bool {
        matches!(self, Self::Run)
    }

    /// The receipt published when the scenario cannot run, or `None` for a run.
    fn non_execution_receipt(&self) -> Option<Value> {
        match self {
            Self::Run => None,
            Self::NotExecuted { reason } => Some(json!({
                "profile": "neovim_lean",
                "disposition": "not_executed",
                "product_evidence": false,
                "reason": reason,
            })),
        }
    }
}

/// Pure classifier behind the binary gate.
///
/// Kept free of harness and process state so "a missing binary is
/// non-execution, not a pass" is executable proof rather than a comment.
fn classify_trace_start(binary_available: bool) -> TraceStart {
    if binary_available {
        return TraceStart::Run;
    }
    TraceStart::NotExecuted { reason: missing_binary_skip().reason }
}

/// The one sanctioned print site in this target.
///
/// Local runs use a print macro; CI needs descriptor IO to bypass libtest
/// capture, which otherwise discards the receipt from the job log. Keep both
/// paths explicit so the source-policy exception is operative.
#[expect(
    clippy::print_stdout,
    reason = "policy:allow-ux-lean-startup-trace-16977: the lean startup trace receipt and its not-executed marker must survive libtest capture in CI logs"
)]
fn emit_lean_startup_trace_receipt(receipt: &Value) -> Result<()> {
    let rendered = serde_json::to_string_pretty(receipt)?;
    if std::env::var_os("GITHUB_ACTIONS").is_some() {
        let stdout = std::io::stdout();
        let mut output = stdout.lock();
        output.write_all(rendered.as_bytes())?;
        output.write_all(b"\n")?;
        output.flush()?;
    } else {
        println!("{rendered}");
    }
    Ok(())
}

fn trace_config(timeout: Duration) -> ScenarioConfig {
    ScenarioConfig {
        timeout,
        path_restriction: None,
        echo_stderr: false,
        extra_env: vec![
            ("PERL_LSP_E2E".to_string(), Some("1".to_string())),
            ("PERL_LSP_DIAGNOSTIC_MODE".to_string(), Some("syntax-only".to_string())),
            ("PERL_LSP_DIAGNOSTIC_DEBOUNCE_MS".to_string(), Some("0".to_string())),
            ("PERL_LSP_EAGER_WORKSPACE_INDEXING".to_string(), Some("false".to_string())),
            ("PERL_LSP_FILE_WATCHERS".to_string(), Some("false".to_string())),
            ("PERL_LSP_QUIET".to_string(), Some("1".to_string())),
            (
                "RUST_LOG".to_string(),
                Some("perl_lsp::runtime::dispatch::lifecycle=debug".to_string()),
            ),
        ],
        workspace_files: Vec::new(),
        workspace_folders: vec![("project".to_string(), "trace-project".to_string())],
        client_capability_overrides: json!({
            "workspace": {
                "didChangeWatchedFiles": {
                    "dynamicRegistration": true
                }
            },
            "textDocument": {
                "inlineCompletion": {
                    "dynamicRegistration": true
                },
                "semanticTokens": {
                    "requests": {
                        "full": true,
                        "range": true
                    }
                }
            }
        }),
        initialization_options: Value::Null,
    }
}

fn record_event(events: &mut Vec<Value>, name: &str, start: Instant) {
    events.push(json!({
        "name": name,
        "elapsed_ms": start.elapsed().as_secs_f64() * 1000.0,
    }));
}

fn wait_for_stderr_line(harness: &UxHarness, needle: &str, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if harness.client.peek_stderr_lines().iter().any(|line| line.contains(needle)) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    false
}

fn file_watcher_registered(events: &[LspEvent]) -> bool {
    registration_seen(events, "workspace/didChangeWatchedFiles")
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
fn ux_neovim_lean_startup_trace_receipt() -> Result<()> {
    let start_state = classify_trace_start(binary_available());
    if let Some(receipt) = start_state.non_execution_receipt() {
        emit_lean_startup_trace_receipt(&receipt)?;
        return Ok(());
    }
    debug_assert!(
        start_state.is_product_evidence(),
        "a run must be the only product-evidence disposition"
    );

    let start = Instant::now();
    let mut events = Vec::new();
    record_event(&mut events, "process_start_observed", start);

    let harness = UxHarness::new(trace_config(SCENARIO_TIMEOUT))?;
    record_event(&mut events, "initialize_response_received", start);
    record_event(&mut events, "initialized_notification_sent", start);

    let init = harness.client.initialize_result();
    assert_eq!(
        init.pointer("/result/capabilities/semanticTokensProvider/full/delta"),
        Some(&json!(true)),
        "lean startup trace must advertise semantic token delta support (LSP 3.17)"
    );
    record_event(&mut events, "semantic_tokens_capability_checked", start);

    assert_eq!(
        init.pointer("/result/capabilities/workspace/textDocumentContent/schemes"),
        Some(&json!(["perldoc"])),
        "lean startup trace must keep perldoc textDocumentContent capability advertised"
    );
    record_event(&mut events, "text_document_content_capability_checked", start);

    let inline_registered =
        wait_for_registration(&harness, "textDocument/inlineCompletion", ARRIVAL_BUDGET);
    assert!(
        inline_registered,
        "lean startup trace must dynamically register inline completion for LSP4IJ-shaped clients"
    );
    record_event(&mut events, "inline_completion_registration_checked", start);

    let indexing_skip_observed = wait_for_stderr_line(
        &harness,
        "Skipping eager workspace indexing on `initialized`",
        ARRIVAL_BUDGET,
    );
    assert!(
        indexing_skip_observed,
        "lean startup trace must observe eager workspace indexing skipped; stderr={:?}",
        harness.client.peek_stderr_lines()
    );
    record_event(&mut events, "workspace_indexing_decision_observed", start);

    let watcher_registered = file_watcher_registered(&harness.client.peek_events());
    assert!(!watcher_registered, "lean startup trace must not register workspace file watchers");
    record_event(&mut events, "file_watcher_registration_checked", start);

    harness.open_file("project/trace.pl", TRACE_SOURCE)?;
    record_event(&mut events, "did_open_sent", start);

    let diags = perl_lsp_ux_tests::wait_with_subject(
        &format!("diagnostics for {}", "project/trace.pl"),
        harness.wait_for_diagnostics("project/trace.pl", ARRIVAL_BUDGET),
    )?;
    assert!(!diags.is_empty(), "syntax-only lean trace must publish parser diagnostics");
    record_event(&mut events, "first_did_open_processed", start);
    record_event(&mut events, "first_diagnostic_published", start);

    let _items = harness.completion("project/trace.pl", 4, 16)?;
    record_event(&mut events, "first_completion_response", start);

    let receipt = json!({
        "profile": "neovim_lean",
        "disposition": "executed",
        "product_evidence": start_state.is_product_evidence(),
        "workspace_indexing_started": false,
        "workspace_indexing_decision_observed": indexing_skip_observed,
        "file_watchers_registered": watcher_registered,
        "inline_completion_registered": inline_registered,
        "semantic_tokens_delta_advertised": true,
        "text_document_content_schemes": ["perldoc"],
        "diagnostic_mode": "syntax_only",
        "diagnostic_debounce_ms": 0,
        "events": events,
    });
    emit_lean_startup_trace_receipt(&receipt)?;

    harness.assert_no_crash();
    Ok(())
}

/// A missing binary must classify as local non-execution, never a product
/// pass, and a run must not be able to publish the not-executed marker.
///
/// This is the discriminating proof for the #16977 print dispositions: it
/// fails if the skip regresses to a bare `return Ok(())` that reads as a
/// pass, and fails if a run starts emitting non-execution evidence.
#[test]
fn a_missing_binary_is_non_execution_never_a_product_pass() -> Result<()> {
    let skipped = classify_trace_start(false);
    assert!(
        !skipped.is_product_evidence(),
        "a missing perl-lsp must not be product evidence; got {skipped:?}"
    );

    let Some(receipt) = skipped.non_execution_receipt() else {
        anyhow::bail!("a skipped scenario must publish a not-executed receipt")
    };
    assert_eq!(receipt["disposition"], json!("not_executed"));
    assert_eq!(receipt["product_evidence"], json!(false));
    assert_eq!(receipt["profile"], json!("neovim_lean"));
    let reason = receipt["reason"].as_str().ok_or_else(|| {
        anyhow::anyhow!("not-executed receipt must carry a string reason; got {receipt}")
    })?;
    assert!(!reason.is_empty(), "a non-execution receipt must state why it did not execute");
    assert_eq!(reason, missing_binary_skip().reason);

    let ran = classify_trace_start(true);
    assert!(
        ran.is_product_evidence(),
        "an available perl-lsp is the only product-evidence disposition; got {ran:?}"
    );
    assert!(
        ran.non_execution_receipt().is_none(),
        "a run must never publish a not-executed marker"
    );
    Ok(())
}
