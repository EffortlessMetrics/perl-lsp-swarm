//! Fast/slow diagnostic split tests (Issue #4279)
//!
//! Verifies that parse errors are published immediately (fast path) on didChange,
//! while the full diagnostic set follows after the debounce (slow path).
//! The fast path publishes ONLY parse-error-coded diagnostics (PL001).

mod support;

use std::time::Duration;

use serde_json::json;
use support::lsp_harness::LspHarness;

const PARSE_ERROR_CODE: &str = "PL001";

fn push_diagnostics_capabilities() -> serde_json::Value {
    json!({ "textDocument": {} })
}

fn wait_diagnostic_codes(
    harness: &mut LspHarness,
    uri: &str,
    context: &str,
) -> Result<Vec<String>, String> {
    let notification = harness
        .wait_for_notification("textDocument/publishDiagnostics", Duration::from_secs(2))
        .map_err(|err| format!("Expected {context} publishDiagnostics notification: {err}"))?;
    assert_eq!(
        notification.get("uri").and_then(|value| value.as_str()),
        Some(uri),
        "{context} diagnostics must target the changed URI"
    );
    let diagnostics = notification
        .get("diagnostics")
        .and_then(|value| value.as_array())
        .ok_or_else(|| format!("{context} publishDiagnostics missing diagnostics array"))?;

    diagnostics
        .iter()
        .map(|diagnostic| {
            diagnostic
                .get("code")
                .and_then(|value| value.as_str())
                .map(str::to_string)
                .ok_or_else(|| format!("{context} diagnostic missing string code: {diagnostic:?}"))
        })
        .collect()
}

fn assert_codes_include(codes: &[String], expected: &[&str], context: &str) {
    let missing: Vec<&str> = expected
        .iter()
        .copied()
        .filter(|code| !codes.iter().any(|actual| actual == code))
        .collect();
    assert_eq!(missing, Vec::<&str>::new(), "{context} codes were {codes:?}");
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
    harness.initialize(Some(push_diagnostics_capabilities()))?;

    let uri = "file:///fast_slow_test.pl";

    // Open a clean document first
    harness.open(uri, "my $x = 1;\n")?;

    // Drain the initial didOpen diagnostics before testing the didChange fast path.
    let did_open_codes = wait_diagnostic_codes(&mut harness, uri, "didOpen")?;
    assert_codes_include(&did_open_codes, &["PL100", "PL101"], "didOpen");
    let _ = harness.drain_notifications(Some("textDocument/publishDiagnostics"), 100);

    // Now change to a document with a parse error
    harness.change_full(uri, 2, "my $broken = ;\n")?;

    // The FIRST notification must be the fast path: only parse-error diagnostics
    let first_codes = wait_diagnostic_codes(&mut harness, uri, "fast didChange")?;
    assert_eq!(first_codes, vec![PARSE_ERROR_CODE.to_string()]);

    Ok(())
}

/// Two-phase delivery: didChange produces two publishDiagnostics notifications.
/// First: parse-errors only (fast). Second: full set (slow, replaces the first).
#[test]
fn test_two_phase_diagnostic_delivery_on_change() -> Result<(), String> {
    let mut harness = LspHarness::new();
    harness.initialize(Some(push_diagnostics_capabilities()))?;

    let uri = "file:///two_phase_test.pl";

    harness.open(uri, "my $x = 1;\n")?;

    // Drain didOpen diagnostics before testing didChange delivery order.
    let did_open_codes = wait_diagnostic_codes(&mut harness, uri, "didOpen")?;
    assert_codes_include(&did_open_codes, &["PL100", "PL101"], "didOpen");
    let _ = harness.drain_notifications(Some("textDocument/publishDiagnostics"), 100);

    // Change to a document with a parse error
    harness.change_full(uri, 2, "my $broken = ;\n")?;

    // First notification: parse errors only
    let first_codes = wait_diagnostic_codes(&mut harness, uri, "fast didChange")?;
    assert_eq!(first_codes, vec![PARSE_ERROR_CODE.to_string()]);

    // Last notification: full set - must include parse and non-parse diagnostics
    let second_codes = wait_diagnostic_codes(&mut harness, uri, "slow didChange")?;
    assert_codes_include(&second_codes, &[PARSE_ERROR_CODE, "PL100", "PL101"], "slow didChange");

    Ok(())
}
