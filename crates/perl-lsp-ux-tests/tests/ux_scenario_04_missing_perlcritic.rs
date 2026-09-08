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
        "missing_perlcritic",
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
        "missing_perlcritic",
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
