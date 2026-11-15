//! Direction-preserving incoming envelopes (#7626).
//!
//! This module owns decoding, not method dispatch, response correlation, or
//! application effects. The legacy carrier adapter remains in `incoming` until
//! #7010 switches the shipped ingress paths. Do not extract that adapter with
//! this module. Request-version admission retains the existing contract;
//! #9636 owns the final structural model and its product disposition.

use super::incoming_error::IncomingMessageError;
use crate::protocol::{JSONRPC_VERSION, JsonRpcError, JsonRpcId, JsonRpcRequest};
use serde_json::{Map, Value};

/// An incoming envelope retains its direction before application routing.
#[derive(Debug)]
pub enum IncomingMessage {
    /// A method call with an identifier that requires a response.
    Request(IncomingRequest),
    /// A method call without an identifier; no response is requested.
    Notification(IncomingNotification),
    /// Completion of a server-originated request, not a method call.
    Response(IncomingResponse),
}

/// A request cannot lose its required identifier by becoming a notification.
#[derive(Debug)]
pub struct IncomingRequest {
    /// Version observed on the wire, retained for application admission.
    pub jsonrpc: String,
    /// Integer and string identifiers remain distinct.
    pub id: JsonRpcId,
    /// Standard or extension method name.
    pub method: String,
    /// Raw method-specific parameters; interpretation belongs to the handler.
    pub params: Option<Value>,
}

impl IncomingRequest {
    /// Adapt only a method-bearing request to the existing dispatch interface.
    #[must_use]
    pub fn into_method_call(self) -> JsonRpcRequest {
        JsonRpcRequest {
            _jsonrpc: self.jsonrpc,
            id: Some(self.id),
            method: self.method,
            params: self.params,
        }
    }
}

/// A notification has no request identifier field.
#[derive(Debug)]
pub struct IncomingNotification {
    /// Version observed on the wire, retained for application admission.
    pub jsonrpc: String,
    /// Standard or extension method name.
    pub method: String,
    /// Raw method-specific parameters; interpretation belongs to the handler.
    pub params: Option<Value>,
}

impl IncomingNotification {
    /// Adapt only a method-bearing notification to the existing dispatch interface.
    #[must_use]
    pub fn into_method_call(self) -> JsonRpcRequest {
        JsonRpcRequest {
            _jsonrpc: self.jsonrpc,
            id: None,
            method: self.method,
            params: self.params,
        }
    }
}

/// A response has no method name and exactly one result or error payload.
#[derive(Debug)]
pub struct IncomingResponse {
    /// `None` represents an explicit JSON `null`, never an omitted identifier.
    /// Such a response is uncorrelatable; it cannot select a pending request.
    pub id: Option<JsonRpcId>,
    /// Successful `null` remains a result, not an absent payload.
    pub payload: IncomingResponsePayload,
}

/// Exactly one response outcome; success and failure cannot coexist.
#[derive(Debug)]
pub enum IncomingResponsePayload {
    /// Successful result, including JSON `null`.
    Result(Value),
    /// Validated error code, message, and optional data.
    Error(JsonRpcError),
}

/// Decode a complete body without replacing a response with a method name.
///
/// This preserves strict UTF-8 and the existing typed syntax/shape failures.
/// Responses require JSON-RPC 2.0, an explicit integer/string/null identifier,
/// and exactly one result or well-formed error. Method-bearing objects retain
/// the existing string-version and raw-parameter admission pending #9636.
pub fn decode_incoming_message(body: &[u8]) -> Result<IncomingMessage, IncomingMessageError> {
    let text = std::str::from_utf8(body).map_err(|error| IncomingMessageError::InvalidUtf8 {
        payload_bytes: body.len(),
        valid_up_to: error.valid_up_to(),
    })?;
    let value: Value = serde_json::from_str(text).map_err(|error| {
        IncomingMessageError::MalformedJson {
            payload_bytes: body.len(),
            line: error.line(),
            column: error.column(),
        }
    })?;
    let mut object = match value {
        Value::Object(object) => object,
        Value::Array(_) => {
            return Err(IncomingMessageError::UnsupportedBatch { payload_bytes: body.len() });
        }
        _ => return Err(invalid_shape(body.len(), None)),
    };
    let recovered_id = object.get("id").and_then(JsonRpcId::from_value);
    let shape_error = || invalid_shape(body.len(), recovered_id.clone());
    if object.contains_key("method") {
        if object.contains_key("result") || object.contains_key("error") {
            return Err(shape_error());
        }
        if !matches!(object.get("jsonrpc"), Some(Value::String(_))) {
            return Err(IncomingMessageError::InvalidJsonRpc {
                payload_bytes: body.len(),
                recoverable_id: recovered_id,
            });
        }
        let request: JsonRpcRequest =
            serde_json::from_value(Value::Object(object)).map_err(|_| shape_error())?;
        return Ok(match request.id {
            Some(id) => IncomingMessage::Request(IncomingRequest {
                jsonrpc: request._jsonrpc,
                id,
                method: request.method,
                params: request.params,
            }),
            None => IncomingMessage::Notification(IncomingNotification {
                jsonrpc: request._jsonrpc,
                method: request.method,
                params: request.params,
            }),
        });
    }

    // Membership, not value truthiness, distinguishes missing from result:null.
    if object.contains_key("result") == object.contains_key("error") {
        return Err(shape_error());
    }
    let id = match object.get("id") {
        Some(Value::Null) => None,
        Some(value) => Some(JsonRpcId::from_value(value).ok_or_else(shape_error)?),
        None => return Err(shape_error()),
    };
    match object.get("jsonrpc") {
        Some(Value::String(version)) if version == JSONRPC_VERSION => {}
        Some(Value::String(_)) => return Err(shape_error()),
        _ => {
            return Err(IncomingMessageError::InvalidJsonRpc {
                payload_bytes: body.len(),
                recoverable_id: recovered_id,
            });
        }
    }
    let payload = if let Some(result) = object.remove("result") {
        IncomingResponsePayload::Result(result)
    } else {
        let error = object.remove("error").ok_or_else(shape_error)?;
        IncomingResponsePayload::Error(decode_response_error(error).ok_or_else(shape_error)?)
    };
    Ok(IncomingMessage::Response(IncomingResponse { id, payload }))
}

fn invalid_shape(payload_bytes: usize, recoverable_id: Option<JsonRpcId>) -> IncomingMessageError {
    IncomingMessageError::InvalidMessageShape { payload_bytes, recoverable_id }
}

fn decode_response_error(value: Value) -> Option<JsonRpcError> {
    let Value::Object(mut object) = value else { return None };
    let code = object.get("code")?.as_i64().and_then(|code| i32::try_from(code).ok())?;
    let message = take_string(&mut object, "message")?;
    Some(JsonRpcError { code, message, data: object.remove("data") })
}

fn take_string(object: &mut Map<String, Value>, key: &str) -> Option<String> {
    match object.remove(key)? {
        Value::String(value) => Some(value),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::{ContentLengthMessageReader, frame, read_incoming_message_outcome};
    use std::io::{self, BufReader, Cursor, Read};

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    fn decoded(value: Value) -> Result<IncomingMessage, Box<dyn std::error::Error>> {
        Ok(decode_incoming_message(&serde_json::to_vec(&value)?)?)
    }

    #[test]
    fn requests_preserve_integer_string_and_extreme_ids() -> TestResult {
        for id in [serde_json::json!(i64::MIN), serde_json::json!(i64::MAX), serde_json::json!("7")] {
            let IncomingMessage::Request(request) = decoded(serde_json::json!({
                "jsonrpc": "2.0", "id": id, "method": "custom/query", "params": [1, "λ"]
            }))? else {
                return Err("request lost its direction".into());
            };
            assert_eq!(request.id.to_value(), id);
            let call = request.into_method_call();
            assert_eq!(call.id.as_ref().map(JsonRpcId::to_value), Some(id));
            assert_eq!(call.method, "custom/query");
            assert_eq!(call.params, Some(serde_json::json!([1, "λ"])));
        }
        Ok(())
    }

    #[test]
    fn notifications_and_explicit_null_request_ids_are_not_conflated() -> TestResult {
        let IncomingMessage::Notification(notification) = decoded(serde_json::json!({
            "jsonrpc": "2.0", "method": "initialized"
        }))? else {
            return Err("missing id must denote a notification".into());
        };
        assert!(notification.into_method_call().id.is_none());
        assert!(decode_incoming_message(br#"{"jsonrpc":"2.0","id":null,"method":"x"}"#).is_err());
        Ok(())
    }

    #[test]
    fn successful_null_is_present_and_is_not_an_error() -> TestResult {
        let IncomingMessage::Response(response) = decoded(serde_json::json!({
            "jsonrpc": "2.0", "id": "7", "result": null
        }))? else {
            return Err("response became a method".into());
        };
        assert_eq!(response.id, Some(JsonRpcId::String("7".into())));
        assert!(matches!(response.payload, IncomingResponsePayload::Result(Value::Null)));
        Ok(())
    }

    #[test]
    fn error_response_keeps_typed_code_message_and_data() -> TestResult {
        let IncomingMessage::Response(response) = decoded(serde_json::json!({
            "jsonrpc": "2.0", "id": 7,
            "error": {"code": -32603, "message": "failed", "data": {"retry": false}}
        }))? else {
            return Err("error response became a method".into());
        };
        let IncomingResponsePayload::Error(error) = response.payload else {
            return Err("error became success".into());
        };
        assert_eq!(response.id, Some(JsonRpcId::Integer(7)));
        assert_eq!(error.code, -32603);
        assert_eq!(error.message, "failed");
        assert_eq!(error.data, Some(serde_json::json!({"retry": false})));
        Ok(())
    }

    #[test]
    fn explicit_null_response_id_is_not_a_missing_id() -> TestResult {
        let IncomingMessage::Response(response) = decoded(serde_json::json!({
            "jsonrpc": "2.0", "id": null, "error": {"code": -32700, "message": "Parse error"}
        }))? else {
            return Err("null-id response lost its direction".into());
        };
        assert!(response.id.is_none());
        assert!(decode_incoming_message(br#"{"jsonrpc":"2.0","result":null}"#).is_err());
        Ok(())
    }

    #[test]
    fn invalid_response_and_mixed_envelopes_are_not_promoted_to_success() {
        for wire in [
            r#"{"id":7}"#,
            r#"{"jsonrpc":"2.0","id":7}"#,
            r#"{"jsonrpc":"2.0","id":7,"result":null,"error":null}"#,
            r#"{"jsonrpc":"2.0","id":7,"result":{},"error":{"code":1,"message":"x"}}"#,
            r#"{"jsonrpc":"2.0","id":7,"method":"x","result":{}}"#,
            r#"{"jsonrpc":"2.0","method":"x","error":null}"#,
            r#"{"jsonrpc":"2.0","id":true,"result":{}}"#,
            r#"{"jsonrpc":"2.0","id":1.5,"result":{}}"#,
            r#"{"jsonrpc":"2.0","id":18446744073709551615,"result":{}}"#,
            r#"{"jsonrpc":"2.0","id":7,"error":null}"#,
            r#"{"jsonrpc":"2.0","id":7,"error":{}}"#,
            r#"{"jsonrpc":"2.0","id":7,"error":{"code":1.5,"message":"x"}}"#,
            r#"{"jsonrpc":"2.0","id":7,"error":{"code":2147483648,"message":"x"}}"#,
            r#"{"jsonrpc":"2.0","id":7,"error":{"code":1,"message":false}}"#,
        ] {
            assert!(
                matches!(
                    decode_incoming_message(wire.as_bytes()),
                    Err(IncomingMessageError::InvalidMessageShape { .. })
                ),
                "invalid envelope was admitted: {wire}"
            );
            assert!(crate::transport::decode_incoming_body(wire.as_bytes()).is_err());
        }
    }

    #[test]
    fn response_version_is_checked_without_changing_method_version_admission() -> TestResult {
        for wire in [
            r#"{"id":7,"result":{}}"#,
            r#"{"jsonrpc":2,"id":7,"result":{}}"#,
        ] {
            assert!(matches!(
                decode_incoming_message(wire.as_bytes()),
                Err(IncomingMessageError::InvalidJsonRpc { .. })
            ));
        }
        assert!(matches!(
            decode_incoming_message(br#"{"jsonrpc":"1.0","id":7,"result":{}}"#),
            Err(IncomingMessageError::InvalidMessageShape { .. })
        ));
        let IncomingMessage::Request(request) = decoded(serde_json::json!({
            "jsonrpc": "1.0", "id": 7, "method": "x"
        }))? else {
            return Err("method-version admission changed outside #9636".into());
        };
        assert_eq!(request.jsonrpc, "1.0");
        Ok(())
    }

    #[test]
    fn syntax_encoding_batch_and_shape_remain_distinct() {
        for (body, stage) in [
            (&[0xff][..], super::super::IncomingMessageStage::Encoding),
            (&b"{bad"[..], super::super::IncomingMessageStage::Json),
            (&b"null"[..], super::super::IncomingMessageStage::JsonRpcShape),
        ] {
            let result = decode_incoming_message(body);
            assert!(matches!(result, Err(error) if error.stage() == stage));
        }
        assert!(matches!(
            decode_incoming_message(b"[]"),
            Err(IncomingMessageError::UnsupportedBatch { payload_bytes: 2 })
        ));
    }

    #[test]
    fn invalid_response_diagnostics_do_not_include_payloads() -> TestResult {
        let wire = br#"{"jsonrpc":"2.0","id":7,"error":{"code":false,"message":"private-token-7626"}}"#;
        let error = decode_incoming_message(wire).err().ok_or("invalid response was admitted")?;
        assert_eq!(error.recoverable_id(), Some(&JsonRpcId::Integer(7)));
        assert_eq!(error.payload_bytes(), Some(wire.len()));
        assert!(!format!("{error:?} {error}").contains("private-token-7626"));
        Ok(())
    }

    #[test]
    fn stateful_reader_preserves_mixed_directions_at_every_chunk_size() -> TestResult {
        struct Chunked<'a> {
            remaining: &'a [u8],
            chunk: usize,
        }
        impl Read for Chunked<'_> {
            fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
                let count = self.remaining.len().min(self.chunk).min(output.len());
                output[..count].copy_from_slice(&self.remaining[..count]);
                self.remaining = &self.remaining[count..];
                Ok(count)
            }
        }
        let mut wire = frame(br#"{"jsonrpc":"2.0","id":"b","result":null}"#);
        wire.extend(frame(&[0xff]));
        wire.extend(frame(br#"{"jsonrpc":"2.0","method":"initialized"}"#));
        wire.extend(frame(br#"{"jsonrpc":"2.0","id":"a","error":{"code":-1,"message":"x"}}"#));
        wire.extend(frame(br#"{"jsonrpc":"2.0","id":7,"method":"custom/query"}"#));
        for chunk in 1..=wire.len() {
            let mut stream = Chunked { remaining: &wire, chunk };
            let mut reader = ContentLengthMessageReader::new();
            assert!(matches!(
                reader.read_next_message_outcome(&mut stream)?,
                Some(Ok(IncomingMessage::Response(_)))
            ));
            assert!(matches!(
                reader.read_next_message_outcome(&mut stream)?,
                Some(Err(IncomingMessageError::InvalidUtf8 { .. }))
            ));
            assert!(matches!(
                reader.read_next_message_outcome(&mut stream)?,
                Some(Ok(IncomingMessage::Notification(_)))
            ));
            assert!(matches!(
                reader.read_next_message_outcome(&mut stream)?,
                Some(Ok(IncomingMessage::Response(_)))
            ));
            assert!(matches!(
                reader.read_next_message_outcome(&mut stream)?,
                Some(Ok(IncomingMessage::Request(_)))
            ));
            assert!(reader.read_next_message_outcome(&mut stream)?.is_none());
        }
        Ok(())
    }

    #[test]
    fn one_shot_reader_keeps_the_following_message() -> TestResult {
        let mut wire = frame(br#"{"jsonrpc":"2.0","id":7,"result":null}"#);
        wire.extend(frame(br#"{"jsonrpc":"2.0","id":"7","method":"shutdown"}"#));
        let mut stream = BufReader::new(Cursor::new(wire));
        assert!(matches!(
            read_incoming_message_outcome(&mut stream)?,
            Some(Ok(IncomingMessage::Response(_)))
        ));
        let Some(Ok(IncomingMessage::Request(request))) = read_incoming_message_outcome(&mut stream)?
        else {
            return Err("following method-bearing message was lost".into());
        };
        assert_eq!(request.id, JsonRpcId::String("7".into()));
        assert!(read_incoming_message_outcome(&mut stream)?.is_none());
        Ok(())
    }

    #[test]
    fn method_spelling_never_changes_a_notification_into_a_response() -> TestResult {
        let IncomingMessage::Notification(notification) = decoded(serde_json::json!({
            "jsonrpc": "2.0", "method": "$/perl-lsp/clientResponse",
            "params": {"id": 7, "result": null}
        }))? else {
            return Err("method spelling was mistaken for envelope direction".into());
        };
        assert_eq!(notification.method, "$/perl-lsp/clientResponse");
        Ok(())
    }

    #[test]
    fn response_ids_do_not_require_a_pending_registry_entry_at_decode() -> TestResult {
        for id in [serde_json::json!(7), serde_json::json!("7"), serde_json::json!("unknown")] {
            let IncomingMessage::Response(response) = decoded(serde_json::json!({
                "jsonrpc": "2.0", "id": id, "result": {}
            }))? else {
                return Err("response was routed by its identifier".into());
            };
            assert_eq!(response.id.as_ref().map(JsonRpcId::to_value), Some(id));
        }
        Ok(())
    }

    #[test]
    fn error_code_boundaries_and_optional_null_data_survive() -> TestResult {
        for code in [i32::MIN, i32::MAX] {
            let IncomingMessage::Response(response) = decoded(serde_json::json!({
                "jsonrpc": "2.0", "id": 7,
                "error": {"code": code, "message": "x", "data": null}
            }))? else {
                return Err("valid error code boundary was rejected".into());
            };
            let IncomingResponsePayload::Error(error) = response.payload else {
                return Err("client error became success".into());
            };
            assert_eq!(error.code, code);
            assert_eq!(error.data, Some(Value::Null));
        }
        Ok(())
    }

    #[test]
    fn legacy_projection_preserves_standard_response_fields() -> TestResult {
        for (wire, expected) in [
            (serde_json::json!({"jsonrpc": "2.0", "id": "7", "result": null}),
             serde_json::json!({"id": "7", "result": null, "error": null})),
            (serde_json::json!({"jsonrpc": "2.0", "id": 7,
                "error": {"code": -32603, "message": "failed", "data": [1, 2]}}),
             serde_json::json!({"id": 7, "result": null,
                "error": {"code": -32603, "message": "failed", "data": [1, 2]}})),
            (serde_json::json!({"jsonrpc": "2.0", "id": null,
                "error": {"code": -32700, "message": "Parse error"}}),
             serde_json::json!({"id": null, "result": null,
                "error": {"code": -32700, "message": "Parse error"}})),
        ] {
            let call = crate::transport::decode_incoming_body(&serde_json::to_vec(&wire)?)?;
            assert!(call.id.is_none());
            assert_eq!(call.method, "$/perl-lsp/clientResponse");
            assert_eq!(call.params, Some(expected));
        }
        Ok(())
    }
}
