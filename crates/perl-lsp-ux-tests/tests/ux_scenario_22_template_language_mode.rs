//! Scenario 22 — Template language mode should not poison Perl UX flows.
//!
//! Why this is high-impact:
//! - Mojolicious/TT template files are frequently opened in HTML mode.
//! - Regressions here can flood users with parse noise or break navigation in
//!   neighboring Perl files during the first few minutes of editor usage.
//!
//! Contract:
//! - Opening a template-like file (`*.html.ep`) in non-Perl language mode MUST
//!   not crash.
//! - Template diagnostics in this mode SHOULD stay empty (parse intentionally skipped).
//! - Core navigation in normal Perl files in the same workspace MUST still work.

use anyhow::Result;
use perl_lsp_ux_tests::{ScenarioConfig, UxHarness, binary_available};
use std::time::Duration;

const APP_SOURCE: &str = r#"use strict;
use warnings;

sub index {
    return helper();
}

sub helper {
    return 'ok';
}

index();
"#;

const TEMPLATE_SOURCE: &str = r#"% my $user = shift;
<h1><%= $user %></h1>
% if ($user) {
  <p>Welcome!</p>
% }
"#;

fn source_reconciliation_scenario_22_template_in_html_mode_preserves_neighboring_perl_navigation()
-> Result<()> {
    let harness = UxHarness::new(
        ScenarioConfig::default()
            .with_file("app.pl", APP_SOURCE)
            .with_file("templates/index.html.ep", TEMPLATE_SOURCE),
    )?;

    harness.open_file_with_language_id("templates/index.html.ep", TEMPLATE_SOURCE, "html")?;
    harness.open_file("app.pl", APP_SOURCE)?;

    std::thread::sleep(Duration::from_millis(500));

    // An html-mode template intentionally publishes no Perl diagnostics, so a
    // live deadline is the expected outcome and stays accepted as absence; only
    // a closed or failed stream fails the scenario.
    let template_diags = perl_lsp_ux_tests::optional_wait_with_subject(
        &format!("diagnostics for {}", "templates/index.html.ep"),
        harness.wait_for_diagnostics("templates/index.html.ep", Duration::from_millis(1200)),
    )?
    .unwrap_or_default();
    assert!(
        template_diags.is_empty(),
        "template opened as html should skip Perl parse diagnostics, got: {template_diags:?}"
    );

    // `helper` call in `return helper();` (line 4, character 11).
    let defs = harness.definition("app.pl", 4, 11)?;
    assert!(
        !defs.is_empty(),
        "expected goto-definition in neighboring Perl file to keep working after template open"
    );

    harness.assert_no_crash();
    Ok(())
}

#[test]
fn scenario_22_template_in_html_mode_preserves_neighboring_perl_navigation() {
    use perl_lsp_ux_tests::{
        UxCiTier, UxComponent, UxEvidenceClass, missing_binary_skip,
        run_ux_scenario_with_evidence_class,
    };
    run_ux_scenario_with_evidence_class(
        "template_language_mode_resilience",
        "ux_scenario_22_template_language_mode.rs",
        "scenario_22_template_in_html_mode_preserves_neighboring_perl_navigation",
        UxCiTier::Pr,
        Some(UxComponent::GotoDefinition),
        UxEvidenceClass::SemanticProof,
        |recorder| {
            if !binary_available() {
                return Err(missing_binary_skip().into());
            }
            // Preserve source assertions; their helper does not expose an exact timing boundary.
            source_reconciliation_scenario_22_template_in_html_mode_preserves_neighboring_perl_navigation().map_err(|error| anyhow::anyhow!("{error}"))?;
            recorder.check("current source assertions for scenario_22_template_in_html_mode_preserves_neighboring_perl_navigation completed successfully", true)?;
            // Aggregate completion records no request/first-useful timing boundary.
            Ok(())
        },
    );
}
