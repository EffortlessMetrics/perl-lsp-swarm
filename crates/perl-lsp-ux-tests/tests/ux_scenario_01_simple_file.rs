// Test infrastructure — allow test-friendly patterns.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! Scenario 01 — Clean install, simple file.
//!
//! Verifies that the first simple-file read interactions return useful answers,
//! not merely successful JSON-RPC exchanges.
//!
//! Acceptance criteria:
//! - server starts and accepts `didOpen`;
//! - hover on the static `$x` declaration returns protocol-valid content that
//!   identifies that scalar variable;
//! - completion on the static `pri` prefix returns a protocol-valid `print`
//!   candidate;
//! - TTFR is marked only after the useful predicate succeeds;
//! - no crash signatures are observed.

use anyhow::Result;
use perl_lsp_ux_tests::{
    ScenarioConfig, UxCiTier, UxComponent, UxHarness, binary_available, missing_binary_skip,
    run_ux_scenario,
};
use serde_json::{Map, Value, json};
use std::collections::BTreeSet;
use std::time::Duration;

const HOVER_FILE: &str = "test.pl";
const HOVER_SOURCE: &str = "use strict;\nuse warnings;\n\nmy $x = 42;\nmy $y = $x + 1;\n";
const HOVER_LINE: u32 = 3;
const HOVER_CHARACTER: u32 = 3;
const HOVER_ATTEMPTS: usize = 5;
const HOVER_RETRY_DELAY: Duration = Duration::from_millis(200);
const HOVER_MARKERS: [&str; 2] = ["$x", "Scalar Variable"];

const COMPLETION_FILE: &str = "complete.pl";
const COMPLETION_SOURCE: &str = "pri\n";
const COMPLETION_LABEL: &str = "print";
const COMPLETION_ATTEMPTS: usize = 5;
const COMPLETION_RETRY_DELAY: Duration = Duration::from_millis(200);

fn object_keys(map: &Map<String, Value>) -> BTreeSet<&str> {
    map.keys().map(String::as_str).collect()
}

fn string_field<'a>(map: &'a Map<String, Value>, field: &str, raw: &Value) -> Result<&'a str> {
    map.get(field)
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("`{field}` must be a string: {raw:?}"))
}

fn marked_string_text(raw: &Value) -> Result<String> {
    if let Some(text) = raw.as_str() {
        return Ok(text.to_string());
    }

    let map = raw.as_object().ok_or_else(|| {
        anyhow::anyhow!("MarkedString must be a string or `{{language, value}}`: {raw:?}")
    })?;
    anyhow::ensure!(
        object_keys(map) == BTreeSet::from(["language", "value"]),
        "MarkedString object must carry exactly `language` and `value`: {raw:?}"
    );
    string_field(map, "language", raw)?;
    Ok(string_field(map, "value", raw)?.to_string())
}

fn hover_contents_text(contents: &Value) -> Result<String> {
    match contents {
        Value::String(_) => marked_string_text(contents),
        Value::Object(map) if map.contains_key("kind") => {
            anyhow::ensure!(
                object_keys(map) == BTreeSet::from(["kind", "value"]),
                "MarkupContent must carry exactly `kind` and `value`: {contents:?}"
            );
            let kind = string_field(map, "kind", contents)?;
            anyhow::ensure!(
                matches!(kind, "markdown" | "plaintext"),
                "unsupported MarkupContent kind `{kind}`: {contents:?}"
            );
            Ok(string_field(map, "value", contents)?.to_string())
        }
        Value::Object(_) => marked_string_text(contents),
        Value::Array(items) => {
            anyhow::ensure!(!items.is_empty(), "hover contents array must not be empty");
            Ok(items
                .iter()
                .map(marked_string_text)
                .collect::<Result<Vec<_>>>()?
                .join("\n"))
        }
        other => anyhow::bail!(
            "hover contents must be MarkupContent, MarkedString, or MarkedString[]: {other:?}"
        ),
    }
}

fn useful_static_variable_hover(result: &Value) -> Result<String> {
    let contents = result
        .as_object()
        .and_then(|map| map.get("contents"))
        .ok_or_else(|| anyhow::anyhow!("hover result must be an object with contents: {result:?}"))?;
    let text = hover_contents_text(contents)?;
    anyhow::ensure!(!text.trim().is_empty(), "hover contents must not be empty");
    for marker in HOVER_MARKERS {
        anyhow::ensure!(
            text.contains(marker),
            "hover on `$x` must contain `{marker}`; got {text:?}"
        );
    }
    Ok(text)
}

fn static_variable_hover_with_retry(harness: &UxHarness) -> Result<Value> {
    for attempt in 1..=HOVER_ATTEMPTS {
        if let Some(result) = harness.hover(HOVER_FILE, HOVER_LINE, HOVER_CHARACTER)? {
            useful_static_variable_hover(&result)?;
            return Ok(result);
        }
        if attempt < HOVER_ATTEMPTS {
            std::thread::sleep(HOVER_RETRY_DELAY);
        }
    }
    anyhow::bail!(
        "expected useful hover for `$x` at {HOVER_FILE}:{HOVER_LINE}:{HOVER_CHARACTER} after {HOVER_ATTEMPTS} attempts"
    )
}

fn position_is_valid(position: &Value) -> bool {
    position.get("line").and_then(Value::as_u64).is_some()
        && position.get("character").and_then(Value::as_u64).is_some()
}

fn range_is_valid(range: &Value) -> bool {
    range.get("start").is_some_and(position_is_valid)
        && range.get("end").is_some_and(position_is_valid)
}

fn text_edit_is_valid(edit: &Value) -> bool {
    let Some(map) = edit.as_object() else {
        return false;
    };
    if map.get("newText").and_then(Value::as_str).is_none() {
        return false;
    }
    if let Some(range) = map.get("range") {
        return !map.contains_key("insert") && !map.contains_key("replace") && range_is_valid(range);
    }
    map.get("insert").is_some_and(range_is_valid)
        && map.get("replace").is_some_and(range_is_valid)
}

fn useful_completion_candidate(item: &Value) -> Result<bool> {
    let map = item
        .as_object()
        .ok_or_else(|| anyhow::anyhow!("completion item must be an object: {item:?}"))?;
    let label = string_field(map, "label", item)?;
    if label != COMPLETION_LABEL {
        return Ok(false);
    }

    if let Some(insert_text) = map.get("insertText") {
        anyhow::ensure!(
            insert_text.is_string(),
            "`print` insertText must be a string: {item:?}"
        );
    }
    if let Some(format) = map.get("insertTextFormat") {
        anyhow::ensure!(
            matches!(format.as_u64(), Some(1 | 2)),
            "`print` insertTextFormat must be 1 or 2: {item:?}"
        );
    }
    if let Some(text_edit) = map.get("textEdit") {
        anyhow::ensure!(
            text_edit_is_valid(text_edit),
            "`print` textEdit must be a valid TextEdit or InsertReplaceEdit: {item:?}"
        );
    }

    Ok(true)
}

fn includes_useful_completion(items: &[Value]) -> Result<bool> {
    for item in items {
        if useful_completion_candidate(item)? {
            return Ok(true);
        }
    }
    Ok(false)
}

fn completion_with_retry(harness: &UxHarness) -> Result<Vec<Value>> {
    for attempt in 1..=COMPLETION_ATTEMPTS {
        let items = harness.completion(COMPLETION_FILE, 0, 3)?;
        if includes_useful_completion(&items)? {
            return Ok(items);
        }
        if attempt == COMPLETION_ATTEMPTS {
            anyhow::bail!(
                "expected protocol-valid `print` completion for `pri` after {COMPLETION_ATTEMPTS} attempts; last items: {items:?}"
            );
        }
        std::thread::sleep(COMPLETION_RETRY_DELAY);
    }
    anyhow::bail!("completion retry loop exhausted without an attempt")
}

#[test]
fn scenario_01_server_starts_and_accepts_open() {
    run_ux_scenario(
        "simple_file_smoke",
        "ux_scenario_01_simple_file.rs",
        "scenario_01_server_starts_and_accepts_open",
        UxCiTier::Pr,
        Some(UxComponent::Infra),
        |recorder| {
            if !binary_available() {
                return Err(missing_binary_skip().into());
            }

            let source = "#!/usr/bin/env perl\nuse strict;\n\nprint \"Hello, world!\\n\";\n";
            let harness = UxHarness::new(ScenarioConfig::default())?;
            harness.open_file("hello.pl", source)?;
            recorder.check("didOpen accepted without error", true)?;
            harness.assert_no_crash();
            recorder.check("no crash signatures in event log", true)?;
            Ok(())
        },
    );
}

#[test]
fn scenario_01_hover_on_simple_variable_returns_useful_card() {
    run_ux_scenario(
        "simple_file_smoke",
        "ux_scenario_01_simple_file.rs",
        "scenario_01_hover_on_simple_variable_returns_useful_card",
        UxCiTier::Pr,
        Some(UxComponent::Hover),
        |recorder| {
            if !binary_available() {
                return Err(missing_binary_skip().into());
            }

            let harness =
                UxHarness::new(ScenarioConfig::default().with_file(HOVER_FILE, HOVER_SOURCE))?;
            harness.open_file(HOVER_FILE, HOVER_SOURCE)?;

            recorder.mark_request_start("hover");
            let result = static_variable_hover_with_retry(&harness)?;
            let text = useful_static_variable_hover(&result)?;
            recorder.check("hover contents identify `$x`", text.contains("$x"))?;
            recorder.check(
                "hover contents identify a scalar variable",
                text.contains("Scalar Variable"),
            )?;
            recorder.mark_first_useful_result("hover");

            harness.assert_no_crash();
            recorder.check("no crash signatures in event log", true)?;
            Ok(())
        },
    );
}

#[test]
fn scenario_01_completion_on_builtin_prefix_returns_print() {
    run_ux_scenario(
        "simple_file_smoke",
        "ux_scenario_01_simple_file.rs",
        "scenario_01_completion_on_builtin_prefix_returns_print",
        UxCiTier::Pr,
        Some(UxComponent::Completion),
        |recorder| {
            if !binary_available() {
                return Err(missing_binary_skip().into());
            }

            let harness = UxHarness::new(
                ScenarioConfig::default().with_file(COMPLETION_FILE, COMPLETION_SOURCE),
            )?;
            harness.open_file(COMPLETION_FILE, COMPLETION_SOURCE)?;

            recorder.mark_request_start("completion");
            let items = completion_with_retry(&harness)?;
            recorder.check("completion response is non-empty", !items.is_empty())?;
            recorder.check(
                "completion includes protocol-valid `print` candidate",
                includes_useful_completion(&items)?,
            )?;
            recorder.mark_first_useful_result("completion");

            harness.assert_no_crash();
            recorder.check("no crash signatures in event log", true)?;
            Ok(())
        },
    );
}

#[test]
fn static_variable_hover_predicate_rejects_wrong_results() {
    for case in [
        json!(null),
        json!({ "contents": { "kind": "html", "value": "$x" } }),
        json!({ "contents": { "kind": "markdown", "value": "Scalar Variable `$y`" } }),
        json!({ "contents": { "kind": "markdown", "value": "$x" } }),
        json!({ "contents": [] }),
    ] {
        assert!(
            useful_static_variable_hover(&case).is_err(),
            "wrong hover must be rejected: {case:?}"
        );
    }
}

#[test]
fn static_variable_hover_predicate_accepts_declared_shapes() {
    for case in [
        json!({ "contents": { "kind": "markdown", "value": "**Scalar Variable**\n\n`$x`" } }),
        json!({ "contents": { "kind": "plaintext", "value": "Scalar Variable\n$x" } }),
        json!({ "contents": ["Scalar Variable", { "language": "perl", "value": "$x" }] }),
    ] {
        assert!(
            useful_static_variable_hover(&case).is_ok(),
            "valid hover must be accepted: {case:?}"
        );
    }
}

#[test]
fn completion_predicate_rejects_empty_unrelated_and_malformed_results() {
    assert!(includes_useful_completion(&[]).is_ok_and(|found| !found));
    assert!(
        includes_useful_completion(&[json!({ "label": "printf" })]).is_ok_and(|found| !found)
    );

    for item in [
        json!({ "label": "print", "insertText": 7 }),
        json!({ "label": "print", "insertTextFormat": 9 }),
        json!({ "label": "print", "textEdit": { "newText": "print" } }),
        json!({ "label": 7 }),
    ] {
        assert!(
            includes_useful_completion(&[item.clone()]).is_err(),
            "malformed completion must be rejected: {item:?}"
        );
    }
}

#[test]
fn completion_predicate_accepts_label_and_valid_text_edit() {
    for item in [
        json!({ "label": "print" }),
        json!({ "label": "print", "insertText": "print", "insertTextFormat": 1 }),
        json!({
            "label": "print",
            "textEdit": {
                "newText": "print",
                "range": {
                    "start": { "line": 0, "character": 0 },
                    "end": { "line": 0, "character": 3 }
                }
            }
        }),
    ] {
        assert!(
            includes_useful_completion(&[item.clone()]).is_ok_and(|found| found),
            "valid print completion must be accepted: {item:?}"
        );
    }
}
