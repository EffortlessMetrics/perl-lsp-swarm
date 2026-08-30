// Test infrastructure — allow test-friendly patterns.
#![allow(clippy::unwrap_used, clippy::expect_used)]

//! Scenario 14 companion: block-leading `BEGIN` mutation of effective `@INC`.
//!
//! The exact-symbol fixture drives diagnostics, goto-definition, and hover.
//! The prefix fixture separately drives completion, preserving the consumer
//! shape contract documented for the main Scenario 14 grid.

use perl_lsp_ux_tests::binary_available;
use perl_lsp_ux_tests::{ScenarioConfig, UxHarness};
use std::time::Duration;

const PL701: &str = "PL701";

const EXACT_SOURCE: &str = "\
use strict;\n\
use warnings;\n\
BEGIN {\n\
    use lib qw(lib);\n\
}\n\
use BeginIncModule;\n\
\n\
my $value = BeginIncModule::value();\n\
";

const COMPLETION_SOURCE: &str = "\
use strict;\n\
use warnings;\n\
BEGIN {\n\
    use lib qw(lib);\n\
}\n\
use Beg\n\
";

const MODULE_SOURCE: &str = "\
package BeginIncModule;\n\
\n\
use strict;\n\
use warnings;\n\
\n\
sub value {\n\
    return 42;\n\
}\n\
\n\
1;\n\
";

fn has_pl701(diags: &[serde_json::Value]) -> bool {
    diags.iter().any(|d| {
        d.get("code").and_then(|c| c.as_str()).map(|c| c == PL701).unwrap_or(false)
            || d.get("code").and_then(|c| c.as_u64()).map(|c| c == 701).unwrap_or(false)
    })
}

fn completion_has_module(items: &[serde_json::Value], module_name: &str) -> bool {
    items.iter().any(|item| {
        item.get("label").and_then(|label| label.as_str()) == Some(module_name)
            || item.get("insertText").and_then(|text| text.as_str()) == Some(module_name)
    })
}

#[test]
fn scenario_14_begin_scoped_use_lib_consumer_consistency() -> Result<(), String> {
    if !binary_available() {
        eprintln!(
            "SKIP scenario_14_begin_scoped_use_lib_consumer_consistency: \
             perl-lsp binary not found"
        );
        return Ok(());
    }

    let harness = UxHarness::new(
        ScenarioConfig { timeout: Duration::from_secs(20), ..Default::default() }
            .with_file("fixture.pl", EXACT_SOURCE)
            .with_file("lib/BeginIncModule.pm", MODULE_SOURCE),
    )
    .expect("Failed to create UX harness");

    harness.open_file("fixture.pl", EXACT_SOURCE).expect("didOpen should succeed");
    let diags = harness.wait_for_diagnostics("fixture.pl", Duration::from_secs(5));
    assert!(
        !has_pl701(&diags),
        "Expected no PL701 when a block-leading BEGIN use lib exposes the module.\n\
         diagnostics: {diags:?}"
    );

    // `use BeginIncModule` is at zero-based line 5, column 4.
    let definitions = harness.definition("fixture.pl", 5, 4).expect("definition must not error");
    assert!(
        !definitions.is_empty(),
        "Expected goto-definition to resolve through BEGIN-scoped use lib.\n\
         diagnostics: {diags:?}"
    );

    let hover = harness.hover("fixture.pl", 5, 4).expect("hover must not error");
    if let Some(hover) = hover {
        assert!(
            hover.get("contents").is_some(),
            "Hover result must have a contents field: {hover:?}"
        );
    }

    harness
        .change_file_full("fixture.pl", COMPLETION_SOURCE)
        .expect("didChange to completion fixture should succeed");
    let _ = harness.wait_for_diagnostics("fixture.pl", Duration::from_secs(5));

    // `use Beg` is at zero-based line 5, cursor column 7.
    let completions = harness.completion("fixture.pl", 5, 7).expect("completion must not error");
    assert!(
        completion_has_module(&completions, "BeginIncModule"),
        "Expected completion to include BeginIncModule through BEGIN-scoped use lib; \
         completions: {completions:?}"
    );

    harness.assert_no_crash();
    Ok(())
}
