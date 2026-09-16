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
    ScenarioConfig { path_restriction: Some(Vec::new()), ..Default::default() }
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

            harness
                .open_file("no_perl.pl", source)
                .context("didOpen should succeed without perl")?;
            recorder.check("didOpen accepted without perl", true)?;

            harness.assert_no_crash();
            recorder.check("no crash signatures in event log", true)?;
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

            harness.open_file("degraded.pl", source).context("didOpen should succeed")?;
            harness
                .hover("degraded.pl", 0, 3)
                .context("hover should not return a transport error in degraded mode")?;
            recorder.check("hover transport completed without perl", true)?;

            harness.assert_no_crash();
            recorder.check("no crash signatures in event log", true)?;
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

            harness.open_file("complete.pl", source).context("didOpen should succeed")?;
            harness
                .completion("complete.pl", 0, 7)
                .context("completion should not return a transport error in degraded mode")?;
            recorder.check("completion transport completed without perl", true)?;

            harness.assert_no_crash();
            recorder.check("no crash signatures in event log", true)?;
            Ok(())
        },
    );
}
