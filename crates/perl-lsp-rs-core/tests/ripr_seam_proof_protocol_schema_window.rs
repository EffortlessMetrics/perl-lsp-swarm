//! Seam proofs for the window-message protocol schema family.
//!
//! RIPR exposure policy homes focused seam proofs in tests/ripr_seam_proof_*.rs
//! so production modules stay small enough for review-comments to finish.
#![allow(clippy::expect_used, clippy::panic)]
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

#[test]
fn show_message_and_log_message_accept_lsp_317_message_types() {
    for method in ["window/showMessage", "window/logMessage"] {
        let message = json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": { "type": 3, "message": "hello" }
        });
        let validated = validate(&message, Direction::ServerToClient, Some(method))
            .unwrap_or_else(|error| panic!("{method} should validate: {error}"));
        assert_eq!(validated.kind, MessageKind::Notification);
        assert_eq!(validated.direction, Direction::ServerToClient);
        assert_eq!(validated.version, ProtocolVersion::Lsp317);
        assert_eq!(validated.method, method);
    }
}

#[test]
fn window_message_type_rejects_zero_and_debug() {
    let missing = json!({
        "jsonrpc": "2.0",
        "method": "window/showMessage",
        "params": { "message": "hello" }
    });
    let missing_error = validate(&missing, Direction::ServerToClient, Some("window/showMessage"))
        .expect_err("missing type must fail");
    assert_eq!(missing_error.path, "$.params.type");

    for bad_type in [0, 5] {
        let message = json!({
            "jsonrpc": "2.0",
            "method": "window/logMessage",
            "params": { "type": bad_type, "message": "hello" }
        });
        let error = validate(&message, Direction::ServerToClient, Some("window/logMessage"))
            .expect_err("MessageType 1..=4 is the 3.17 contract");
        assert_eq!(error.path, "$.params.type");
        assert_eq!(error.expected, "MessageType integer 1..=4");
        assert_eq!(error.observed, bad_type.to_string());
    }
}

#[test]
fn show_message_request_validates_actions_and_nullable_result() {
    let request = json!({
        "jsonrpc": "2.0",
        "id": 7,
        "method": "window/showMessageRequest",
        "params": {
            "type": 1,
            "message": "retry?",
            "actions": [{ "title": "Retry" }, { "title": "Ignore" }]
        }
    });
    let request_validated =
        validate(&request, Direction::ServerToClient, Some("window/showMessageRequest"))
            .expect("showMessageRequest params should validate");
    assert_eq!(request_validated.kind, MessageKind::Request);
    assert_eq!(request_validated.direction, Direction::ServerToClient);

    let chosen = json!({
        "jsonrpc": "2.0",
        "id": 7,
        "result": { "title": "Retry" }
    });
    let chosen_validated =
        validate(&chosen, Direction::ClientToServer, Some("window/showMessageRequest"))
            .expect("selected action travels opposite the request");
    assert_eq!(chosen_validated.kind, MessageKind::SuccessResponse);
    assert_eq!(chosen_validated.direction, Direction::ClientToServer);

    let dismissed = json!({ "jsonrpc": "2.0", "id": 7, "result": null });
    let dismissed_validated =
        validate(&dismissed, Direction::ClientToServer, Some("window/showMessageRequest"))
            .expect("dismissed request result is null");
    assert_eq!(dismissed_validated.kind, MessageKind::SuccessResponse);
}

#[test]
fn show_message_request_reports_positional_action_failures() {
    let not_array = json!({
        "jsonrpc": "2.0",
        "id": 8,
        "method": "window/showMessageRequest",
        "params": { "type": 2, "message": "choose", "actions": "Retry" }
    });
    let array_error =
        validate(&not_array, Direction::ServerToClient, Some("window/showMessageRequest"))
            .expect_err("actions must be an array");
    assert_eq!(array_error.path, "$.params.actions");
    assert_eq!(array_error.expected, "array");

    let missing_title = json!({
        "jsonrpc": "2.0",
        "id": 8,
        "method": "window/showMessageRequest",
        "params": { "type": 2, "message": "choose", "actions": [{ "id": "retry" }] }
    });
    let title_error =
        validate(&missing_title, Direction::ServerToClient, Some("window/showMessageRequest"))
            .expect_err("action items require title");
    assert_eq!(title_error.path, "$.params.actions[0].title");

    let bad_result = json!({
        "jsonrpc": "2.0",
        "id": 8,
        "result": { "id": "retry" }
    });
    let result_error =
        validate(&bad_result, Direction::ClientToServer, Some("window/showMessageRequest"))
            .expect_err("result action items also require title");
    assert_eq!(result_error.path, "$.result.title");
}

#[test]
fn neighboring_window_methods_remain_unregistered() {
    let show_document = json!({
        "jsonrpc": "2.0",
        "id": 9,
        "method": "window/showDocument",
        "params": { "uri": "file:///workspace/main.pl" }
    });
    let error = validate(&show_document, Direction::ServerToClient, None)
        .expect_err("showDocument belongs to a later window slice");
    assert_eq!(error.path, "$.method");
    assert!(error.expected.contains("registered"));

    let progress = json!({
        "jsonrpc": "2.0",
        "method": "$/progress",
        "params": { "token": "work-1", "value": { "kind": "begin", "title": "Index" } }
    });
    let error = validate(&progress, Direction::ServerToClient, None)
        .expect_err("progress is a separate family");
    assert_eq!(error.path, "$.method");

    let wrong_way = json!({
        "jsonrpc": "2.0",
        "method": "window/showMessage",
        "params": { "type": 3, "message": "hello" }
    });
    let error = validate(&wrong_way, Direction::ClientToServer, None)
        .expect_err("showMessage is server-to-client only");
    assert_eq!(error.path, "$.method");
}
