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
//! - A present hover range MUST be well formed and contain the request cursor.
//! - No crash signatures after the request.

use anyhow::{Context, Result};
use perl_lsp_ux_tests::{
    ScenarioConfig, UxCiTier, UxComponent, UxEvidenceClass, UxHarness, binary_available,
    missing_binary_skip, run_ux_scenario, run_ux_scenario_with_evidence_class,
};
use serde_json::Value;
use std::time::Duration;

const WORKFLOW_ID: &str = "hover_core";
const SCENARIO_FILE: &str = "ux_scenario_11_hover.rs";

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

fn position_coordinates(range: &Value, endpoint: &str) -> Result<(u64, u64)> {
    let position = range
        .get(endpoint)
        .and_then(Value::as_object)
        .with_context(|| format!("hover range.{endpoint} must be an object: {range:?}"))?;
    let line = position
        .get("line")
        .and_then(Value::as_u64)
        .with_context(|| format!("hover range.{endpoint}.line must be a non-negative integer"))?;
    let character = position.get("character").and_then(Value::as_u64).with_context(|| {
        format!("hover range.{endpoint}.character must be a non-negative integer")
    })?;
    Ok((line, character))
}

fn validate_optional_range_contains_cursor(
    result: &Value,
    cursor_line: u32,
    cursor_character: u32,
) -> Result<()> {
    let Some(range) = result.get("range") else {
        return Ok(());
    };
    anyhow::ensure!(range.is_object(), "hover range must be an object when present: {range:?}");

    let (start_line, start_character) = position_coordinates(range, "start")?;
    let (end_line, end_character) = position_coordinates(range, "end")?;
    anyhow::ensure!(
        start_line < end_line || (start_line == end_line && start_character <= end_character),
        "hover range start must not follow its end: {range:?}"
    );

    let cursor_line = u64::from(cursor_line);
    let cursor_character = u64::from(cursor_character);
    let starts_before_cursor = start_line < cursor_line
        || (start_line == cursor_line && start_character <= cursor_character);
    let ends_after_cursor =
        end_line > cursor_line || (end_line == cursor_line && end_character >= cursor_character);
    anyhow::ensure!(
        starts_before_cursor && ends_after_cursor,
        "hover range must contain the request cursor: range={range:?}, \
         cursor=({cursor_line}, {cursor_character})"
    );
    Ok(())
}

#[test]
fn scenario_11_hover_on_variable_does_not_error() {
    run_ux_scenario_with_evidence_class(
        WORKFLOW_ID,
        SCENARIO_FILE,
        "scenario_11_hover_on_variable_does_not_error",
        UxCiTier::Pr,
        Some(UxComponent::Hover),
        UxEvidenceClass::TransportCharacterization,
        |recorder| {
            if !binary_available() {
                return Err(missing_binary_skip().into());
            }

            let harness = UxHarness::new(
                ScenarioConfig { timeout: Duration::from_secs(15), ..Default::default() }
                    .with_file("calc.pl", HOVER_SOURCE),
            )
            .context("Failed to create UX harness")?;

            harness.open_file("calc.pl", HOVER_SOURCE).context("didOpen should succeed")?;
            harness
                .hover("calc.pl", VARIABLE_LINE, VARIABLE_CHARACTER)
                .context("hover on `$result` must not return a transport error")?;
            recorder.check("variable hover transport completed", true)?;

            harness.assert_no_crash();
            recorder.check("no crash signatures in event log", true)?;
            Ok(())
        },
    );
}

#[test]
fn scenario_11_static_subroutine_call_returns_useful_hover() {
    run_ux_scenario(
        WORKFLOW_ID,
        SCENARIO_FILE,
        "scenario_11_static_subroutine_call_returns_useful_hover",
        UxCiTier::Pr,
        Some(UxComponent::Hover),
        |recorder| {
            if !binary_available() {
                return Err(missing_binary_skip().into());
            }

            let harness = UxHarness::new(
                ScenarioConfig { timeout: Duration::from_secs(15), ..Default::default() }
                    .with_file("calc.pl", HOVER_SOURCE),
            )
            .context("Failed to create UX harness")?;

            harness.open_file("calc.pl", HOVER_SOURCE).context("didOpen should succeed")?;

            recorder.mark_request_start("textDocument/hover");
            let result = static_call_hover_with_retry(&harness)?;
            let text = useful_hover_text(&result)?;
            anyhow::ensure!(
                text.contains(STATIC_CALL_SUBJECT),
                "static call hover must identify `{STATIC_CALL_SUBJECT}`, but the card was about \
                 something else: {text:?}"
            );
            recorder.check("static call hover identifies calculate_sum", true)?;
            recorder.mark_first_useful_result("subject-bound hover card");

            // Wrong-symbol falsifier: `$result` on the same line also returns a useful,
            // non-empty card. If a bare non-empty check were sufficient, that card would
            // satisfy the assertion above — so prove the subject marker discriminates by
            // position rather than merely detecting that hover returned something.
            let variable =
                harness.hover("calc.pl", VARIABLE_LINE, VARIABLE_CHARACTER)?.ok_or_else(|| {
                    anyhow::anyhow!("expected a hover card for the `$result` control")
                })?;
            let variable_text = useful_hover_text(&variable)?;
            anyhow::ensure!(
                !variable_text.contains(STATIC_CALL_SUBJECT),
                "the `$result` control must not carry the `{STATIC_CALL_SUBJECT}` marker, \
                 otherwise the call-site assertion cannot distinguish the two subjects: \
                 {variable_text:?}"
            );
            recorder.check("wrong-symbol control remains discriminating", true)?;

            harness.assert_no_crash();
            recorder.check("no crash signatures in event log", true)?;
            Ok(())
        },
    );
}

#[test]
fn scenario_11_hover_on_sub_name_does_not_crash() {
    run_ux_scenario_with_evidence_class(
        WORKFLOW_ID,
        SCENARIO_FILE,
        "scenario_11_hover_on_sub_name_does_not_crash",
        UxCiTier::Pr,
        Some(UxComponent::Hover),
        UxEvidenceClass::TransportCharacterization,
        |recorder| {
            if !binary_available() {
                return Err(missing_binary_skip().into());
            }

            let harness = UxHarness::new(
                ScenarioConfig { timeout: Duration::from_secs(15), ..Default::default() }
                    .with_file("calc.pl", HOVER_SOURCE),
            )
            .context("Failed to create UX harness")?;

            harness.open_file("calc.pl", HOVER_SOURCE).context("didOpen should succeed")?;
            harness
                .hover("calc.pl", 3, 4)
                .context("hover on the sub declaration must not return a transport error")?;
            recorder.check("sub-declaration hover transport completed", true)?;

            harness.assert_no_crash();
            recorder.check("no crash signatures in event log", true)?;
            Ok(())
        },
    );
}

#[test]
fn scenario_11_hover_range_contains_cursor_when_present() {
    run_ux_scenario_with_evidence_class(
        WORKFLOW_ID,
        SCENARIO_FILE,
        "scenario_11_hover_range_contains_cursor_when_present",
        UxCiTier::Pr,
        Some(UxComponent::Hover),
        UxEvidenceClass::TransportCharacterization,
        |recorder| {
            if !binary_available() {
                return Err(missing_binary_skip().into());
            }

            let harness = UxHarness::new(
                ScenarioConfig { timeout: Duration::from_secs(15), ..Default::default() }
                    .with_file("calc.pl", HOVER_SOURCE),
            )
            .context("Failed to create UX harness")?;

            harness.open_file("calc.pl", HOVER_SOURCE).context("didOpen should succeed")?;

            let result = static_call_hover_with_retry(&harness)?;
            validate_optional_range_contains_cursor(
                &result,
                STATIC_CALL_LINE,
                STATIC_CALL_CHARACTER,
            )?;
            recorder.check("optional hover range is valid and cursor-bound", true)?;

            harness.assert_no_crash();
            recorder.check("no crash signatures in event log", true)?;
            Ok(())
        },
    );
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

#[cfg(test)]
mod range_shape {
    use super::validate_optional_range_contains_cursor;
    use anyhow::Result;
    use serde_json::json;

    #[test]
    fn accepts_absent_and_cursor_containing_ranges() -> Result<()> {
        validate_optional_range_contains_cursor(&json!({"contents": "card"}), 8, 14)?;
        validate_optional_range_contains_cursor(
            &json!({
                "contents": "card",
                "range": {
                    "start": {"line": 8, "character": 13},
                    "end": {"line": 8, "character": 26}
                }
            }),
            8,
            14,
        )?;
        Ok(())
    }

    #[test]
    fn rejects_malformed_reversed_and_noncontaining_ranges() {
        for malformed in [
            json!({"range": null}),
            json!({"range": {}}),
            json!({
                "range": {
                    "start": {"line": 8, "character": "13"},
                    "end": {"line": 8, "character": 26}
                }
            }),
            json!({
                "range": {
                    "start": {"line": 8, "character": 26},
                    "end": {"line": 8, "character": 13}
                }
            }),
            json!({
                "range": {
                    "start": {"line": 8, "character": 15},
                    "end": {"line": 8, "character": 26}
                }
            }),
            json!({
                "range": {
                    "start": {"line": 7, "character": 0},
                    "end": {"line": 7, "character": 30}
                }
            }),
        ] {
            assert!(
                validate_optional_range_contains_cursor(&malformed, 8, 14).is_err(),
                "malformed or noncontaining range must be rejected: {malformed:?}"
            );
        }
    }
}
