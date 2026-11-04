//! Regression tests for #17296: hovering a built-in or a call-site package
//! name inside a subroutine must not return the enclosing subroutine's card.

mod support;

use serde_json::json;
use support::lsp_harness::LspHarness;

type TestResult = Result<(), Box<dyn std::error::Error>>;

const DOC: &str = "\
package WidgetGarden;
sub new { my ($class, %args) = @_; return bless \\%args, $class; }
package main;
sub run {
    my $garden = WidgetGarden->new(name => 'demo');
    my %counts = (a => 1, b => 2);
    my $kcount = keys %counts;
    return $garden;
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
            "textDocument": {"uri": "file:///hover_17296.pm"},
            "position": {"line": line, "character": character}
        }),
    )?;
    if result.is_null() {
        return Ok(None);
    }
    Ok(hover_value(&result))
}

#[test]
fn hover_builtin_and_package_callsite_do_not_return_enclosing_sub_card() -> TestResult {
    let mut harness = LspHarness::new();
    harness.initialize(None)?;
    harness.open_document("file:///hover_17296.pm", DOC)?;
    harness.barrier();

    // `keys` at line 6, chars 17..21: not the `sub run` card.
    let keys = hover_at(&mut harness, 6, 18)?;
    if let Some(val) = &keys {
        assert!(
            !val.contains("`sub run`") && !val.contains("**Subroutine**"),
            "hover on `keys` must not present the enclosing sub, got: {val}"
        );
    }

    // `WidgetGarden` at line 4, chars 17..29: a package call site, not `sub run`.
    let pkg = hover_at(&mut harness, 4, 20)?;
    if let Some(val) = &pkg {
        assert!(
            !val.contains("`sub run`") && !val.contains("**Subroutine**"),
            "hover on `WidgetGarden->new` must not present the enclosing sub, got: {val}"
        );
    }

    // Control: hovering the `run` name at its definition still answers.
    let run = hover_at(&mut harness, 3, 5)?;
    let run_val = run.ok_or("hover on `sub run` definition should return a card")?;
    assert!(run_val.contains("run"), "definition hover should name the sub, got: {run_val}");
    Ok(())
}
