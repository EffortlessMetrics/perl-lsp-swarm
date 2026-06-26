// Test infrastructure — allow test-friendly patterns used throughout this module.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! Scenario 22 — Index-readiness race regression guards.
//!
//! Verifies that the seven providers fixed in #3095 no longer return empty/partial
//! results when the request arrives immediately after workspace open (while the
//! workspace index is still in `IndexState::Building`).
//!
//! ## Design
//!
//! Each test issues its request with NO explicit settle delay after `open_file`.
//! This is the same pattern as `scenario_20_completion_imported_symbol_helper_hard_assert`
//! (which locks #3069) — it exposes the race by driving the provider before the
//! indexer has a chance to finish.
//!
//! ## The references test strategy
//!
//! The text-search fallback (`on_references`) only searches OPEN documents.
//! To make the race observable for references we open ONLY Base.pm (where `sub shared`
//! is declared) and NOT App.pm (which contains the call sites).  With no wait guard
//! the index is Partial and cross-file refs return empty; with the wait the index is
//! Ready and App.pm call sites are found even though the file was never opened.
//!
//! ## Fixture layout (same four-file RealBaseline workspace as scenario_20/21)
//!
//! ```text
//! lib/
//!   RealBaseline/
//!     App.pm   — use parent 'RealBaseline::Base'; calls $self->shared (lines 15,16,17)
//!     Base.pm  — sub shared { ... }
//!     Util.pm  — sub helper { ... }  exported via @EXPORT_OK
//! script/
//!   real-baseline.pl
//! ```
//!
//! ## Acceptance commands
//!
//! ```bash
//! RUST_TEST_THREADS=2 cargo test -p perl-lsp-ux-tests \
//!     --test ux_scenario_22_index_race_regression -- --nocapture --test-threads=1
//! ```

use perl_lsp_ux_tests::binary_available;
use perl_lsp_ux_tests::{ScenarioConfig, UxHarness};
use serde_json::json;
use std::time::Duration;

// ── Fixture sources (same as scenario_20/21) ─────────────────────────────────

const APP_PM: &str = r#"package RealBaseline::App;
use strict;
use warnings;
use parent 'RealBaseline::Base';
use RealBaseline::Util qw(helper alias);

sub new {
    my ($class, %args) = @_;
    return bless \%args, $class;
}

sub run {
    my ($self) = @_;
    helper($self->name);
    alias($self->shared);
    return $self->shared;
}

sub name {
    return $_[0]->{name};
}

1;
"#;

const BASE_PM: &str = r#"package RealBaseline::Base;
use strict;
use warnings;

sub shared {
    return 'shared';
}

sub reset {
    return 1;
}

1;
"#;

const UTIL_PM: &str = r#"package RealBaseline::Util;
use strict;
use warnings;
use Exporter 'import';

our @EXPORT_OK = qw(helper alias);

sub helper {
    return shift;
}

*alias = \&helper;

sub bounce {
    goto &helper;
}

1;
"#;

const SCRIPT_PL: &str = r#"use strict;
use warnings;
use lib 'lib';
use RealBaseline::App;

my $app = RealBaseline::App->new(name => 'demo');
$app->run;
"#;

// ── Harness factory ──────────────────────────────────────────────────────────

fn create_harness() -> anyhow::Result<UxHarness> {
    UxHarness::new(
        ScenarioConfig::default()
            .with_file("lib/RealBaseline/App.pm", APP_PM)
            .with_file("lib/RealBaseline/Base.pm", BASE_PM)
            .with_file("lib/RealBaseline/Util.pm", UTIL_PM)
            .with_file("script/real-baseline.pl", SCRIPT_PL),
    )
}

// ═══════════════════════════════════════════════════════════════════════════
//  RACE GUARD 1: textDocument/references — cross-file without text-fallback mask
// ═══════════════════════════════════════════════════════════════════════════

/// Regression lock — #3095: references must find cross-file usages immediately
/// after open, even in files that are NOT opened in the editor.
///
/// **Why this test catches the race:**
/// We open ONLY Base.pm (which declares `sub shared`) and NOT App.pm (which calls
/// `$self->shared` on lines 14-16).  The text-search fallback (`on_references`)
/// only searches open documents — so if the workspace index is still in
/// `IndexState::Building` when the request arrives, the call sites in App.pm are
/// invisible and the response is empty.
///
/// With `wait_for_index_ready_if_building()` guarding `handle_references_inner`,
/// the request waits until `IndexState::Ready` before calling `route_index_access`,
/// so the workspace-index path fires and App.pm call sites are found.
///
/// No settle delay is used — the request fires immediately after `open_file` to
/// maximise the chance of racing against the indexer.
///
/// Closes #3095.
#[test]
fn scenario_22_references_cross_file_no_open_file_masking_hard_assert() -> anyhow::Result<()> {
    if !binary_available() {
        eprintln!("SKIP scenario_22: perl-lsp binary not found");
        return Ok(());
    }

    let harness = create_harness()?;
    // Open ONLY Base.pm — App.pm (which calls `shared`) is intentionally NOT opened.
    // This prevents the text-search fallback from finding the cross-file usages.
    harness.open_file("lib/RealBaseline/Base.pm", BASE_PM)?;

    // No settle delay — fire immediately to exercise the race guard.
    let refs = harness.references("lib/RealBaseline/Base.pm", 4, 4, false)?;

    // The workspace index must have finished building (wait_for_index_ready_if_building
    // blocked until Ready) so App.pm usages are found via the index path.
    assert!(
        !refs.is_empty(),
        "find-all-refs on `sub shared` in Base.pm must return cross-file App.pm usages \
         even when App.pm is not open. Got empty result — index-readiness race not fixed. \
         (#3095 regression)"
    );

    let has_app_pm = refs.iter().any(|e| {
        e.get("uri")
            .or_else(|| e.get("targetUri"))
            .and_then(|u| u.as_str())
            .map(|u| u.ends_with("App.pm"))
            .unwrap_or(false)
    });
    assert!(
        has_app_pm,
        "find-all-refs on `sub shared` must include an App.pm reference (cross-file, \
         non-open file). References found: {refs:?}"
    );

    harness.assert_no_crash();
    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════════
//  RACE GUARD 2: textDocument/hover — inherited method path
// ═══════════════════════════════════════════════════════════════════════════

/// Regression lock — #3095: hover on inherited method call `$self->shared` in App.pm
/// must return a result immediately after open (no settle delay).
///
/// **Why this test catches the race:**
/// `handle_hover` dispatches to `build_inherited_method_hover` for the InheritedMethod
/// case, which calls `self.coordinator().index()` directly.  If the coordinator is in
/// `IndexState::Building`, the index is partial and the method lookup returns None —
/// hover returns null for the inherited method call.
///
/// With `wait_for_index_ready_if_building()` placed before the `InheritedMethod` arm,
/// the lookup runs against a Ready index and the method is found.
///
/// Line 15 (0-indexed) in App.pm: `    return $self->shared;`
/// Col 18 is inside the `shared` token.  (Same position used by
/// `scenario_20_hover_inherited_method_call_hard_assert`, which passes with 300ms delay.)
///
/// Closes #3095.
#[test]
fn scenario_22_hover_inherited_method_no_settle_delay_hard_assert() -> anyhow::Result<()> {
    if !binary_available() {
        eprintln!("SKIP scenario_22: perl-lsp binary not found");
        return Ok(());
    }

    let harness = create_harness()?;
    harness.open_file("lib/RealBaseline/App.pm", APP_PM)?;
    harness.open_file("lib/RealBaseline/Base.pm", BASE_PM)?;

    // No settle delay — fire immediately to exercise the race guard.
    // Line 15 (0-indexed): `    return $self->shared;`  col 18 inside `shared`.
    let result = harness.hover("lib/RealBaseline/App.pm", 15, 18)?;

    // The inherited method hover requires a Ready index.  With the wait guard,
    // `build_inherited_method_hover` sees a complete index and returns a result.
    assert!(
        result.is_some(),
        "hover on inherited `$self->shared` call in App.pm must return a non-null result \
         immediately after open (no settle delay). Got null — inherited-method hover \
         index-readiness race not fixed. (#3095 regression)"
    );

    let hover = result.unwrap();
    assert!(
        hover.get("contents").is_some(),
        "hover result for inherited `$self->shared` must have a `contents` field. \
         Got: {hover:?}"
    );

    harness.assert_no_crash();
    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════════
//  RACE GUARD 3: textDocument/signatureHelp — cross-file workspace method
// ═══════════════════════════════════════════════════════════════════════════

/// Regression lock — #3095: signatureHelp for an imported function call must return
/// a non-empty signature immediately after open (no settle delay).
///
/// **Why this test catches the race:**
/// `handle_signature_help` calls `resolve_method_in_workspace`, which calls
/// `route_index_access`.  If the index is in `IndexState::Building`, the access mode
/// is Partial (not Full) and `resolve_method_in_workspace` returns None — the
/// signatureHelp response has an empty `signatures` array.
///
/// With `wait_for_index_ready_if_building()` placed before the workspace method
/// resolution path, the lookup runs against a Ready index and `helper`'s signature
/// is found in Util.pm.
///
/// Line 13 (0-indexed) in App.pm: `    helper($self->name);`
/// Col 11 is just after the `(` opening the call — inside the argument list.
///
/// Closes #3095.
#[test]
fn scenario_22_signature_help_imported_fn_no_settle_delay_hard_assert() -> anyhow::Result<()> {
    if !binary_available() {
        eprintln!("SKIP scenario_22: perl-lsp binary not found");
        return Ok(());
    }

    let harness = create_harness()?;
    harness.open_file("lib/RealBaseline/App.pm", APP_PM)?;
    harness.open_file("lib/RealBaseline/Util.pm", UTIL_PM)?;

    // No settle delay — fire immediately to exercise the race guard.
    // Line 13 (0-indexed): `    helper($self->name);`  col 11 inside arg list of `helper(`.
    let uri = harness.workspace.uri("lib/RealBaseline/App.pm");
    let resp = harness.client.request(
        "textDocument/signatureHelp",
        json!({
            "textDocument": { "uri": uri },
            "position": { "line": 13, "character": 11 }
        }),
        Duration::from_secs(5),
    )?;

    assert!(
        resp.get("error").is_none(),
        "signatureHelp must not return a JSON-RPC error for `helper(` call in App.pm. \
         Got: {:?}",
        resp.get("error")
    );

    // With the index-readiness wait, `resolve_method_in_workspace` finds `helper` in
    // Util.pm and returns its signature.  Without the wait, `result` is null or has
    // an empty `signatures` array.
    assert!(
        !resp["result"].is_null(),
        "signatureHelp for imported `helper(` call in App.pm must return a non-null result \
         immediately after open (no settle delay). Got null — signatureHelp \
         index-readiness race not fixed. (#3095 regression)"
    );

    let sigs = resp["result"].get("signatures").and_then(|s| s.as_array()).ok_or_else(|| {
        anyhow::anyhow!("signatureHelp result has no `signatures` array: {:?}", resp["result"])
    })?;

    assert!(
        !sigs.is_empty(),
        "signatureHelp for `helper(` call must have at least one signature entry. \
         Got empty array — workspace index was not Ready at request time. \
         (#3095 regression)"
    );

    harness.assert_no_crash();
    Ok(())
}
