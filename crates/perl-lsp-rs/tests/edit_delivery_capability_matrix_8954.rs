//! Edit delivery capability matrix and Kate-shaped regression (#8954).

mod support;

use serde_json::{Value, json};
use support::lsp_harness::LspHarness;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

fn apply_edit_safe_delete_source() -> &'static str {
    r#"package ApplyEdit::Pilot;
use strict;
use warnings;

sub reset {
    return 1;
}

1;
"#
}

/// Kate-shaped profile stand-in until #8939 lands: no applyEdit, no metadata,
/// ordinary inline `WorkspaceEdit` shape only.
fn kate_shaped_client_caps() -> Value {
    json!({
        "textDocument": {
            "codeAction": {
                "codeActionLiteralSupport": {
                    "codeActionKind": {
                        "valueSet": ["quickfix", "refactor", "source"]
                    }
                }
            }
        },
        "workspace": {
            "workspaceEdit": {}
        }
    })
}

#[test]
fn kate_shaped_execute_command_safe_delete_refuses_without_silent_noop() -> TestResult {
    let uri = "file:///workspace/lib/ApplyEdit/Pilot.pm";
    let source = apply_edit_safe_delete_source();
    let (line, character) = (4_u32, 4_u32);

    let mut harness = LspHarness::new_raw();
    harness.initialize_ready("file:///workspace", Some(kate_shaped_client_caps()))?;
    harness.drain_server_requests(250);
    harness.open(uri, source)?;
    let result = harness.request(
        "workspace/executeCommand",
        json!({
            "command": "perl.safeDeleteSymbol",
            "arguments": [{
                "textDocument": { "uri": uri },
                "position": { "line": line, "character": character }
            }]
        }),
    )?;

    assert_ne!(
        result.get("live_symbol_delete_enabled").and_then(Value::as_bool),
        Some(true),
        "Kate-shaped executeCommand must not report live delete without applyEdit: {result}"
    );
    assert_eq!(result.get("returned_workspace_edit_count").and_then(Value::as_u64), Some(0));
    assert_eq!(
        result.get("edit_delivery_disposition").and_then(Value::as_str),
        Some("refused_unsupported_client"),
        "Kate-shaped executeCommand must record explicit refusal disposition: {result}"
    );
    assert_absent(&result, "/apply_edit_requested")?;
    assert!(
        !harness
            .drain_server_requests(250)
            .iter()
            .any(|m| m.get("method").and_then(Value::as_str) == Some("workspace/applyEdit")),
        "Kate-shaped profile must not emit workspace/applyEdit"
    );
    Ok(())
}

fn assert_absent(value: &Value, pointer: &str) -> TestResult {
    assert!(value.pointer(pointer).is_none(), "{pointer} must be absent from {value}");
    Ok(())
}
