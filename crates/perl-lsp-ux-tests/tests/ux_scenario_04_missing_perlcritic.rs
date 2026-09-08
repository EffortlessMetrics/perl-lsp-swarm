// Test infrastructure — allow test-friendly patterns.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! Scenario 04 — Missing perlcritic.
//!
//! Simulates a user who has perl-lsp installed without perlcritic.
//!
//! Acceptance criteria:
//! - Server MUST start and accept `initialize`.
//! - `textDocument/didOpen` MUST succeed.
//! - Server MUST NOT crash when it tries to run perlcritic and fails.
//! - Server must remain responsive after the diagnostic pass.

use anyhow::Context;
use perl_lsp_ux_tests::{
    ScenarioConfig, UxCiTier, UxComponent, UxEvidenceClass, UxHarness, binary_available,
    missing_binary_skip, run_ux_scenario_with_evidence_class,
};
use std::time::Duration;

const WORKFLOW_ID: &str = "missing_perlcritic_graceful_degradation";

#[test]
fn scenario_04_workflow_id_matches_matrix() -> anyhow::Result<()> {
    let matrix: serde_json::Value =
        serde_json::from_str(include_str!("../fixtures/editor_ux_fixture_matrix.json"))?;
    let workflows = matrix
        .get("workflows")
        .and_then(serde_json::Value::as_array)
        .context("fixture matrix workflows missing")?;
    let entry = workflows
        .iter()
        .find(|entry| {
            entry.get("scenario_file").and_then(serde_json::Value::as_str)
                == Some("ux_scenario_04_missing_perlcritic.rs")
        })
        .context("scenario 04 missing from fixture matrix")?;
    anyhow::ensure!(
        entry.get("id").and_then(serde_json::Value::as_str) == Some(WORKFLOW_ID),
        "scenario 04 receipt identity differs from fixture matrix"
    );
    anyhow::ensure!(
        entry.pointer("/instrumentation/run_receipt").and_then(serde_json::Value::as_bool)
            == Some(true),
        "scenario 04 receipt instrumentation must be enabled"
    );
    Ok(())
}

fn config_without_perlcritic() -> ScenarioConfig {
    // Exclude only perlcritic from PATH, leaving perl and other tools available.
    // This accurately simulates "user has perl but not perlcritic installed".
    let sep = if cfg!(windows) { ';' } else { ':' };
    let dirs: Vec<String> = std::env::var("PATH")
        .unwrap_or_default()
        .split(sep)
        .filter(|entry| !entry.contains("perlcritic"))
        .map(String::from)
        .collect();
    ScenarioConfig { path_restriction: Some(dirs), ..Default::default() }
}

#[test]
fn scenario_04_diagnostics_without_perlcritic_no_crash() {
    run_ux_scenario_with_evidence_class(
        WORKFLOW_ID,
        "ux_scenario_04_missing_perlcritic.rs",
        "scenario_04_diagnostics_without_perlcritic_no_crash",
        UxCiTier::Pr,
        Some(UxComponent::Diagnostics),
        UxEvidenceClass::TransportCharacterization,
        |_recorder| {
            if !binary_available() {
                return Err(missing_binary_skip().into());
            }

            let source = "sub foo {\n    my $unused = 1;\n    return 42;\n}\n";
            let harness = UxHarness::new(config_without_perlcritic())
                .context("Failed to create UX harness")?;

            harness.open_file("critic.pl", source).context("didOpen should succeed")?;
            std::thread::sleep(Duration::from_secs(1));

            harness.assert_no_crash();
            Ok(())
        },
    );
}

#[test]
fn scenario_04_server_responsive_without_perlcritic() {
    run_ux_scenario_with_evidence_class(
        WORKFLOW_ID,
        "ux_scenario_04_missing_perlcritic.rs",
        "scenario_04_server_responsive_without_perlcritic",
        UxCiTier::Pr,
        Some(UxComponent::Diagnostics),
        UxEvidenceClass::TransportCharacterization,
        |_recorder| {
            if !binary_available() {
                return Err(missing_binary_skip().into());
            }

            let source = "my $x = 1;\n";
            let harness = UxHarness::new(config_without_perlcritic())
                .context("Failed to create UX harness")?;

            harness.open_file("responsive.pl", source).context("didOpen should succeed")?;
            std::thread::sleep(Duration::from_millis(500));

            let hover = harness.hover("responsive.pl", 0, 3);
            assert!(
                hover.is_ok(),
                "Server became unresponsive after perlcritic failure — UX regression: {:?}",
                hover
            );
            Ok(())
        },
    );
}
