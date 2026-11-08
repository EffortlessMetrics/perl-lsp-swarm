//! Regression (#19206): a `#` inside a string or regex must not make the
//! definition/completion comment guard treat the rest of the line as a
//! comment. The guard adjudicates a naive `#`-earlier-on-line hit with the
//! snapshot `SourceRegionIndex`; a real trailing comment still blocks.

mod support;

use serde_json::{Value, json};
use support::lsp_harness::LspHarness;

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn pos_of(source: &str, needle: &str) -> Result<(u32, u32), String> {
    for (line_idx, line) in source.lines().enumerate() {
        if let Some(idx) = line.find(needle) {
            return Ok((line_idx as u32, idx as u32));
        }
    }
    Err(format!("needle {needle:?} not found"))
}

fn assert_definition_at_declaration(result: &Value, label: &str) {
    let location = match result {
        Value::Array(items) => items.first(),
        Value::Object(_) => Some(result),
        _ => None,
    };
    let location = location.unwrap_or_else(|| panic!("{label} must resolve, got: {result}"));
    let line = location
        .pointer("/range/start/line")
        .or_else(|| location.pointer("/targetRange/start/line"))
        .and_then(Value::as_u64)
        .unwrap_or_else(|| panic!("{label} location has no start line, got: {result}"));
    assert_eq!(line, 0, "{label} must land on the declaration (line 0), got: {result}");
}

fn definition_resolves_to_declaration(doc: &str, uri: &str, label: &str) -> TestResult {
    let mut harness = LspHarness::new();
    harness.initialize(None)?;
    harness.open_document(uri, doc)?;
    harness.barrier();

    let (line, character) = pos_of(doc, "target_sub();")?;
    let result = harness.request(
        "textDocument/definition",
        json!({
            "textDocument": {"uri": uri},
            "position": {"line": line, "character": character + 2}
        }),
    )?;
    assert_definition_at_declaration(&result, label);
    Ok(())
}

#[test]
fn definition_after_hash_in_string_resolves() -> TestResult {
    definition_resolves_to_declaration(
        "sub target_sub { return 1; }\nmy $sep = \"#\"; target_sub();\n",
        "file:///hash_string.pl",
        "definition after \"#\" string",
    )
}

#[test]
fn definition_after_hash_in_single_quotes_resolves() -> TestResult {
    definition_resolves_to_declaration(
        "sub target_sub { return 1; }\nmy $sep = '#'; target_sub();\n",
        "file:///hash_single_quote.pl",
        "definition after '#' string",
    )
}

#[test]
fn definition_after_hash_in_regex_resolves() -> TestResult {
    definition_resolves_to_declaration(
        "sub target_sub { return 1; }\nmy $s = \"ab\"; $s =~ s/#.*//; target_sub();\n",
        "file:///hash_regex.pl",
        "definition after regex #",
    )
}

#[test]
fn definition_inside_real_comment_still_refused() -> TestResult {
    let doc = "sub target_sub { return 1; }\nmy $x = 1; # target_sub();\n";
    let mut harness = LspHarness::new();
    harness.initialize(None)?;
    harness.open_document("file:///real_comment.pl", doc)?;
    harness.barrier();

    let (line, character) = pos_of(doc, "# target_sub")?;
    let result = harness.request(
        "textDocument/definition",
        json!({
            "textDocument": {"uri": "file:///real_comment.pl"},
            "position": {"line": line, "character": character + 4}
        }),
    )?;
    assert!(
        result.is_null() || result.as_array().is_some_and(|items| items.is_empty()),
        "definition inside a real comment must refuse, got: {result}"
    );
    Ok(())
}

#[test]
fn completion_after_hash_in_string_offers_symbols() -> TestResult {
    let doc = "sub target_sub { return 1; }\nmy $sep = \"#\"; targ\n";
    let mut harness = LspHarness::new();
    harness.initialize(None)?;
    harness.open_document("file:///hash_completion.pl", doc)?;
    harness.barrier();

    // Anchored needle: plain "targ" would first match `target_sub` on line 0.
    let (line, character) = pos_of(doc, "; targ")?;
    let result = harness.request(
        "textDocument/completion",
        json!({
            "textDocument": {"uri": "file:///hash_completion.pl"},
            "position": {"line": line, "character": character + 6}
        }),
    )?;
    let items = result
        .get("items")
        .and_then(Value::as_array)
        .or_else(|| result.as_array())
        .cloned()
        .unwrap_or_default();
    assert!(
        items.iter().any(|item| item.get("label").and_then(Value::as_str) == Some("target_sub")),
        "completion after \"#\" string must offer target_sub, got: {result}"
    );
    Ok(())
}

#[test]
fn completion_inside_real_comment_still_empty() -> TestResult {
    let doc = "sub target_sub { return 1; }\n# targ\n";
    let mut harness = LspHarness::new();
    harness.initialize(None)?;
    harness.open_document("file:///comment_completion.pl", doc)?;
    harness.barrier();

    // Anchored needle: plain "targ" would first match `target_sub` on line 0.
    let (line, character) = pos_of(doc, "# targ")?;
    let result = harness.request(
        "textDocument/completion",
        json!({
            "textDocument": {"uri": "file:///comment_completion.pl"},
            "position": {"line": line, "character": character + 6}
        }),
    )?;
    let items = result
        .get("items")
        .and_then(Value::as_array)
        .or_else(|| result.as_array())
        .cloned()
        .unwrap_or_default();
    assert!(items.is_empty(), "completion inside a real comment must be empty, got: {result}");
    Ok(())
}
