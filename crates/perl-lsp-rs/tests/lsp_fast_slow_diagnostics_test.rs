//! Fast/slow diagnostic split tests (Issue #4279)
//!
//! Verifies that parse errors are published immediately (fast path) on didChange,
//! while the full diagnostic set follows after the debounce (slow path).
//! The fast path publishes ONLY parse-error-coded diagnostics (PL001).

mod support;

use std::time::Duration;

use serde_json::{Value, json};
use support::lsp_harness::LspHarness;

const DIAGNOSTIC_TIMEOUT: Duration = Duration::from_secs(2);

fn wait_for_publish_diagnostics(harness: &mut LspHarness, context: &str) -> Result<Value, String> {
    harness
        .wait_for_notification("textDocument/publishDiagnostics", DIAGNOSTIC_TIMEOUT)
        .map_err(|err| format!("{context}: {err}"))
}

/// Fast path: the first publishDiagnostics notification after didChange must
/// contain only parse-error diagnostics (code "PL001"), not scope-analysis
/// or quality-check diagnostics.
///
/// In production this fires before the 250ms debounce; in tests both fire
/// synchronously so we check that the FIRST notification is parse-errors-only.
#[test]
fn test_fast_path_contains_only_parse_errors() -> Result<(), String> {
    let mut harness = LspHarness::new();
    harness.initialize(Some(json!({
        "textDocument": {
            // No pull-diagnostics capability: force push path
        }
    })))?;

    let uri = "file:///fast_slow_test.pl";

    // Open a clean document first
    harness.open(uri, "my $x = 1;\n")?;

    // Drain the initial didOpen diagnostics before testing the didChange fast path.
    let _ = wait_for_publish_diagnostics(&mut harness, "Expected didOpen diagnostics")?;
    let _ = harness.drain_notifications(Some("textDocument/publishDiagnostics"), 100);

    // Now change to a document with a parse error
    harness.change_full(uri, 2, "my $broken = ;\n")?;

    let first = wait_for_publish_diagnostics(
        &mut harness,
        "Expected at least one publishDiagnostics notification after didChange",
    )?;

    // The FIRST notification must be the fast path: only parse-error diagnostics
    let first_diags = first["diagnostics"].as_array().cloned().unwrap_or_default();

    assert!(
        !first_diags.is_empty(),
        "Fast-path notification must contain parse-error diagnostics, got empty list"
    );

    let parse_error_code = "PL001";
    assert!(
        first_diags
            .iter()
            .all(|d| { d["code"].as_str().map(|c| c == parse_error_code).unwrap_or(false) }),
        "Fast-path publishDiagnostics must ONLY contain parse-error diagnostics \
         (code {}), but found other codes: {:?}",
        parse_error_code,
        first_diags.iter().map(|d| d["code"].as_str()).collect::<Vec<_>>()
    );

    Ok(())
}

/// Two-phase delivery: didChange produces two publishDiagnostics notifications.
/// First: parse-errors only (fast). Second: full set (slow, replaces the first).
#[test]
fn test_two_phase_diagnostic_delivery_on_change() -> Result<(), String> {
    let mut harness = LspHarness::new();
    harness.initialize(Some(json!({
        "textDocument": {}
    })))?;

    let uri = "file:///two_phase_test.pl";

    harness.open(uri, "my $x = 1;\n")?;

    // Drain didOpen diagnostics before testing didChange delivery order.
    let _ = wait_for_publish_diagnostics(&mut harness, "Expected didOpen diagnostics")?;
    let _ = harness.drain_notifications(Some("textDocument/publishDiagnostics"), 100);

    // Change to a document with a parse error
    harness.change_full(uri, 2, "my $broken = ;\n")?;

    let first = wait_for_publish_diagnostics(
        &mut harness,
        "Expected fast publishDiagnostics notification after didChange",
    )?;
    let second = wait_for_publish_diagnostics(
        &mut harness,
        "Expected slow publishDiagnostics notification after didChange",
    )?;

    // First notification: parse errors only
    let first_diags = first["diagnostics"].as_array().cloned().unwrap_or_default();
    assert!(
        first_diags.iter().all(|d| d["code"].as_str() == Some("PL001")),
        "First (fast-path) notification must contain only parse errors (PL001), \
         got codes: {:?}",
        first_diags.iter().map(|d| d["code"].as_str()).collect::<Vec<_>>()
    );

    // Last notification: full set - must include non-parse-error diagnostics
    let last_diags = second["diagnostics"].as_array().cloned().unwrap_or_default();
    assert!(
        last_diags.iter().any(|d| d["code"].as_str() != Some("PL001")),
        "Last (slow-path) notification must contain the full diagnostic set \
         including non-parse-error codes, but only found: {:?}",
        last_diags.iter().map(|d| d["code"].as_str()).collect::<Vec<_>>()
    );

    Ok(())
}
