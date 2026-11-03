//! Regression tests for #17297: `for my $item (...)` loop variables are
//! indexed, so hover at the declaration and at use sites returns the
//! variable card instead of the enclosing subroutine's card.

mod support;

use serde_json::json;
use support::lsp_harness::LspHarness;

type TestResult = Result<(), Box<dyn std::error::Error>>;

const DOC: &str = "\
package WidgetGarden::Reducer;
sub fold {
    my ($self, $items, $initial, $combiner) = @_;
    my $acc = $initial;
    for my $item (@$items) {
        $acc = $combiner->($acc, $item);
    }
    return $acc;
}
";

fn hover_value(result: &serde_json::Value) -> Option<String> {
    result
        .get("contents")
        .and_then(|c| c.get("value"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

fn hover_at(
    harness: &mut LspHarness,
    line: u32,
    character: u32,
) -> Result<Option<String>, Box<dyn std::error::Error>> {
    let result = harness.request(
        "textDocument/hover",
        json!({
            "textDocument": {"uri": "file:///hover_17297.pm"},
            "position": {"line": line, "character": character}
        }),
    )?;
    if result.is_null() {
        return Ok(None);
    }
    Ok(hover_value(&result))
}

#[test]
fn hover_foreach_my_variable_decl_and_use_show_variable_card() -> TestResult {
    let mut harness = LspHarness::new();
    harness.initialize(None)?;
    harness.open_document("file:///hover_17297.pm", DOC)?;
    harness.barrier();

    // Declaration: `for my $item` at line 4, chars 11..16.
    let decl = hover_at(&mut harness, 4, 12)?.ok_or("decl hover should return a card")?;
    assert!(
        decl.contains("$item") && !decl.contains("`sub fold`"),
        "decl hover should be the $item variable card, got: {decl}"
    );

    // Use: `$item` in `$combiner->($acc, $item)` at line 5, chars 33..38.
    let use_site = hover_at(&mut harness, 5, 34)?.ok_or("use hover should return a card")?;
    assert!(
        use_site.contains("$item") && !use_site.contains("`sub fold`"),
        "use hover should be the $item variable card, got: {use_site}"
    );

    // Definition from the use site must land on the `for my` declaration
    // (line 4), not on the enclosing sub or nowhere.
    let definition = harness.request(
        "textDocument/definition",
        json!({
            "textDocument": {"uri": "file:///hover_17297.pm"},
            "position": {"line": 5, "character": 34}
        }),
    )?;
    let def_line = definition
        .pointer("/0/range/start/line")
        .or_else(|| definition.pointer("/range/start/line"))
        .and_then(|l| l.as_u64())
        .ok_or("definition should return a location")?;
    assert_eq!(
        def_line, 4,
        "definition should land on the for-my declaration line, got: {definition}"
    );

    // References from the declaration must include the use site (line 5).
    let references = harness.request(
        "textDocument/references",
        json!({
            "textDocument": {"uri": "file:///hover_17297.pm"},
            "position": {"line": 4, "character": 12},
            "context": {"includeDeclaration": true}
        }),
    )?;
    let ref_lines: Vec<u64> = references
        .as_array()
        .ok_or("references should return an array")?
        .iter()
        .filter_map(|r| r.get("range")?.get("start")?.get("line")?.as_u64())
        .collect();
    assert!(
        ref_lines.contains(&5),
        "references should include the use at line 5, got lines: {ref_lines:?}"
    );
    Ok(())
}
