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
//! - On the external perltidy engine the error MUST name `perltidy` and carry
//!   the canonical install guidance, so the user can act on it.
//! - No Rust panic traces in error messages.
//! - The server MUST still be alive after the failed formatting request.

use perl_lsp_ux_tests::binary_available;
use perl_lsp_ux_tests::{FormatResult, ScenarioConfig, UxHarness};

/// Distinctive classification substring of the missing-formatter error
/// (`FormattingError::PerltidyNotFound` Display).
const PERLTIDY_NOT_FOUND: &str = "perltidy not found";
/// Distinctive install-guidance substring carried by the same error.
const PERLTIDY_INSTALL_GUIDANCE: &str = "cpanm Perl::Tidy";
/// A seeded project config that routes formatting through the external perltidy
/// adapter, where a missing perltidy is a user-actionable error instead of a
/// transparent native fallback.
const EXTERNAL_ENGINE_PROJECT_CONFIG: &str = "[formatting]\nengine = \"external-perltidy\"\n";

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

/// Deterministic "perltidy is not invocable" environment for the external
/// engine: an empty child PATH guarantees the `perltidy` spawn fails on every
/// platform. A directory-name filter alone cannot do this — on CI perltidy
/// installs to `/usr/bin`, whose name does not contain "perltidy".
///
/// The external engine is selected through the seeded project config so the
/// format request lands on the external adapter instead of the native one.
fn config_external_engine_without_perltidy() -> ScenarioConfig {
    ScenarioConfig::with_empty_path().with_file(".perl-lsp.toml", EXTERNAL_ENGINE_PROJECT_CONFIG)
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

#[test]
fn scenario_02_external_engine_format_error_names_perltidy_and_install_guidance()
-> Result<(), String> {
    if !binary_available() {
        eprintln!("SKIP scenario_02: perl-lsp binary not found");
        return Ok(());
    }

    let source = "sub test{my$x=1;return$x;}\n";
    let harness = UxHarness::new(config_external_engine_without_perltidy())
        .map_err(|error| format!("Failed to create UX harness: {error}"))?;

    harness
        .open_file("format_external.pl", source)
        .map_err(|error| format!("didOpen should succeed: {error}"))?;

    match harness.format_document("format_external.pl") {
        Ok(FormatResult::Error(error_value)) => {
            let message = error_value.get("message").and_then(|value| value.as_str()).unwrap_or("");
            assert!(
                !message.contains("panicked at") && !message.contains("SIGABRT"),
                "Error message looks like a Rust panic: {message}"
            );
            assert!(
                message.contains("perltidy"),
                "external-engine format failure must name perltidy: {message}"
            );
            assert!(
                message.contains(PERLTIDY_NOT_FOUND),
                "external-engine format failure must classify the missing formatter \
                 ({PERLTIDY_NOT_FOUND}): {message}"
            );
            assert!(
                message.contains(PERLTIDY_INSTALL_GUIDANCE),
                "external-engine format failure must carry the install guidance \
                 ({PERLTIDY_INSTALL_GUIDANCE}): {message}"
            );
        }
        Ok(FormatResult::Edits(_)) | Ok(FormatResult::Empty) => {
            return Err(
                "the external-perltidy engine formatted successfully although perltidy is not \
                 invocable — the missing-formatter error contract regressed into a silent \
                 success"
                    .to_string(),
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
