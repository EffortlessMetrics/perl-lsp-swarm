//! Method/direction and payload proofs for LSP 3.17 documentHighlight.

use anyhow::{Result, ensure};
use perl_lsp_rs_core::protocol::schema::{
    Direction, MessageKind, ProtocolSchemaValidator, ProtocolVersion, ValidationContext,
};
use serde_json::{Value, json};

const METHOD: &str = "textDocument/documentHighlight";

fn params() -> Value {
    json!({"textDocument": {"uri": "untitled:highlight"}, "position": {"line": 0, "character": 3}})
}

fn request(params: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": "highlight-1", "method": METHOD, "params": params})
}

fn response(result: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": "highlight-1", "result": result})
}

fn range() -> Value {
    json!({"start": {"line": 0, "character": 3}, "end": {"line": 0, "character": 9}})
}

fn rejects_at(frame: &Value, direction: Direction, path: &str) -> Result<()> {
    let error = ProtocolSchemaValidator::default()
        .validate(frame, ValidationContext { direction, method: Some(METHOD) })
        .err()
        .ok_or_else(|| anyhow::anyhow!("invalid highlight frame accepted: {frame}"))?;
    ensure!(error.method.as_deref() == Some(METHOD), "wrong error method: {error}");
    ensure!(error.path == path, "expected {path}, observed {error}");
    Ok(())
}

#[test]
fn request_and_success_result_use_actual_wire_directions() -> Result<()> {
    let mut input = params();
    input["workDoneToken"] = json!("work");
    input["partialResultToken"] = json!(-7);
    let validated = ProtocolSchemaValidator::default().validate(
        &request(input),
        ValidationContext { direction: Direction::ClientToServer, method: Some(METHOD) },
    )?;
    ensure!(validated.method == METHOD);
    ensure!(validated.direction == Direction::ClientToServer);
    ensure!(validated.kind == MessageKind::Request);
    ensure!(validated.version == ProtocolVersion::Lsp317);

    let result = json!([
        {"range": range()},
        {"range": range(), "kind": 1},
        {"range": range(), "kind": 2},
        {"range": range(), "kind": 3}
    ]);
    let validated = ProtocolSchemaValidator::default().validate(
        &response(result),
        ValidationContext { direction: Direction::ServerToClient, method: Some(METHOD) },
    )?;
    ensure!(validated.direction == Direction::ServerToClient);
    ensure!(validated.kind == MessageKind::SuccessResponse);
    ensure!(validated.version == ProtocolVersion::Lsp317);
    Ok(())
}

#[test]
fn success_result_allows_null_and_empty_array() -> Result<()> {
    for result in [Value::Null, json!([])] {
        ProtocolSchemaValidator::default().validate(
            &response(result),
            ValidationContext { direction: Direction::ServerToClient, method: Some(METHOD) },
        )?;
    }
    Ok(())
}

#[test]
fn request_known_fields_fail_at_exact_paths() -> Result<()> {
    for (pointer, value, path) in [
        ("/textDocument", Value::Null, "$.params.textDocument"),
        ("/textDocument/uri", json!(7), "$.params.textDocument.uri"),
        ("/position", Value::Null, "$.params.position"),
        ("/position/line", json!(-1), "$.params.position.line"),
        ("/position/character", json!(2147483648_u64), "$.params.position.character"),
    ] {
        let mut input = params();
        *input
            .pointer_mut(pointer)
            .ok_or_else(|| anyhow::anyhow!("missing fixture {pointer}"))? = value;
        rejects_at(&request(input), Direction::ClientToServer, path)?;
    }
    for (input, path) in [
        (json!({"position": {"line": 0, "character": 0}}), "$.params.textDocument"),
        (json!({"textDocument": {"uri": "file:///highlight.pl"}}), "$.params.position"),
    ] {
        rejects_at(&request(input), Direction::ClientToServer, path)?;
    }
    Ok(())
}

#[test]
fn progress_token_known_fields_are_integer_or_string() -> Result<()> {
    for field in ["workDoneToken", "partialResultToken"] {
        for token in [json!(""), json!(-2147483648_i64), json!(2147483647_i64)] {
            let mut input = params();
            input[field] = token;
            ProtocolSchemaValidator::default().validate(
                &request(input),
                ValidationContext { direction: Direction::ClientToServer, method: Some(METHOD) },
            )?;
        }
        for token in [Value::Null, json!(true), json!(1.5), json!(2147483648_i64), json!([])] {
            let mut input = params();
            input[field] = token;
            rejects_at(&request(input), Direction::ClientToServer, &format!("$.params.{field}"))?;
        }
    }
    Ok(())
}

#[test]
fn wrong_result_arm_and_missing_range_do_not_pass_generic_envelopes() -> Result<()> {
    for result in
        [json!({"contents": "hover"}), json!({"range": range()}), json!(false), json!("text")]
    {
        rejects_at(&response(result), Direction::ServerToClient, "$.result")?;
    }
    for result in [json!([null]), json!(["text"])] {
        rejects_at(&response(result), Direction::ServerToClient, "$.result[0]")?;
    }
    rejects_at(&response(json!([{}])), Direction::ServerToClient, "$.result[0].range")?;
    Ok(())
}

#[test]
fn result_known_fields_fail_at_exact_paths() -> Result<()> {
    for kind in [Value::Null, json!(0), json!(4), json!(-1), json!(1.5), json!("Read")] {
        rejects_at(
            &response(json!([{"range": range(), "kind": kind}])),
            Direction::ServerToClient,
            "$.result[0].kind",
        )?;
    }
    let mut bad_range = range();
    bad_range["end"]["character"] = json!(-1);
    rejects_at(
        &response(json!([{"range": bad_range}])),
        Direction::ServerToClient,
        "$.result[0].range.end.character",
    )?;
    Ok(())
}

#[test]
fn opposite_directions_and_notification_envelopes_are_not_registered() -> Result<()> {
    rejects_at(&request(params()), Direction::ServerToClient, "$.method")?;
    rejects_at(&response(json!([])), Direction::ClientToServer, "$.method")?;
    let mut notification = request(params());
    notification
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("fixture must be object"))?
        .remove("id");
    rejects_at(&notification, Direction::ClientToServer, "$.method")?;
    Ok(())
}

#[test]
fn unknown_fields_preserve_existing_forward_compatibility() -> Result<()> {
    let mut input = params();
    input["_future"] = json!({"extra": true});
    ProtocolSchemaValidator::default().validate(
        &request(input),
        ValidationContext { direction: Direction::ClientToServer, method: Some(METHOD) },
    )?;
    ProtocolSchemaValidator::default().validate(
        &response(json!([{"range": range(), "_future": true}])),
        ValidationContext { direction: Direction::ServerToClient, method: Some(METHOD) },
    )?;
    Ok(())
}
