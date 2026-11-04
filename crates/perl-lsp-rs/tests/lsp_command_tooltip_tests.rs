//! Wire proofs for non-CodeLens LSP 3.18 `Command.tooltip` coverage.
//!
//! CodeLens receipts remain in `lsp_codelens_tests.rs`. This file covers every
//! other reachable production Command producer and the surfaces that cannot
//! serialize an LSP Command.

mod support;

use serde_json::{Value, json};
use support::lsp_harness::LspHarness;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

fn lsp_command_objects<'a>(value: &'a Value, out: &mut Vec<&'a Value>) {
    match value {
        Value::Object(map) => {
            let title = map.get("title").and_then(Value::as_str);
            let command = map.get("command").and_then(Value::as_str);
            if title.is_some() && command.is_some() {
                out.push(value);
            }
            for child in map.values() {
                lsp_command_objects(child, out);
            }
        }
        Value::Array(items) => {
            for child in items {
                lsp_command_objects(child, out);
            }
        }
        _ => {}
    }
}

fn assert_command_objects_carry_tooltip(value: &Value) -> TestResult {
    let mut commands = Vec::new();
    lsp_command_objects(value, &mut commands);
    for command in commands {
        let title = command.get("title").and_then(Value::as_str);
        let tooltip = command.get("tooltip").and_then(Value::as_str);
        let tooltip = tooltip.ok_or_else(|| format!("LSP Command missing tooltip: {command}"))?;
        assert!(!tooltip.is_empty(), "Command.tooltip must be non-empty: {command}");
        assert_ne!(Some(tooltip), title, "Command.tooltip must not replace title: {command}");
    }
    Ok(())
}

fn command_by_id<'a>(actions: &'a [Value], command_id: &str) -> TestResult<&'a Value> {
    actions
        .iter()
        .find_map(|action| {
            let command = action.get("command")?;
            (command.get("command").and_then(Value::as_str) == Some(command_id)).then_some(command)
        })
        .ok_or_else(|| format!("missing command {command_id} in {actions:?}").into())
}

#[test]
fn generate_test_command_includes_lsp_318_tooltip() -> TestResult {
    let mut harness = LspHarness::new();
    harness.initialize(None)?;
    harness.open(
        "file:///generate-test.pl",
        "sub calculate {\n    my ($a, $b) = @_;\n    return $a + $b;\n}\n",
    )?;

    let actions = harness.request(
        "textDocument/codeAction",
        json!({
            "textDocument": { "uri": "file:///generate-test.pl" },
            "range": {
                "start": { "line": 0, "character": 0 },
                "end": { "line": 3, "character": 1 }
            },
            "context": { "diagnostics": [], "triggerKind": 1 }
        }),
    )?;
    let actions = actions.as_array().ok_or("code action result must be an array")?;
    let command = command_by_id(actions, "perl.generateTest")?;

    assert_eq!(command.get("title").and_then(Value::as_str), Some("Generate test"));
    assert_eq!(command.get("command").and_then(Value::as_str), Some("perl.generateTest"));
    assert_eq!(
        command.get("tooltip").and_then(Value::as_str),
        Some("Insert a Test::More skeleton for this subroutine")
    );
    assert_eq!(command.pointer("/arguments/0/name").and_then(Value::as_str), Some("calculate"));
    assert!(
        command
            .pointer("/arguments/0/test")
            .and_then(Value::as_str)
            .is_some_and(|test| test.contains("calculate")),
        "generate-test arguments must keep the subroutine skeleton: {command}"
    );
    assert_command_objects_carry_tooltip(&Value::Array(actions.clone()))?;
    Ok(())
}

#[test]
fn explain_diagnostic_command_includes_lsp_318_tooltip() -> TestResult {
    let mut harness = LspHarness::new();
    harness.initialize(None)?;
    harness.open("file:///explain.pl", "use Missing::Payload;\n")?;

    let actions = harness.request(
        "textDocument/codeAction",
        json!({
            "textDocument": { "uri": "file:///explain.pl" },
            "range": {
                "start": { "line": 0, "character": 0 },
                "end": { "line": 0, "character": 20 }
            },
            "context": {
                "diagnostics": [{
                    "range": {
                        "start": { "line": 0, "character": 4 },
                        "end": { "line": 0, "character": 20 }
                    },
                    "severity": 2,
                    "code": "PL701",
                    "source": "perl-lsp",
                    "message": "Module 'Missing::Payload' not found"
                }],
                "triggerKind": 1
            }
        }),
    )?;
    let actions = actions.as_array().ok_or("code action result must be an array")?;
    let command = command_by_id(actions, "perl-lsp.explainDiagnostic")?;

    assert_eq!(command.get("title").and_then(Value::as_str), Some("Explain this diagnostic"));
    assert_eq!(command.get("command").and_then(Value::as_str), Some("perl-lsp.explainDiagnostic"));
    assert_eq!(
        command.get("tooltip").and_then(Value::as_str),
        Some("Show why this diagnostic was produced and its claim boundary")
    );
    assert_eq!(
        command.pointer("/arguments/0/provider").and_then(Value::as_str),
        Some("diagnostics")
    );
    assert_command_objects_carry_tooltip(&Value::Array(actions.clone()))?;
    Ok(())
}

#[test]
fn code_action_documentation_commands_include_lsp_318_tooltip() -> TestResult {
    let mut harness = LspHarness::new();
    let init = harness.initialize(Some(json!({
        "textDocument": {
            "codeAction": {
                "documentationSupport": true
            }
        }
    })))?;
    let docs = init
        .pointer("/capabilities/codeActionProvider/documentation")
        .cloned()
        .ok_or_else(|| format!("expected CodeActionOptions.documentation: {init}"))?;
    let entries = docs.as_array().ok_or("documentation must be an array")?;
    assert_eq!(entries.len(), 3);

    let expected = [
        (
            "quickfix",
            "Explain Perl quick fixes",
            "Show why Perl quick-fix code actions are offered",
        ),
        ("refactor", "Explain Perl refactors", "Show why Perl refactor code actions are offered"),
        (
            "source.fixAll",
            "Explain Perl fix-all actions",
            "Show why Perl source.fixAll actions are offered",
        ),
    ];
    for (kind, title, tooltip) in expected {
        let command = entries
            .iter()
            .find(|entry| entry.get("kind").and_then(Value::as_str) == Some(kind))
            .and_then(|entry| entry.get("command"))
            .ok_or_else(|| format!("missing documentation command for {kind}: {entries:?}"))?;
        assert_eq!(command.get("title").and_then(Value::as_str), Some(title));
        assert_eq!(
            command.get("command").and_then(Value::as_str),
            Some("perl.explainProviderDecision")
        );
        assert_eq!(command.get("tooltip").and_then(Value::as_str), Some(tooltip));
        assert_eq!(
            command.pointer("/arguments/0/receipt_id").and_then(Value::as_str),
            Some(
                "docs/specs/PLSP-SPEC-0029-lsp-318-conformance-boundary.md#code-action-documentation"
            )
        );
    }
    assert_command_objects_carry_tooltip(&docs)?;
    Ok(())
}

#[test]
fn completion_and_document_link_do_not_produce_lsp_commands() -> TestResult {
    let mut harness = LspHarness::new();
    harness.initialize(None)?;
    harness.open("file:///no-command.pl", "use strict;\nprint 1;\n")?;

    let completion = harness.request(
        "textDocument/completion",
        json!({
            "textDocument": { "uri": "file:///no-command.pl" },
            "position": { "line": 1, "character": 5 },
            "context": { "triggerKind": 1 }
        }),
    )?;
    let mut commands = Vec::new();
    lsp_command_objects(&completion, &mut commands);
    assert!(commands.is_empty(), "completion items do not carry LSP Command objects: {commands:?}");

    let links = harness.request(
        "textDocument/documentLink",
        json!({
            "textDocument": { "uri": "file:///no-command.pl" }
        }),
    )?;
    commands.clear();
    lsp_command_objects(&links, &mut commands);
    assert!(commands.is_empty(), "document links do not carry LSP Command objects: {commands:?}");
    Ok(())
}

#[test]
fn inline_completion_items_do_not_produce_lsp_commands() -> TestResult {
    let mut harness = LspHarness::new();
    harness.initialize(None)?;
    harness.open("file:///inline-no-command.pl", "use ")?;

    let result = harness.request(
        "textDocument/inlineCompletion",
        json!({
            "textDocument": { "uri": "file:///inline-no-command.pl" },
            "position": { "line": 0, "character": 4 },
            "context": { "triggerKind": 1 }
        }),
    )?;
    let mut commands = Vec::new();
    lsp_command_objects(&result, &mut commands);
    assert!(
        commands.is_empty(),
        "inline completion items currently leave Command unset: {commands:?}"
    );
    Ok(())
}
