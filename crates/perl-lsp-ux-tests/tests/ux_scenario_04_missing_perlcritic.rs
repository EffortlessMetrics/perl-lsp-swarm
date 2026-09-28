//! Scenario 04 — Missing perlcritic.
//!
//! Simulates a user who has perl-lsp installed without perlcritic.
//!
//! Acceptance criteria:
//! - Server MUST start and accept `initialize`.
//! - `textDocument/didOpen` MUST succeed.
//! - Server MUST NOT crash when it tries to run perlcritic and fails.
//! - Server must remain responsive after the diagnostic pass.
//! - The missing perlcritic MUST be explicitly silent in the default session:
//!   no `window/showMessage` or `window/logMessage` may name perlcritic. The
//!   default critic engine is the in-process native critic
//!   (`ServerConfig::critic_engine` defaults to `CriticEngine::Native`), which
//!   never probes for a `perlcritic` binary, so a popup naming the tool would
//!   be a regression that forces a deliberate decision.
//!
//! # Environment note
//!
//! A directory-name PATH filter cannot remove perlcritic where it installs to
//! `/usr/bin` (CI) or Strawberry's `perl\bin` (Windows). The strict test below
//! therefore empties the child PATH, which makes perlcritic un-invocable on
//! every platform; the silence pin holds regardless of perlcritic availability
//! because the native engine never looks for it.

use anyhow::Context;
use perl_lsp_ux_tests::{
    LspEvent, ScenarioConfig, UxCiTier, UxComponent, UxEvidenceClass, UxHarness, binary_available,
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

            harness
                .hover("responsive.pl", 0, 3)
                .context("Server became unresponsive after perlcritic failure — UX regression")?;
            Ok(())
        },
    );
}

/// Bound on the event-driven wait for the didOpen-triggered diagnostics
/// publication that settles the debounced diagnostic pass. This only needs to
/// cover transport latency plus the debounce, never product work.
const DIAGNOSTICS_SETTLE: Duration = Duration::from_secs(15);

fn config_without_perlcritic_strict() -> ScenarioConfig {
    // The only cross-platform way to make perlcritic un-invocable: an empty
    // child PATH. A directory-name filter cannot remove perltidy/perlcritic on
    // CI, where they install to /usr/bin.
    ScenarioConfig::with_empty_path()
}

#[test]
fn scenario_04_missing_perlcritic_is_explicitly_silent_and_session_stays_responsive() {
    run_ux_scenario_with_evidence_class(
        WORKFLOW_ID,
        "ux_scenario_04_missing_perlcritic.rs",
        "scenario_04_missing_perlcritic_is_explicitly_silent_and_session_stays_responsive",
        UxCiTier::Pr,
        Some(UxComponent::Diagnostics),
        UxEvidenceClass::TransportCharacterization,
        |recorder| {
            if !binary_available() {
                return Err(missing_binary_skip().into());
            }

            let source = "sub foo {\n    my $unused = 1;\n    return 42;\n}\n";
            let harness = UxHarness::new(config_without_perlcritic_strict())
                .context("Failed to create UX harness without perlcritic")?;

            harness.open_file("critic_silent.pl", source).context("didOpen should succeed")?;

            // Hover round-trip: proves the server processed the didOpen and
            // that the session stays responsive without the tool.
            harness
                .hover("critic_silent.pl", 0, 3)
                .context("Server became unresponsive without perlcritic — UX regression")?;
            recorder.check("hover round-trip completed without perlcritic", true)?;

            // Settle the full diagnostic pass before asserting silence: the
            // runtime publishes diagnostics on a debounced trailing edge, so a
            // hover round-trip alone does not prove the pass ran. The
            // didOpen-triggered publication (even an empty payload) is the
            // explicit settlement signal; a regressed perlcritic popup emitted
            // during that pass is therefore already in the event buffer.
            harness
                .wait_for_diagnostics_event("critic_silent.pl", DIAGNOSTICS_SETTLE)
                .map_err(|end| {
                    anyhow::anyhow!(
                        "diagnostics publication for the opened file never arrived ({})",
                        end.describe()
                    )
                })?;

            let tool_messages: Vec<String> = harness
                .client
                .peek_events()
                .iter()
                .filter_map(|event| match event {
                    LspEvent::WindowMessage { message, .. }
                    | LspEvent::LogMessage { message, .. }
                        if message.to_lowercase().contains("perlcritic") =>
                    {
                        Some(message.clone())
                    }
                    _ => None,
                })
                .collect();

            recorder.check(
                "no window message names perlcritic in the default session",
                tool_messages.is_empty(),
            )?;
            anyhow::ensure!(
                tool_messages.is_empty(),
                "the default native-critic session must stay explicitly silent about \
                 perlcritic; a tool popup is a UX regression: {tool_messages:?}"
            );

            harness.assert_no_crash();
            Ok(())
        },
    );
}
