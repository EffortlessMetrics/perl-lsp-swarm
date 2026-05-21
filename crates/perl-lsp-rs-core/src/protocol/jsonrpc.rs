//! JSON-RPC 2.0 message types
//!
//! Core request, response, and error types for JSON-RPC communication.

use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as DeError};
use serde_json::Value;
use std::fmt;

/// JSON-RPC 2.0 request identifier.
///
/// Per the JSON-RPC 2.0 specification, request IDs are either an integer or a
/// string. The specification also permits `null`, but using `null` as an ID is
/// discouraged because it cannot be distinguished from a malformed response;
/// LSP servers commonly treat `null`-id requests as notifications.
///
/// This type encodes the *valid* shapes of a request ID and refuses everything
/// else at the serde boundary (fractional numbers, objects, arrays, `null`).
/// That makes it impossible to construct a `JsonRpcId` whose shape we'd later
/// have to defend against deep inside the dispatcher.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum JsonRpcId {
    /// Integer identifier (the common case for LSP clients).
    Integer(i64),
    /// String identifier (used by some clients, e.g. for human-readable IDs).
    String(String),
}

impl JsonRpcId {
    /// Construct an integer ID. Provided for ergonomics in tests and callers
    /// that already have an `i64`.
    #[must_use]
    pub fn integer(id: i64) -> Self {
        Self::Integer(id)
    }

    /// Construct a string ID. Provided for ergonomics in tests.
    pub fn string(id: impl Into<String>) -> Self {
        Self::String(id.into())
    }

    /// Render this ID as a JSON value, suitable for echoing back in a response
    /// or for embedding in transport-layer payloads.
    #[must_use]
    pub fn to_value(&self) -> Value {
        match self {
            Self::Integer(raw) => Value::from(*raw),
            Self::String(raw) => Value::from(raw.clone()),
        }
    }

    /// Attempt to convert a [`Value`] into a valid request ID. Returns `None`
    /// for `null`, fractional numbers, objects, arrays, or anything else that
    /// is not a valid request ID shape.
    #[must_use]
    pub fn try_from_value(value: &Value) -> Option<Self> {
        match value {
            Value::Number(n) => n.as_i64().map(Self::Integer),
            Value::String(s) => Some(Self::String(s.clone())),
            _ => None,
        }
    }
}

impl fmt::Display for JsonRpcId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Integer(raw) => write!(f, "{raw}"),
            Self::String(raw) => write!(f, "{raw}"),
        }
    }
}

impl From<&JsonRpcId> for Value {
    fn from(id: &JsonRpcId) -> Self {
        id.to_value()
    }
}

impl From<JsonRpcId> for Value {
    fn from(id: JsonRpcId) -> Self {
        id.to_value()
    }
}

impl Serialize for JsonRpcId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::Integer(raw) => serializer.serialize_i64(*raw),
            Self::String(raw) => serializer.serialize_str(raw),
        }
    }
}

impl<'de> Deserialize<'de> for JsonRpcId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        deserialize_id_from_value::<D>(value)
    }
}

fn deserialize_id_from_value<'de, D>(value: Value) -> Result<JsonRpcId, D::Error>
where
    D: Deserializer<'de>,
{
    match value {
        Value::Number(n) => n.as_i64().map(JsonRpcId::Integer).ok_or_else(|| {
            D::Error::custom("JSON-RPC request id must be an integer (no fractional numbers)")
        }),
        Value::String(s) => Ok(JsonRpcId::String(s)),
        Value::Null => Err(D::Error::custom("JSON-RPC request id must not be null")),
        Value::Bool(_) => Err(D::Error::custom("JSON-RPC request id must not be a boolean")),
        Value::Array(_) => Err(D::Error::custom("JSON-RPC request id must not be an array")),
        Value::Object(_) => Err(D::Error::custom("JSON-RPC request id must not be an object")),
    }
}

/// Strict deserializer for `JsonRpcRequest::id`.
///
/// LSP request IDs are either integer or string. A missing `id` means the
/// message is a notification (handled via `Option::None`). Explicit `null` is
/// rejected because there is no valid response shape for it. Fractional
/// numbers, objects, and arrays are also rejected so we can never accept a
/// well-formed-but-uninterpretable request ID and then fail later when trying
/// to match the response.
///
/// Use via:
/// ```ignore
/// #[serde(default, deserialize_with = "deserialize_optional_request_id_as_value")]
/// pub id: Option<Value>,
/// ```
pub fn deserialize_optional_request_id_as_value<'de, D>(
    deserializer: D,
) -> Result<Option<Value>, D::Error>
where
    D: Deserializer<'de>,
{
    // Read the raw Value directly: the surrounding `#[serde(default)]` already
    // handles a missing field by producing `None`, so when this function is
    // called the field IS present and we must reject `null` explicitly rather
    // than silently treating it as a notification.
    let value = Value::deserialize(deserializer)?;
    let id = deserialize_id_from_value::<D>(value)?;
    // Re-render the validated id as a Value so existing dispatch/cancellation
    // surfaces that key off Value continue to work without further migration
    // in this PR.
    Ok(Some(id.to_value()))
}

/// JSON-RPC 2.0 request message
///
/// Represents an incoming request from the LSP client.
/// The `id` field is `None` for notifications and is rejected as malformed for
/// invalid shapes (null, fractional, object, array) at deserialization time.
#[derive(Debug, Deserialize)]
pub struct JsonRpcRequest {
    /// JSON-RPC version (always "2.0")
    #[serde(rename = "jsonrpc")]
    pub _jsonrpc: String,

    /// Request identifier (None for notifications)
    #[serde(default, deserialize_with = "deserialize_optional_request_id_as_value")]
    pub id: Option<Value>,

    /// Method name to invoke
    pub method: String,

    /// Method parameters
    pub params: Option<Value>,
}

/// JSON-RPC 2.0 response message
///
/// Represents an outgoing response to the LSP client.
/// Either `result` or `error` should be set, but not both.
#[derive(Debug, Serialize)]
pub struct JsonRpcResponse {
    /// JSON-RPC version (always "2.0")
    pub jsonrpc: String,

    /// Request identifier (matches the request's id)
    pub id: Option<Value>,

    /// Success result (mutually exclusive with error)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,

    /// Error result (mutually exclusive with result)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

impl JsonRpcResponse {
    /// Create a success response
    pub fn success(id: Option<Value>, result: Value) -> Self {
        Self { jsonrpc: "2.0".to_string(), id, result: Some(result), error: None }
    }

    /// Create an error response
    pub fn error(id: Option<Value>, error: JsonRpcError) -> Self {
        Self { jsonrpc: "2.0".to_string(), id, result: None, error: Some(error) }
    }

    /// Create a null result response (for methods that return nothing)
    pub fn null(id: Option<Value>) -> Self {
        Self { jsonrpc: "2.0".to_string(), id, result: Some(Value::Null), error: None }
    }
}

/// JSON-RPC 2.0 error object
///
/// Represents an error that occurred during request processing.
#[derive(Debug, Serialize, Clone)]
pub struct JsonRpcError {
    /// Error code (see protocol/errors.rs for standard codes)
    pub code: i32,

    /// Human-readable error message
    pub message: String,

    /// Additional error data (optional)
    pub data: Option<Value>,
}

impl JsonRpcError {
    /// Create a new error
    pub fn new(code: i32, message: impl Into<String>) -> Self {
        Self { code, message: message.into(), data: None }
    }

    /// Create an error with additional data
    pub fn with_data(code: i32, message: impl Into<String>, data: Value) -> Self {
        Self { code, message: message.into(), data: Some(data) }
    }
}

impl std::fmt::Display for JsonRpcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for JsonRpcError {}

/// Server-generated request identifier for outbound (server→client) requests.
///
/// LSP4IJ and other clients reject server-generated request IDs that exceed
/// 32-bit signed range (see issue: file watcher registration crash when the
/// server used `SystemTime` epoch-millis as the ID). All outbound request IDs
/// must therefore fit in `i32`; making that an invariant of a dedicated type
/// means the allocator is the only place that can construct one, and the
/// outbound transport can refuse anything else by typing.
///
/// The wire representation is a positive 32-bit integer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ServerRequestId(i32);

impl ServerRequestId {
    /// Construct a `ServerRequestId` from a raw integer. Returns `None` for
    /// non-positive values; reserving zero and negatives means we can never
    /// emit `0` (which some clients also reject) and gives us room for
    /// "uninitialized" sentinels elsewhere.
    #[must_use]
    pub fn new(id: i32) -> Option<Self> {
        if id > 0 { Some(Self(id)) } else { None }
    }

    /// Get the raw `i32` wire value.
    #[must_use]
    pub fn as_i32(self) -> i32 {
        self.0
    }

    /// Get the value as `i64` for callers that need to mix with `Value::Number`.
    #[must_use]
    pub fn as_i64(self) -> i64 {
        i64::from(self.0)
    }

    /// Attempt to interpret a `JsonRpcId` as a `ServerRequestId`. Returns
    /// `None` for string IDs or for integer IDs outside positive i32 range.
    /// Used by the client-response path that turns inbound responses back
    /// into the server's allocator domain.
    #[must_use]
    pub fn from_json_rpc_id(id: &JsonRpcId) -> Option<Self> {
        match id {
            JsonRpcId::Integer(raw) => {
                let narrowed = i32::try_from(*raw).ok()?;
                Self::new(narrowed)
            }
            JsonRpcId::String(_) => None,
        }
    }

    /// Attempt to interpret a JSON `Value` as a `ServerRequestId`.
    #[must_use]
    pub fn from_value(value: &Value) -> Option<Self> {
        let raw = value.as_i64()?;
        let narrowed = i32::try_from(raw).ok()?;
        Self::new(narrowed)
    }
}

impl fmt::Display for ServerRequestId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<ServerRequestId> for Value {
    fn from(id: ServerRequestId) -> Self {
        Value::from(id.as_i32())
    }
}

impl From<ServerRequestId> for JsonRpcId {
    fn from(id: ServerRequestId) -> Self {
        JsonRpcId::Integer(id.as_i64())
    }
}

impl Serialize for ServerRequestId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_i32(self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn parse_id_field(raw: &str) -> Result<Option<Value>, serde_json::Error> {
        #[derive(Deserialize)]
        struct Probe {
            #[serde(default, deserialize_with = "deserialize_optional_request_id_as_value")]
            id: Option<Value>,
        }
        let parsed: Probe = serde_json::from_str(raw)?;
        Ok(parsed.id)
    }

    #[test]
    fn json_rpc_request_accepts_integer_id() {
        let req: JsonRpcRequest =
            serde_json::from_str(r#"{"jsonrpc":"2.0","id":42,"method":"textDocument/hover"}"#)
                .expect("integer ids should be accepted");
        assert_eq!(req.id, Some(json!(42)));
    }

    #[test]
    fn json_rpc_request_accepts_string_id() {
        let req: JsonRpcRequest = serde_json::from_str(
            r#"{"jsonrpc":"2.0","id":"abc-123","method":"textDocument/hover"}"#,
        )
        .expect("string ids should be accepted");
        assert_eq!(req.id, Some(json!("abc-123")));
    }

    #[test]
    fn json_rpc_request_accepts_missing_id_as_notification() {
        let req: JsonRpcRequest =
            serde_json::from_str(r#"{"jsonrpc":"2.0","method":"initialized"}"#)
                .expect("missing id should be treated as a notification");
        assert_eq!(req.id, None);
    }

    #[test]
    fn json_rpc_request_rejects_null_id() {
        let err = serde_json::from_str::<JsonRpcRequest>(
            r#"{"jsonrpc":"2.0","id":null,"method":"textDocument/hover"}"#,
        )
        .expect_err("explicit null id must be rejected, not silently treated as notification");
        assert!(err.to_string().contains("null"), "error should explain why: {err}");
    }

    #[test]
    fn json_rpc_request_rejects_fractional_id() {
        let err = serde_json::from_str::<JsonRpcRequest>(
            r#"{"jsonrpc":"2.0","id":3.5,"method":"textDocument/hover"}"#,
        )
        .expect_err("fractional ids must be rejected");
        assert!(err.to_string().contains("integer"), "error should explain why: {err}");
    }

    #[test]
    fn json_rpc_request_rejects_object_id() {
        let err = serde_json::from_str::<JsonRpcRequest>(
            r#"{"jsonrpc":"2.0","id":{"foo":1},"method":"textDocument/hover"}"#,
        )
        .expect_err("object ids must be rejected");
        assert!(err.to_string().contains("object"), "error should explain why: {err}");
    }

    #[test]
    fn json_rpc_request_rejects_array_id() {
        let err = serde_json::from_str::<JsonRpcRequest>(
            r#"{"jsonrpc":"2.0","id":[1,2],"method":"textDocument/hover"}"#,
        )
        .expect_err("array ids must be rejected");
        assert!(err.to_string().contains("array"), "error should explain why: {err}");
    }

    #[test]
    fn json_rpc_response_echoes_string_id() {
        let response = JsonRpcResponse::success(Some(json!("abc-123")), json!({"ok": true}));
        let serialized = serde_json::to_value(&response).expect("response should serialize");
        assert_eq!(serialized["id"], json!("abc-123"));
    }

    #[test]
    fn deserialize_optional_request_id_helper_accepts_valid_shapes() {
        assert_eq!(parse_id_field(r#"{"id":7}"#).expect("integer should parse"), Some(json!(7)));
        assert_eq!(
            parse_id_field(r#"{"id":"abc"}"#).expect("string should parse"),
            Some(json!("abc"))
        );
        assert_eq!(parse_id_field(r#"{}"#).expect("missing should parse"), None);
    }

    #[test]
    fn deserialize_optional_request_id_helper_rejects_null() {
        let err = parse_id_field(r#"{"id":null}"#).expect_err("null must be rejected");
        assert!(err.to_string().contains("null"));
    }

    #[test]
    fn json_rpc_id_to_value_round_trips_integer() {
        let id = JsonRpcId::integer(99);
        assert_eq!(id.to_value(), json!(99));
        assert_eq!(JsonRpcId::try_from_value(&json!(99)), Some(JsonRpcId::Integer(99)));
    }

    #[test]
    fn json_rpc_id_to_value_round_trips_string() {
        let id = JsonRpcId::string("xyz");
        assert_eq!(id.to_value(), json!("xyz"));
        assert_eq!(
            JsonRpcId::try_from_value(&json!("xyz")),
            Some(JsonRpcId::String("xyz".to_string()))
        );
    }

    #[test]
    fn json_rpc_id_try_from_value_rejects_invalid_shapes() {
        assert_eq!(JsonRpcId::try_from_value(&json!(null)), None);
        assert_eq!(JsonRpcId::try_from_value(&json!(3.5)), None);
        assert_eq!(JsonRpcId::try_from_value(&json!({})), None);
        assert_eq!(JsonRpcId::try_from_value(&json!([])), None);
        assert_eq!(JsonRpcId::try_from_value(&json!(true)), None);
    }

    #[test]
    fn server_request_id_new_rejects_non_positive() {
        assert!(ServerRequestId::new(0).is_none(), "zero should be rejected");
        assert!(ServerRequestId::new(-1).is_none(), "negative should be rejected");
    }

    #[test]
    fn server_request_id_new_accepts_positive_i32() {
        let id = ServerRequestId::new(1).expect("one is valid");
        assert_eq!(id.as_i32(), 1);
        let id = ServerRequestId::new(i32::MAX).expect("i32 max is valid");
        assert_eq!(id.as_i32(), i32::MAX);
    }

    #[test]
    fn server_request_id_from_value_rejects_overflow() {
        let overflow = i64::from(i32::MAX) + 1;
        assert!(
            ServerRequestId::from_value(&json!(overflow)).is_none(),
            "ids above i32::MAX must be rejected (LSP4IJ crash root cause)"
        );
    }

    #[test]
    fn server_request_id_from_value_rejects_negative() {
        assert!(ServerRequestId::from_value(&json!(-5)).is_none());
        assert!(ServerRequestId::from_value(&json!(0)).is_none());
    }

    #[test]
    fn server_request_id_serializes_as_plain_integer() {
        let id = ServerRequestId::new(123).expect("positive id");
        let serialized = serde_json::to_value(id).expect("serializable");
        assert_eq!(serialized, json!(123));
    }

    #[test]
    fn server_request_id_from_json_rpc_id_round_trips_integers() {
        let id = JsonRpcId::Integer(77);
        assert_eq!(ServerRequestId::from_json_rpc_id(&id).map(|s| s.as_i32()), Some(77));
    }

    #[test]
    fn server_request_id_from_json_rpc_id_refuses_strings() {
        let id = JsonRpcId::String("client-7".to_string());
        assert_eq!(ServerRequestId::from_json_rpc_id(&id), None);
    }
}
