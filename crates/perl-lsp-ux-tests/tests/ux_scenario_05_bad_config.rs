//! Scenario 05 — Bad configuration.
//!
//! Simulates a user who has set invalid values in their configuration.
//!
//! Acceptance criteria:
//! - Server MUST NOT crash on startup with invalid config.
//! - Error messages MUST NOT contain raw Rust panic traces.
//! - Server MUST remain responsive after the config error.
//! - A malformed `.perl-lsp.toml` MUST surface a `window/showMessage` that
//!   names the remedy.
//!
//! The startup case proves process survival through `didOpen`; it does not claim
//! that a configuration or tool-resolution transition has settled.
//!
//! The bad-tool-path cases below are transport characterization: they prove the
//! server survives, and they pin no user-visible message. The user-visible half
//! of "bad configuration" — that a user who mistypes their config file is
//! actually told what to do about it — is covered by
//! `scenario_05_malformed_project_config_warns_with_a_remedy`, which is what a
//! user hits on a first run with a hand-written `.perl-lsp.toml` (#16549).
//!
//! Not covered here, and deliberately not claimed: warning *suppression*. The
//! broken-config popup is deduped per session (#16548), but a workspace-rooted
//! session reads `.perl-lsp.toml` once at initialize, so the repeat that dedup
//! suppresses is only reachable from a rootless session — and `ScenarioConfig`
//! always sends a `rootUri`, so this harness cannot drive that path at all.

use anyhow::{Context, Result};
use perl_lsp_ux_tests::{
    FormatResult, ScenarioConfig, UxCiTier, UxComponent, UxEvidenceClass, UxHarness,
    binary_available, missing_binary_skip, run_ux_scenario_with_evidence_class,
};
use serde_json::Value;
use std::time::Duration;

const WORKFLOW_ID: &str = "bad_config_resilience";
const SCENARIO_FILE: &str = "ux_scenario_05_bad_config.rs";

/// A `.perl-lsp.toml` the TOML parser cannot read: the `[perl` table header is
/// never closed, so parsing fails before any field is considered.
///
/// Deliberately a *syntax* error rather than an unknown key. An unknown key
/// deserializes into `#[serde(default)] ProjectConfig` and is silently ignored,
/// which would make this case a test of nothing.
const MALFORMED_PROJECT_CONFIG: &str = "[perl\ninclude_paths = [\"lib\"\n";

/// The remedy half of the project-config warning, shared verbatim by the
/// single-file and folder emit paths (`runtime/lifecycle/workspace.rs`).
///
/// Pinning the remedy rather than the whole sentence is deliberate. A refactor
/// that kept a vague "invalid configuration" and dropped "Fix the error in
/// .perl-lsp.toml and reload the window" leaves the user with no next action,
/// and that is the regression this needs to catch.
const PROJECT_CONFIG_REMEDY: &str = "Fix the error in .perl-lsp.toml";

fn config_with_bad_tool_paths() -> ScenarioConfig {
    ScenarioConfig::default()
        .env("PERLTIDY_PATH", "/nonexistent/path/to/perltidy")
        .env("PERLCRITIC_PATH", "/nonexistent/path/to/perlcritic")
}

fn config_with_malformed_project_config() -> ScenarioConfig {
    ScenarioConfig::default().with_file(".perl-lsp.toml", MALFORMED_PROJECT_CONFIG)
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

            harness.open_file("config_responsive.pl", source).context("didOpen should succeed")?;
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

            harness.open_file("format_bad.pl", source).context("didOpen should succeed")?;

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

#[test]
fn scenario_05_malformed_project_config_warns_with_a_remedy() {
    run_ux_scenario_with_evidence_class(
        WORKFLOW_ID,
        SCENARIO_FILE,
        "scenario_05_malformed_project_config_warns_with_a_remedy",
        UxCiTier::Pr,
        Some(UxComponent::Infra),
        UxEvidenceClass::TransportCharacterization,
        |recorder| {
            if !binary_available() {
                return Err(missing_binary_skip().into());
            }

            let harness = UxHarness::new(config_with_malformed_project_config())
                .context("Failed to create UX harness with a malformed .perl-lsp.toml")?;

            harness
                .open_file("malformed_config.pl", "my $x = 1;\n")
                .context("didOpen should succeed with a malformed .perl-lsp.toml")?;

            // The three transport cases in this file all pass against a server
            // that says nothing at all, because they only assert that nothing
            // crashed. A user who mistypes their config needs to be told which
            // file to fix and that a reload applies it (#16549).
            let warned =
                harness.client.wait_for_message(PROJECT_CONFIG_REMEDY, Duration::from_secs(15));
            recorder
                .check("a malformed .perl-lsp.toml surfaces a warning naming the remedy", warned)?;

            harness.assert_no_crash();
            Ok(())
        },
    )
}

#[cfg(test)]
mod contract_tests {
    use super::{ensure_no_panic_trace, protocol_error_message};
    use anyhow::Result;
    use serde_json::json;

    #[test]
    fn protocol_error_requires_typed_code_and_message() -> Result<()> {
        let valid = json!({"code": -32603, "message": "bounded formatting failure"});
        assert_eq!(protocol_error_message(&valid)?, "bounded formatting failure");

        for malformed in [
            json!(null),
            json!([]),
            json!({}),
            json!({"message": "missing code"}),
            json!({"code": "-32603", "message": "string code"}),
            json!({"code": -32603}),
            json!({"code": -32603, "message": 7}),
        ] {
            assert!(
                protocol_error_message(&malformed).is_err(),
                "malformed protocol error must be rejected: {malformed:?}"
            );
        }
        Ok(())
    }

    #[test]
    fn panic_signatures_are_rejected() {
        assert!(ensure_no_panic_trace("tool unavailable").is_ok());
        assert!(ensure_no_panic_trace("thread panicked at src/main.rs:1").is_err());
        assert!(ensure_no_panic_trace("child terminated with SIGABRT").is_err());
    }
}
