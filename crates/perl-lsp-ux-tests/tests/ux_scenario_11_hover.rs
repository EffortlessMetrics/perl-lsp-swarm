// Test infrastructure — allow test-friendly patterns.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! Scenario 11 — Hover feature grid coverage.
//!
//! Verifies that `textDocument/hover` is wired up end-to-end for the LSP
//! feature advertised in `features.toml`.
//!
//! Acceptance criteria:
//! - `textDocument/hover` MUST NOT return a JSON-RPC error.
//! - The static same-file `calculate_sum` call MUST return useful non-empty
//!   hover contents after bounded readiness settlement.
//! - When a result is returned it MUST have either `contents` (MarkupContent or
//!   MarkedString) and optionally a `range`.
//! - Null remains acceptable only for targets whose exact hover support is not
//!   established by this scenario.
//! - No crash signatures after the request.

use anyhow::Result;
use perl_lsp_ux_tests::binary_available;
use perl_lsp_ux_tests::{ScenarioConfig, UxHarness};
use serde_json::Value;
use std::time::Duration;

/// Perl source with a clearly-named sub and variable for hover targets.
const HOVER_SOURCE: &str = "\
use strict;\n\
use warnings;\n\
\n\
sub calculate_sum {\n\
    my ($a, $b) = @_;\n\
    return $a + $b;\n\
}\n\
\n\
my $result = calculate_sum(3, 7);\n\
print $result;\n\
";

const STATIC_CALL_LINE: u32 = 8;
const STATIC_CALL_CHARACTER: u32 = 14;
const HOVER_ATTEMPTS: usize = 5;
const HOVER_RETRY_DELAY: Duration = Duration::from_millis(200);

fn hover_contents_has_text(contents: &Value) -> bool {
    if let Some(text) = contents.as_str() {
        return !text.trim().is_empty();
    }

    if let Some(text) = contents.get("value").and_then(Value::as_str) {
        return !text.trim().is_empty();
    }

    contents
        .as_array()
        .is_some_and(|items| items.iter().any(hover_contents_has_text))
}

fn static_call_hover_with_retry(harness: &UxHarness) -> Result<Value> {
    for attempt in 1..=HOVER_ATTEMPTS {
        if let Some(result) = harness.hover(
            "calc.pl",
            STATIC_CALL_LINE,
            STATIC_CALL_CHARACTER,
        )? {
            return Ok(result);
        }

        if attempt < HOVER_ATTEMPTS {
            std::thread::sleep(HOVER_RETRY_DELAY);
        }
    }

    anyhow::bail!(
        "expected non-null hover for static same-file calculate_sum call at calc.pl:{}:{} after {} attempts",
        STATIC_CALL_LINE,
        STATIC_CALL_CHARACTER,
        HOVER_ATTEMPTS
    )
}

#[test]
fn scenario_11_hover_on_variable_does_not_error() {
    if !binary_available() {
        eprintln!("SKIP scenario_11: perl-lsp binary not found");
        return;
    }

    let harness = UxHarness::new(
        ScenarioConfig { timeout: Duration::from_secs(15), ..Default::default() }
            .with_file("calc.pl", HOVER_SOURCE),
    )
    .expect("Failed to create UX harness");

    harness.open_file("calc.pl", HOVER_SOURCE).expect("didOpen should succeed");

    std::thread::sleep(Duration::from_millis(300));

    // Hover on `$result` — line 8, char 3 (inside `$result`).
    let hover_result = harness.hover("calc.pl", 8, 3);
    assert!(
        hover_result.is_ok(),
        "textDocument/hover must not return a JSON-RPC error — feature grid regression: {:?}",
        hover_result
    );

    harness.assert_no_crash();
}

#[test]
fn scenario_11_static_subroutine_call_returns_useful_hover() -> Result<()> {
    if !binary_available() {
        eprintln!("SKIP scenario_11: perl-lsp binary not found");
        return Ok(());
    }

    let harness = UxHarness::new(
        ScenarioConfig { timeout: Duration::from_secs(15), ..Default::default() }
            .with_file("calc.pl", HOVER_SOURCE),
    )?;

    harness.open_file("calc.pl", HOVER_SOURCE)?;

    let result = static_call_hover_with_retry(&harness)?;
    let contents = result
        .get("contents")
        .ok_or_else(|| anyhow::anyhow!("static call hover has no contents field: {result:?}"))?;

    assert!(
        hover_contents_has_text(contents),
        "static call hover contents must contain non-empty user-visible text: {contents:?}"
    );

    harness.assert_no_crash();
    Ok(())
}

#[test]
fn scenario_11_hover_on_sub_name_does_not_crash() {
    if !binary_available() {
        eprintln!("SKIP scenario_11: perl-lsp binary not found");
        return;
    }

    let harness = UxHarness::new(
        ScenarioConfig { timeout: Duration::from_secs(15), ..Default::default() }
            .with_file("calc.pl", HOVER_SOURCE),
    )
    .expect("Failed to create UX harness");

    harness.open_file("calc.pl", HOVER_SOURCE).expect("didOpen should succeed");

    std::thread::sleep(Duration::from_millis(300));

    // Hover on `calculate_sum` sub declaration — line 3, char 4.
    let hover_result = harness.hover("calc.pl", 3, 4);
    assert!(hover_result.is_ok(), "Hover on sub declaration must not error: {:?}", hover_result);

    harness.assert_no_crash();
}

#[test]
fn scenario_11_hover_range_contains_cursor_when_present() -> Result<()> {
    if !binary_available() {
        eprintln!("SKIP scenario_11: perl-lsp binary not found");
        return Ok(());
    }

    let harness = UxHarness::new(
        ScenarioConfig { timeout: Duration::from_secs(15), ..Default::default() }
            .with_file("calc.pl", HOVER_SOURCE),
    )?;

    harness.open_file("calc.pl", HOVER_SOURCE)?;

    let result = static_call_hover_with_retry(&harness)?;
    if let Some(range) = result.get("range") {
        let start_line = range["start"]["line"].as_u64();
        let start_char = range["start"]["character"].as_u64();
        let end_line = range["end"]["line"].as_u64();
        let end_char = range["end"]["character"].as_u64();

        assert!(start_line.is_some(), "Hover range.start.line must be numeric");
        assert!(start_char.is_some(), "Hover range.start.character must be numeric");
        assert!(end_line.is_some(), "Hover range.end.line must be numeric");
        assert!(end_char.is_some(), "Hover range.end.character must be numeric");

        let (start_line, start_char, end_line, end_char) = (
            start_line.unwrap_or_default(),
            start_char.unwrap_or_default(),
            end_line.unwrap_or_default(),
            end_char.unwrap_or_default(),
        );

        assert!(
            start_line <= end_line,
            "Hover range start line must be <= end line: {:?}",
            range
        );
        if start_line == end_line {
            assert!(
                start_char <= end_char,
                "Hover range start char must be <= end char on same line: {:?}",
                range
            );
        }

        let cursor_line = u64::from(STATIC_CALL_LINE);
        let cursor_char = u64::from(STATIC_CALL_CHARACTER);
        let starts_before_cursor =
            start_line < cursor_line || (start_line == cursor_line && start_char <= cursor_char);
        let ends_after_cursor =
            end_line > cursor_line || (end_line == cursor_line && end_char >= cursor_char);
        assert!(
            starts_before_cursor && ends_after_cursor,
            "Hover range should contain the cursor when provided: range={:?}, cursor=({}, {})",
            range,
            cursor_line,
            cursor_char
        );
    }

    harness.assert_no_crash();
    Ok(())
}
