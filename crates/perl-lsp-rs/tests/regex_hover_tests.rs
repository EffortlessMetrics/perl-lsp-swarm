//! Integration tests for regex-family hover (#2048, canonical path #7048).

mod support;

use serde_json::json;
use std::time::Duration;
use support::lsp_harness::LspHarness;

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn hover_value(result: &serde_json::Value) -> Option<String> {
    result
        .get("contents")
        .and_then(|c| c.get("value"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

fn open_ready(harness: &mut LspHarness, uri: &str, doc: &str) -> Result<(), String> {
    harness.initialize_ready("file:///workspace", None)?;
    harness.open_document(uri, doc)?;
    harness.wait_for_idle(Duration::from_millis(500));
    Ok(())
}

fn hover_at(harness: &mut LspHarness, uri: &str, line: u64, character: u64) -> serde_json::Value {
    harness
        .request_with_timeout(
            "textDocument/hover",
            json!({
                "textDocument": {"uri": uri},
                "position": {"line": line, "character": character}
            }),
            Duration::from_secs(5),
        )
        .unwrap_or(json!(null))
}

fn assert_canonical_regex_hover(val: &str) {
    assert!(
        val.contains("Pattern:")
            || val.contains("Regex")
            || val.contains("Substitution")
            || val.contains("Capture"),
        "expected canonical regex hover; got: {val}"
    );
    assert!(
        !val.contains("| Token | Meaning |"),
        "legacy metacharacter table must not answer under retention; got: {val}"
    );
}

#[test]
fn test_hover_regex_simple_digit_plus() -> TestResult {
    let uri = "file:///regex_simple.pl";
    let doc = "if ($x =~ /\\d+/) {\n}\n";
    let mut harness = LspHarness::new();
    open_ready(&mut harness, uri, doc)?;

    let result = hover_at(&mut harness, uri, 0, 11);
    let val = hover_value(&result).ok_or("Expected hover contents for regex")?;
    assert_canonical_regex_hover(&val);
    Ok(())
}

#[test]
fn test_hover_regex_fat_arrow_pattern() -> TestResult {
    let uri = "file:///regex_fat_arrow.pl";
    let doc = "my $re = /^(\\w+)\\s*=>\\s*(.*)$/;\n";
    let mut harness = LspHarness::new();
    open_ready(&mut harness, uri, doc)?;

    let result = hover_at(&mut harness, uri, 0, 10);
    let val = hover_value(&result).ok_or("Expected hover contents for regex")?;
    assert_canonical_regex_hover(&val);
    Ok(())
}

#[test]
fn test_hover_regex_capture_group_explanation() -> TestResult {
    let uri = "file:///regex_capture.pl";
    let doc = "my @m = ($str =~ /(\\w+)/);\n";
    let mut harness = LspHarness::new();
    open_ready(&mut harness, uri, doc)?;

    let result = hover_at(&mut harness, uri, 0, 18);
    let val = hover_value(&result).ok_or("Expected hover contents for regex")?;
    assert!(
        val.contains("Capture") || val.contains("capture") || val.contains("$1"),
        "Hover should describe capture declaration; got: {val}"
    );
    Ok(())
}

#[test]
fn test_hover_substitution_regex_explanation() -> TestResult {
    let uri = "file:///regex_subst.pl";
    let doc = "$str =~ s/\\s+/ /g;\n";
    let mut harness = LspHarness::new();
    open_ready(&mut harness, uri, doc)?;

    let result = hover_at(&mut harness, uri, 0, 11);
    let val = hover_value(&result).ok_or("Expected hover contents for substitution regex")?;
    assert!(
        val.contains("Substitution") || val.contains("Pattern:"),
        "Hover should describe substitution operator; got: {val}"
    );
    Ok(())
}

#[test]
fn test_hover_regex_returns_markdown_kind() -> TestResult {
    let uri = "file:///regex_markdown.pl";
    let doc = "if ($x =~ /\\d+/) {\n}\n";
    let mut harness = LspHarness::new();
    open_ready(&mut harness, uri, doc)?;

    let result = hover_at(&mut harness, uri, 0, 11);
    if !result.is_null() {
        let kind = result.get("contents").and_then(|c| c.get("kind")).and_then(|k| k.as_str());
        assert_eq!(kind, Some("markdown"), "Hover kind should be 'markdown'; got: {kind:?}");
    }
    Ok(())
}

#[test]
fn test_hover_regex_non_capturing_group() -> TestResult {
    let uri = "file:///regex_non_capturing.pl";
    let doc = "my $x = /(?:\\d+)/;\n";
    let mut harness = LspHarness::new();
    open_ready(&mut harness, uri, doc)?;

    let result = hover_at(&mut harness, uri, 0, 10);
    let val = hover_value(&result).ok_or("Expected hover contents")?;
    assert_canonical_regex_hover(&val);
    Ok(())
}

#[test]
fn test_hover_regex_positive_lookahead() -> TestResult {
    let uri = "file:///regex_lookahead.pl";
    let doc = "if ($x =~ /\\w+(?=\\s)/) {\n}\n";
    let mut harness = LspHarness::new();
    open_ready(&mut harness, uri, doc)?;

    let result = hover_at(&mut harness, uri, 0, 15);
    let val = hover_value(&result).ok_or("Expected hover contents for lookahead")?;
    assert_canonical_regex_hover(&val);
    Ok(())
}

#[test]
fn test_hover_no_false_positive_division_after_brace() -> TestResult {
    let uri = "file:///no_regex_division.pl";
    let doc = "my $x = $h{key}/2;\n";
    let mut harness = LspHarness::new();
    open_ready(&mut harness, uri, doc)?;

    let result = hover_at(&mut harness, uri, 0, 15);
    if let Some(val) = hover_value(&result) {
        assert!(
            !val.contains("Regex (bare") && !val.contains("**Regex"),
            "Should not treat division as regex; got hover: {val}"
        );
    }
    Ok(())
}

#[test]
fn test_hover_regex_word_boundary() -> TestResult {
    let uri = "file:///regex_boundary.pl";
    let doc = "if ($x =~ /\\bfoo\\b/) {\n}\n";
    let mut harness = LspHarness::new();
    open_ready(&mut harness, uri, doc)?;

    let result = hover_at(&mut harness, uri, 0, 11);
    let val = hover_value(&result).ok_or("Expected hover contents for word boundary")?;
    assert_canonical_regex_hover(&val);
    Ok(())
}

#[test]
fn test_hover_regex_negated_char_class() -> TestResult {
    let uri = "file:///regex_negated_class.pl";
    let doc = "my $x = /[^a-z]+/;\n";
    let mut harness = LspHarness::new();
    open_ready(&mut harness, uri, doc)?;

    let result = hover_at(&mut harness, uri, 0, 9);
    let val = hover_value(&result).ok_or("Expected hover contents for char class")?;
    assert_canonical_regex_hover(&val);
    Ok(())
}

#[test]
fn test_hover_qr_operator() -> TestResult {
    let uri = "file:///regex_qr.pl";
    let doc = "my $re = qr/\\d{2,4}/;\n";
    let mut harness = LspHarness::new();
    open_ready(&mut harness, uri, doc)?;

    let result = hover_at(&mut harness, uri, 0, 12);
    let val = hover_value(&result).ok_or("Expected hover contents for qr//")?;
    assert!(
        val.contains("Quote regex") || val.contains("Pattern:"),
        "Hover should describe qr// from canonical analysis; got: {val}"
    );
    Ok(())
}
