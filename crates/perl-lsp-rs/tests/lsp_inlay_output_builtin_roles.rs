//! Exact-process coverage for conservative output-builtin inlay roles (#18132).

mod support;

use serde_json::{Value, json};
use serial_test::serial;
use support::lsp_harness::LspHarness;

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn inlay_hint_caps() -> Value {
    json!({
        "textDocument": {
            "inlayHint": {
                "dynamicRegistration": true,
                "resolveSupport": { "properties": ["tooltip"] }
            }
        }
    })
}

fn request_hints(harness: &mut LspHarness, uri: &str) -> Result<Value, Box<dyn std::error::Error>> {
    harness
        .request(
            "textDocument/inlayHint",
            json!({
                "textDocument": { "uri": uri },
                "range": {
                    "start": { "line": 0, "character": 0 },
                    "end": { "line": 20, "character": 0 }
                }
            }),
        )
        .map_err(Into::into)
}

fn role_rows(hints: &[Value]) -> Vec<(&str, u64)> {
    hints
        .iter()
        .filter_map(|hint| {
            let label = perl_lsp_rs_core::providers::inlay_hints::inlay_hint_label_str(hint)?;
            if !matches!(label, "filehandle:" | "format:") {
                return None;
            }
            let line = hint.get("position")?.get("line")?.as_u64()?;
            Some((label, line))
        })
        .collect()
}

#[test]
#[serial]
fn output_builtin_roles_are_fail_closed_and_source_backed() -> TestResult {
    let mut harness = LspHarness::new();
    harness.initialize(Some(inlay_hint_caps()))?;

    let uri = "file:///output_builtin_roles.pl";
    let source = r#"print $value;
print($fh, "x");
print STDERR "x";
printf "%s", $value;
printf $out "%s", $value;
"#;
    harness.open(uri, source)?;

    let result = request_hints(&mut harness, uri)?;
    let hints = result.as_array().ok_or("inlayHint should return an array")?;
    let roles = role_rows(hints);

    assert_eq!(
        roles,
        vec![
            ("filehandle:", 2),
            ("format:", 3),
            ("filehandle:", 4),
            ("format:", 4),
        ],
        "ordinary and flattened print calls must not receive guessed roles"
    );
    Ok(())
}

#[test]
#[serial]
fn parameter_hint_configuration_suppresses_output_roles() -> TestResult {
    let mut harness = LspHarness::new();
    harness.initialize(Some(inlay_hint_caps()))?;
    harness.notify(
        "workspace/didChangeConfiguration",
        json!({
            "settings": {
                "perl": {
                    "inlayHints": {
                        "enabled": true,
                        "parameterHints": false,
                        "typeHints": true
                    }
                }
            }
        }),
    );

    let uri = "file:///output_builtin_roles_disabled.pl";
    harness.open(uri, r#"print STDERR "x"; printf $out "%s", $value;"#)?;

    let result = request_hints(&mut harness, uri)?;
    let hints = result.as_array().ok_or("inlayHint should return an array")?;
    assert!(
        hints.iter().all(|hint| hint.get("kind").and_then(Value::as_u64) != Some(2)),
        "parameterHints=false must suppress output roles: {hints:#?}"
    );
    Ok(())
}
