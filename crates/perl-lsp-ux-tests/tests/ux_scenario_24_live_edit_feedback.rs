//! Scenario 24 — Live-edit UX feedback loop for diagnostics + definition.
//!
//! BDD contract:
//! - Given a file with an undefined variable, when it is opened, then diagnostics
//!   should surface a strict warning/error for that variable.
//! - Given the declaration was added, when go-to-definition runs on the use-site,
//!   then the transport stays responsive (no protocol error, no crash).
//!
//! Evidence classification (#13570): the post-edit navigation row below is
//! **transport-responsiveness characterization only**
//! (`UxEvidenceClass::TransportCharacterization`). A successful empty
//! `textDocument/definition` result passes this row and is NOT evidence that
//! the newly declared variable is navigable, that a result belongs to the
//! edited generation, or that an empty result is legitimate editor
//! intelligence. The exact definition-correctness/currentness replacement is
//! owned by #10675 (substrate: #10662); do not promote this row until that
//! proof lands.

use anyhow::{Context, Result};
use perl_lsp_ux_tests::binary_available;
use perl_lsp_ux_tests::{
    ScenarioConfig, ScenarioScore, UxCiTier, UxComponent, UxEvidenceClass, UxHarness,
    aggregate_editor_ux_scorecard, ensure_evidence_supports_projection,
    ensure_score_evidence_consistent, missing_binary_skip, run_ux_scenario_with_evidence_class,
};
use std::time::Duration;

const UNDECLARED_SOURCE: &str = r#"use strict;
use warnings;

print $name;
"#;

const DECLARED_SOURCE: &str = r#"use strict;
use warnings;

my $name = 'world';
print $name;
"#;

fn has_global_symbol_diagnostic(diags: &[serde_json::Value], symbol: &str) -> bool {
    diags.iter().any(|diag| {
        let message = diag.get("message").and_then(serde_json::Value::as_str).unwrap_or_default();
        let code = diag.get("code").and_then(serde_json::Value::as_str).unwrap_or_default();
        message.contains(symbol) || (code.contains("Global symbol") && message.contains(symbol))
    })
}

#[test]
fn given_undeclared_variable_when_opened_then_strict_diagnostic_is_published() -> Result<()> {
    if !binary_available() {
        eprintln!("SKIP scenario_24: perl-lsp binary not found");
        return Ok(());
    }
    let harness =
        UxHarness::new(ScenarioConfig::default().with_file("live_edit.pl", UNDECLARED_SOURCE))?;

    harness.open_file("live_edit.pl", UNDECLARED_SOURCE)?;

    let diagnostics = harness.wait_for_diagnostics("live_edit.pl", Duration::from_secs(6));
    assert!(
        has_global_symbol_diagnostic(&diagnostics, "$name"),
        "expected strict diagnostics for undeclared $name, got: {:?}",
        diagnostics
    );

    harness.assert_no_crash();
    Ok(())
}

/// Post-edit navigation transport check — characterization only (#13570).
///
/// Emits receipts stamped `UxEvidenceClass::TransportCharacterization` so UX
/// status projections cannot count a responsive `Ok` (including an empty
/// result) as definition correctness, recovery exactness, or
/// first-correct-answer evidence. The current assertion intentionally stays a
/// no-protocol-error transport regression; semantic replacement: #10675.
#[test]
fn given_live_edit_when_variable_is_declared_then_navigation_stays_transport_responsive() {
    run_ux_scenario_with_evidence_class(
        "live_edit_feedback",
        "ux_scenario_24_live_edit_feedback.rs",
        "given_live_edit_when_variable_is_declared_then_navigation_stays_transport_responsive",
        UxCiTier::Pr,
        Some(UxComponent::GotoDefinition),
        UxEvidenceClass::TransportCharacterization,
        |recorder| {
            if !binary_available() {
                return Err(missing_binary_skip().into());
            }
            let harness = UxHarness::new(
                ScenarioConfig::default().with_file("live_edit.pl", UNDECLARED_SOURCE),
            )?;

            harness.open_file("live_edit.pl", UNDECLARED_SOURCE)?;

            let before = harness.wait_for_diagnostics("live_edit.pl", Duration::from_secs(6));
            recorder.check(
                "precondition: undeclared symbol diagnostic before edit",
                has_global_symbol_diagnostic(&before, "$name"),
            )?;

            let diagnostics_seen_before_edit = harness.diagnostics_event_count("live_edit.pl");
            harness.change_file_full("live_edit.pl", DECLARED_SOURCE)?;

            let post_edit_diagnostics = harness
                .wait_for_diagnostics_after_count(
                    "live_edit.pl",
                    diagnostics_seen_before_edit,
                    Duration::from_secs(6),
                )
                .context("expected diagnostics after declaring $name")?;
            recorder.check(
                "declared $name diagnostic clears after edit",
                !has_global_symbol_diagnostic(&post_edit_diagnostics, "$name"),
            )?;

            // Transport-responsiveness-only assertion (#13570): `Ok` includes
            // the empty result. This is NOT a definition-correctness claim —
            // exact replacement is owned by #10675.
            let definitions = harness.definition("live_edit.pl", 4, 7);
            recorder.check(
                "transport: definition request returned without protocol error",
                definitions.is_ok(),
            )?;

            harness.assert_no_crash();
            recorder.check("no crash signatures in event log", true)?;
            Ok(())
        },
    );
}

/// Mechanical classification contract for the row above (#13570).
///
/// Proves that the Scenario 24 navigation row, represented as it is emitted
/// (`TransportCharacterization`), cannot feed definition-correctness
/// projections: the consistency guard rejects a carried semantic metric and
/// the scorecard aggregator excludes the row from the semantic percentage.
#[test]
fn scenario_24_row_cannot_feed_definition_correctness_projections() -> Result<()> {
    // The exact misread #13570 forbids: counting a responsive (possibly
    // empty) `Ok` as an exact definition hit.
    let misread = ScenarioScore {
        scenario_id: "scenario-24-post-edit-navigation".to_string(),
        definition_exact_hit: Some(true),
        evidence_class: UxEvidenceClass::TransportCharacterization,
        ..Default::default()
    };

    let rejection = ensure_score_evidence_consistent(&misread);
    assert!(
        matches!(&rejection, Err(message) if message.contains("definition_exact_hit")),
        "characterization row must not carry definition_exact_hit, got {rejection:?}"
    );
    assert!(
        ensure_evidence_supports_projection(
            UxEvidenceClass::TransportCharacterization,
            "definition_exact_hit"
        )
        .is_err(),
        "transport characterization must be rejected at the projection gate"
    );

    // Defense in depth: even if a misclassified row reaches aggregation, the
    // semantic definition percentage must not see it.
    let semantic_row = ScenarioScore {
        scenario_id: "scenario-10-exact-definition".to_string(),
        definition_exact_hit: Some(true),
        evidence_class: UxEvidenceClass::SemanticProof,
        ..Default::default()
    };
    let scorecard = aggregate_editor_ux_scorecard(&[semantic_row, misread]);
    assert_eq!(
        scorecard.definition_exact_hit_pct,
        Some(100.0),
        "characterization row must not count as definition correctness"
    );
    assert_eq!(scorecard.scenario_count, 2, "characterization rows still count as scenarios");

    Ok(())
}
