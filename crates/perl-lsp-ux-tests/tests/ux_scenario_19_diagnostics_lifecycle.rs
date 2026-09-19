// Test infrastructure — allow test-friendly patterns.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! Scenario 19 — diagnostics lifecycle during active editing.
//!
//! This scenario covers an editor-critical UX flow: a user introduces a parse
//! error, sees diagnostics, fixes the file, and expects diagnostics to clear.
//!
//! # Robustness note
//!
//! LSP servers may clear diagnostics in two ways:
//! 1. Explicit empty `textDocument/publishDiagnostics` (empty array).
//! 2. Silently — no notification after fix.
//!
//! The test accepts either: it drains the pre-fix event queue, waits for any
//! stale in-flight broken-content results to arrive and be absorbed, then
//! checks whether the server sends an explicit empty notification or remains
//! silent (silence = cleared) within the post-settle window.
//!
//! # Race condition history
//!
//! The core challenge is that the LSP server runs diagnostics asynchronously.
//! After `textDocument/didChange` is sent with the fixed content, the server
//! may still be mid-analysis on the broken content, and those stale results
//! can arrive in the event queue after the fix is sent.
//!
//! Solution: a two-phase drain around the fix:
//!   Phase 1 (pre-fix):  drain + short settle to absorb events buffered before
//!                        `change_file_full` is called.
//!   Phase 2 (post-fix): a longer settle + drain immediately after
//!                        `change_file_full` to absorb stale in-flight results
//!                        from an analysis that was already running when the
//!                        fix arrived. Only then enter the clean-window check.

use perl_lsp_ux_tests::binary_available;
use perl_lsp_ux_tests::{ScenarioConfig, UxHarness};
use std::time::Duration;

const BROKEN_SOURCE: &str = "use strict;\nuse warnings;\nmy $x = ;\n";
const FIXED_SOURCE: &str = "use strict;\nuse warnings;\nmy $x = 1;\nprint $x;\n";

/// Verifies the diagnostics edit lifecycle:
///   1. Broken content → diagnostics appear.
///   2. Fixed content → diagnostics clear (either explicitly or by silence).
#[test]
fn scenario_19_diagnostics_clear_after_fix() {
    if !binary_available() {
        eprintln!("SKIP scenario_19_diagnostics_clear_after_fix: perl-lsp binary not found");
        return;
    }

    let harness = UxHarness::new(
        ScenarioConfig { timeout: Duration::from_secs(15), ..Default::default() }
            .with_file("live.pl", BROKEN_SOURCE),
    )
    .expect("Failed to create UX harness");

    // Given: a workspace file opened with a syntax error.
    harness.open_file("live.pl", BROKEN_SOURCE).expect("didOpen should succeed");

    // When: diagnostics are first published for the broken content.
    let diagnostics = harness.wait_for_diagnostics("live.pl", Duration::from_secs(5));
    assert!(
        !diagnostics.is_empty(),
        "Expected diagnostics for broken source, but none were published."
    );

    // Drain the broken-generation events before the edit. The post-edit
    // active-document-ready event is the synchronization boundary.
    harness.collect_notifications();

    // When: the user fixes the file via a full-document didChange.
    harness.change_file_full("live.pl", FIXED_SOURCE).expect("didChange should succeed");

    let uri = harness.workspace.uri("live.pl");
    assert!(
        harness.wait_for_active_document_ready(&uri, Duration::from_secs(5)),
        "fixed generation did not reach active-document readiness"
    );
    let diagnostics = harness.wait_for_latest_diagnostics("live.pl", Duration::from_secs(5));
    assert!(
        diagnostics.is_empty(),
        "fixed generation should publish no diagnostics: {diagnostics:?}"
    );
    harness.assert_no_crash();
}
