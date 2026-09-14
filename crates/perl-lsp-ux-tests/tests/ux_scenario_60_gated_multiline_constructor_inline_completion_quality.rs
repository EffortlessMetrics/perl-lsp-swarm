//! Scenario 60 - gated multiline constructor inline-completion quality receipt.
//!
//! This receipt proves the first narrow multiline inline-completion exception:
//! constructor body ghost text is available on explicit invoke, suppressed on
//! automatic trigger, suppressed when it conflicts with selected completion
//! state, and safe to apply through its LSP range.

// UX receipt tests intentionally write structured receipts to stderr for --nocapture logs.
#![allow(clippy::print_stderr)]

use anyhow::{Context, Result, anyhow, bail};
use perl_lsp_ux_tests::client::EventSource;
use perl_lsp_ux_tests::{
    LspEvent, ScenarioConfig, UxCiTier, UxComponent, UxHarness, binary_available,
    missing_binary_skip, run_ux_scenario,
};
use serde::Serialize;
use serde_json::{Value, json};
use std::time::{Duration, Instant};

const SCENARIO_FILE: &str =
    "ux_scenario_60_gated_multiline_constructor_inline_completion_quality.rs";
const CONSTRUCTOR_PATH: &str = "lib/Inline/GatedMultilineConstructor.pm";
// A fresh harness opens version 1 and applies exactly one full-document edit.
const APPLIED_DOCUMENT_VERSION: i64 = 2;

const CONSTRUCTOR_SOURCE: &str = r#"package Inline::GatedMultilineConstructor;
use strict;
use warnings;

sub existing {
    my $self = shift;
    return $self;
}

sub new"#;

const EXPECTED_MULTILINE_CONSTRUCTOR: &str =
    " {\n    my $class = shift;\n    my $self = bless {}, $class;\n    return $self;\n}";

const EXPECTED_APPLIED_SOURCE: &str = r#"package Inline::GatedMultilineConstructor;
use strict;
use warnings;

sub existing {
    my $self = shift;
    return $self;
}

sub new {
    my $class = shift;
    my $self = bless {}, $class;
    return $self;
}"#;

#[derive(Debug, Serialize)]
struct MultilineConstructorReport {
    trigger_kind: u8,
    candidate_count: usize,
    insert_texts: Vec<String>,
    expected_present: bool,
    multiline_insert_texts: Vec<String>,
}

#[derive(Debug, Serialize)]
struct AppliedEditReport {
    range_source: &'static str,
    applied_matches_expected: bool,
    parse_diagnostics_absent: bool,
    diagnostics_document_version: i64,
    diagnostics_after_apply: Vec<Value>,
    parse_diagnostics_after_apply: Vec<Value>,
}

fn create_harness() -> Result<UxHarness> {
    let mut config = ScenarioConfig::default().with_file(CONSTRUCTOR_PATH, CONSTRUCTOR_SOURCE);
    config.client_capability_overrides = json!({
        "textDocument": {
            "inlineCompletion": {
                "dynamicRegistration": true
            }
        }
    });

    UxHarness::new(config)
}

fn cursor_at_end(source: &str) -> Result<(u32, u32)> {
    position_from_byte_offset(source, source.len())
}

fn position_from_byte_offset(source: &str, byte_offset: usize) -> Result<(u32, u32)> {
    let prefix = source
        .get(..byte_offset)
        .with_context(|| format!("byte offset {byte_offset} is not a UTF-8 boundary"))?;
    let line = prefix.bytes().filter(|byte| *byte == b'\n').count();
    let character = prefix.rsplit('\n').next().map(str::chars).map(Iterator::count).unwrap_or(0);
    Ok((u32::try_from(line)?, u32::try_from(character)?))
}

fn inline_insert_text(item: &Value) -> Option<String> {
    item.get("insertText").and_then(Value::as_str).map(str::to_string)
}

fn insert_texts_for(items: &[Value]) -> Vec<String> {
    items.iter().filter_map(inline_insert_text).collect()
}

fn multiline_insert_texts_for(items: &[Value]) -> Vec<String> {
    insert_texts_for(items)
        .into_iter()
        .filter(|insert_text| insert_text.contains('\n') || insert_text.contains('\r'))
        .collect()
}

fn item_has_inline_shape(item: &Value) -> bool {
    item.get("insertText").and_then(Value::as_str).is_some()
}

fn inline_registration_seen(events: &[LspEvent]) -> bool {
    events.iter().any(|event| {
        let LspEvent::Other { method, params } = event else {
            return false;
        };
        method == "client/registerCapability"
            && params.get("registrations").and_then(Value::as_array).into_iter().flatten().any(
                |registration| {
                    registration.get("method").and_then(Value::as_str)
                        == Some("textDocument/inlineCompletion")
                        && registration.get("id").and_then(Value::as_str)
                            == Some("perl-inlineCompletion")
                },
            )
    })
}

fn wait_for_inline_registration(harness: &UxHarness) -> bool {
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        if inline_registration_seen(&harness.client.peek_events()) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    false
}

fn probe_multiline_constructor(
    harness: &UxHarness,
    line: u32,
    character: u32,
    trigger_kind: u8,
) -> Result<MultilineConstructorReport> {
    let items = harness.inline_completion_with_trigger_kind(
        CONSTRUCTOR_PATH,
        line,
        character,
        trigger_kind,
    )?;
    for item in &items {
        anyhow::ensure!(
            item_has_inline_shape(item),
            "inline item must include insertText: {item:?}"
        );
    }
    let insert_texts = insert_texts_for(&items);
    let expected_present = insert_texts.iter().any(|text| text == EXPECTED_MULTILINE_CONSTRUCTOR);
    let multiline_insert_texts = multiline_insert_texts_for(&items);

    Ok(MultilineConstructorReport {
        trigger_kind,
        candidate_count: items.len(),
        insert_texts,
        expected_present,
        multiline_insert_texts,
    })
}

fn selected_completion_conflict_items(
    harness: &UxHarness,
    line: u32,
    character: u32,
) -> Result<Vec<Value>> {
    harness.inline_completion_with_context(
        CONSTRUCTOR_PATH,
        line,
        character,
        json!({
            "triggerKind": 1,
            "selectedCompletionInfo": {
                "range": {
                    "start": { "line": line, "character": character },
                    "end": { "line": line, "character": character }
                },
                "text": "new"
            }
        }),
    )
}

fn lsp_position_to_byte_offset(source: &str, line: u32, character: u32) -> Result<usize> {
    let target_line = usize::try_from(line)?;
    let target_character = usize::try_from(character)?;
    let mut current_line = 0usize;
    let mut current_character = 0usize;

    for (offset, ch) in source.char_indices() {
        if current_line == target_line && current_character == target_character {
            return Ok(offset);
        }
        if ch == '\n' {
            current_line += 1;
            current_character = 0;
        } else {
            current_character += ch.len_utf16();
        }
    }

    if current_line == target_line && current_character == target_character {
        return Ok(source.len());
    }

    bail!(
        "LSP position ({line}, {character}) is outside source ending at ({current_line}, {current_character})"
    )
}

fn range_position(item: &Value, path: &[&str]) -> Result<Option<(u32, u32)>> {
    let Some(range) = item.get("range") else {
        return Ok(None);
    };
    let Some(position) = path.iter().try_fold(range, |value, key| value.get(*key)) else {
        return Err(anyhow!("inline completion range missing path {path:?}: {range:?}"));
    };
    let line = position
        .get("line")
        .and_then(Value::as_u64)
        .ok_or_else(|| anyhow!("inline completion range path {path:?} missing line"))?;
    let character = position
        .get("character")
        .and_then(Value::as_u64)
        .ok_or_else(|| anyhow!("inline completion range path {path:?} missing character"))?;

    Ok(Some((u32::try_from(line)?, u32::try_from(character)?)))
}

fn apply_inline_item(source: &str, line: u32, character: u32, item: &Value) -> Result<String> {
    let insert_text = item
        .get("insertText")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("inline item missing insertText: {item:?}"))?;
    let (start_line, start_character) =
        range_position(item, &["start"])?.unwrap_or((line, character));
    let (end_line, end_character) = range_position(item, &["end"])?.unwrap_or((line, character));
    let start = lsp_position_to_byte_offset(source, start_line, start_character)?;
    let end = lsp_position_to_byte_offset(source, end_line, end_character)?;
    if start > end {
        bail!("inline completion range start {start} is after end {end}");
    }

    let before = source
        .get(..start)
        .ok_or_else(|| anyhow!("inline completion range start {start} is not UTF-8 aligned"))?;
    let after = source
        .get(end..)
        .ok_or_else(|| anyhow!("inline completion range end {end} is not UTF-8 aligned"))?;
    let mut applied = String::with_capacity(before.len() + insert_text.len() + after.len());
    applied.push_str(before);
    applied.push_str(insert_text);
    applied.push_str(after);
    Ok(applied)
}

// This is a versioned observation for this one-open/one-edit fixture, not a
// general transition cursor for reused versions, reopened files or config changes.
fn wait_for_applied_diagnostics(
    source: &impl EventSource,
    uri: &str,
    timeout: Duration,
) -> Result<(i64, Vec<Value>)> {
    source
        .wait_for_events(timeout, |events| {
            events.iter().rev().find_map(|event| match event {
                LspEvent::Diagnostics {
                    uri: observed_uri,
                    version: Some(version),
                    diagnostics,
                } if observed_uri == uri && *version == APPLIED_DOCUMENT_VERSION => {
                    Some((*version, diagnostics.clone()))
                }
                _ => None,
            })
        })
        .map_err(|end| {
            anyhow!(
                "no diagnostics for applied document {uri} version {APPLIED_DOCUMENT_VERSION}: {}",
                end.describe()
            )
        })
}

fn apply_invoked_constructor_edit(
    harness: &UxHarness,
    line: u32,
    character: u32,
    items: &[Value],
) -> Result<AppliedEditReport> {
    let item = items
        .iter()
        .find(|item| {
            item.get("insertText").and_then(Value::as_str) == Some(EXPECTED_MULTILINE_CONSTRUCTOR)
        })
        .ok_or_else(|| anyhow!("expected multiline constructor item was not returned"))?;
    let applied = apply_inline_item(CONSTRUCTOR_SOURCE, line, character, item)?;
    let range_source = if item.get("range").is_some() { "explicit" } else { "cursor" };
    let applied_matches_expected = applied == EXPECTED_APPLIED_SOURCE;

    harness.assert_no_crash();
    harness.change_file_full(CONSTRUCTOR_PATH, applied.as_str())?;
    let (diagnostics_document_version, diagnostics_after_apply) = wait_for_applied_diagnostics(
        &harness.client,
        &harness.workspace.uri(CONSTRUCTOR_PATH),
        Duration::from_secs(5),
    )?;
    let parse_diagnostics_after_apply = parser_diagnostics(&diagnostics_after_apply);
    let parse_diagnostics_absent = parse_diagnostics_after_apply.is_empty();

    Ok(AppliedEditReport {
        range_source,
        applied_matches_expected,
        parse_diagnostics_absent,
        diagnostics_document_version,
        diagnostics_after_apply,
        parse_diagnostics_after_apply,
    })
}

#[cfg(test)]
mod applied_diagnostics_tests {
    use super::*;
    use anyhow::ensure;
    use perl_lsp_ux_tests::{Inbox, StreamEnd};

    const URI: &str = "file:///workspace/constructor.pm";

    fn publish(inbox: &Inbox, uri: &str, version: Option<i64>, diagnostics: Vec<Value>) {
        inbox.push_event(json!({
            "jsonrpc": "2.0", "method": "textDocument/publishDiagnostics",
            "params": {"uri": uri, "version": version, "diagnostics": diagnostics}
        }));
    }

    #[test]
    fn rejects_stale_unversioned_and_unrequested_future_publications() -> Result<()> {
        for version in [Some(1), None, Some(3)] {
            let inbox = Inbox::new();
            publish(&inbox, URI, version, vec![]);
            ensure!(
                wait_for_applied_diagnostics(&inbox, URI, Duration::ZERO).is_err(),
                "version {version:?} cannot prove the applied version"
            );
        }
        Ok(())
    }

    #[test]
    fn rejects_another_document_at_the_applied_version() -> Result<()> {
        let inbox = Inbox::new();
        publish(&inbox, "file:///workspace/other.pm", Some(2), vec![]);
        ensure!(
            wait_for_applied_diagnostics(&inbox, URI, Duration::ZERO).is_err(),
            "another document cannot prove this edit"
        );
        Ok(())
    }

    #[test]
    fn missing_publication_does_not_mean_clean_diagnostics() -> Result<()> {
        let inbox = Inbox::new();
        ensure!(
            wait_for_applied_diagnostics(&inbox, URI, Duration::ZERO).is_err(),
            "silence cannot prove an empty publication"
        );
        Ok(())
    }

    #[test]
    fn transport_failure_remains_an_error_with_its_reason() -> Result<()> {
        let inbox = Inbox::new();
        inbox
            .close(StreamEnd::TransportFailure { detail: "fixture transport failure".to_string() });
        let error = wait_for_applied_diagnostics(&inbox, URI, Duration::ZERO)
            .err()
            .ok_or_else(|| anyhow!("transport failure must not become clean diagnostics"))?;
        ensure!(
            error.to_string().contains("fixture transport failure"),
            "missing failure reason: {error}"
        );
        Ok(())
    }

    #[test]
    fn drained_required_evidence_cannot_satisfy_the_wait() -> Result<()> {
        let inbox = Inbox::new();
        publish(&inbox, URI, Some(2), vec![]);
        let _ = inbox.drain_events();
        publish(&inbox, URI, Some(1), vec![]);
        ensure!(
            wait_for_applied_diagnostics(&inbox, URI, Duration::ZERO).is_err(),
            "draining evidence must not promote a stale publication"
        );
        Ok(())
    }

    #[test]
    fn exact_version_clear_is_not_overwritten_by_a_late_stale_error() -> Result<()> {
        let inbox = Inbox::new();
        publish(&inbox, URI, Some(2), vec![]);
        publish(&inbox, URI, Some(1), vec![json!({"code": "PL001", "severity": 1})]);
        let (version, diagnostics) = wait_for_applied_diagnostics(&inbox, URI, Duration::ZERO)?;
        ensure!(
            version == 2 && diagnostics.is_empty(),
            "expected the explicit applied-version clear"
        );
        Ok(())
    }

    #[test]
    fn latest_exact_version_error_is_preserved_for_the_parser_oracle() -> Result<()> {
        let inbox = Inbox::new();
        publish(&inbox, URI, Some(2), vec![]);
        let expected = vec![json!({"code": "PL001", "severity": 1})];
        publish(&inbox, URI, Some(2), expected.clone());
        publish(&inbox, URI, Some(1), vec![]);
        let (version, diagnostics) = wait_for_applied_diagnostics(&inbox, URI, Duration::ZERO)?;
        ensure!(
            version == 2 && diagnostics == expected,
            "latest applied-version errors must remain visible"
        );
        ensure!(
            !parser_diagnostics(&diagnostics).is_empty(),
            "the existing parser check must reject this payload"
        );
        Ok(())
    }
}

fn parser_diagnostics(diagnostics: &[Value]) -> Vec<Value> {
    diagnostics
        .iter()
        .filter(|diagnostic| {
            let is_error = diagnostic.get("severity").and_then(Value::as_u64) == Some(1);
            (diagnostic.get("source").and_then(Value::as_str) == Some("perl-lsp") && is_error)
                || diagnostic
                    .get("code")
                    .and_then(Value::as_str)
                    .is_some_and(|code| code.starts_with("PL"))
        })
        .cloned()
        .collect()
}

#[test]
fn scenario_60_gated_multiline_constructor_inline_completion_quality_receipt() {
    run_ux_scenario(
        "gated_multiline_constructor_inline_completion_quality",
        SCENARIO_FILE,
        "scenario_60_gated_multiline_constructor_inline_completion_quality_receipt",
        UxCiTier::Pr,
        Some(UxComponent::Completion),
        |recorder| {
            if !binary_available() {
                return Err(missing_binary_skip().into());
            }

            let harness = create_harness()?;
            harness.open_file(CONSTRUCTOR_PATH, CONSTRUCTOR_SOURCE)?;
            std::thread::sleep(Duration::from_millis(250));

            recorder.mark_request_start("dynamic_inline_registration");
            let dynamic_registration_seen = wait_for_inline_registration(&harness);
            if dynamic_registration_seen {
                recorder.mark_first_useful_result("dynamic_inline_registration");
            }

            let (line, character) = cursor_at_end(CONSTRUCTOR_SOURCE)?;

            recorder.mark_request_start("invoked_multiline_constructor_inline_completion");
            let invoked_report = probe_multiline_constructor(&harness, line, character, 1)?;
            if invoked_report.expected_present {
                recorder
                    .mark_first_useful_result("invoked_multiline_constructor_inline_completion");
            }

            recorder.mark_request_start("automatic_multiline_constructor_suppression");
            let automatic_report = probe_multiline_constructor(&harness, line, character, 2)?;
            if automatic_report.multiline_insert_texts.is_empty() {
                recorder.mark_first_useful_result("automatic_multiline_constructor_suppression");
            }

            recorder.mark_request_start("selected_completion_conflict_suppression");
            let selected_conflict_items =
                selected_completion_conflict_items(&harness, line, character)?;
            if selected_conflict_items.is_empty() {
                recorder.mark_first_useful_result("selected_completion_conflict_suppression");
            }

            recorder.mark_request_start("applied_multiline_constructor_edit");
            let invoked_items = harness.inline_completion_with_trigger_kind(
                CONSTRUCTOR_PATH,
                line,
                character,
                1,
            )?;
            let applied_report =
                apply_invoked_constructor_edit(&harness, line, character, &invoked_items)?;
            if applied_report.applied_matches_expected && applied_report.parse_diagnostics_absent {
                recorder.mark_first_useful_result("applied_multiline_constructor_edit");
            }

            let receipt = json!({
                "schema_version": 1,
                "receipt": "gated_multiline_constructor_inline_completion_quality",
                "claim_boundary": "stdio inline-completion multiline constructor receipt only; no broad multiline behavior, source mirror, release action, next-edit, or AI behavior",
                "dynamic_registration_seen": dynamic_registration_seen,
                "invoked_report": invoked_report,
                "automatic_report": automatic_report,
                "selected_completion_conflict_candidate_count": selected_conflict_items.len(),
                "applied_edit_report": applied_report,
            });
            eprintln!(
                "gated_multiline_constructor_inline_completion_quality_receipt={}",
                serde_json::to_string_pretty(&receipt)?
            );

            recorder
                .check("dynamic inline registration was observed", dynamic_registration_seen)?;
            recorder.check(
                "invoked trigger returned the fixture-backed multiline constructor",
                invoked_report.expected_present,
            )?;
            recorder.check(
                "automatic trigger suppressed multiline constructor ghost text",
                automatic_report.multiline_insert_texts.is_empty(),
            )?;
            recorder.check(
                "selectedCompletionInfo conflict suppressed multiline constructor ghost text",
                selected_conflict_items.is_empty(),
            )?;
            recorder.check(
                "accepted multiline constructor edit matched expected source",
                applied_report.applied_matches_expected,
            )?;
            recorder.check(
                "accepted multiline constructor edit produced no parser diagnostics",
                applied_report.parse_diagnostics_absent,
            )?;

            harness.assert_no_crash();
            Ok(())
        },
    );
}
