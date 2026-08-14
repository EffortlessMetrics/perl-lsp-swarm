//! Versioned, bounded validation for JSON-RPC and LSP method payloads.
//!
//! The validator is intentionally independent of provider semantics. It proves
//! that a captured message has the correct envelope, direction, protocol
//! version, and method-specific payload shape. Exact-process coverage is wired
//! separately by #7116.

mod methods;
mod payloads;

use serde_json::{Map, Value};
use std::error::Error;
use std::fmt;

pub use methods::registered_schema_identities;

/// Pinned upstream protocol source used by the checked registry.
pub const SCHEMA_SOURCE_JSON: &str = include_str!("../../../protocol_schema_source.json");
/// Upstream `gh-pages` commit containing the pinned 3.17 and 3.18 specifications.
pub const UPSTREAM_PROTOCOL_COMMIT: &str = "8d5153933153aed3a488b9b8f46b22ed0f90f552";

/// Direction of a protocol message on the LSP connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Direction {
    /// The editor/client sent the message to `perllsp`.
    ClientToServer,
    /// `perllsp` sent the message to the editor/client.
    ServerToClient,
}

impl Direction {
    pub(crate) const fn schema_token(self) -> &'static str {
        match self {
            Self::ClientToServer => "client_to_server",
            Self::ServerToClient => "server_to_client",
        }
    }

    const fn opposite(self) -> Self {
        match self {
            Self::ClientToServer => Self::ServerToClient,
            Self::ServerToClient => Self::ClientToServer,
        }
    }
}

/// Protocol version authority for one method schema.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ProtocolVersion {
    /// Stable Language Server Protocol 3.17 surface.
    Lsp317,
    /// Individually selected LSP 3.18 method while upstream remains under development.
    Lsp318Development,
    /// Project-specific extension method.
    PerlLspExtension,
}

impl ProtocolVersion {
    pub(crate) const fn schema_token(self) -> &'static str {
        match self {
            Self::Lsp317 => "3.17",
            Self::Lsp318Development => "3.18-development",
            Self::PerlLspExtension => "perl-lsp-extension",
        }
    }
}

/// JSON-RPC envelope class.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum MessageKind {
    /// Request with an integer or string ID.
    Request,
    /// Notification without an ID.
    Notification,
    /// Successful response containing `result`.
    SuccessResponse,
    /// Error response containing `error`.
    ErrorResponse,
}

impl MessageKind {
    pub(crate) const fn schema_token(self) -> &'static str {
        match self {
            Self::Request => "request",
            Self::Notification => "notification",
            Self::SuccessResponse => "success_response",
            Self::ErrorResponse => "error_response",
        }
    }

    const fn is_response(self) -> bool {
        matches!(self, Self::SuccessResponse | Self::ErrorResponse)
    }
}

/// Validation context supplied by a capture harness.
#[derive(Debug, Clone, Copy)]
pub struct ValidationContext<'a> {
    /// Actual direction of this captured message.
    pub direction: Direction,
    /// Method identity. Required for responses because JSON-RPC responses do not carry it.
    pub method: Option<&'a str>,
    /// Whether an individually registered 3.18-development method is allowed.
    pub allow_lsp_318_development: bool,
}

/// Limits applied before shape validation.
#[derive(Debug, Clone, Copy)]
pub struct ValidationLimits {
    /// Maximum JSON nesting depth.
    pub max_depth: usize,
    /// Maximum total JSON nodes.
    pub max_nodes: usize,
    /// Maximum bytes in any individual string or object key.
    pub max_string_bytes: usize,
}

impl Default for ValidationLimits {
    fn default() -> Self {
        Self { max_depth: 64, max_nodes: 200_000, max_string_bytes: 1 << 20 }
    }
}

/// Successful method/direction/schema classification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedMessage {
    /// Method associated with the message.
    pub method: String,
    /// Actual message direction.
    pub direction: Direction,
    /// JSON-RPC envelope class.
    pub kind: MessageKind,
    /// Protocol version authority used for validation.
    pub version: ProtocolVersion,
}

/// Precise protocol validation failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaError {
    /// Method being validated, when known.
    pub method: Option<String>,
    /// Stable JSON-path-like location of the failure.
    pub path: String,
    /// Expected shape or rule.
    pub expected: String,
    /// Bounded observed type/value summary.
    pub observed: String,
}

impl SchemaError {
    pub(crate) fn new(
        method: Option<&str>,
        path: impl Into<String>,
        expected: impl Into<String>,
        observed: impl Into<String>,
    ) -> Self {
        Self {
            method: method.map(str::to_string),
            path: path.into(),
            expected: expected.into(),
            observed: observed.into(),
        }
    }

    pub(crate) fn at_value(
        method: Option<&str>,
        path: impl Into<String>,
        expected: impl Into<String>,
        value: &Value,
    ) -> Self {
        Self::new(method, path, expected, observed(value))
    }
}

impl fmt::Display for SchemaError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(method) = &self.method {
            write!(
                formatter,
                "{method} {}: expected {}; observed {}",
                self.path, self.expected, self.observed
            )
        } else {
            write!(
                formatter,
                "{}: expected {}; observed {}",
                self.path, self.expected, self.observed
            )
        }
    }
}

impl Error for SchemaError {}

/// Reusable validator for captured protocol messages.
#[derive(Debug, Clone, Copy, Default)]
pub struct ProtocolSchemaValidator {
    limits: ValidationLimits,
}

impl ProtocolSchemaValidator {
    /// Construct a validator with explicit bounded-input limits.
    #[must_use]
    pub const fn with_limits(limits: ValidationLimits) -> Self {
        Self { limits }
    }

    /// Validate one JSON-RPC/LSP message against its method and direction.
    pub fn validate(
        &self,
        message: &Value,
        context: ValidationContext<'_>,
    ) -> Result<ValidatedMessage, SchemaError> {
        validate_limits(message, self.limits)?;
        let object = expect_object(None, "$", message)?;
        expect_exact_string(None, "$.jsonrpc", object.get("jsonrpc"), "2.0")?;

        let kind = classify_envelope(object)?;
        let envelope_method = object.get("method").and_then(Value::as_str);
        let method = match kind {
            MessageKind::Request | MessageKind::Notification => {
                let method = envelope_method
                    .ok_or_else(|| SchemaError::new(None, "$.method", "string", "missing"))?;
                if let Some(expected) = context.method
                    && expected != method
                {
                    return Err(SchemaError::new(Some(method), "$.method", expected, method));
                }
                method
            }
            MessageKind::SuccessResponse | MessageKind::ErrorResponse => {
                context.method.ok_or_else(|| {
                    SchemaError::new(
                        None,
                        "$.method",
                        "capture-supplied response method",
                        "missing",
                    )
                })?
            }
        };

        validate_id(method, kind, object.get("id"))?;
        validate_envelope_members(method, kind, object)?;

        // Method schemas are registered in the originating request/notification
        // direction. A response frame travels in the opposite direction, so
        // lookup must invert only for responses while preserving the actual
        // captured direction in the validated result.
        let schema_direction =
            if kind.is_response() { context.direction.opposite() } else { context.direction };
        let Some(schema) = methods::schema_for(method, schema_direction, kind) else {
            if is_project_extension(method) {
                validate_extension_payload(method, kind, object)?;
                return Ok(ValidatedMessage {
                    method: method.to_string(),
                    direction: context.direction,
                    kind,
                    version: ProtocolVersion::PerlLspExtension,
                });
            }
            return Err(SchemaError::new(
                Some(method),
                "$.method",
                "registered method/direction schema",
                method,
            ));
        };

        if schema.version == ProtocolVersion::Lsp318Development
            && !context.allow_lsp_318_development
        {
            return Err(SchemaError::new(
                Some(method),
                "$.method",
                "explicitly enabled 3.18-development method",
                "3.18-development disabled",
            ));
        }

        match kind {
            MessageKind::Request | MessageKind::Notification => {
                let params = object.get("params").unwrap_or(&Value::Null);
                (schema.params)(method, params)?;
            }
            MessageKind::SuccessResponse => {
                let result = object.get("result").ok_or_else(|| {
                    SchemaError::new(Some(method), "$.result", "required success result", "missing")
                })?;
                let validator = schema.result.ok_or_else(|| {
                    SchemaError::new(
                        Some(method),
                        "$.result",
                        "no response for notification",
                        observed(result),
                    )
                })?;
                validator(method, result)?;
            }
            MessageKind::ErrorResponse => validate_error_object(
                method,
                object.get("error").ok_or_else(|| {
                    SchemaError::new(Some(method), "$.error", "error object", "missing")
                })?,
            )?,
        }

        Ok(ValidatedMessage {
            method: method.to_string(),
            direction: context.direction,
            kind,
            version: schema.version,
        })
    }
}

fn classify_envelope(object: &Map<String, Value>) -> Result<MessageKind, SchemaError> {
    let has_method = object.get("method").is_some();
    let has_id = object.get("id").is_some();
    let has_result = object.get("result").is_some();
    let has_error = object.get("error").is_some();

    match (has_method, has_id, has_result, has_error) {
        (true, true, false, false) => Ok(MessageKind::Request),
        (true, false, false, false) => Ok(MessageKind::Notification),
        (false, true, true, false) => Ok(MessageKind::SuccessResponse),
        (false, true, false, true) => Ok(MessageKind::ErrorResponse),
        _ => Err(SchemaError::new(
            None,
            "$",
            "one request, notification, success-response, or error-response envelope",
            format!("method={has_method},id={has_id},result={has_result},error={has_error}"),
        )),
    }
}

fn validate_id(method: &str, kind: MessageKind, id: Option<&Value>) -> Result<(), SchemaError> {
    match kind {
        MessageKind::Request => match id {
            Some(Value::Number(number)) if number.as_i64().is_some() => Ok(()),
            Some(Value::String(_)) => Ok(()),
            Some(value) => Err(SchemaError::at_value(
                Some(method),
                "$.id",
                "integer or string request ID",
                value,
            )),
            None => Err(SchemaError::new(Some(method), "$.id", "request ID", "missing")),
        },
        MessageKind::Notification => {
            if id.is_none() {
                Ok(())
            } else {
                Err(SchemaError::new(Some(method), "$.id", "no notification ID", "present"))
            }
        }
        MessageKind::SuccessResponse | MessageKind::ErrorResponse => match id {
            Some(Value::Number(number)) if number.as_i64().is_some() => Ok(()),
            Some(Value::String(_) | Value::Null) => Ok(()),
            Some(value) => Err(SchemaError::at_value(
                Some(method),
                "$.id",
                "integer, string, or null response ID",
                value,
            )),
            None => Err(SchemaError::new(Some(method), "$.id", "response ID", "missing")),
        },
    }
}

fn validate_envelope_members(
    method: &str,
    kind: MessageKind,
    object: &Map<String, Value>,
) -> Result<(), SchemaError> {
    if matches!(kind, MessageKind::Request | MessageKind::Notification)
        && let Some(params) = object.get("params")
        && !params.is_object()
        && !params.is_array()
    {
        return Err(SchemaError::at_value(
            Some(method),
            "$.params",
            "object or array when present",
            params,
        ));
    }
    Ok(())
}

fn validate_error_object(method: &str, value: &Value) -> Result<(), SchemaError> {
    let object = expect_object(Some(method), "$.error", value)?;
    expect_integer(Some(method), "$.error.code", object.get("code"))?;
    expect_string(Some(method), "$.error.message", object.get("message"))?;
    Ok(())
}

fn validate_extension_payload(
    method: &str,
    kind: MessageKind,
    object: &Map<String, Value>,
) -> Result<(), SchemaError> {
    match kind {
        MessageKind::Request | MessageKind::Notification => {
            if let Some(params) = object.get("params")
                && !params.is_object()
                && !params.is_array()
            {
                return Err(SchemaError::at_value(
                    Some(method),
                    "$.params",
                    "extension object or array",
                    params,
                ));
            }
        }
        MessageKind::SuccessResponse => {
            let _ = object.get("result").ok_or_else(|| {
                SchemaError::new(Some(method), "$.result", "extension result", "missing")
            })?;
        }
        MessageKind::ErrorResponse => validate_error_object(method, &object["error"])?,
    }
    Ok(())
}

fn is_project_extension(method: &str) -> bool {
    method.starts_with("$/perl-lsp/") || method.starts_with("perl/")
}

fn validate_limits(value: &Value, limits: ValidationLimits) -> Result<(), SchemaError> {
    let mut stack = vec![(value, 0usize, "$".to_string())];
    let mut nodes = 0usize;
    while let Some((value, depth, path)) = stack.pop() {
        nodes = nodes.saturating_add(1);
        if nodes > limits.max_nodes {
            return Err(SchemaError::new(
                None,
                path,
                format!("at most {} JSON nodes", limits.max_nodes),
                format!(">{} nodes", limits.max_nodes),
            ));
        }
        if depth > limits.max_depth {
            return Err(SchemaError::new(
                None,
                path,
                format!("depth at most {}", limits.max_depth),
                depth.to_string(),
            ));
        }
        match value {
            Value::String(text) if text.len() > limits.max_string_bytes => {
                return Err(SchemaError::new(
                    None,
                    path,
                    format!("string at most {} bytes", limits.max_string_bytes),
                    format!("{} bytes", text.len()),
                ));
            }
            Value::Array(values) => {
                for (index, child) in values.iter().enumerate().rev() {
                    stack.push((child, depth + 1, format!("{path}[{index}]")));
                }
            }
            Value::Object(values) => {
                for (key, child) in values.iter().rev() {
                    if key.len() > limits.max_string_bytes {
                        return Err(SchemaError::new(
                            None,
                            format!("{path}.<key>"),
                            format!("key at most {} bytes", limits.max_string_bytes),
                            format!("{} bytes", key.len()),
                        ));
                    }
                    stack.push((child, depth + 1, format!("{path}.{key}")));
                }
            }
            _ => {}
        }
    }
    Ok(())
}

pub(super) fn expect_object<'a>(
    method: Option<&str>,
    path: &str,
    value: &'a Value,
) -> Result<&'a Map<String, Value>, SchemaError> {
    value.as_object().ok_or_else(|| SchemaError::at_value(method, path, "object", value))
}

pub(super) fn expect_array<'a>(
    method: Option<&str>,
    path: &str,
    value: &'a Value,
) -> Result<&'a Vec<Value>, SchemaError> {
    value.as_array().ok_or_else(|| SchemaError::at_value(method, path, "array", value))
}

pub(super) fn expect_string<'a>(
    method: Option<&str>,
    path: &str,
    value: Option<&'a Value>,
) -> Result<&'a str, SchemaError> {
    let value = value.ok_or_else(|| SchemaError::new(method, path, "string", "missing"))?;
    value.as_str().ok_or_else(|| SchemaError::at_value(method, path, "string", value))
}

pub(super) fn expect_exact_string(
    method: Option<&str>,
    path: &str,
    value: Option<&Value>,
    expected: &str,
) -> Result<(), SchemaError> {
    let observed = expect_string(method, path, value)?;
    if observed == expected {
        Ok(())
    } else {
        Err(SchemaError::new(method, path, expected, observed))
    }
}

pub(super) fn expect_boolean(
    method: Option<&str>,
    path: &str,
    value: Option<&Value>,
) -> Result<bool, SchemaError> {
    let value = value.ok_or_else(|| SchemaError::new(method, path, "boolean", "missing"))?;
    value.as_bool().ok_or_else(|| SchemaError::at_value(method, path, "boolean", value))
}

pub(super) fn expect_integer(
    method: Option<&str>,
    path: &str,
    value: Option<&Value>,
) -> Result<i64, SchemaError> {
    let value = value.ok_or_else(|| SchemaError::new(method, path, "integer", "missing"))?;
    value.as_i64().ok_or_else(|| SchemaError::at_value(method, path, "integer", value))
}

pub(super) fn expect_uinteger(
    method: Option<&str>,
    path: &str,
    value: Option<&Value>,
) -> Result<u64, SchemaError> {
    let value = value.ok_or_else(|| SchemaError::new(method, path, "uinteger", "missing"))?;
    value.as_u64().ok_or_else(|| SchemaError::at_value(method, path, "uinteger", value))
}

pub(super) fn expect_null(
    method: Option<&str>,
    path: &str,
    value: &Value,
) -> Result<(), SchemaError> {
    if value.is_null() { Ok(()) } else { Err(SchemaError::at_value(method, path, "null", value)) }
}

pub(super) fn observed(value: &Value) -> String {
    match value {
        Value::Null => "null".to_string(),
        Value::Bool(value) => format!("boolean:{value}"),
        Value::Number(value) => format!("number:{value}"),
        Value::String(value) => format!("string(len={})", value.len()),
        Value::Array(value) => format!("array(len={})", value.len()),
        Value::Object(value) => {
            let keys = value.keys().take(8).cloned().collect::<Vec<_>>().join(",");
            format!("object(len={},keys=[{}])", value.len(), keys)
        }
    }
}

#[cfg(test)]
mod tests;
