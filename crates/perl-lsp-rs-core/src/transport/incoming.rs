//! Strict incoming JSON-RPC decoding for the LSP transport.
//!
//! The bounded [`ContentLengthFramer`] owns byte framing. This module preserves
//! each complete frame as either one request or one typed failure; it never
//! rewrites invalid bytes or includes payload contents in diagnostics.

use super::framing::{ContentLengthFramer, FramingError};
use crate::protocol::JsonRpcRequest;
use serde_json::{Value, json};
use std::fmt;
use std::io::{self, Read};

const CLIENT_RESPONSE_METHOD: &str = "$/perl-lsp/clientResponse";
const READ_CHUNK_BYTES: usize = 8 * 1024;

/// A caller-visible failure while decoding one incoming JSON-RPC frame.
///
/// Normal [`std::fmt::Debug`] and [`std::fmt::Display`] output contains
/// only bounded metadata. Typed decoder causes remain available through
/// [`std::error::Error::source`] for deliberate local inspection.
#[non_exhaustive]
pub enum IncomingMessageError {
    /// The `Content-Length` frame was malformed or exceeded a safety bound.
    Framing(FramingError),
    /// A complete frame body was not valid UTF-8.
    InvalidUtf8 {
        /// Number of bytes in the complete frame body.
        payload_bytes: usize,
        /// First byte offset that was not valid UTF-8.
        valid_up_to: usize,
        /// Invalid sequence length, or `None` when the sequence was truncated.
        error_len: Option<usize>,
        /// Original strict UTF-8 decoder error.
        source: std::str::Utf8Error,
    },
    /// The UTF-8 frame body was not syntactically valid JSON.
    MalformedJson {
        /// Number of bytes in the complete frame body.
        payload_bytes: usize,
        /// One-based JSON error line.
        line: usize,
        /// One-based JSON error column.
        column: usize,
        /// Original JSON syntax error.
        source: serde_json::Error,
    },
    /// The body was valid JSON but not a shape accepted by the current runtime.
    InvalidMessageShape {
        /// Number of bytes in the complete frame body.
        payload_bytes: usize,
        /// Original typed deserialization error.
        source: serde_json::Error,
    },
}

impl IncomingMessageError {
    const fn kind(&self) -> &'static str {
        match self {
            Self::Framing(_) => "framing",
            Self::InvalidUtf8 { .. } => "invalid_utf8",
            Self::MalformedJson { .. } => "malformed_json",
            Self::InvalidMessageShape { .. } => "invalid_message_shape",
        }
    }

    const fn payload_bytes(&self) -> Option<usize> {
        match self {
            Self::Framing(_) => None,
            Self::InvalidUtf8 { payload_bytes, .. }
            | Self::MalformedJson { payload_bytes, .. }
            | Self::InvalidMessageShape { payload_bytes, .. } => Some(*payload_bytes),
        }
    }
}

impl fmt::Debug for IncomingMessageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Framing(source) => f.debug_tuple("Framing").field(source).finish(),
            Self::InvalidUtf8 {
                payload_bytes,
                valid_up_to,
                error_len,
                ..
            } => f
                .debug_struct("InvalidUtf8")
                .field("payload_bytes", payload_bytes)
                .field("valid_up_to", valid_up_to)
                .field("error_len", error_len)
                .finish_non_exhaustive(),
            Self::MalformedJson {
                payload_bytes,
                line,
                column,
                ..
            } => f
                .debug_struct("MalformedJson")
                .field("payload_bytes", payload_bytes)
                .field("line", line)
                .field("column", column)
                .finish_non_exhaustive(),
            Self::InvalidMessageShape { payload_bytes, .. } => f
                .debug_struct("InvalidMessageShape")
                .field("payload_bytes", payload_bytes)
                .finish_non_exhaustive(),
        }
    }
}

impl fmt::Display for IncomingMessageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Framing(source) => write!(f, "{source}"),
            Self::InvalidUtf8 {
                payload_bytes,
                valid_up_to,
                error_len: Some(error_len),
                ..
            } => write!(
                f,
                "incoming body contains invalid UTF-8 at byte {valid_up_to} \
                 (invalid sequence length {error_len}; {payload_bytes} payload bytes)"
            ),
            Self::InvalidUtf8 {
                payload_bytes,
                valid_up_to,
                error_len: None,
                ..
            } => write!(
                f,
                "incoming body ends with a truncated UTF-8 sequence at byte {valid_up_to} \
                 ({payload_bytes} payload bytes)"
            ),
            Self::MalformedJson {
                payload_bytes,
                line,
                column,
                ..
            } => write!(
                f,
                "incoming body contains malformed JSON at line {line}, column {column} \
                 ({payload_bytes} payload bytes)"
            ),
            Self::InvalidMessageShape { payload_bytes, .. } => write!(
                f,
                "incoming body is not accepted by the current JSON-RPC message shape \
                 ({payload_bytes} payload bytes)"
            ),
        }
    }
}

impl std::error::Error for IncomingMessageError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Framing(source) => Some(source),
            Self::InvalidUtf8 { source, .. } => Some(source),
            Self::MalformedJson { source, .. } | Self::InvalidMessageShape { source, .. } => {
                Some(source)
            }
        }
    }
}

impl From<FramingError> for IncomingMessageError {
    fn from(source: FramingError) -> Self {
        Self::Framing(source)
    }
}

/// Stateful reader for bounded `Content-Length` framed JSON-RPC messages.
///
/// [`Self::read_next_outcome`] exposes exactly one frame outcome to callers.
/// [`Self::read_next`] preserves the current runtime's skip-and-continue policy
/// while using the same strict decoder and payload-private diagnostics.
#[derive(Default)]
pub struct ContentLengthMessageReader {
    framer: ContentLengthFramer,
}

impl ContentLengthMessageReader {
    /// Create a new reader with empty frame state.
    #[must_use]
    pub fn new() -> Self {
        Self {
            framer: ContentLengthFramer::new(),
        }
    }

    /// Read one complete frame outcome from the underlying byte stream.
    ///
    /// Returns `Ok(None)` only on EOF before another complete outcome exists.
    /// Framing, UTF-8, JSON syntax, and current message-shape failures are
    /// returned as distinct inner errors. The failed frame is consumed, so a
    /// later call can process a following valid frame.
    pub fn read_next_outcome(
        &mut self,
        reader: &mut dyn Read,
    ) -> io::Result<Option<Result<JsonRpcRequest, IncomingMessageError>>> {
        let mut chunk = [0_u8; READ_CHUNK_BYTES];

        loop {
            match self.framer.try_next() {
                Ok(Some(body)) => return Ok(Some(decode_request(&body))),
                Ok(None) => {}
                Err(error) => return Ok(Some(Err(error.into()))),
            }

            let bytes_read = reader.read(&mut chunk)?;
            if bytes_read == 0 {
                return Ok(None);
            }
            self.framer.push(&chunk[..bytes_read]);
        }
    }

    /// Read the next valid request while preserving the current compatibility policy.
    ///
    /// Rejected frames are logged with stage and bounded metadata only, then
    /// skipped. New protocol-policy code should prefer
    /// [`Self::read_next_outcome`] so it can choose continue, respond, or close.
    pub fn read_next(&mut self, reader: &mut dyn Read) -> io::Result<Option<JsonRpcRequest>> {
        loop {
            match self.read_next_outcome(reader)? {
                Some(Ok(request)) => return Ok(Some(request)),
                Some(Err(error)) => {
                    tracing::warn!(
                        error_kind = error.kind(),
                        payload_bytes = ?error.payload_bytes(),
                        %error,
                        "incoming JSON-RPC message rejected"
                    );
                }
                None => return Ok(None),
            }
        }
    }
}

fn decode_request(body: &[u8]) -> Result<JsonRpcRequest, IncomingMessageError> {
    let text = std::str::from_utf8(body).map_err(|source| IncomingMessageError::InvalidUtf8 {
        payload_bytes: body.len(),
        valid_up_to: source.valid_up_to(),
        error_len: source.error_len(),
        source,
    })?;

    let value: Value =
        serde_json::from_str(text).map_err(|source| IncomingMessageError::MalformedJson {
            payload_bytes: body.len(),
            line: source.line(),
            column: source.column(),
            source,
        })?;

    decode_current_message_shape(value, body.len())
}

fn decode_current_message_shape(
    value: Value,
    payload_bytes: usize,
) -> Result<JsonRpcRequest, IncomingMessageError> {
    if value.get("method").is_some() {
        return serde_json::from_value(value).map_err(|source| {
            IncomingMessageError::InvalidMessageShape {
                payload_bytes,
                source,
            }
        });
    }

    // Preserve the current response-routing compatibility until #7626 owns
    // first-class Request | Notification | Response direction.
    if let Some(id) = value.get("id").cloned() {
        let params = json!({
            "id": id,
            "result": value.get("result").cloned().unwrap_or(Value::Null),
            "error": value.get("error").cloned(),
        });
        return Ok(JsonRpcRequest {
            _jsonrpc: "2.0".to_string(),
            id: None,
            method: CLIENT_RESPONSE_METHOD.to_string(),
            params: Some(params),
        });
    }

    serde_json::from_value(value).map_err(|source| {
        IncomingMessageError::InvalidMessageShape {
            payload_bytes,
            source,
        }
    })
}
