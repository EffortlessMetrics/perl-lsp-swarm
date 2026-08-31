//! End-to-end hover routing tests for #4967.
//!
//! The generic symbol/token/builtin fallback may only answer in proven code.
//! Identifier-shaped text inside comments, POD, literals, quote-likes, heredocs,
//! `__DATA__`, and recovery-ambiguous input must produce `null` (fail closed),
//! while semantic islands (pragmas, module targets, special variables, regex
//! constructs) and proven-code fallbacks keep working.

mod support;

use serde_json::Value;
use support::lsp_harness::LspHarness;

type TestResult = Result<(), Box<dyn std::error::Error>>;

const URI: &str = "file:///hover_routing_4967.pl";

/// Byte-accurate position of `needle` (first occurrence), in UTF-16 offsets.
fn position_of(doc: &str, needle: &str) -> Result<Value, String> {
    let (line_idx, line) = doc
        .lines()
        .enumerate()
        .find(|(_, line)| line.contains(needle))
        .ok_or_else(|| format!("needle `{needle}` not found in document"))?;
    let byte_offset =
        line.find(needle).ok_or_else(|| format!("needle `{needle}` missing from its line"))?;
    let character: usize = line[..byte_offset].chars().map(char::len_utf16).sum();
    let line_number = u32::try_from(line_idx).map_err(|e| e.to_string())?;
    let character = u32::try_from(character).map_err(|e| e.to_string())?;
    Ok(serde_json::json!({
        "textDocument": { "uri": URI },
        "position": { "line": line_number, "character": character }
    }))
}

fn hover_markdown(hover: &Value) -> Option<String> {
    let contents = hover.get("contents")?;
    contents
        .as_str()
        .map(str::to_string)
        .or_else(|| contents.get("value").and_then(Value::as_str).map(str::to_string))
}

/// Helper: run one hover request against an opened document.
fn hover(
    harness: &mut LspHarness,
    doc: &str,
    needle: &str,
) -> Result<Value, Box<dyn std::error::Error>> {
    let params = position_of(doc, needle).map_err(Box::<dyn std::error::Error>::from)?;
    harness
        .request("textDocument/hover", params)
        .map_err(|e| -> Box<dyn std::error::Error> { e.into() })
}

/// Negative: identifier-shaped prose inside a comment gets no generic card.
#[test]
fn hover_suppresses_generic_fallback_in_comment() -> TestResult {
    let doc = "sub real_code { 1 }\n\n# call process_data_helper for details\nmy $x = 1;\n";
    let mut harness = LspHarness::new();
    harness.initialize(None).map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
    harness.open_document(URI, doc).map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;

    let result = hover(&mut harness, doc, "process_data_helper")?;
    assert!(result.is_null(), "generic fallback must fail closed inside a comment, got: {result}");
    Ok(())
}

/// Negative: identifier-shaped text inside POD gets no generic card.
#[test]
fn hover_suppresses_generic_fallback_in_pod() -> TestResult {
    let doc = "=pod\n\nSee process_data_helper documentation here.\n\n=cut\n\nmy $x = 1;\n";
    let mut harness = LspHarness::new();
    harness.initialize(None).map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
    harness.open_document(URI, doc).map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;

    let result = hover(&mut harness, doc, "process_data_helper")?;
    assert!(result.is_null(), "generic fallback must fail closed inside POD, got: {result}");
    Ok(())
}

/// Negative: a builtin-looking word inside a string gets no builtin card.
#[test]
fn hover_suppresses_builtin_card_inside_string() -> TestResult {
    let doc = "my $tip = 'please sprintf this value';\nmy $y = 2;\n";
    let mut harness = LspHarness::new();
    harness.initialize(None).map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
    harness.open_document(URI, doc).map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;

    let result = hover(&mut harness, doc, "sprintf")?;
    assert!(
        result.is_null(),
        "builtin hover must not appear inside a string literal, got: {result}"
    );
    Ok(())
}

/// Negative: identifier inside a qw() list gets no generic card.
#[test]
fn hover_suppresses_generic_fallback_in_qw_list() -> TestResult {
    let doc = "use Exporter 'import';\nour @EXPORT_OK = qw(process_data_helper other_thing);\n";
    let mut harness = LspHarness::new();
    harness.initialize(None).map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
    harness.open_document(URI, doc).map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;

    let result = hover(&mut harness, doc, "process_data_helper")?;
    assert!(
        result.is_null(),
        "qw() members are literal fragments, not code symbols, got: {result}"
    );
    Ok(())
}

/// Negative: identifier inside a heredoc body gets no generic card.
#[test]
fn hover_suppresses_generic_fallback_in_heredoc() -> TestResult {
    let doc = "my $sql = <<\"SQL\";\nSELECT process_data_helper FROM table\nSQL\nmy $z = 3;\n";
    let mut harness = LspHarness::new();
    harness.initialize(None).map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
    harness.open_document(URI, doc).map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;

    let result = hover(&mut harness, doc, "process_data_helper")?;
    assert!(result.is_null(), "heredoc bodies are not proven code, got: {result}");
    Ok(())
}

/// Negative: `__DATA__` payload gets no generic card.
#[test]
fn hover_suppresses_generic_fallback_in_data_section() -> TestResult {
    let doc = "my $w = 4;\n__DATA__\nprocess_data_helper is data here\n";
    let mut harness = LspHarness::new();
    harness.initialize(None).map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
    harness.open_document(URI, doc).map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;

    let result = hover(&mut harness, doc, "process_data_helper")?;
    assert!(result.is_null(), "__DATA__ content is not proven code, got: {result}");
    Ok(())
}

/// Negative: unclosed literal (recovery-ambiguous) fails closed.
#[test]
fn hover_suppresses_generic_fallback_in_recovery_input() -> TestResult {
    let doc = "my $unclosed = \"starts and never ends process_data_helper\n";
    let mut harness = LspHarness::new();
    harness.initialize(None).map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
    harness.open_document(URI, doc).map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;

    let result = hover(&mut harness, doc, "process_data_helper")?;
    assert!(
        result.is_null(),
        "recovery-ambiguous input must not produce a generic code token, got: {result}"
    );
    Ok(())
}

/// Positive control: a code call after a string still hovers.
#[test]
fn hover_keeps_code_call_after_string() -> TestResult {
    let doc = "my $message = \"see #123 notes\";\nfoo();\n";
    let mut harness = LspHarness::new();
    harness.initialize(None).map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
    harness.open_document(URI, doc).map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;

    let result = hover(&mut harness, doc, "foo()")?;
    assert!(!result.is_null(), "proven-code fallback must still answer, got null");
    let markdown = hover_markdown(&result).unwrap_or_default();
    assert!(markdown.contains("foo"), "hover must mention the token `foo`, got: {result}");
    Ok(())
}

/// Positive control: comments containing apostrophes do not poison later code hover.
#[test]
fn hover_keeps_code_hover_after_comment_with_quotes() -> TestResult {
    let doc = "# don't worry about \"quotes\" here\nmy $value_after = 42;\n";
    let mut harness = LspHarness::new();
    harness.initialize(None).map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
    harness.open_document(URI, doc).map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;

    let result = hover(&mut harness, doc, "value_after")?;
    assert!(!result.is_null(), "code hover after tricky comment must survive");
    let markdown = hover_markdown(&result).unwrap_or_default();
    assert!(markdown.contains("value_after"), "hover must mention the variable, got: {result}");
    Ok(())
}

/// Positive control: pragma hover on a `use strict` target still works.
#[test]
fn hover_keeps_pragma_island() -> TestResult {
    let doc = "use strict;\nuse warnings;\nmy $p = 1;\n";
    let mut harness = LspHarness::new();
    harness.initialize(None).map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
    harness.open_document(URI, doc).map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;

    let result = hover(&mut harness, doc, "strict")?;
    assert!(!result.is_null(), "pragma hover is a semantic island and must survive");
    let markdown = hover_markdown(&result).unwrap_or_default();
    assert!(
        markdown.to_lowercase().contains("pragma") || markdown.contains("strict"),
        "expected pragma documentation, got: {result}"
    );
    Ok(())
}

/// Positive control: builtin hover on a real call in code still works.
#[test]
fn hover_keeps_builtin_in_proven_code() -> TestResult {
    let doc = "my $len = length('abc');\n";
    let mut harness = LspHarness::new();
    harness.initialize(None).map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
    harness.open_document(URI, doc).map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;

    let result = hover(&mut harness, doc, "length")?;
    let markdown = hover_markdown(&result).unwrap_or_default();
    assert!(
        markdown.contains("Built-in") || markdown.contains("length"),
        "builtin hover in code must survive, got: {result}"
    );
    Ok(())
}

/// Positive control: keyword hover in proven code still works.
#[test]
fn hover_keeps_keyword_in_proven_code() -> TestResult {
    let doc = "for my $i (1 .. 3) { last if $i == 2; }\n";
    let mut harness = LspHarness::new();
    harness.initialize(None).map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
    harness.open_document(URI, doc).map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;

    let result = hover(&mut harness, doc, "last")?;
    assert!(!result.is_null(), "keyword hover in code must survive, got null");
    Ok(())
}

/// Negative (review 5062479350 leak 1): regex-shaped text inside a comment has
/// no RegexLike region evidence and must not produce a Regex Pattern card.
#[test]
fn hover_suppresses_regex_island_in_comment() -> TestResult {
    let doc = "# match /ab+c/ please\nmy $x = 1;\n";
    let mut harness = LspHarness::new();
    harness.initialize(None).map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
    harness.open_document(URI, doc).map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;

    let result = hover(&mut harness, doc, "ab")?;
    assert!(result.is_null(), "regex heuristic must fail closed inside a comment, got: {result}");
    Ok(())
}

/// Negative (review 5062479350 leak 1): regex-shaped text inside a string
/// literal must not produce a Regex Pattern card.
#[test]
fn hover_suppresses_regex_island_in_string() -> TestResult {
    let doc = "my $tip = \"match /ab+c/ please\";\nmy $y = 2;\n";
    let mut harness = LspHarness::new();
    harness.initialize(None).map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
    harness.open_document(URI, doc).map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;

    let result = hover(&mut harness, doc, "ab")?;
    assert!(
        result.is_null(),
        "regex heuristic must fail closed inside a string literal, got: {result}"
    );
    Ok(())
}

/// Negative (review 5062479350 leak 2): hovering non-code INSIDE a named sub
/// must not leak the enclosing sub's generic Subroutine card through the
/// containment-based analyzer fallback.
#[test]
fn hover_suppresses_subroutine_containment_from_comment_inside_sub() -> TestResult {
    let doc = "sub outer {\n    # process_data_helper handles the request\n    my $inner = 1;\n}\nmy $after = 2;\n";
    let mut harness = LspHarness::new();
    harness.initialize(None).map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
    harness.open_document(URI, doc).map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;

    let result = hover(&mut harness, doc, "process_data_helper")?;
    assert!(
        result.is_null(),
        "containment fallback must not answer with the enclosing sub inside a comment, got: {result}"
    );
    Ok(())
}

/// Negative (review 5062479350 leak 2): same containment leak shape through a
/// string literal inside the sub body.
#[test]
fn hover_suppresses_subroutine_containment_from_string_inside_sub() -> TestResult {
    let doc = "sub outer {\n    my $greeting = \"process_data_helper ready\";\n}\nmy $after = 2;\n";
    let mut harness = LspHarness::new();
    harness.initialize(None).map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
    harness.open_document(URI, doc).map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;

    let result = hover(&mut harness, doc, "process_data_helper")?;
    assert!(
        result.is_null(),
        "containment fallback must not answer with the enclosing sub inside a string, got: {result}"
    );
    Ok(())
}

/// Positive control: a genuine regex literal in code keeps its island card.
#[test]
fn hover_keeps_regex_island_in_proven_code() -> TestResult {
    let doc = "if ($x =~ /\\d+/) {\n}\n";
    let mut harness = LspHarness::new();
    harness.initialize(None).map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
    harness.open_document(URI, doc).map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;

    let result = hover(&mut harness, doc, "\\d")?;
    assert!(!result.is_null(), "regex island must survive in genuine regex code, got null");
    let markdown = hover_markdown(&result).unwrap_or_default();
    assert!(
        markdown.contains("Regex") || markdown.to_lowercase().contains("digit"),
        "expected regex explanation for a real regex literal, got: {result}"
    );
    Ok(())
}

/// Positive control: hovering the declared sub name still returns the
/// Subroutine card.
#[test]
fn hover_keeps_subroutine_hover_on_declaration() -> TestResult {
    let doc = "sub outer { my $inner = 1; }\nmy $after = 2;\n";
    let mut harness = LspHarness::new();
    harness.initialize(None).map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
    harness.open_document(URI, doc).map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;

    let result = hover(&mut harness, doc, "outer")?;
    assert!(!result.is_null(), "sub declaration hover must survive, got null");
    let markdown = hover_markdown(&result).unwrap_or_default();
    assert!(
        markdown.contains("Subroutine"),
        "expected Subroutine card on the declaration, got: {result}"
    );
    Ok(())
}

/// Positive control: proven code inside a sub body still hovers — the
/// containment gate must not suppress real code hover.
#[test]
fn hover_keeps_proven_code_hover_inside_sub_body() -> TestResult {
    let doc = "sub outer {\n    my $inner = 1;\n}\n";
    let mut harness = LspHarness::new();
    harness.initialize(None).map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
    harness.open_document(URI, doc).map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;

    let result = hover(&mut harness, doc, "$inner")?;
    assert!(!result.is_null(), "proven-code hover inside a sub body must survive, got null");
    Ok(())
}

/// Positive control: Unicode before the cursor does not change routing.
#[test]
fn hover_unaffected_by_unicode_earlier_on_line() -> TestResult {
    let doc = "# \u{1F389} note with \u{65E5}\u{672C}\u{8A9E} text\nmy $unicode_before = 5;\n";
    let mut harness = LspHarness::new();
    harness.initialize(None).map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
    harness.open_document(URI, doc).map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;

    let result = hover(&mut harness, doc, "unicode_before")?;
    assert!(!result.is_null(), "unicode earlier on the line must not break code hover");
    let markdown = hover_markdown(&result).unwrap_or_default();
    assert!(
        markdown.contains("unicode_before"),
        "hover must mention the variable despite leading unicode, got: {result}"
    );
    Ok(())
}
