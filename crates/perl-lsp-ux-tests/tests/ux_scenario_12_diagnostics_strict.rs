//! Scenario 12 — `textDocument/publishDiagnostics` feature grid coverage.
//!
//! Verifies that the server emits diagnostics notifications when Perl code has
//! known issues. This exercises the `textDocument/publishDiagnostics`
//! capability advertised in `features.toml`.
//!
//! Acceptance criteria:
//! - After `didOpen`, the server MUST eventually send a
//!   `textDocument/publishDiagnostics` notification for the exact opened URI.
//! - The notification MUST NOT crash the server.
//! - If diagnostics are returned they MUST be well-formed objects with valid
//!   `range` and `message` fields.
//! - A present `severity` MUST be an integer in the LSP range 1 through 4.
//! - A clean file MAY publish an explicit empty diagnostics array; silence is
//!   not an empty current result.

use anyhow::{Context, Result};
use perl_lsp_ux_tests::{
    ScenarioConfig, UxCiTier, UxComponent, UxHarness, binary_available, missing_binary_skip,
    run_ux_scenario,
};
use serde_json::Value;
use std::time::Duration;

const WORKFLOW_ID: &str = "strict_diagnostics";
const SCENARIO_FILE: &str = "ux_scenario_12_diagnostics_strict.rs";
const DIAGNOSTICS_TIMEOUT: Duration = Duration::from_secs(5);

/// Source that is syntactically valid Perl and may publish an empty diagnostics array.
const CLEAN_SOURCE: &str = "\
use strict;\n\
use warnings;\n\
\n\
my $x = 42;\n\
print \"$x\\n\";\n\
";

/// Source with a declared-but-unused-under-strict variable. Some diagnostics
/// providers flag this; others do not. This scenario verifies publication and
/// payload shape, not a provider-specific diagnostic count.
const STRICT_SOURCE: &str = "\
use strict;\n\
use warnings;\n\
\n\
my $unused_var = 99;\n\
print \"done\\n\";\n\
";

fn validate_position(value: Option<&Value>, field: &str, diagnostic: &Value) -> Result<()> {
    let position = value
        .and_then(Value::as_object)
        .with_context(|| format!("diagnostic {field} must be an object: {diagnostic:?}"))?;
    for coordinate in ["line", "character"] {
        position.get(coordinate).and_then(Value::as_u64).with_context(|| {
            format!(
                "diagnostic {field}.{coordinate} must be a non-negative integer: \
                 {diagnostic:?}"
            )
        })?;
    }
    Ok(())
}

fn validate_diagnostic(diagnostic: &Value) -> Result<()> {
    let object = diagnostic
        .as_object()
        .with_context(|| format!("diagnostic must be an object: {diagnostic:?}"))?;
    let range = object
        .get("range")
        .and_then(Value::as_object)
        .with_context(|| format!("diagnostic range must be an object: {diagnostic:?}"))?;

    validate_position(range.get("start"), "range.start", diagnostic)?;
    validate_position(range.get("end"), "range.end", diagnostic)?;
    object
        .get("message")
        .and_then(Value::as_str)
        .with_context(|| format!("diagnostic message must be a string: {diagnostic:?}"))?;

    if let Some(raw_severity) = object.get("severity") {
        let severity = raw_severity.as_u64().with_context(|| {
            format!("diagnostic severity must be an integer from 1 through 4: {diagnostic:?}")
        })?;
        anyhow::ensure!(
            (1..=4).contains(&severity),
            "diagnostic severity must be in 1 through 4, got {severity}: {diagnostic:?}"
        );
    }
    Ok(())
}

fn open_and_wait_for_diagnostics(
    harness: &UxHarness,
    relative_path: &str,
    source: &str,
) -> Result<Vec<Value>> {
    let already_seen = harness.diagnostics_event_count(relative_path);
    harness
        .open_file(relative_path, source)
        .with_context(|| format!("didOpen should succeed for {relative_path}"))?;
    harness
        .wait_for_diagnostics_after_count(relative_path, already_seen, DIAGNOSTICS_TIMEOUT)
        .with_context(|| {
            format!(
                "no post-open publishDiagnostics notification arrived for {relative_path} \
                 within {DIAGNOSTICS_TIMEOUT:?}"
            )
        })
}

fn validate_diagnostics(diagnostics: &[Value]) -> Result<()> {
    for diagnostic in diagnostics {
        validate_diagnostic(diagnostic)?;
    }
    Ok(())
}

#[test]
fn scenario_12_strict_file_publishes_well_formed_diagnostics() {
    run_ux_scenario(
        WORKFLOW_ID,
        SCENARIO_FILE,
        "scenario_12_strict_file_publishes_well_formed_diagnostics",
        UxCiTier::Pr,
        Some(UxComponent::Diagnostics),
        |recorder| {
            if !binary_available() {
                return Err(missing_binary_skip().into());
            }

            let harness = UxHarness::new(
                ScenarioConfig { timeout: Duration::from_secs(15), ..Default::default() }
                    .with_file("strict_test.pl", STRICT_SOURCE),
            )
            .context("Failed to create UX harness")?;

            recorder.mark_request_start("publishDiagnostics");
            let diagnostics =
                open_and_wait_for_diagnostics(&harness, "strict_test.pl", STRICT_SOURCE)?;
            recorder
                .check("post-open diagnostics publication observed for strict_test.pl", true)?;
            validate_diagnostics(&diagnostics)?;
            recorder.check("every returned diagnostic has valid required shape", true)?;
            recorder.mark_first_useful_result("publishDiagnostics");

            harness.assert_no_crash();
            recorder.check("no crash signatures in event log", true)?;
            Ok(())
        },
    );
}

#[test]
fn scenario_12_clean_file_publishes_current_diagnostics() {
    run_ux_scenario(
        WORKFLOW_ID,
        SCENARIO_FILE,
        "scenario_12_clean_file_publishes_current_diagnostics",
        UxCiTier::Pr,
        Some(UxComponent::Diagnostics),
        |recorder| {
            if !binary_available() {
                return Err(missing_binary_skip().into());
            }

            let harness = UxHarness::new(
                ScenarioConfig { timeout: Duration::from_secs(15), ..Default::default() }
                    .with_file("clean.pl", CLEAN_SOURCE),
            )
            .context("Failed to create UX harness")?;

            recorder.mark_request_start("publishDiagnostics");
            let diagnostics = open_and_wait_for_diagnostics(&harness, "clean.pl", CLEAN_SOURCE)?;
            recorder.check("post-open diagnostics publication observed for clean.pl", true)?;
            validate_diagnostics(&diagnostics)?;
            recorder
                .check("clean-file diagnostics publication is explicit and well formed", true)?;
            recorder.mark_first_useful_result("publishDiagnostics");

            harness.assert_no_crash();
            recorder.check("no crash signatures in event log", true)?;
            Ok(())
        },
    );
}
