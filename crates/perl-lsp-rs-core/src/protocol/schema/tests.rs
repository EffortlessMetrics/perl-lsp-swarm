use super::*;
use serde_json::{Value, json};

fn context<'a>(
    direction: Direction,
    method: Option<&'a str>,
    allow_lsp_318_development: bool,
) -> ValidationContext<'a> {
    ValidationContext { direction, method, allow_lsp_318_development }
}

fn validate(
    message: &Value,
    direction: Direction,
    method: Option<&str>,
) -> Result<ValidatedMessage, SchemaError> {
    ProtocolSchemaValidator::default().validate(message, context(direction, method, false))
}

#[test]
fn pinned_manifest_and_rust_registry_are_consistent() {
    // This is intentionally a checked-in manifest/registry consistency check.
    // It does not independently verify the manifest against upstream without
    // a network-dependent source acquisition step.
    let source: Value =
        serde_json::from_str(SCHEMA_SOURCE_JSON).expect("pinned schema source must be valid JSON");
    assert_eq!(
        source.pointer("/upstream/commit").and_then(Value::as_str),
        Some(UPSTREAM_PROTOCOL_COMMIT)
    );
    assert_eq!(source.pointer("/upstream/lsp_3_17/status").and_then(Value::as_str), Some("stable"));
    assert_eq!(
        source.pointer("/upstream/lsp_3_18/status").and_then(Value::as_str),
        Some("under_development")
    );
    assert_eq!(
        source.pointer("/base_protocol/batch_supported").and_then(Value::as_bool),
        Some(false)
    );

    let declared = source["registry"]
        .as_array()
        .expect("source registry must be an array")
        .iter()
        .map(|value| value.as_str().expect("source registry entry must be a string").to_string())
        .collect::<Vec<_>>();
    assert_eq!(registered_schema_identities(), declared);
}

#[test]
fn unregistration_params_require_the_pinned_historical_field_name() {
    let historical = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "client/unregisterCapability",
        "params": {
            "unregisterations": [{
                "id": "registration-1",
                "method": "textDocument/hover"
            }]
        }
    });
    validate(&historical, Direction::ServerToClient, Some("client/unregisterCapability"))
        .expect("the pinned historical LSP field name should validate");

    let corrected = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "client/unregisterCapability",
        "params": {
            "unregistrations": [{
                "id": "registration-1",
                "method": "textDocument/hover"
            }]
        }
    });
    let error =
        validate(&corrected, Direction::ServerToClient, Some("client/unregisterCapability"))
            .expect_err("the corrected spelling is outside the pinned schema contract");
    assert_eq!(error.path, "$.params.unregisterations");
    assert_eq!(error.expected, "unregistration array");
    assert_eq!(error.observed, "missing");
}

#[test]
fn initialize_request_and_response_validate_in_actual_wire_directions() {
    let request = json!({
        "jsonrpc": "2.0",
        "id": "initialize-1",
        "method": "initialize",
        "params": {
            "processId": null,
            "rootUri": null,
            "capabilities": {},
            "workspaceFolders": null
        }
    });
    let request_validated = validate(&request, Direction::ClientToServer, Some("initialize"))
        .expect("initialize request should validate");
    assert_eq!(request_validated.kind, MessageKind::Request);
    assert_eq!(request_validated.direction, Direction::ClientToServer);
    assert_eq!(request_validated.version, ProtocolVersion::Lsp317);

    let response = json!({
        "jsonrpc": "2.0",
        "id": "initialize-1",
        "result": {
            "capabilities": {
                "hoverProvider": true,
                "experimental": {
                    "perlLsp": { "schemaVersion": 1 }
                }
            },
            "serverInfo": { "name": "perllsp" }
        }
    });
    let response_validated = validate(&response, Direction::ServerToClient, Some("initialize"))
        .expect("initialize response should use the opposite registered direction");
    assert_eq!(response_validated.kind, MessageKind::SuccessResponse);
    assert_eq!(response_validated.direction, Direction::ServerToClient);
    assert_eq!(response_validated.version, ProtocolVersion::Lsp317);
}

#[test]
fn server_request_and_client_response_use_opposite_schema_directions() {
    let request = json!({
        "jsonrpc": "2.0",
        "id": 7,
        "method": "workspace/configuration",
        "params": {
            "items": [
                { "scopeUri": "file:///workspace", "section": "perl" }
            ]
        }
    });
    let request_validated =
        validate(&request, Direction::ServerToClient, Some("workspace/configuration"))
            .expect("server-originated request should validate");
    assert_eq!(request_validated.direction, Direction::ServerToClient);

    let response = json!({
        "jsonrpc": "2.0",
        "id": 7,
        "result": [{ "formatting": { "enabled": true } }]
    });
    let response_validated =
        validate(&response, Direction::ClientToServer, Some("workspace/configuration"))
            .expect("client response should resolve the originating server-request schema");
    assert_eq!(response_validated.kind, MessageKind::SuccessResponse);
    assert_eq!(response_validated.direction, Direction::ClientToServer);
}

#[test]
fn response_requires_capture_supplied_method_identity() {
    let error = validate(
        &json!({ "jsonrpc": "2.0", "id": 1, "result": null }),
        Direction::ServerToClient,
        None,
    )
    .expect_err("response without correlated method must fail");
    assert_eq!(error.path, "$.method");
    assert!(error.expected.contains("capture-supplied"));
}

#[test]
fn batch_and_invalid_request_ids_fail_at_the_envelope_boundary() {
    let batch = json!([
        { "jsonrpc": "2.0", "id": 1, "method": "shutdown", "params": null }
    ]);
    let batch_error = validate(&batch, Direction::ClientToServer, None)
        .expect_err("JSON-RPC batch input is unsupported");
    assert_eq!(batch_error.path, "$");
    assert_eq!(batch_error.expected, "object");

    let invalid_id = json!({
        "jsonrpc": "2.0",
        "id": null,
        "method": "shutdown",
        "params": null
    });
    let id_error = validate(&invalid_id, Direction::ClientToServer, Some("shutdown"))
        .expect_err("request IDs cannot be null");
    assert_eq!(id_error.path, "$.id");
    assert!(id_error.expected.contains("integer or string"));
}

#[test]
fn known_field_type_errors_report_the_exact_json_path() {
    let message = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "processId": null,
            "capabilities": "not-an-object"
        }
    });
    let error = validate(&message, Direction::ClientToServer, Some("initialize"))
        .expect_err("wrong known-field type must fail");
    assert_eq!(error.method.as_deref(), Some("initialize"));
    assert_eq!(error.path, "$.params.capabilities");
    assert_eq!(error.expected, "object");
    assert!(error.observed.starts_with("string"));
}

#[test]
fn wrong_location_union_variant_is_rejected() {
    let response = json!({
        "jsonrpc": "2.0",
        "id": 9,
        "result": {
            "range": {
                "start": { "line": 0, "character": 0 },
                "end": { "line": 0, "character": 3 }
            }
        }
    });
    let error = validate(&response, Direction::ServerToClient, Some("textDocument/definition"))
        .expect_err("object must be a Location or LocationLink");
    assert_eq!(error.path, "$.result.targetUri");
}

#[test]
fn selected_318_methods_require_explicit_development_opt_in() {
    let request = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "textDocument/inlineCompletion",
        "params": {
            "textDocument": { "uri": "file:///workspace/main.pl" },
            "position": { "line": 0, "character": 0 },
            "context": { "triggerKind": 0 }
        }
    });
    let denied = ProtocolSchemaValidator::default()
        .validate(
            &request,
            context(Direction::ClientToServer, Some("textDocument/inlineCompletion"), false),
        )
        .expect_err("3.18-development method must fail closed by default");
    assert_eq!(denied.path, "$.method");
    assert!(denied.expected.contains("3.18-development"));

    let allowed = ProtocolSchemaValidator::default()
        .validate(
            &request,
            context(Direction::ClientToServer, Some("textDocument/inlineCompletion"), true),
        )
        .expect("explicitly selected 3.18-development method should validate");
    assert_eq!(allowed.version, ProtocolVersion::Lsp318Development);
}

#[test]
fn project_extensions_are_bounded_to_deliberate_method_namespaces() {
    let extension = json!({
        "jsonrpc": "2.0",
        "id": "watchdog",
        "method": "$/perl-lsp/watchdog",
        "params": {}
    });
    let validated = validate(&extension, Direction::ClientToServer, None)
        .expect("project extension namespace should validate generically");
    assert_eq!(validated.version, ProtocolVersion::PerlLspExtension);

    let unknown_standard = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "workspace/notActuallyStandard",
        "params": {}
    });
    let error = validate(&unknown_standard, Direction::ClientToServer, None)
        .expect_err("unknown standard-looking method must not bypass the registry");
    assert_eq!(error.path, "$.method");
    assert!(error.expected.contains("registered"));
}

#[test]
fn initialize_extensions_are_confined_to_capabilities_experimental() {
    let forbidden = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "result": {
            "capabilities": {},
            "perlLsp": { "schemaVersion": 1 }
        }
    });
    let error = validate(&forbidden, Direction::ServerToClient, Some("initialize"))
        .expect_err("project metadata must not occupy a standard top-level field");
    assert_eq!(error.path, "$.result.perlLsp");

    let allowed = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "result": {
            "capabilities": {
                "experimental": {
                    "perlLsp": { "schemaVersion": 1 }
                }
            }
        }
    });
    validate(&allowed, Direction::ServerToClient, Some("initialize"))
        .expect("experimental extension surface should remain available");
}

#[test]
fn semantic_token_data_is_a_complete_five_integer_stream() {
    let invalid = json!({
        "jsonrpc": "2.0",
        "id": 3,
        "result": { "data": [0, 0, 3, 1] }
    });
    let error =
        validate(&invalid, Direction::ServerToClient, Some("textDocument/semanticTokens/full"))
            .expect_err("incomplete semantic token tuple must fail");
    assert_eq!(error.path, "$.result.data");
    assert!(error.expected.contains("divisible by 5"));

    let valid = json!({
        "jsonrpc": "2.0",
        "id": 3,
        "result": { "resultId": "current-1", "data": [0, 0, 3, 1, 0] }
    });
    validate(&valid, Direction::ServerToClient, Some("textDocument/semanticTokens/full"))
        .expect("complete semantic token tuple should validate");
}

#[test]
fn apply_edit_rejection_result_preserves_typed_failure_fields() {
    let response = json!({
        "jsonrpc": "2.0",
        "id": "apply-edit-1",
        "result": {
            "applied": false,
            "failureReason": "document changed",
            "failedChange": 2
        }
    });
    let validated = validate(&response, Direction::ClientToServer, Some("workspace/applyEdit"))
        .expect("typed ApplyWorkspaceEditResult rejection should validate");
    assert_eq!(validated.direction, Direction::ClientToServer);
    assert_eq!(validated.kind, MessageKind::SuccessResponse);
}

#[test]
fn null_success_and_error_response_remain_distinct() {
    let shutdown = json!({ "jsonrpc": "2.0", "id": 1, "result": null });
    let success = validate(&shutdown, Direction::ServerToClient, Some("shutdown"))
        .expect("shutdown success must be null");
    assert_eq!(success.kind, MessageKind::SuccessResponse);

    let error = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "error": {
            "code": -32602,
            "message": "invalid params",
            "data": { "path": "$.params" }
        }
    });
    let validated_error = validate(&error, Direction::ServerToClient, Some("initialize"))
        .expect("well-formed error response should validate");
    assert_eq!(validated_error.kind, MessageKind::ErrorResponse);
}

#[test]
fn pathological_depth_node_and_string_inputs_are_bounded() {
    let validator = ProtocolSchemaValidator::with_limits(ValidationLimits {
        max_depth: 3,
        max_nodes: 20,
        max_string_bytes: 16,
    });

    let deep = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "$/perl-lsp/deep",
        "params": { "a": { "b": { "c": { "d": true } } } }
    });
    let depth_error = validator
        .validate(&deep, context(Direction::ClientToServer, None, false))
        .expect_err("deep payload must be bounded before method validation");
    assert!(depth_error.expected.contains("depth at most"));

    let long = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "$/perl-lsp/long",
        "params": { "value": "this string is longer than sixteen bytes" }
    });
    let string_error = validator
        .validate(&long, context(Direction::ClientToServer, None, false))
        .expect_err("long string must be bounded");
    assert!(string_error.expected.contains("string at most"));

    let many = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "$/perl-lsp/many",
        "params": [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15]
    });
    let node_error = validator
        .validate(&many, context(Direction::ClientToServer, None, false))
        .expect_err("large node population must be bounded");
    assert!(node_error.expected.contains("JSON nodes"));
}
