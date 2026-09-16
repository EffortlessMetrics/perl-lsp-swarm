//! Scenario 05 — Bad configuration.
//!
//! Simulates a user who has set invalid values in their configuration.
//!
//! Acceptance criteria:
//! - Server MUST NOT crash on startup with invalid config.
//! - Error messages MUST NOT contain raw Rust panic traces.
//! - Server MUST remain responsive after the config error.
//!
//! The startup case proves process survival through `didOpen`; it does not claim
//! that a configuration or tool-resolution transition has settled.

use anyhow::{Context, Result};
use perl_lsp_ux_tests::{
    FormatResult, ScenarioConfig, UxCiTier, UxComponent, UxEvidenceClass, UxHarness,
    binary_available, missing_binary_skip, run_ux_scenario_with_evidence_class,
};
use serde_json::Value;

const WORKFLOW_ID: &str = "bad_config_resilience";
const SCENARIO_FILE: &str = "ux_scenario_05_bad_config.rs";

fn config_with_bad_tool_paths() -> ScenarioConfig {
    ScenarioConfig::default()
        .env("PERLTIDY_PATH", "/nonexistent/path/to/perltidy")
        .env("PERLCRITIC_PATH", "/nonexistent/path/to/perlcritic")
}

fn ensure_no_panic_trace(message: &str) -> Result<()> {
    anyhow::ensure!(
        !message.contains("panicked at") && !message.contains("SIGABRT"),
        "error message contains a Rust panic signature: {message}"
    );
    Ok(())
}

fn protocol_error_message(error: &Value) -> Result<&str> {
    let object = error.as_object().context("formatting protocol error must be an object")?;
    object
        .get("code")
        .and_then(Value::as_i64)
        .context("formatting protocol error must contain an integer code")?;
    object
        .get("message")
        .and_then(Value::as_str)
        .context("formatting protocol error must contain a string message")
}

#[test]
fn scenario_05_bad_tool_path_does_not_crash_server() {
    run_ux_scenario_with_evidence_class(
        WORKFLOW_ID,
        SCENARIO_FILE,
        "scenario_05_bad_tool_path_does_not_crash_server",
        UxCiTier::Pr,
        Some(UxComponent::Infra),
        UxEvidenceClass::TransportCharacterization,
        |recorder| {
            if !binary_available() {
                return Err(missing_binary_skip().into());
            }

            let source = "my $x = 1;\n";
            let harness = UxHarness::new(config_with_bad_tool_paths())
                .context("Failed to create UX harness with bad config")?;

            harness
                .open_file("config_test.pl", source)
                .context("didOpen should succeed with bad tool paths")?;
            recorder.check("didOpen accepted with bad tool paths", true)?;

            harness.assert_no_crash();
            recorder.check("no crash signatures in event log", true)?;
            Ok(())
        },
    );
}

#[test]
fn scenario_05_server_responsive_with_bad_config() {
    run_ux_scenario_with_evidence_class(
        WORKFLOW_ID,
        SCENARIO_FILE,
        "scenario_05_server_responsive_with_bad_config",
        UxCiTier::Pr,
        Some(UxComponent::Hover),
        UxEvidenceClass::TransportCharacterization,
        |recorder| {
            if !binary_available() {
                return Err(missing_binary_skip().into());
            }

            let source = "my $x = 1;\nmy $y = $x + 1;\n";
            let harness = UxHarness::new(config_with_bad_tool_paths())
                .context("Failed to create UX harness with bad config")?;

            harness
                .open_file("config_responsive.pl", source)
                .context("didOpen should succeed")?;
            harness
                .hover("config_responsive.pl", 0, 3)
                .context("server became unresponsive to hover with bad tool paths")?;
            recorder.check("independent hover request completed with bad tool paths", true)?;

            harness.assert_no_crash();
            recorder.check("no crash signatures in event log", true)?;
            Ok(())
        },
    );
}

#[test]
fn scenario_05_format_with_bad_perltidy_path_returns_graceful_error() {
    run_ux_scenario_with_evidence_class(
        WORKFLOW_ID,
        SCENARIO_FILE,
        "scenario_05_format_with_bad_perltidy_path_returns_graceful_error",
        UxCiTier::Pr,
        Some(UxComponent::Infra),
        UxEvidenceClass::TransportCharacterization,
        |recorder| {
            if !binary_available() {
                return Err(missing_binary_skip().into());
            }

            let source = "sub foo{my$x=1;}\n";
            let harness = UxHarness::new(config_with_bad_tool_paths())
                .context("Failed to create UX harness with bad config")?;

            harness
                .open_file("format_bad.pl", source)
                .context("didOpen should succeed")?;

            match harness
                .format_document("format_bad.pl")
                .context("formatting failed at the UX harness boundary")?
            {
                FormatResult::Edits(_) => {
                    recorder.check("formatting returned a bounded edits result", true)?;
                }
                FormatResult::Empty => {
                    recorder.check("formatting returned an explicit empty result", true)?;
                }
                FormatResult::Error(error) => {
                    let message = protocol_error_message(&error)?;
                    ensure_no_panic_trace(message)?;
                    recorder.check("formatting returned a bounded protocol error", true)?;
                }
            }

            harness.assert_no_crash();
            recorder.check("no crash signatures in event log", true)?;
            Ok(())
        },
    );
}
