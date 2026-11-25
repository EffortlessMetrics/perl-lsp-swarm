//! Seam proofs for push diagnostics (`textDocument/publishDiagnostics`).

#![allow(clippy::expect_used)]
use perl_lsp_rs_core::protocol::schema::{
    Direction, MessageKind, ProtocolSchemaValidator, ProtocolVersion, SchemaError,
    ValidatedMessage, ValidationContext,
};
use serde_json::{Value, json};

const METHOD: &str = "textDocument/publishDiagnostics";

fn context<'a>(direction: Direction, method: Option<&'a str>) -> ValidationContext<'a> {
    ValidationContext { direction, method }
}

fn validate(
    message: &Value,
    direction: Direction,
    method: Option<&str>,
) -> Result<ValidatedMessage, SchemaError> {
    ProtocolSchemaValidator::default().validate(message, context(direction, method))
}

fn notification(params: Value) -> Value {
    json!({"jsonrpc": "2.0", "method": METHOD, "params": params})
}

#[test]
fn publish_diagnostics_accepts_empty_and_populated_batches() -> Result<(), SchemaError> {
    for params in [
        json!({"uri": "file:///workspace/main.pl", "diagnostics": []}),
        json!({
            "uri": "file:///workspace/main.pl",
            "version": 3,
            "diagnostics": [{
                "range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 1}},
                "severity": 1,
                "code": "PL002",
                "source": "perl-lsp",
                "message": "syntax error",
                "tags": [1],
                "relatedInformation": [{
                    "location": {
                        "uri": "file:///workspace/lib.pm",
                        "range": {"start": {"line": 1, "character": 0}, "end": {"line": 1, "character": 4}}
                    },
                    "message": "imported here"
                }],
                "codeDescription": {"href": "https://example.com/rules/PL002"}
            }]
        }),
    ] {
        let validated = validate(&notification(params), Direction::ServerToClient, None)?;
        assert_eq!(validated.method, METHOD);
        assert_eq!(validated.kind, MessageKind::Notification);
        assert_eq!(validated.direction, Direction::ServerToClient);
        assert_eq!(validated.version, ProtocolVersion::Lsp317);
    }
    Ok(())
}

#[test]
fn publish_diagnostics_rejects_wrong_known_fields_at_exact_paths() {
    for (params, path) in [
        (json!({}), "$.params.uri"),
        (json!({"uri": "no-scheme", "diagnostics": []}), "$.params.uri"),
        (
            json!({"uri": "file:///main.pl", "version": "bad", "diagnostics": []}),
            "$.params.version",
        ),
        (json!({"uri": "file:///main.pl"}), "$.params.diagnostics"),
        (json!({"uri": "file:///main.pl", "diagnostics": [{}]}), "$.params.diagnostics[0].range"),
        (
            json!({
                "uri": "file:///main.pl",
                "diagnostics": [{
                    "range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 1}},
                    "message": "bad",
                    "severity": 0
                }]
            }),
            "$.params.diagnostics[0].severity",
        ),
        (
            json!({
                "uri": "file:///main.pl",
                "diagnostics": [{
                    "range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 1}},
                    "message": "bad",
                    "code": 2147483648_i64
                }]
            }),
            "$.params.diagnostics[0].code",
        ),
        (
            json!({
                "uri": "file:///main.pl",
                "diagnostics": [{
                    "range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 1}},
                    "message": "bad",
                    "codeDescription": {"href": "not-a-uri"}
                }]
            }),
            "$.params.diagnostics[0].codeDescription.href",
        ),
    ] {
        let frame = notification(params);
        let error = validate(&frame, Direction::ServerToClient, None)
            .expect_err("malformed publishDiagnostics field must fail");
        assert_eq!(error.method.as_deref(), Some(METHOD));
        assert_eq!(error.path, path);
    }
}

#[test]
fn publish_diagnostics_accepts_empty_message_and_file_href() -> Result<(), SchemaError> {
    let frame = notification(json!({
        "uri": "file:///main.pl",
        "diagnostics": [{
            "range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 1}},
            "message": "",
            "codeDescription": {"href": "file:///workspace/docs/PL002.html"}
        }]
    }));
    validate(&frame, Direction::ServerToClient, None)?;
    Ok(())
}

#[test]
fn publish_diagnostics_is_only_a_server_notification() {
    let params = json!({"uri": "file:///main.pl", "diagnostics": []});
    let mut request = notification(params.clone());
    request["id"] = json!(1);
    for (frame, direction, method) in [
        (notification(params), Direction::ClientToServer, None),
        (request, Direction::ServerToClient, None),
        (
            json!({"jsonrpc": "2.0", "id": 1, "result": null}),
            Direction::ClientToServer,
            Some(METHOD),
        ),
    ] {
        let error = validate(&frame, direction, method)
            .expect_err("publishDiagnostics has no client or response schema");
        assert_eq!(error.path, "$.method");
        assert_eq!(error.expected, "registered method/direction schema");
    }
}

#[test]
fn publish_diagnostics_preserves_forward_compatible_unknown_fields() -> Result<(), SchemaError> {
    let frame = notification(json!({
        "uri": "file:///main.pl",
        "diagnostics": [{
            "range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 1}},
            "message": "warn",
            "data": {"future": true},
            "_future": true
        }],
        "_future": []
    }));
    validate(&frame, Direction::ServerToClient, None)?;
    Ok(())
}
