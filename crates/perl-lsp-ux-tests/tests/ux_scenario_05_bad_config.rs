//! Scenario 05 — Bad configuration.
//!
//! Simulates a user who has set invalid values in their configuration.
//!
//! Acceptance criteria:
//! - Server MUST NOT crash on startup with invalid config.
//! - Error messages MUST NOT contain raw Rust panic traces.
//! - Server MUST remain responsive after the config error.
//! - A malformed `.perl-lsp.toml` MUST surface exactly one bounded
//!   `window/showMessage` Warning naming the file, so the user can fix it.
//!
//! The startup case proves process survival through `didOpen`; it does not claim
//! that a configuration or tool-resolution transition has settled.

use anyhow::{Context, Result};
use perl_lsp_ux_tests::{
    DiagnosticsTracker, FormatResult, LspEvent, ScenarioConfig, UxCiTier, UxComponent,
    UxEvidenceClass, UxHarness, binary_available, missing_binary_skip,
    run_ux_scenario_with_evidence_class,
};
use serde_json::{Value, json};
use std::time::Duration;

const WORKFLOW_ID: &str = "bad_config_resilience";
const SCENARIO_FILE: &str = "ux_scenario_05_bad_config.rs";

/// Distinctive tail of the single-file config warning
/// (`load_and_apply_project_config`, `MessageType::Warning` arm).
const FIX_THE_FILE_GUIDANCE: &str = "Fix the error in .perl-lsp.toml";

/// A TOML table header left unclosed, so the project config parse fails.
const MALFORMED_PROJECT_CONFIG: &str = "[perl\nversion = \"5.20\"\n";

/// Bound on the event-driven wait for the config warning. `didOpen` is a
/// notification with no response to synchronize on, so the wait replaces a
/// sleep; the warning is emitted during the didOpen-time project config
/// refresh, never later.
const CONFIG_WARNING_WAIT: Duration = Duration::from_secs(15);

fn config_with_bad_tool_paths() -> ScenarioConfig {
    ScenarioConfig::default()
        .env("PERLTIDY_PATH", "/nonexistent/path/to/perltidy")
        .env("PERLCRITIC_PATH", "/nonexistent/path/to/perlcritic")
}

/// Response barrier after initialized: inspect server state without scanning or
/// creating configuration, so later discovery cannot have happened at startup.
fn ensure_rootless_initialized(harness: &UxHarness) -> Result<()> {
    let report = harness.client.request(
        "workspace/executeCommand",
        json!({"command": "perl.workspaceTrustReport", "arguments": []}),
        CONFIG_WARNING_WAIT,
    )?;
    anyhow::ensure!(report.get("error").is_none(), "rootless state report failed: {report}");
    let workspace =
        report.pointer("/result/workspace").context("server workspace report missing")?;
    anyhow::ensure!(
        workspace["workspace_folder_count"] == 0,
        "server retained workspace folders: {report}"
    );
    anyhow::ensure!(
        workspace.get("root_path").is_some_and(Value::is_null),
        "server retained a root: {report}"
    );
    anyhow::ensure!(
        workspace["open_document_count"] == 0,
        "initialization unexpectedly opened a document: {report}"
    );
    Ok(())
}

fn config_message_events(events: &[LspEvent]) -> Vec<&LspEvent> {
    events
        .iter()
        .filter(|event| {
            matches!(event, LspEvent::WindowMessage { message, .. }
                | LspEvent::LogMessage { message, .. }
                if message.contains(".perl-lsp.toml"))
        })
        .collect()
}

#[test]
fn scenario_05_wrong_severity_config_duplicate_still_counts_twice() -> Result<()> {
    let message = format!("{FIX_THE_FILE_GUIDANCE}: invalid table");
    let events = [
        LspEvent::WindowMessage { message_type: 2, message: message.clone() },
        LspEvent::WindowMessage { message_type: 1, message },
    ];
    anyhow::ensure!(
        config_message_events(&events).len() == 2,
        "a wrong-severity config duplicate evaded the count"
    );
    Ok(())
}

#[test]
fn scenario_05_wrong_channel_config_duplicate_still_counts_twice() -> Result<()> {
    let message = format!("{FIX_THE_FILE_GUIDANCE}: invalid table");
    let events = [
        LspEvent::WindowMessage { message_type: 2, message: message.clone() },
        LspEvent::LogMessage { message_type: 2, message },
    ];
    anyhow::ensure!(
        config_message_events(&events).len() == 2,
        "a wrong-channel config duplicate evaded the count"
    );
    Ok(())
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
fn scenario_05_malformed_project_config_emits_one_bounded_warning_naming_the_file() {
    run_ux_scenario_with_evidence_class(
        WORKFLOW_ID,
        SCENARIO_FILE,
        "scenario_05_malformed_project_config_emits_one_bounded_warning_naming_the_file",
        UxCiTier::Pr,
        Some(UxComponent::Infra),
        UxEvidenceClass::TransportCharacterization,
        |recorder| {
            if !binary_available() {
                return Err(missing_binary_skip().into());
            }

            let source = "my $x = 1;\n";
            let harness = UxHarness::new(ScenarioConfig::default().without_workspace_root())
                .context("Failed to create UX harness with malformed .perl-lsp.toml")?;

            ensure_rootless_initialized(&harness)?;
            anyhow::ensure!(
                config_message_events(&harness.client.peek_events()).is_empty(),
                "configuration warning preceded didOpen-time discovery"
            );
            harness.workspace.write(".perl-lsp.toml", MALFORMED_PROJECT_CONFIG)?;

            // Only didOpen can discover this newly created config: initialization
            // completed with rootUri:null, no folders, and no open documents.
            harness
                .open_file("config_warning.pl", source)
                .context("didOpen should succeed even with a malformed project config")?;

            // Event-driven settle for the asynchronous didOpen notification.
            let observed = harness
                .client
                .wait_for_events(CONFIG_WARNING_WAIT, |events| {
                    events
                        .iter()
                        .any(|event| {
                            matches!(event, LspEvent::WindowMessage { message, .. }
                                if message.contains(".perl-lsp.toml"))
                        })
                        .then_some(())
                })
                .map_err(|end| {
                    anyhow::anyhow!("malformed .perl-lsp.toml produced no config warning: {end:?}")
                });
            recorder.check("config warning observed after didOpen", observed.is_ok())?;
            observed?;

            // A second unique document in the same directory discovers the same
            // selected malformed config. Its diagnostics and request response
            // establish processing before counting all warning channels.
            harness.open_file("config_warning_second.pl", source)?;
            harness
                .wait_for_diagnostics_event("config_warning_second.pl", CONFIG_WARNING_WAIT)
                .map_err(|end| {
                    anyhow::anyhow!("second didOpen did not publish diagnostics: {end:?}")
                })?;
            harness
                .hover("config_warning_second.pl", 0, 3)
                .context("server became unresponsive after the malformed config warning")?;
            recorder.check(
                "second didOpen diagnostics and hover completed before warning count",
                true,
            )?;

            let events = harness.client.peek_events();
            let warnings = config_message_events(&events);

            recorder
                .check("exactly one config warning naming .perl-lsp.toml after two opens", warnings.len() == 1)
                .with_context(|| format!(
                    "expected exactly one .perl-lsp.toml message after two opens, got {}: {warnings:?}",
                    warnings.len()
                ))?;
            let Some(LspEvent::WindowMessage { message_type: 2, message }) =
                warnings.first().copied()
            else {
                anyhow::bail!("the sole .perl-lsp.toml message must be a Warning popup");
            };
            anyhow::ensure!(
                message.contains(FIX_THE_FILE_GUIDANCE),
                "config warning must tell the user to fix .perl-lsp.toml \
                 ({FIX_THE_FILE_GUIDANCE}): {message}"
            );
            recorder.check(
                "config warning carries the fix-the-file guidance",
                message.contains(FIX_THE_FILE_GUIDANCE),
            )?;
            ensure_no_panic_trace(message)?;

            harness.assert_no_crash();
            recorder.check("no crash signatures in event log", true)?;
            Ok(())
        },
    );
}

#[test]
fn scenario_05_rootless_did_open_discovers_new_project_version() {
    run_ux_scenario_with_evidence_class(
        WORKFLOW_ID,
        SCENARIO_FILE,
        "scenario_05_rootless_did_open_discovers_new_project_version",
        UxCiTier::Pr,
        Some(UxComponent::Diagnostics),
        UxEvidenceClass::SemanticProof,
        |recorder| {
            if !binary_available() {
                return Err(missing_binary_skip().into());
            }
            let harness = UxHarness::new(ScenarioConfig::default().without_workspace_root())?;
            ensure_rootless_initialized(&harness)?;
            let source = "use builtin 'inf'; builtin::inf();\n";
            harness.open_file("version_control.pl", source)?;
            let before = harness
                .wait_for_diagnostics("version_control.pl", CONFIG_WARNING_WAIT)
                .map_err(|end| anyhow::anyhow!("no-config control did not publish: {end:?}"))?;
            anyhow::ensure!(
                !before.iter().any(|diagnostic| diagnostic["code"] == "PL900"),
                "no-config control already carried PL900: {before:?}"
            );
            recorder.check("no-config rootless control has no project-version diagnostic", true)?;
            harness.workspace.write(".perl-lsp.toml", "[perl]\nversion = \"5.20\"\n")?;
            harness.open_file("version_configured.pl", source)?;
            let after = DiagnosticsTracker::wait_for_uri_matching(
                &harness.client,
                &harness.workspace.uri("version_configured.pl"),
                CONFIG_WARNING_WAIT,
                |diagnostics| diagnostics.iter().any(|diagnostic| diagnostic["code"] == "PL900"),
            )
            .map_err(|end| anyhow::anyhow!("configured didOpen did not publish PL900: {end:?}"))?;
            let diagnostic = after
                .iter()
                .find(|diagnostic| diagnostic["code"] == "PL900")
                .context("didOpen did not discover project Perl5.20 for builtin inf")?;
            anyhow::ensure!(
                diagnostic["message"].as_str().is_some_and(|message| message.contains("5.20")),
                "PL900 must name discovered project version5.20: {diagnostic}"
            );
            recorder.check(
                "post-initialization config discovered by didOpen emits PL900 for5.20",
                true,
            )?;
            harness.assert_no_crash();
            Ok(())
        },
    );
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
