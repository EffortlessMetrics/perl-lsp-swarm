//! Seam proofs for the document-sync schema registry slice (#10477).
//!
//! Every registered method gets one valid fixture and one discriminating
//! invalid fixture per envelope class, and every failure must report a stable
//! method plus JSON path that traces to a registered 3.17 entry.

#![allow(clippy::expect_used)]
use perl_lsp_rs_core::protocol::schema::{
    Direction, MessageKind, ProtocolSchemaValidator, ProtocolVersion, SchemaError,
    ValidatedMessage, ValidationContext,
};
use serde_json::{Value, json};

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

const DID_OPEN_METHOD: &str = "textDocument/didOpen";
const DID_CHANGE_METHOD: &str = "textDocument/didChange";
const DID_CLOSE_METHOD: &str = "textDocument/didClose";

fn did_open(uri: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "method": DID_OPEN_METHOD,
        "params": {
            "textDocument": {
                "uri": uri,
                "languageId": "perl",
                "version": 1,
                "text": "print qq{hello\n};"
            }
        }
    })
}

#[test]
fn did_open_validates_as_client_to_server_notification() {
    let validated =
        validate(&did_open("file:///workspace/main.pl"), Direction::ClientToServer, None)
            .expect("valid didOpen must validate");
    assert_eq!(validated.method, DID_OPEN_METHOD);
    assert_eq!(validated.kind, MessageKind::Notification);
    assert_eq!(validated.direction, Direction::ClientToServer);
    assert_eq!(validated.version, ProtocolVersion::Lsp317);
}

#[test]
fn did_change_validates_whole_document_and_incremental_variants() {
    let whole_document = json!({
        "jsonrpc": "2.0",
        "method": DID_CHANGE_METHOD,
        "params": {
            "textDocument": { "uri": "file:///workspace/main.pl", "version": 7 },
            "contentChanges": [{ "text": "package main;" }]
        }
    });
    let validated = validate(&whole_document, Direction::ClientToServer, None)
        .expect("whole-document change event must validate");
    assert_eq!(validated.method, DID_CHANGE_METHOD);

    let incremental = json!({
        "jsonrpc": "2.0",
        "method": DID_CHANGE_METHOD,
        "params": {
            "textDocument": { "uri": "file:///workspace/main.pl", "version": 8 },
            "contentChanges": [{
                "range": {
                    "start": { "line": 0, "character": 0 },
                    "end": { "line": 0, "character": 7 }
                },
                "rangeLength": 7,
                "text": "package strict;"
            }],
            "_clientMetadata": { "internal": true }
        }
    });
    let validated_incremental = validate(&incremental, Direction::ClientToServer, None)
        .expect("incremental change with deprecated rangeLength must validate");
    assert_eq!(validated_incremental.version, ProtocolVersion::Lsp317);

    let empty_batch = json!({
        "jsonrpc": "2.0",
        "method": DID_CHANGE_METHOD,
        "params": {
            "textDocument": { "uri": "untitled:Untitled-1", "version": -3 },
            "contentChanges": []
        }
    });
    validate(&empty_batch, Direction::ClientToServer, None)
        .expect("empty contentChanges batch and negative document version are structurally valid");
}

#[test]
fn did_change_rejects_null_version_required_by_lsp_3_17() {
    let null_version = json!({
        "jsonrpc": "2.0",
        "method": DID_CHANGE_METHOD,
        "params": {
            "textDocument": { "uri": "untitled:Untitled-1", "version": null },
            "contentChanges": []
        }
    });
    let error = validate(&null_version, Direction::ClientToServer, None).expect_err(
        "VersionedTextDocumentIdentifier.version is integer in 3.17; only OptionalVersionedTextDocumentIdentifier allows null",
    );
    assert_eq!(error.method.as_deref(), Some(DID_CHANGE_METHOD));
    assert_eq!(error.path, "$.params.textDocument.version");
    assert_eq!(error.expected, "integer");
    assert_eq!(error.observed, "null");
}

#[test]
fn did_close_validates_with_text_document_identifier() {
    let close = json!({
        "jsonrpc": "2.0",
        "method": DID_CLOSE_METHOD,
        "params": { "textDocument": { "uri": "file:///workspace/main.pl" } }
    });
    let validated =
        validate(&close, Direction::ClientToServer, None).expect("valid didClose must validate");
    assert_eq!(validated.kind, MessageKind::Notification);
    assert_eq!(validated.direction, Direction::ClientToServer);
}

#[test]
fn did_open_missing_text_document_fails_at_stable_path() {
    let missing = json!({
        "jsonrpc": "2.0",
        "method": DID_OPEN_METHOD,
        "params": {}
    });
    let error = validate(&missing, Direction::ClientToServer, None)
        .expect_err("didOpen without textDocument must fail");
    assert_eq!(error.method.as_deref(), Some(DID_OPEN_METHOD));
    assert_eq!(error.path, "$.params.textDocument");
    assert_eq!(error.expected, "object");
    assert_eq!(error.observed, "missing");
}

#[test]
fn did_open_wrong_known_field_types_report_exact_paths() {
    let mut string_version = did_open("file:///workspace/main.pl");
    string_version["params"]["textDocument"]["version"] = json!("one");
    let version_error = validate(&string_version, Direction::ClientToServer, None)
        .expect_err("string document version must fail");
    assert_eq!(version_error.path, "$.params.textDocument.version");
    assert_eq!(version_error.expected, "integer");
    assert!(version_error.observed.starts_with("string"));

    let mut numeric_uri = did_open("file:///workspace/main.pl");
    numeric_uri["params"]["textDocument"]["uri"] = json!({"raw": "file:///x.pl"});
    let uri_error = validate(&numeric_uri, Direction::ClientToServer, None)
        .expect_err("object URI must fail at the exact path");
    assert_eq!(uri_error.path, "$.params.textDocument.uri");

    let mut missing_language = did_open("file:///workspace/main.pl");
    let _ = missing_language["params"]["textDocument"]
        .as_object_mut()
        .map(|item| item.remove("languageId"));
    let language_error = validate(&missing_language, Direction::ClientToServer, None)
        .expect_err("didOpen requires languageId");
    assert_eq!(language_error.path, "$.params.textDocument.languageId");
    assert_eq!(language_error.expected, "string");
}

#[test]
fn document_uri_requires_scheme_shaped_string() {
    let scheme_less = did_open("main.pl");
    let error = validate(&scheme_less, Direction::ClientToServer, None)
        .expect_err("URI without a scheme separator must fail");
    assert_eq!(error.path, "$.params.textDocument.uri");
    assert!(error.expected.contains("URI"));
}

#[test]
fn did_change_content_changes_must_be_an_array() {
    let object_changes = json!({
        "jsonrpc": "2.0",
        "method": DID_CHANGE_METHOD,
        "params": {
            "textDocument": { "uri": "file:///workspace/main.pl", "version": 1 },
            "contentChanges": { "text": "not an array" }
        }
    });
    let error = validate(&object_changes, Direction::ClientToServer, None)
        .expect_err("object contentChanges must fail");
    assert_eq!(error.path, "$.params.contentChanges");
    assert_eq!(error.expected, "array");

    let absent_changes = json!({
        "jsonrpc": "2.0",
        "method": DID_CHANGE_METHOD,
        "params": {
            "textDocument": { "uri": "file:///workspace/main.pl", "version": 1 }
        }
    });
    let missing = validate(&absent_changes, Direction::ClientToServer, None)
        .expect_err("didChange requires contentChanges");
    assert_eq!(missing.path, "$.params.contentChanges");
    assert_eq!(missing.observed, "missing");
}

#[test]
fn did_change_range_positions_are_bounded_unsigned_integers() {
    let negative_line = json!({
        "jsonrpc": "2.0",
        "method": DID_CHANGE_METHOD,
        "params": {
            "textDocument": { "uri": "file:///workspace/main.pl", "version": 1 },
            "contentChanges": [{
                "range": {
                    "start": { "line": -1, "character": 0 },
                    "end": { "line": 0, "character": 1 }
                },
                "text": "x"
            }]
        }
    });
    let error = validate(&negative_line, Direction::ClientToServer, None)
        .expect_err("negative position line must fail");
    assert_eq!(
        error.path, "$.params.contentChanges[0].range.start.line",
        "failure path must name the exact offending array element"
    );
    assert_eq!(error.expected, "unsigned integer");

    let boundary_batch = json!({
        "jsonrpc": "2.0",
        "method": DID_CHANGE_METHOD,
        "params": {
            "textDocument": { "uri": "file:///workspace/main.pl", "version": 1 },
            "contentChanges": [{
                "range": {
                    "start": { "line": 2147483647, "character": 2147483647 },
                    "end": { "line": 2147483647, "character": 2147483647 }
                },
                "rangeLength": 2147483647,
                "text": "x"
            }]
        }
    });
    validate(&boundary_batch, Direction::ClientToServer, None)
        .expect("uinteger fields at the inclusive LSP maximum 2^31-1 are structurally valid");

    let overflowing_character = json!({
        "jsonrpc": "2.0",
        "method": DID_CHANGE_METHOD,
        "params": {
            "textDocument": { "uri": "file:///workspace/main.pl", "version": 1 },
            "contentChanges": [{
                "range": {
                    "start": { "line": 0, "character": 0 },
                    "end": { "line": 0, "character": 2147483648u64 }
                },
                "text": "x"
            }]
        }
    });
    let overflow_error = validate(&overflowing_character, Direction::ClientToServer, None)
        .expect_err("the first value past the uinteger maximum 2^31-1 must fail");
    assert_eq!(overflow_error.path, "$.params.contentChanges[0].range.end.character");
    assert!(overflow_error.expected.contains("0..=2147483647"));

    let u32_overflow_character = json!({
        "jsonrpc": "2.0",
        "method": DID_CHANGE_METHOD,
        "params": {
            "textDocument": { "uri": "file:///workspace/main.pl", "version": 1 },
            "contentChanges": [{
                "range": {
                    "start": { "line": 0, "character": 0 },
                    "end": { "line": 0, "character": 4294967296u64 }
                },
                "text": "x"
            }]
        }
    });
    let u32_overflow_error = validate(&u32_overflow_character, Direction::ClientToServer, None)
        .expect_err("values past the unsigned 32-bit shape must also fail");
    assert_eq!(u32_overflow_error.path, "$.params.contentChanges[0].range.end.character");
}

#[test]
fn document_versions_are_bounded_to_the_lsp_integer_range() {
    let make_change = |version: i64| {
        json!({
            "jsonrpc": "2.0",
            "method": DID_CHANGE_METHOD,
            "params": {
                "textDocument": { "uri": "untitled:Untitled-1", "version": version },
                "contentChanges": []
            }
        })
    };

    validate(&make_change(2147483647), Direction::ClientToServer, None)
        .expect("didChange version at the inclusive integer maximum is valid");
    validate(&make_change(-2147483648), Direction::ClientToServer, None)
        .expect("didChange version at the inclusive integer minimum is valid");

    let above_max = validate(&make_change(2147483648), Direction::ClientToServer, None)
        .expect_err("didChange version past 2^31-1 leaves the LSP integer range");
    assert_eq!(above_max.method.as_deref(), Some(DID_CHANGE_METHOD));
    assert_eq!(above_max.path, "$.params.textDocument.version");
    assert_eq!(above_max.expected, "integer within -2147483648..=2147483647");
    assert_eq!(above_max.observed, "2147483648");

    let below_min = validate(&make_change(-2147483649), Direction::ClientToServer, None)
        .expect_err("didChange version below -2^31 leaves the LSP integer range");
    assert_eq!(below_min.path, "$.params.textDocument.version");

    let mut open_above_max = did_open("file:///workspace/main.pl");
    open_above_max["params"]["textDocument"]["version"] = json!(2147483648i64);
    let open_error = validate(&open_above_max, Direction::ClientToServer, None)
        .expect_err("didOpen TextDocumentItem.version shares the same LSP integer range");
    assert_eq!(open_error.path, "$.params.textDocument.version");
}

#[test]
fn did_change_deprecated_range_length_keeps_exact_type() {
    let string_length = json!({
        "jsonrpc": "2.0",
        "method": DID_CHANGE_METHOD,
        "params": {
            "textDocument": { "uri": "file:///workspace/main.pl", "version": 1 },
            "contentChanges": [{
                "range": {
                    "start": { "line": 0, "character": 0 },
                    "end": { "line": 0, "character": 1 }
                },
                "rangeLength": "one",
                "text": "x"
            }]
        }
    });
    let error = validate(&string_length, Direction::ClientToServer, None)
        .expect_err("deprecated rangeLength is still typed uint");
    assert_eq!(error.path, "$.params.contentChanges[0].rangeLength");
    assert_eq!(error.expected, "unsigned integer");
}

#[test]
fn did_change_rejects_range_length_without_range_on_the_union() {
    let orphan_range_length = json!({
        "jsonrpc": "2.0",
        "method": DID_CHANGE_METHOD,
        "params": {
            "textDocument": { "uri": "file:///workspace/main.pl", "version": 1 },
            "contentChanges": [{
                "rangeLength": 7,
                "text": "package strict;"
            }]
        }
    });
    let error = validate(&orphan_range_length, Direction::ClientToServer, None).expect_err(
        "TextDocumentContentChangeEvent is a union: rangeLength belongs only to the incremental arm that also carries range",
    );
    assert_eq!(error.method.as_deref(), Some(DID_CHANGE_METHOD));
    assert_eq!(error.path, "$.params.contentChanges[0].rangeLength");
}

#[test]
fn wrong_direction_and_response_classes_fail_closed() {
    let server_direction = did_open("file:///workspace/main.pl");
    let direction_error = validate(&server_direction, Direction::ServerToClient, None)
        .expect_err("didOpen sent server-to-client has no registered schema");
    assert_eq!(direction_error.path, "$.method");
    assert!(direction_error.expected.contains("registered"));

    let response_to_notification = json!({ "jsonrpc": "2.0", "id": 9, "result": null });
    let response_error =
        validate(&response_to_notification, Direction::ServerToClient, Some(DID_CHANGE_METHOD))
            .expect_err("notifications have no request schema, so no response can correlate");
    assert_eq!(response_error.path, "$.method");
    assert!(response_error.expected.contains("registered"));

    let error_response =
        json!({ "jsonrpc": "2.0", "id": 9, "error": { "code": -32603, "message": "boom" } });
    let notification_error_response =
        validate(&error_response, Direction::ServerToClient, Some(DID_CLOSE_METHOD))
            .expect_err("notifications cannot receive any response, not even an error response");
    assert_eq!(notification_error_response.path, "$.method");
    assert!(notification_error_response.expected.contains("registered"));

    let lifecycle_error = validate(&error_response, Direction::ServerToClient, Some("initialize"))
        .expect("error responses still validate when correlated with a registered request");
    assert_eq!(lifecycle_error.kind, MessageKind::ErrorResponse);
}

#[test]
fn unknown_fields_stay_forward_compatible_while_known_types_stay_exact() {
    let mut extended = did_open("file:///workspace/main.pl");
    extended["params"]["futureStandardField"] = json!({ "anything": true });
    validate(&extended, Direction::ClientToServer, None)
        .expect("unknown standard-looking fields follow the forward-compatibility rule");

    let mut smuggled = did_open("file:///workspace/main.pl");
    smuggled["params"]["textDocument"]["version"] = json!({ "major": 1 });
    let error = validate(&smuggled, Direction::ClientToServer, None)
        .expect_err("forward compatibility must not excuse a wrong type on a known field");
    assert_eq!(error.path, "$.params.textDocument.version");
}
