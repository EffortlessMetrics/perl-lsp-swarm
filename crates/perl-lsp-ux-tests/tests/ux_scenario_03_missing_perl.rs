#![expect(
    clippy::print_stderr,
    reason = "Scenario 03 reports local non-execution when the required perllsp binary is unavailable."
)]

//! Scenario 03 — Missing perl interpreter.
//!
//! Simulates perl-lsp running without `perl` on PATH.
//!
//! Acceptance criteria:
//! - Server MUST start (it is a Rust binary).
//! - Server MUST accept `initialize` and `textDocument/didOpen`.
//! - Server MUST NOT crash during initialization.
//! - Hover and completion may return null/empty — that is acceptable.
//!
//! Warning-channel presence is not part of this scenario's required behavior.
//! This file proves bounded degraded service continuity, not user-visible warning delivery.

use perl_lsp_ux_tests::binary_available;
use perl_lsp_ux_tests::{ScenarioConfig, UxHarness};

fn config_without_perl() -> ScenarioConfig {
    ScenarioConfig { path_restriction: Some(Vec::new()), ..Default::default() }
}

#[test]
fn scenario_03_server_starts_without_perl() -> Result<(), String> {
    if !binary_available() {
        eprintln!("SKIP scenario_03: perl-lsp binary not found");
        return Ok(());
    }

    let source = "use strict;\nmy $x = 1;\n";
    let harness = UxHarness::new(config_without_perl())
        .map_err(|error| format!("Failed to create UX harness (no perl): {error}"))?;

    harness
        .open_file("no_perl.pl", source)
        .map_err(|error| format!("didOpen should succeed without perl: {error}"))?;

    harness.assert_no_crash();
    Ok(())
}

#[test]
fn scenario_03_degraded_mode_hover_does_not_crash() -> Result<(), String> {
    if !binary_available() {
        eprintln!("SKIP scenario_03: perl-lsp binary not found");
        return Ok(());
    }

    let source = "my $x = 42;\n";
    let harness = UxHarness::new(config_without_perl())
        .map_err(|error| format!("Failed to create UX harness (no perl): {error}"))?;

    harness
        .open_file("degraded.pl", source)
        .map_err(|error| format!("didOpen should succeed: {error}"))?;

    harness.hover("degraded.pl", 0, 3).map_err(|error| {
        format!("hover should not return a transport error in degraded mode: {error}")
    })?;

    harness.assert_no_crash();
    Ok(())
}

#[test]
fn scenario_03_degraded_mode_completion_does_not_crash() -> Result<(), String> {
    if !binary_available() {
        eprintln!("SKIP scenario_03: perl-lsp binary not found");
        return Ok(());
    }

    let source = "use str\n";
    let harness = UxHarness::new(config_without_perl())
        .map_err(|error| format!("Failed to create UX harness (no perl): {error}"))?;

    harness
        .open_file("complete.pl", source)
        .map_err(|error| format!("didOpen should succeed: {error}"))?;

    harness.completion("complete.pl", 0, 7).map_err(|error| {
        format!("completion should not return a transport error in degraded mode: {error}")
    })?;

    harness.assert_no_crash();
    Ok(())
}
