//! Focused LSP code-action UX tests.

mod support;

use serde_json::{Value, json};
use support::lsp_harness::LspHarness;

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn find_action_by_title<'a>(actions: &'a [Value], title: &str) -> Option<&'a Value> {
    actions.iter().find(|action| action.get("title").and_then(Value::as_str) == Some(title))
}

#[test]
fn pl410_remove_label_action_is_available_over_lsp() -> TestResult {
    let source = "use v5.40;\nwhile (1) {\n    next MISSING;\n}\n";
    let uri = "file:///pl410.pl";
    let mut harness = LspHarness::new();
    harness.initialize(None)?;
    harness.open(uri, source)?;
    harness.barrier();

    let response = harness.request(
        "textDocument/codeAction",
        json!({
            "textDocument": { "uri": uri },
            "range": {
                "start": { "line": 2, "character": 4 },
                "end": { "line": 2, "character": 16 }
            },
            "context": {
                "diagnostics": [],
                "only": ["quickfix"],
                "triggerKind": 1
            }
        }),
    )?;

    let actions =
        response.as_array().ok_or_else(|| format!("expected actions array: {response}"))?;
    let action = find_action_by_title(actions, "Remove undefined label")
        .ok_or_else(|| format!("missing PL410 remove-label action: {actions:?}"))?;
    assert_eq!(action.get("kind").and_then(Value::as_str), Some("quickfix"));

    let edit = action
        .get("edit")
        .and_then(|edit| edit.get("changes"))
        .and_then(|changes| changes.get(uri))
        .and_then(Value::as_array)
        .and_then(|edits| edits.first())
        .ok_or_else(|| format!("missing edit for {uri}: {action}"))?;
    assert_eq!(edit.pointer("/range/start/line").and_then(Value::as_u64), Some(2));
    assert_eq!(edit.pointer("/range/start/character").and_then(Value::as_u64), Some(8));
    assert_eq!(edit.pointer("/range/end/line").and_then(Value::as_u64), Some(2));
    assert_eq!(edit.pointer("/range/end/character").and_then(Value::as_u64), Some(16));
    assert_eq!(edit.get("newText").and_then(Value::as_str), Some(""));

    Ok(())
}
