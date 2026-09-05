// Test infrastructure — allow test-friendly patterns.
#![allow(clippy::unwrap_used, clippy::expect_used)]

//! Scenario 14 companion: block-leading `BEGIN` mutation of effective `@INC`.
//!
//! The exact-symbol fixture drives diagnostics, goto-definition, and hover.
//! The prefix fixture separately drives completion, preserving the consumer
//! shape contract documented for the main Scenario 14 grid.
//!
//! The module deliberately lives under `vendor/`, not `lib/`. The server adds
//! conventional workspace directories such as `lib/` to the effective `@INC`
//! on its own, so a `lib/` fixture resolves whether or not the pragma is seen
//! and every assertion below would pass against a scanner that ignored the
//! `BEGIN` block entirely. `vendor/` is reachable only through the pragma, so
//! the control fixture genuinely raises PL701 and the positive assertions
//! genuinely fail when the pragma stops reaching the effective `@INC`.
//!
//! What this scenario does *not* prove: it does not falsify the bounded
//! textual scanner's `BEGIN` peel. The HIR path recognizes the block form
//! independently, so disabling the peel in
//! `perl_module::resolution::use_lib::statements` leaves every assertion here
//! green (verified by mutation). The peel is falsified at its own layer by
//! `perl-module`'s `use_lib_begin_block_tests`; the ordering seam between the
//! two sources is falsified by `inc_context`'s
//! `effective_inc_context_merges_hir_roots_with_source_roots`. This scenario
//! covers the end-to-end consumer contract: whichever source resolves it, the
//! block-leading pragma must move the editor consumers together.

use perl_lsp_ux_tests::binary_available;
use perl_lsp_ux_tests::{ScenarioConfig, UxHarness};
use std::time::Duration;

const PL701: &str = "PL701";

const EXACT_SOURCE: &str = "\
use strict;\n\
use warnings;\n\
BEGIN {\n\
    use lib qw(vendor);\n\
}\n\
use BeginIncModule;\n\
\n\
my $value = BeginIncModule::value();\n\
";

const COMPLETION_SOURCE: &str = "\
use strict;\n\
use warnings;\n\
BEGIN {\n\
    use lib qw(vendor);\n\
}\n\
use Beg\n\
";

const CONTROL_SOURCE: &str = "\
use strict;\n\
use warnings;\n\
use BeginIncModule;\n\
\n\
my $value = BeginIncModule::value();\n\
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
            .with_file("control.pl", CONTROL_SOURCE)
            .with_file("vendor/BeginIncModule.pm", MODULE_SOURCE),
    )
    .expect("Failed to create UX harness");

    let diagnostics_seen_before_open = harness.diagnostics_event_count("fixture.pl");
    harness.open_file("fixture.pl", EXACT_SOURCE).expect("didOpen should succeed");
    let diags = harness
        .wait_for_diagnostics_after_count(
            "fixture.pl",
            diagnostics_seen_before_open,
            Duration::from_secs(5),
        )
        .expect("didOpen should publish diagnostics");
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

    let hover = harness
        .hover("fixture.pl", 5, 4)
        .expect("hover must not error")
        .expect("Expected hover for the exact resolved module fixture");
    assert!(
        hover.get("contents").is_some_and(|contents| !contents.is_null()),
        "Hover result must have non-null contents: {hover:?}"
    );

    let diagnostics_seen_before_edit = harness.diagnostics_event_count("fixture.pl");
    harness
        .change_file_full("fixture.pl", COMPLETION_SOURCE)
        .expect("didChange to completion fixture should succeed");
    harness
        .wait_for_diagnostics_after_count(
            "fixture.pl",
            diagnostics_seen_before_edit,
            Duration::from_secs(5),
        )
        .expect("didChange should publish diagnostics after the edit");

    // `use Beg` is at zero-based line 5, cursor column 7.
    let completions = harness.completion("fixture.pl", 5, 7).expect("completion must not error");
    assert!(
        completion_has_module(&completions, "BeginIncModule"),
        "Expected completion to include BeginIncModule through BEGIN-scoped use lib; \
         completions: {completions:?}"
    );

    let control_diagnostics_seen_before_open = harness.diagnostics_event_count("control.pl");
    harness.open_file("control.pl", CONTROL_SOURCE).expect("control didOpen should succeed");
    let control_diags = harness
        .wait_for_diagnostics_after_count(
            "control.pl",
            control_diagnostics_seen_before_open,
            Duration::from_secs(5),
        )
        .expect("control didOpen should publish diagnostics");
    assert!(
        has_pl701(&control_diags),
        "Expected PL701 without BEGIN use lib in the control fixture.\n\
         diagnostics: {control_diags:?}"
    );

    harness.assert_no_crash();
    Ok(())
}
