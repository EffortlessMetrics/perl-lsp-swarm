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
//! - Those contents MUST match a closed LSP `Hover.contents` shape
//!   (`MarkupContent`, `MarkedString`, or `MarkedString[]`) and MUST identify
//!   the hovered subject, not merely carry some non-empty string.
//! - Null remains acceptable only for targets whose exact hover support is not
//!   established by this scenario.
//! - No crash signatures after the request.

use anyhow::Result;
use perl_lsp_ux_tests::binary_available;
use perl_lsp_ux_tests::{ScenarioConfig, UxHarness};
use serde_json::{Map, Value, json};
use std::collections::BTreeSet;
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

/// `MarkupKind` values LSP 3.17 permits for `MarkupContent.kind`.
const MARKUP_KINDS: [&str; 2] = ["markdown", "plaintext"];

/// The subject the static call-site hover must actually be about.
///
/// Non-empty text alone does not prove the answer belongs to this cursor: a
/// stale card for `$result`, a `loading` placeholder, or documentation for an
/// unrelated symbol are all non-empty. The known-answer fixture below defines
/// exactly one correct subject, so name it.
const STATIC_CALL_SUBJECT: &str = "calculate_sum";

/// Markers the static call-site hover must carry to count as the right answer.
///
/// The fixture is static and checked in, so the correct answer is known
/// exactly: the same-file subroutine definition, rendered as a signature. The
/// subject name alone would still admit a card for the wrong symbol kind (for
/// example a scalar named `calculate_sum`), so the declaration form is
/// required too. If the provider deliberately changes this card, update these
/// markers as a decision rather than relaxing the assertion to non-empty text.
const STATIC_CALL_HOVER_MARKERS: [&str; 2] = [STATIC_CALL_SUBJECT, "sub calculate_sum"];

fn object_keys(map: &Map<String, Value>) -> BTreeSet<&str> {
    map.keys().map(String::as_str).collect()
}

fn string_field<'a>(map: &'a Map<String, Value>, field: &str, raw: &Value) -> Result<&'a str> {
    map.get(field)
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("`{field}` must be a string: {raw:?}"))
}

/// `MarkedString` — a plain string, or `{ language, value }` with both strings.
fn marked_string_text(raw: &Value) -> Result<String> {
    if let Some(text) = raw.as_str() {
        return Ok(text.to_string());
    }

    let map = raw.as_object().ok_or_else(|| {
        anyhow::anyhow!("MarkedString must be a string or `{{language, value}}` object: {raw:?}")
    })?;

    if object_keys(map) != BTreeSet::from(["language", "value"]) {
        anyhow::bail!("MarkedString object must carry exactly `language` and `value`: {raw:?}");
    }

    string_field(map, "language", raw)?;
    Ok(string_field(map, "value", raw)?.to_string())
}

/// `MarkupContent` — `{ kind, value }` where `kind` is a `MarkupKind`.
fn markup_content_text(raw: &Value) -> Result<String> {
    let map = raw
        .as_object()
        .ok_or_else(|| anyhow::anyhow!("MarkupContent must be an object: {raw:?}"))?;

    if object_keys(map) != BTreeSet::from(["kind", "value"]) {
        anyhow::bail!("MarkupContent must carry exactly `kind` and `value`: {raw:?}");
    }

    let kind = string_field(map, "kind", raw)?;
    anyhow::ensure!(
        MARKUP_KINDS.contains(&kind),
        "MarkupContent `kind` must be one of {MARKUP_KINDS:?}; got `{kind}`: {raw:?}"
    );

    Ok(string_field(map, "value", raw)?.to_string())
}

/// Extract user-visible text from `Hover.contents`, enforcing the closed LSP
/// shapes instead of accepting any object that happens to carry a `value`.
///
/// Accepted, per LSP 3.17 `Hover.contents`:
/// - `MarkupContent` — `{ kind, value }`;
/// - `MarkedString` — a plain string or `{ language, value }`;
/// - `MarkedString[]` — every member must itself be a valid `MarkedString`.
///
/// Rejected: unknown or missing keys, non-string `kind`/`value`/`language`, a
/// `kind` outside `MarkupKind`, `MarkupContent` nested inside the array form
/// (which the protocol does not allow), and any other JSON type.
fn hover_contents_text(contents: &Value) -> Result<String> {
    match contents {
        Value::String(_) => marked_string_text(contents),
        Value::Object(map) => {
            if map.contains_key("kind") {
                markup_content_text(contents)
            } else {
                marked_string_text(contents)
            }
        }
        Value::Array(items) => {
            anyhow::ensure!(
                !items.is_empty(),
                "hover contents array must not be empty: {contents:?}"
            );
            // Every member is checked: one good member must not certify a
            // malformed sibling.
            let parts = items.iter().map(marked_string_text).collect::<Result<Vec<_>>>()?;
            Ok(parts.join("\n"))
        }
        other => anyhow::bail!(
            "hover contents must be MarkupContent, MarkedString, or MarkedString[]: {other:?}"
        ),
    }
}

/// A hover is useful for this fixture only when it is structurally valid,
/// carries non-empty user-visible text, and names the hovered subject.
fn assert_useful_static_call_hover(contents: &Value) -> Result<()> {
    let text = hover_contents_text(contents)?;

    anyhow::ensure!(
        !text.trim().is_empty(),
        "static call hover contents must contain non-empty user-visible text: {contents:?}"
    );
    for marker in STATIC_CALL_HOVER_MARKERS {
        anyhow::ensure!(
            text.contains(marker),
            "static call hover must identify `{STATIC_CALL_SUBJECT}` by carrying `{marker}`; got: {text:?}"
        );
    }

    Ok(())
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
    let contents = result
        .get("contents")
        .ok_or_else(|| anyhow::anyhow!("static call hover has no contents field: {result:?}"))?;

    assert_useful_static_call_hover(contents)?;

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

// ---------------------------------------------------------------------------
// Falsifiers for the acceptance predicate itself.
//
// The real-process assertions above are only as strong as the predicate they
// call. These run without the server binary and fail if the predicate ever
// widens back into "any non-empty string anywhere is a useful hover".
// ---------------------------------------------------------------------------

#[test]
fn hover_contents_accepts_the_declared_protocol_shapes() {
    let cases = [
        json!("plain MarkedString"),
        json!({ "kind": "markdown", "value": "**Subroutine**" }),
        json!({ "kind": "plaintext", "value": "Subroutine" }),
        json!({ "language": "perl", "value": "sub calculate_sum" }),
        json!(["first", { "language": "perl", "value": "second" }]),
    ];

    for case in cases {
        assert!(
            hover_contents_text(&case).is_ok(),
            "declared LSP hover shape must be accepted: {case:?}"
        );
    }
}

#[test]
fn hover_contents_rejects_malformed_and_unknown_shapes() {
    let cases = [
        // Non-string `kind` — the previous predicate accepted this.
        json!({ "value": "x", "kind": 123 }),
        // `kind` outside `MarkupKind`.
        json!({ "kind": "html", "value": "x" }),
        // Unrelated object carrying a `value` but no `kind`/`language`.
        json!({ "value": "x" }),
        // Extra keys are not the declared closed shape.
        json!({ "kind": "markdown", "value": "x", "extra": 1 }),
        // Non-string `value`.
        json!({ "kind": "markdown", "value": 7 }),
        // `MarkupContent` is not a legal member of the array form.
        json!([{ "kind": "markdown", "value": "x" }]),
        // One good member must not certify a malformed sibling.
        json!(["ok", { "value": "x", "kind": 123 }]),
        // Neither an object, a string, nor an array.
        json!(42),
        json!(null),
        json!([]),
    ];

    for case in cases {
        assert!(
            hover_contents_text(&case).is_err(),
            "malformed hover contents must be rejected: {case:?}"
        );
    }
}

#[test]
fn static_call_hover_rejects_non_empty_but_wrong_or_useless_answers() {
    let cases = [
        // A stale card for the assignment target on the same line.
        json!({ "kind": "markdown", "value": "**Scalar Variable**\n\n`$result`" }),
        // A constant placeholder.
        json!({ "kind": "markdown", "value": "loading" }),
        // Documentation for an unrelated symbol.
        json!({ "kind": "markdown", "value": "**Built-in Function**\n\n```\nprint LIST\n```" }),
        // Right name, wrong symbol kind — no subroutine declaration.
        json!({ "kind": "markdown", "value": "**Scalar Variable**\n\n`$calculate_sum`" }),
        // Structurally valid but empty user-visible text.
        json!({ "kind": "markdown", "value": "   " }),
    ];

    for case in cases {
        assert!(
            assert_useful_static_call_hover(&case).is_err(),
            "hover that does not answer for `{STATIC_CALL_SUBJECT}` must be rejected: {case:?}"
        );
    }
}

#[test]
fn static_call_hover_accepts_a_card_that_names_the_subject() {
    let contents = json!({
        "kind": "markdown",
        "value": "**Subroutine**\n\n`sub calculate_sum($a, $b)`",
    });

    assert!(
        assert_useful_static_call_hover(&contents).is_ok(),
        "a subroutine card naming the hovered subject must be accepted: {contents:?}"
    );
}
