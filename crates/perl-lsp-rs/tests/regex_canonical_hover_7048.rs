//! Shipped-process smoke for canonical regex hover (#7048).

mod support;

use serde_json::json;
use std::time::Duration;
use support::lsp_harness::LspHarness;

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn hover_markdown(result: &serde_json::Value) -> Option<String> {
    result.get("contents").and_then(|c| c.get("value")).and_then(|v| v.as_str()).map(str::to_string)
}

#[test]
fn canonical_regex_hover_uses_retained_analysis_not_metacharacter_table() -> TestResult {
    let doc = "my $re = /(?<name>\\w+)/;\n";
    let mut harness = LspHarness::new();
    harness.initialize_ready("file:///workspace", None)?;
    harness.open_document("file:///regex_canonical_hover.pl", doc)?;
    harness.wait_for_idle(Duration::from_millis(500));

    let result = harness
        .request_with_timeout(
            "textDocument/hover",
            json!({
                "textDocument": {"uri": "file:///regex_canonical_hover.pl"},
                "position": {"line": 0, "character": doc.find("name").unwrap()}
            }),
            Duration::from_secs(5),
        )
        .unwrap_or(json!(null));

    let hover = hover_markdown(&result).ok_or("expected hover contents")?;
    assert!(
        hover.contains("$+{name}"),
        "named capture hover should use Perl access syntax, got: {hover}"
    );
    assert!(
        !hover.contains("| Token | Meaning |"),
        "legacy metacharacter table must not answer when retention is active: {hover}"
    );
    Ok(())
}

#[test]
fn plaintext_client_receives_no_markdown_table_syntax() -> TestResult {
    let doc = "my $re = /(?<tag>\\d+)/;\n";
    let mut harness = LspHarness::new();
    harness.initialize_ready(
        "file:///workspace",
        Some(json!({
            "textDocument": {
                "hover": {
                    "contentFormat": ["plaintext"]
                }
            }
        })),
    )?;
    harness.open_document("file:///regex_plain_hover.pl", doc)?;
    harness.wait_for_idle(Duration::from_millis(500));

    let result = harness
        .request_with_timeout(
            "textDocument/hover",
            json!({
                "textDocument": {"uri": "file:///regex_plain_hover.pl"},
                "position": {"line": 0, "character": doc.find("tag").unwrap()}
            }),
            Duration::from_secs(5),
        )
        .unwrap_or(json!(null));

    let hover = hover_markdown(&result).ok_or("expected hover contents")?;
    let kind =
        result.get("contents").and_then(|c| c.get("kind")).and_then(|k| k.as_str()).unwrap_or("");
    assert_eq!(kind, "plaintext", "expected plaintext MarkupContent kind");
    assert!(
        !hover.contains("**") && !hover.contains('`'),
        "plaintext hover should not retain markdown markers, got: {hover}"
    );
    assert!(hover.contains("$+{tag}"), "got: {hover}");
    Ok(())
}
