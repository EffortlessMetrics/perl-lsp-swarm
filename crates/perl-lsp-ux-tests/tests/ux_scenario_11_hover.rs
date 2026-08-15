// Test infrastructure — allow test-friendly patterns.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! Scenario 11 — Hover feature grid coverage.
//!
//! Verifies that `textDocument/hover` is wired up end-to-end for the LSP
//! feature advertised in `features.toml`.
//!
//! Acceptance criteria:
//! - `textDocument/hover` MUST NOT return a JSON-RPC error.
//! - The static same-file `calculate_sum` call MUST return hover contents that
//!   name that subroutine, after bounded readiness settlement.
//! - When a result is returned its `contents` MUST match one of the closed
//!   protocol shapes (`MarkupContent`, `MarkedString`, or `MarkedString[]`).
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
/// `my $result = ...` on the same line — a different symbol that also has a
/// useful hover card, used as the wrong-symbol falsifier.
const VARIABLE_LINE: u32 = 8;
const VARIABLE_CHARACTER: u32 = 3;
const HOVER_ATTEMPTS: usize = 5;
const HOVER_RETRY_DELAY: Duration = Duration::from_millis(200);

/// Stable subject marker for the call target.
///
/// Non-empty text alone cannot prove the card belongs to `calculate_sum`: a
/// stale `$result` card, a `loading` placeholder, or another symbol's card is
/// also non-empty. The server renders the subroutine card as
/// `` `sub calculate_sum($a, $b)` ``, so requiring the `sub <name>` form binds
/// the assertion to this subject without pinning the full signature or the
/// complexity annotations, which are free to evolve.
const STATIC_CALL_SUBJECT: &str = "sub calculate_sum";

/// Extract user-visible text from `Hover.contents`, enforcing the closed
/// protocol shapes.
///
/// `contents` is `MarkedString | MarkedString[] | MarkupContent`, where
/// `MarkedString` is a bare string or `{ language, value }` and
/// `MarkupContent` is `{ kind, value }` with `kind` restricted to `plaintext`
/// or `markdown`.
///
/// Returns `None` for anything outside those shapes, so an arbitrary object
/// carrying a `value` string — `{"value":"x","kind":123}`, or an unrelated
/// payload with neither `kind` nor `language` — cannot pass as valid hover.
fn hover_contents_text(contents: &Value) -> Option<String> {
    match contents {
        Value::String(text) => Some(text.clone()),
        // An array is `MarkedString[]`; `MarkupContent` is not a legal member,
        // and one good entry must not excuse a malformed sibling.
        Value::Array(items) => items
            .iter()
            .map(marked_string_text)
            .collect::<Option<Vec<_>>>()
            .map(|texts| texts.join("\n")),
        Value::Object(_) => markup_content_text(contents).or_else(|| marked_string_text(contents)),
        _ => None,
    }
}

/// `MarkupContent` — `kind` must be a declared `MarkupKind` and `value` a string.
fn markup_content_text(value: &Value) -> Option<String> {
    let kind = value.get("kind")?.as_str()?;
    if kind != "plaintext" && kind != "markdown" {
        return None;
    }
    Some(value.get("value")?.as_str()?.to_owned())
}

/// `MarkedString` — a bare string, or `{ language, value }` with both strings.
fn marked_string_text(value: &Value) -> Option<String> {
    if let Some(text) = value.as_str() {
        return Some(text.to_owned());
    }
    value.get("language")?.as_str()?;
    Some(value.get("value")?.as_str()?.to_owned())
}

/// Shape-valid, non-empty hover text, or an error naming what was wrong.
fn useful_hover_text(result: &Value) -> Result<String> {
    let contents = result
        .get("contents")
        .ok_or_else(|| anyhow::anyhow!("hover result has no contents field: {result:?}"))?;
    let text = hover_contents_text(contents).ok_or_else(|| {
        anyhow::anyhow!(
            "hover contents must be MarkupContent, MarkedString, or MarkedString[]: {contents:?}"
        )
    })?;
    if text.trim().is_empty() {
        anyhow::bail!("hover contents carried no user-visible text: {contents:?}");
    }
    Ok(text)
}

fn static_call_hover_with_retry(harness: &UxHarness) -> Result<Value> {
    for attempt in 1..=HOVER_ATTEMPTS {
        if let Some(result) = harness.hover("calc.pl", STATIC_CALL_LINE, STATIC_CALL_CHARACTER)? {
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
    let text = useful_hover_text(&result)?;

    assert!(
        text.contains(STATIC_CALL_SUBJECT),
        "static call hover must identify `{STATIC_CALL_SUBJECT}`, but the card was about \
         something else: {text:?}"
    );

    // Wrong-symbol falsifier: `$result` on the same line also returns a useful,
    // non-empty card. If a bare non-empty check were sufficient, that card would
    // satisfy the assertion above — so prove the subject marker discriminates by
    // position rather than merely detecting that hover returned something.
    let variable = harness
        .hover("calc.pl", VARIABLE_LINE, VARIABLE_CHARACTER)?
        .ok_or_else(|| anyhow::anyhow!("expected a hover card for the `$result` control"))?;
    let variable_text = useful_hover_text(&variable)?;
    assert!(
        !variable_text.contains(STATIC_CALL_SUBJECT),
        "the `$result` control must not carry the `{STATIC_CALL_SUBJECT}` marker, otherwise the \
         call-site assertion cannot distinguish the two subjects: {variable_text:?}"
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

        assert!(start_line <= end_line, "Hover range start line must be <= end line: {:?}", range);
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

/// Shape-validation negatives for [`hover_contents_text`].
///
/// These need no server: they pin the boundary between the closed protocol
/// shapes and payloads that merely look close enough to pass a `value` probe.
#[cfg(test)]
mod contents_shape {
    use super::{hover_contents_text, marked_string_text, markup_content_text};
    use serde_json::json;

    #[test]
    fn accepts_the_declared_shapes() {
        assert_eq!(hover_contents_text(&json!("plain")).as_deref(), Some("plain"));
        assert_eq!(
            hover_contents_text(&json!({"kind": "markdown", "value": "md"})).as_deref(),
            Some("md")
        );
        assert_eq!(
            hover_contents_text(&json!({"kind": "plaintext", "value": "txt"})).as_deref(),
            Some("txt")
        );
        assert_eq!(
            hover_contents_text(&json!({"language": "perl", "value": "code"})).as_deref(),
            Some("code")
        );
        assert_eq!(
            hover_contents_text(&json!(["a", {"language": "perl", "value": "b"}])).as_deref(),
            Some("a\nb")
        );
    }

    #[test]
    fn rejects_objects_that_only_look_like_markup() {
        // A non-string `kind` is not a `MarkupKind`, and without `language`
        // this is not a `MarkedString` either.
        assert!(hover_contents_text(&json!({"value": "x", "kind": 123})).is_none());
        // An undeclared markup kind.
        assert!(hover_contents_text(&json!({"value": "x", "kind": "html"})).is_none());
        // A bare `value` with no discriminator at all.
        assert!(hover_contents_text(&json!({"value": "x"})).is_none());
        // Right discriminator, wrong `value` type.
        assert!(hover_contents_text(&json!({"kind": "markdown", "value": 7})).is_none());
        assert!(hover_contents_text(&json!({"language": "perl"})).is_none());
        assert!(hover_contents_text(&json!({})).is_none());
        assert!(hover_contents_text(&json!(42)).is_none());
        assert!(hover_contents_text(&json!(null)).is_none());
    }

    #[test]
    fn rejects_mixed_arrays_rather_than_accepting_one_good_member() {
        // `MarkupContent` is not a legal array member even though it is a
        // legal top-level payload.
        assert!(hover_contents_text(&json!([{"kind": "markdown", "value": "md"}])).is_none());
        // One valid entry must not rescue a malformed sibling.
        assert!(hover_contents_text(&json!(["good", {"value": "x"}])).is_none());
        assert!(hover_contents_text(&json!(["good", 42])).is_none());
    }

    #[test]
    fn helpers_stay_specific() {
        assert!(markup_content_text(&json!({"language": "perl", "value": "code"})).is_none());
        assert!(marked_string_text(&json!({"kind": "markdown", "value": "md"})).is_none());
    }
}
