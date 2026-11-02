//! Serialized completion layer for the #13246 matrix (`textDocument/completion` entry uses
//! the same `CompletionProvider` stack exercised here; one stdio row pins transport shape).

use perl_lsp_rs_core::providers::completion::{CompletionItem, CompletionProvider};
use perl_parser::Parser;
use perl_parser_core::position::offset_to_utf16_line_col;
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::time::Duration;

mod common;

type TestResult = Result<(), Box<dyn std::error::Error>>;

const MATRIX: &str = include_str!("../../../.ci/completion-source-context-conformance/cases.json");

fn matrix() -> Result<Value, serde_json::Error> {
    serde_json::from_str(MATRIX)
}

fn probe_offset(source: &str, marker: &str) -> Result<usize, String> {
    let start = source.find(marker).ok_or_else(|| format!("probe marker `{marker}` not found"))?;
    Ok(start + marker.len())
}

fn completions_at(source: &str, position: usize) -> Vec<CompletionItem> {
    let ast = Parser::new(source).parse_with_recovery().ast;
    CompletionProvider::new_with_index_and_source(&ast, source, None)
        .get_completions(source, position)
}

fn row_has_layer(row: &Value, layer: &str) -> bool {
    row["layers"]
        .as_array()
        .is_some_and(|layers| layers.iter().any(|entry| entry.as_str() == Some(layer)))
}

fn labels(items: &[CompletionItem]) -> Vec<String> {
    items.iter().map(|item| item.label.to_string()).collect()
}

fn observe_serialized_class(
    source: &str,
    position: usize,
    row: &Value,
) -> Result<&'static str, String> {
    let items = completions_at(source, position);
    let item_labels = labels(&items);
    if row["id"].as_str() == Some("pod.for.blank_line.stays_pod")
        && item_labels.iter().any(|label| label == "post" || label == "new")
    {
        return Ok("leaking_nonempty");
    }
    if row["id"].as_str() == Some("comment.trailing_code_eligible")
        && item_labels.iter().any(|label| label == "post")
    {
        return Ok("nonempty_required");
    }
    if items.is_empty() { Ok("suppressed") } else { Ok("nonempty_required") }
}

fn class_matches(expected: &str, observed: &str) -> bool {
    match expected {
        "suppressed" => observed == "suppressed",
        "nonempty_or_exact_empty" => observed == "suppressed" || observed == "nonempty_required",
        "nonempty_required" => observed == "nonempty_required",
        other => panic!("unknown lsp_result.class `{other}`"),
    }
}

#[test]
fn matrix_lsp_layer_matches_authored_result_classes() -> TestResult {
    let doc = matrix()?;
    let rows = doc["rows"].as_array().ok_or("missing rows")?;
    let mut executed = 0usize;
    for row in rows {
        if !row_has_layer(row, "lsp") {
            continue;
        }
        let source = row["source"].as_str().ok_or("missing source")?;
        let marker = row["probe_marker"].as_str().ok_or("missing probe_marker")?;
        let offset = probe_offset(source, marker)?;
        let expected = row["lsp_result"]["class"].as_str().ok_or("missing lsp_result.class")?;
        let observed = observe_serialized_class(source, offset, row)?;
        if !class_matches(expected, observed) {
            return Err(format!(
                "{}: expected lsp_result.class {expected}, observed {observed} (labels {:?})",
                row["id"],
                labels(&completions_at(source, offset))
            )
            .into());
        }
        executed += 1;
    }
    if executed == 0 {
        return Err("lsp layer selected zero rows (NOT_PROVEN)".into());
    }
    Ok(())
}

#[test]
fn matrix_opposite_links_are_present() -> TestResult {
    let doc = matrix()?;
    let rows = doc["rows"].as_array().ok_or("missing rows")?;
    let ids: BTreeSet<_> = rows.iter().filter_map(|row| row["id"].as_str()).collect();
    for row in rows {
        if let Some(opposite) = row.get("opposite").and_then(Value::as_str) {
            if !ids.contains(opposite) {
                return Err(format!("{} opposite `{opposite}` missing", row["id"]).into());
            }
        }
    }
    Ok(())
}

#[test]
fn lsp_stdio_pod_for_blank_line_stays_suppressed() -> TestResult {
    let server = common::start_lsp_server();
    common::initialize_lsp(&server);
    let source =
        "use HTTP::Tiny;\n=for comment\ndocumentation\n\nmy $http = HTTP::Tiny->new;\n$http->po";
    let uri = "file:///tmp/completion-source-context-pod-for.pl";
    common::send_notification(
        &server,
        json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didOpen",
            "params": {
                "textDocument": {
                    "uri": uri,
                    "languageId": "perl",
                    "version": 1,
                    "text": source
                }
            }
        }),
    );
    common::drain_until_quiet(&server, Duration::from_millis(100), Duration::from_secs(2));
    let offset = probe_offset(source, "->po")?;
    let (line, character) = offset_to_utf16_line_col(source, offset);
    let response = common::send_request(
        &server,
        json!({
            "jsonrpc": "2.0",
            "method": "textDocument/completion",
            "params": {
                "textDocument": { "uri": uri },
                "position": { "line": line, "character": character }
            }
        }),
    );
    let items = common::completion_items(&response);
    assert!(
        items.is_empty(),
        "POD quiet path must serialize as suppressed completion, got {:?}",
        items
    );
    Ok(())
}
