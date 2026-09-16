#![expect(
    clippy::print_stderr,
    reason = "Scenario 02 reports a local non-execution reason when the required perllsp binary is unavailable."
)]

//! Scenario 02 — Missing perltidy.
//!
//! Simulates a user who has installed perl-lsp but not perltidy.
//! Formatting requests should degrade gracefully.
//!
//! Acceptance criteria:
//! - The server MUST NOT crash.
//! - `textDocument/formatting` MUST return a graceful error or empty result.
//! - No Rust panic traces in error messages.
//! - The server MUST still be alive after the failed formatting request.

use perl_lsp_ux_tests::binary_available;
use perl_lsp_ux_tests::{FormatResult, ScenarioConfig, UxHarness};

fn config_without_perltidy() -> ScenarioConfig {
    // Exclude only perltidy from PATH, leaving perl and other tools available.
    // This accurately simulates "user has perl but not perltidy installed".
    let sep = if cfg!(windows) { ';' } else { ':' };
    let dirs: Vec<String> = std::env::var("PATH")
        .unwrap_or_default()
        .split(sep)
        .filter(|entry| !entry.contains("perltidy"))
        .map(String::from)
        .collect();
    ScenarioConfig { path_restriction: Some(dirs), ..Default::default() }
}

#[test]
fn scenario_02_formatting_without_perltidy_does_not_crash() -> Result<(), String> {
    if !binary_available() {
        eprintln!("SKIP scenario_02: perl-lsp binary not found");
        return Ok(());
    }

    let source = "sub test{my$x=1;return$x;}\n";
    let harness = UxHarness::new(config_without_perltidy())
        .map_err(|error| format!("Failed to create UX harness: {error}"))?;

    harness
        .open_file("format_me.pl", source)
        .map_err(|error| format!("didOpen should succeed: {error}"))?;

    match harness.format_document("format_me.pl") {
        Ok(FormatResult::Edits(_)) | Ok(FormatResult::Empty) => {}
        Ok(FormatResult::Error(error_value)) => {
            let message = error_value.get("message").and_then(|value| value.as_str()).unwrap_or("");
            assert!(
                !message.contains("panicked at") && !message.contains("SIGABRT"),
                "Error message looks like a Rust panic: {message}"
            );
        }
        Err(error) => {
            return Err(format!(
                "Formatting failed at the UX harness boundary instead of returning a bounded product result: {error}"
            ));
        }
    }

    harness.assert_no_crash();
    Ok(())
}

#[test]
fn scenario_02_server_remains_alive_after_failed_format() -> Result<(), String> {
    if !binary_available() {
        eprintln!("SKIP scenario_02: perl-lsp binary not found");
        return Ok(());
    }

    let source = "my $x = 1;\n";
    let harness = UxHarness::new(config_without_perltidy())
        .map_err(|error| format!("Failed to create UX harness: {error}"))?;

    harness
        .open_file("alive.pl", source)
        .map_err(|error| format!("didOpen should succeed: {error}"))?;

    match harness.format_document("alive.pl") {
        Ok(FormatResult::Edits(_)) | Ok(FormatResult::Empty) | Ok(FormatResult::Error(_)) => {}
        Err(error) => {
            return Err(format!(
                "Formatting failed at the UX harness boundary instead of returning a bounded product result: {error}"
            ));
        }
    }

    harness.hover("alive.pl", 0, 3).map_err(|error| {
        format!("Server unresponsive after failed formatting — UX regression: {error}")
    })?;
    Ok(())
}
