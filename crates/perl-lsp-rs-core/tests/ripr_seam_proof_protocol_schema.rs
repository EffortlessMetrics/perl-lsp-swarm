//! Seam proofs for the protocol schema envelope, bounds, and lifecycle registry.
//!
//! RIPR exposure policy homes focused seam proofs in tests/ripr_seam_proof_*.rs
//! so production modules stay small enough for review-comments to finish.
#![allow(clippy::expect_used)]
use perl_lsp_rs_core::protocol::schema::{
    Direction, MessageKind, ProtocolSchemaValidator, ProtocolVersion, SCHEMA_SOURCE_JSON,
    SCHEMA_SOURCE_MANIFEST_SHA256, SchemaError, UPSTREAM_PROTOCOL_COMMIT, ValidatedMessage,
    ValidationContext, ValidationLimits, registered_schema_identities,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

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
fn pinned_manifest_and_rust_registry_are_consistent() {
    let manifest_digest = Sha256::digest(SCHEMA_SOURCE_JSON.as_bytes());
    let manifest_digest =
        manifest_digest.iter().map(|byte| format!("{byte:02x}")).collect::<String>();
    assert_eq!(
        manifest_digest, SCHEMA_SOURCE_MANIFEST_SHA256,
        "protocol_schema_source.json changed; refresh SCHEMA_SOURCE_MANIFEST_SHA256 to the new digest"
    );

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
    assert_eq!(
        registered_schema_identities(),
        declared,
        "METHOD_SCHEMAS and the pinned manifest registry must list identical entries in identical order"
    );
    assert_eq!(
        declared,
        vec![
            "client_to_server:notification:$/cancelRequest:3.17",
            "client_to_server:notification:exit:3.17",
            "client_to_server:request:initialize:3.17",
            "client_to_server:notification:initialized:3.17",
            "client_to_server:request:shutdown:3.17",
            "server_to_client:notification:$/cancelRequest:3.17",
            "server_to_client:notification:window/logMessage:3.17",
            "server_to_client:notification:window/showMessage:3.17",
            "server_to_client:request:window/showMessageRequest:3.17",
        ]
    );
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
fn cancel_request_validates_in_both_wire_directions() {
    let cancel = json!({
        "jsonrpc": "2.0",
        "method": "$/cancelRequest",
        "params": { "id": 4 }
    });

    for direction in [Direction::ClientToServer, Direction::ServerToClient] {
        let validated = validate(&cancel, direction, Some("$/cancelRequest"))
            .expect("cancellation is valid in both directions");
        assert_eq!(validated.direction, direction);
        assert_eq!(validated.version, ProtocolVersion::Lsp317);
    }
}

#[test]
fn shutdown_null_params_and_result_report_positional_paths() {
    let bad_params = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "shutdown",
        "params": {}
    });
    let params_error = validate(&bad_params, Direction::ClientToServer, Some("shutdown"))
        .expect_err("non-null shutdown params must fail at $.params");
    assert_eq!(params_error.path, "$.params");

    let bad_result = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "result": {}
    });
    let result_error = validate(&bad_result, Direction::ServerToClient, Some("shutdown"))
        .expect_err("non-null shutdown result must fail at $.result");
    assert_eq!(result_error.path, "$.result");
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
fn unregistered_standard_methods_fail_closed() {
    let definition = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "textDocument/definition",
        "params": {
            "textDocument": { "uri": "file:///workspace/main.pl" },
            "position": { "line": 0, "character": 0 }
        }
    });
    let error = validate(&definition, Direction::ClientToServer, None)
        .expect_err("payload methods deferred from this slice must not pass as registered");
    assert_eq!(error.path, "$.method");
    assert!(error.expected.contains("registered"));

    let inline = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "textDocument/inlineCompletion",
        "params": {
            "textDocument": { "uri": "file:///workspace/main.pl" },
            "position": { "line": 0, "character": 0 }
        }
    });
    let error = validate(&inline, Direction::ClientToServer, None)
        .expect_err("3.18 methods are unregistered until the payload follow-up");
    assert_eq!(error.path, "$.method");
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

    for spelling in ["PerlLsp", "PERL_LSP", "Perl"] {
        let mixed_case = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "capabilities": {},
                spelling: { "schemaVersion": 1 }
            }
        });
        let error = validate(&mixed_case, Direction::ServerToClient, Some("initialize"))
            .expect_err("case must not smuggle project metadata to the top level");
        assert_eq!(error.path, format!("$.result.{spelling}"));
    }

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
        .validate(&deep, context(Direction::ClientToServer, None))
        .expect_err("deep payload must be bounded before method validation");
    assert!(depth_error.expected.contains("depth at most"));

    let dotted_validator = ProtocolSchemaValidator::with_limits(ValidationLimits {
        max_depth: 3,
        max_nodes: 20,
        max_string_bytes: 64,
    });
    let dotted = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "$/perl-lsp/dotted",
        "params": { "a.b": { "c": { "d": { "e": true } } } }
    });
    let dotted_error = dotted_validator
        .validate(&dotted, context(Direction::ClientToServer, None))
        .expect_err("dotted object keys must remain distinguishable in failure paths");
    assert!(
        dotted_error.path.contains("['a.b']"),
        "dotted key must be quoted, got {}",
        dotted_error.path
    );
    assert!(
        !dotted_error.path.contains(".a.b."),
        "dotted key must not look like nested members, got {}",
        dotted_error.path
    );

    let long = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "$/perl-lsp/long",
        "params": { "value": "this string is longer than sixteen bytes" }
    });
    let string_error = validator
        .validate(&long, context(Direction::ClientToServer, None))
        .expect_err("long string must be bounded");
    assert!(string_error.expected.contains("string at most"));

    let long_key = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "$/perl-lsp/k",
        "params": { "this key is longer": true }
    });
    let key_error = validator
        .validate(&long_key, context(Direction::ClientToServer, None))
        .expect_err("long object keys must be bounded");
    assert!(key_error.expected.contains("key at most"));
    assert!(key_error.path.contains("['this key is longer']"));

    let many = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "$/perl-lsp/many",
        "params": [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15]
    });
    let node_error = validator
        .validate(&many, context(Direction::ClientToServer, None))
        .expect_err("large node population must be bounded");
    assert!(node_error.expected.contains("JSON nodes"));
}
