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

use anyhow::Context;
use perl_lsp_ux_tests::{
    ScenarioConfig, UxCiTier, UxComponent, UxEvidenceClass, UxHarness, binary_available,
    missing_binary_skip, run_ux_scenario_with_evidence_class,
};

const WORKFLOW_ID: &str = "missing_perl_graceful_degradation";
const SCENARIO_FILE: &str = "ux_scenario_03_missing_perl.rs";

fn config_without_perl() -> ScenarioConfig {
    ScenarioConfig::with_empty_path()
}

#[test]
fn scenario_03_server_starts_without_perl() {
    run_ux_scenario_with_evidence_class(
        WORKFLOW_ID,
        SCENARIO_FILE,
        "scenario_03_server_starts_without_perl",
        UxCiTier::Pr,
        Some(UxComponent::Infra),
        UxEvidenceClass::TransportCharacterization,
        |recorder| {
            if !binary_available() {
                return Err(missing_binary_skip().into());
            }

            let source = "use strict;\nmy $x = 1;\n";
            let harness = UxHarness::new(config_without_perl())
                .context("Failed to create UX harness without perl")?;

            let open_result = harness.open_file("no_perl.pl", source);
            recorder.check("didOpen accepted without perl", open_result.is_ok())?;
            open_result.context("didOpen should succeed without perl")?;

            harness.assert_no_crash();
            Ok(())
        },
    );
}

#[test]
fn scenario_03_degraded_mode_hover_does_not_crash() {
    run_ux_scenario_with_evidence_class(
        WORKFLOW_ID,
        SCENARIO_FILE,
        "scenario_03_degraded_mode_hover_does_not_crash",
        UxCiTier::Pr,
        Some(UxComponent::Hover),
        UxEvidenceClass::TransportCharacterization,
        |recorder| {
            if !binary_available() {
                return Err(missing_binary_skip().into());
            }

            let source = "my $x = 42;\n";
            let harness = UxHarness::new(config_without_perl())
                .context("Failed to create UX harness without perl")?;

            let open_result = harness.open_file("degraded.pl", source);
            recorder.check("didOpen accepted without perl", open_result.is_ok())?;
            open_result.context("didOpen should succeed")?;
            let hover_result = harness.hover("degraded.pl", 0, 3);
            recorder.check("hover transport completed without perl", hover_result.is_ok())?;
            hover_result.context("hover should not return a transport error in degraded mode")?;

            harness.assert_no_crash();
            Ok(())
        },
    );
}

#[test]
fn scenario_03_degraded_mode_completion_does_not_crash() {
    run_ux_scenario_with_evidence_class(
        WORKFLOW_ID,
        SCENARIO_FILE,
        "scenario_03_degraded_mode_completion_does_not_crash",
        UxCiTier::Pr,
        Some(UxComponent::Completion),
        UxEvidenceClass::TransportCharacterization,
        |recorder| {
            if !binary_available() {
                return Err(missing_binary_skip().into());
            }

            let source = "use str\n";
            let harness = UxHarness::new(config_without_perl())
                .context("Failed to create UX harness without perl")?;

            let open_result = harness.open_file("complete.pl", source);
            recorder.check("didOpen accepted without perl", open_result.is_ok())?;
            open_result.context("didOpen should succeed")?;
            let completion_result = harness.completion("complete.pl", 0, 7);
            recorder
                .check("completion transport completed without perl", completion_result.is_ok())?;
            completion_result
                .context("completion should not return a transport error in degraded mode")?;

            harness.assert_no_crash();
            Ok(())
        },
    );
}
