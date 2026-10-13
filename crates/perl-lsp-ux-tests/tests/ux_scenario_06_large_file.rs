#![expect(
    clippy::print_stderr,
    reason = "Scenario 06 reports a local non-execution reason when the required perllsp binary is unavailable."
)]

//! Scenario 06 — Large file.
//!
//! Opens a 10 000-line Perl file and verifies that the server handles it without
//! hanging or OOM-crashing.
//!
//! The heavy tests (10k lines) are gated behind `integration-test` feature.
//! The gate allows the scenario to appear in the default test run (with a
//! reduced 1k-line version) so CI always exercises the code path.

use perl_lsp_ux_tests::binary_available;
use perl_lsp_ux_tests::{ScenarioConfig, UxHarness};
use std::time::Duration;

fn generate_source(line_count: usize) -> String {
    let mut buf = String::with_capacity(line_count * 40);
    buf.push_str("use strict;\nuse warnings;\n\n");
    for i in 0..line_count {
        buf.push_str(&format!("sub func_{i} {{ my $x_{i} = {i}; return $x_{i}; }}\n"));
    }
    buf
}

#[test]
fn scenario_06_medium_file_open_and_hover() -> Result<(), String> {
    // Always runs — 1k lines is fast enough for PR gate.
    if !binary_available() {
        eprintln!("SKIP scenario_06: perl-lsp binary not found");
        return Ok(());
    }

    let source = generate_source(1_000);
    let harness =
        UxHarness::new(ScenarioConfig { timeout: Duration::from_secs(20), ..Default::default() })
            .map_err(|error| format!("Failed to create UX harness: {error}"))?;

    harness
        .open_file("medium.pl", &source)
        .map_err(|error| format!("didOpen should succeed for 1k-line file: {error}"))?;

    harness.hover("medium.pl", 5, 5).map_err(|error| {
        format!("Server hung or crashed on 1k-line file — UX regression: {error}")
    })?;

    harness.assert_no_crash();
    Ok(())
}

#[cfg(feature = "integration-test")]
#[test]
fn scenario_06_large_file_open_does_not_hang() -> Result<(), String> {
    if !binary_available() {
        eprintln!("SKIP scenario_06 (large): perl-lsp binary not found");
        return Ok(());
    }

    let source = generate_source(10_000);
    let harness =
        UxHarness::new(ScenarioConfig { timeout: Duration::from_secs(30), ..Default::default() })
            .map_err(|error| format!("Failed to create UX harness for large file: {error}"))?;

    harness
        .open_file("large.pl", &source)
        .map_err(|error| format!("didOpen should accept a 10k-line file: {error}"))?;

    harness.hover("large.pl", 5, 5).map_err(|error| {
        format!("Server hung or crashed after opening large file — UX regression: {error}")
    })?;

    harness.assert_no_crash();
    Ok(())
}
