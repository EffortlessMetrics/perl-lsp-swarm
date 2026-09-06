//! Strict incoming JSON-RPC decode for `Content-Length` framed bodies.
//!
//! Framing stays in [`super::framing`]. This module owns the stages above a
//! complete frame:
//!
//! ```text
//! complete frame bytes
//! → UTF-8 encoding
//! → JSON syntax
//! → JSON-RPC message shape
//! → JsonRpcRequest
//! ```
//!
//! The reader reports one typed outcome per completed frame. Callers own
//! continue / respond / close disposition. Exact shipped-process wire and
//! exit behavior remain #6720 / #7004.

use super::framing::{ContentLengthFramer, FramingError, MAX_FRAME_SIZE};
use crate::protocol::{JSONRPC_VERSION, JsonRpcId, JsonRpcRequest};
use serde_json::{Value, json};
use std::fmt;
use std::io::{self, BufRead, Read};

const CLIENT_RESPONSE_METHOD: &str = "$/perl-lsp/clientResponse";
const READ_CHUNK_BYTES: usize = 8 * 1024;

/// Stage of the incoming pipeline that rejected a complete frame or header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum IncomingMessageStage {
    /// `Content-Length` framing failed before a body could be decoded.
    Framing,
    /// The frame body was not valid UTF-8.
    Encoding,
    /// The UTF-8 body was not valid JSON.
    Json,
    /// The JSON value was not a current JSON-RPC request/notification shape.
    JsonRpcShape,
}

impl IncomingMessageStage {
    /// Stable log/label token for this stage.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Framing => "framing",
            Self::Encoding => "encoding",
            Self::Json => "json",
            Self::JsonRpcShape => "jsonrpc-shape",
        }
    }
}

impl fmt::Display for IncomingMessageStage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Typed failure for one incoming frame. Ordinary diagnostics are payload-private.
#[derive(Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum IncomingMessageError {
    /// Header/frame extraction failed.
    Framing(FramingError),
    /// Body bytes were not valid UTF-8.
    InvalidUtf8 {
        /// Complete frame body length in bytes.
        payload_bytes: usize,
        /// First invalid UTF-8 offset.
        valid_up_to: usize,
    },
    /// Body was valid UTF-8 but not JSON.
    MalformedJson {
        /// Complete frame body length in bytes.
        payload_bytes: usize,
        /// serde_json 1-based line of the syntax error.
        line: usize,
        /// serde_json 1-based column of the syntax error.
        column: usize,
    },
    /// JSON parsed but was not a JSON-RPC object (scalar, `null`, or object
    /// without a current request/notification/response shape).
    InvalidMessageShape {
        /// Complete frame body length in bytes.
        payload_bytes: usize,
        /// Request id when the protocol legally exposes one.
        recoverable_id: Option<JsonRpcId>,
    },
    /// JSON parsed as an array. JSON-RPC batch is unsupported here.
    UnsupportedBatch {
        /// Complete frame body length in bytes.
        payload_bytes: usize,
    },
    /// Object had a `method` but `jsonrpc` was missing or not a string.
    InvalidJsonRpc {
        /// Complete frame body length in bytes.
        payload_bytes: usize,
        /// Request id when the protocol legally exposes one.
        recoverable_id: Option<JsonRpcId>,
    },
}

impl IncomingMessageError {
    /// Pipeline stage that failed.
    #[must_use]
    pub const fn stage(&self) -> IncomingMessageStage {
        match self {
            Self::Framing(_) => IncomingMessageStage::Framing,
            Self::InvalidUtf8 { .. } => IncomingMessageStage::Encoding,
            Self::MalformedJson { .. } => IncomingMessageStage::Json,
            Self::InvalidMessageShape { .. }
            | Self::UnsupportedBatch { .. }
            | Self::InvalidJsonRpc { .. } => IncomingMessageStage::JsonRpcShape,
        }
    }

    /// Body or claimed-frame size when that metadata exists.
    #[must_use]
    pub const fn payload_bytes(&self) -> Option<usize> {
        match self {
            Self::Framing(FramingError::FrameTooLarge { len }) => Some(*len),
            Self::Framing(_) => None,
            Self::InvalidUtf8 { payload_bytes, .. }
            | Self::MalformedJson { payload_bytes, .. }
            | Self::InvalidMessageShape { payload_bytes, .. }
            | Self::UnsupportedBatch { payload_bytes }
            | Self::InvalidJsonRpc { payload_bytes, .. } => Some(*payload_bytes),
        }
    }

    /// Recovered JSON-RPC id when the value legally supplied one.
    #[must_use]
    pub fn recoverable_id(&self) -> Option<&JsonRpcId> {
        match self {
            Self::InvalidMessageShape { recoverable_id, .. }
            | Self::InvalidJsonRpc { recoverable_id, .. } => recoverable_id.as_ref(),
            Self::Framing(_)
            | Self::InvalidUtf8 { .. }
            | Self::MalformedJson { .. }
            | Self::UnsupportedBatch { .. } => None,
        }
    }
}

impl fmt::Display for IncomingMessageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Framing(error) => write!(f, "incoming framing error: {error}"),
            Self::InvalidUtf8 { payload_bytes, valid_up_to } => write!(
                f,
                "invalid UTF-8 in incoming JSON-RPC body (payload_bytes={payload_bytes}, valid_up_to={valid_up_to})"
            ),
            Self::MalformedJson { payload_bytes, line, column } => write!(
                f,
                "malformed JSON in incoming JSON-RPC body (payload_bytes={payload_bytes}, line={line}, column={column})"
            ),
            Self::InvalidMessageShape { payload_bytes, .. } => {
                write!(f, "incoming JSON is not a JSON-RPC object (payload_bytes={payload_bytes})")
            }
            Self::UnsupportedBatch { payload_bytes } => {
                write!(f, "unsupported JSON-RPC batch array (payload_bytes={payload_bytes})")
            }
            Self::InvalidJsonRpc { payload_bytes, .. } => write!(
                f,
                "incoming JSON-RPC object is missing a string jsonrpc field (payload_bytes={payload_bytes})"
            ),
        }
    }
}

impl fmt::Debug for IncomingMessageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Framing(error) => f.debug_tuple("Framing").field(error).finish(),
            Self::InvalidUtf8 { payload_bytes, valid_up_to } => f
                .debug_struct("InvalidUtf8")
                .field("payload_bytes", payload_bytes)
                .field("valid_up_to", valid_up_to)
                .finish(),
            Self::MalformedJson { payload_bytes, line, column } => f
                .debug_struct("MalformedJson")
                .field("payload_bytes", payload_bytes)
                .field("line", line)
                .field("column", column)
                .finish(),
            Self::InvalidMessageShape { payload_bytes, recoverable_id } => f
                .debug_struct("InvalidMessageShape")
                .field("payload_bytes", payload_bytes)
                .field("recoverable_id", recoverable_id)
                .finish(),
            Self::UnsupportedBatch { payload_bytes } => {
                f.debug_struct("UnsupportedBatch").field("payload_bytes", payload_bytes).finish()
            }
            Self::InvalidJsonRpc { payload_bytes, recoverable_id } => f
                .debug_struct("InvalidJsonRpc")
                .field("payload_bytes", payload_bytes)
                .field("recoverable_id", recoverable_id)
                .finish(),
        }
    }
}

impl std::error::Error for IncomingMessageError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Framing(error) => Some(error),
            Self::InvalidUtf8 { .. }
            | Self::MalformedJson { .. }
            | Self::InvalidMessageShape { .. }
            | Self::UnsupportedBatch { .. }
            | Self::InvalidJsonRpc { .. } => None,
        }
    }
}

impl perl_parser_core::ErrorClass for IncomingMessageError {
    fn error_class(&self) -> perl_parser_core::ErrorCategory {
        match self {
            Self::Framing(error) => error.error_class(),
            Self::InvalidUtf8 { .. }
            | Self::MalformedJson { .. }
            | Self::InvalidMessageShape { .. }
            | Self::UnsupportedBatch { .. }
            | Self::InvalidJsonRpc { .. } => perl_parser_core::ErrorCategory::Protocol,
        }
    }
}

/// Decode one complete frame body through encoding → JSON → current message shape.
pub fn decode_incoming_body(body: &[u8]) -> Result<JsonRpcRequest, IncomingMessageError> {
    let text = match std::str::from_utf8(body) {
        Ok(text) => text,
        Err(error) => {
            return Err(IncomingMessageError::InvalidUtf8 {
                payload_bytes: body.len(),
                valid_up_to: error.valid_up_to(),
            });
        }
    };

    let value: Value = match serde_json::from_str(text) {
        Ok(value) => value,
        Err(error) => {
            return Err(IncomingMessageError::MalformedJson {
                payload_bytes: body.len(),
                line: error.line(),
                column: error.column(),
            });
        }
    };

    decode_jsonrpc_value(value, body.len())
}

fn decode_jsonrpc_value(
    value: Value,
    payload_bytes: usize,
) -> Result<JsonRpcRequest, IncomingMessageError> {
    match value {
        Value::Array(_) => Err(IncomingMessageError::UnsupportedBatch { payload_bytes }),
        Value::Object(_) => decode_jsonrpc_object(value, payload_bytes),
        _ => Err(IncomingMessageError::InvalidMessageShape { payload_bytes, recoverable_id: None }),
    }
}

fn decode_jsonrpc_object(
    value: Value,
    payload_bytes: usize,
) -> Result<JsonRpcRequest, IncomingMessageError> {
    let recoverable_id = value.get("id").and_then(JsonRpcId::from_value);

    if value.get("method").is_some() {
        if !jsonrpc_field_is_string(&value) {
            return Err(IncomingMessageError::InvalidJsonRpc { payload_bytes, recoverable_id });
        }
        return serde_json::from_value(value).map_err(|_| {
            IncomingMessageError::InvalidMessageShape { payload_bytes, recoverable_id }
        });
    }

    // JSON-RPC response to a server-initiated request. Convert to the current
    // internal pseudo-notification so the runtime can route it (#7626 owns
    // first-class response direction).
    if let Some(id) = value.get("id") {
        let params = json!({
            "id": id,
            "result": value.get("result").cloned().unwrap_or(Value::Null),
            "error": value.get("error").cloned(),
        });
        return Ok(JsonRpcRequest {
            _jsonrpc: JSONRPC_VERSION.to_string(),
            id: None,
            method: CLIENT_RESPONSE_METHOD.to_string(),
            params: Some(params),
        });
    }

    Err(IncomingMessageError::InvalidMessageShape { payload_bytes, recoverable_id: None })
}

fn jsonrpc_field_is_string(value: &Value) -> bool {
    matches!(value.get("jsonrpc"), Some(Value::String(_)))
}

fn log_incoming_rejection(error: &IncomingMessageError) {
    tracing::warn!(
        stage = error.stage().as_str(),
        payload_bytes = error.payload_bytes(),
        "incoming message rejected"
    );
}

/// Stateful reader for `Content-Length` framed JSON-RPC requests.
///
/// This reader keeps partial frame state across reads, which allows it to
/// handle split headers, split bodies, and multiple messages arriving in a
/// single transport read.
#[derive(Default)]
pub struct ContentLengthMessageReader {
    framer: ContentLengthFramer,
}

impl ContentLengthMessageReader {
    /// Create a new reader with empty frame state.
    #[must_use]
    pub fn new() -> Self {
        Self { framer: ContentLengthFramer::new() }
    }

    /// Read the next completed-frame outcome from the underlying byte stream.
    ///
    /// Returns:
    /// - `Ok(Some(Ok(request)))` when a complete request is decoded
    /// - `Ok(Some(Err(error)))` when one completed frame failed at a typed stage
    /// - `Ok(None)` on EOF
    /// - `Err(io::Error)` on non-recoverable I/O failure
    ///
    /// One call reports at most one frame. Framing, encoding, JSON, and
    /// message-shape failures do not consume a following frame. Callers decide
    /// whether to continue, respond, or close.
    pub fn read_next_outcome(
        &mut self,
        reader: &mut dyn Read,
    ) -> io::Result<Option<Result<JsonRpcRequest, IncomingMessageError>>> {
        let mut chunk = [0u8; READ_CHUNK_BYTES];

        loop {
            match self.framer.try_next() {
                Ok(Some(body)) => return Ok(Some(decode_incoming_body(&body))),
                Ok(None) => {}
                Err(error) => {
                    return Ok(Some(Err(IncomingMessageError::Framing(error))));
                }
            }

            let bytes_read = reader.read(&mut chunk)?;
            if bytes_read == 0 {
                return Ok(None);
            }
            self.framer.push(&chunk[..bytes_read]);
        }
    }

    /// Read and parse the next JSON-RPC request from the underlying byte stream.
    ///
    /// Returns:
    /// - `Ok(Some(request))` when a complete request is decoded
    /// - `Ok(None)` on EOF
    /// - `Err(io::Error)` on non-recoverable I/O failure
    ///
    /// Malformed frames are logged with payload-private metadata and skipped so
    /// the caller can continue processing subsequent requests. This preserves
    /// the current skip-and-continue runtime policy. Prefer
    /// [`Self::read_next_outcome`] when the caller owns disposition.
    pub fn read_next(&mut self, reader: &mut dyn Read) -> io::Result<Option<JsonRpcRequest>> {
        loop {
            match self.read_next_outcome(reader)? {
                Some(Ok(request)) => return Ok(Some(request)),
                Some(Err(error)) => {
                    log_incoming_rejection(&error);
                    continue;
                }
                None => return Ok(None),
            }
        }
    }
}

fn is_header_terminator(line: &[u8]) -> bool {
    line == b"\r\n" || line == b"\n"
}

fn trim_header_crlf(line: &[u8]) -> &[u8] {
    let without_lf = match line.split_last() {
        Some((b'\n', rest)) => rest,
        _ => line,
    };
    match without_lf.split_last() {
        Some((b'\r', rest)) => rest,
        _ => without_lf,
    }
}

fn drain_to_header_end(reader: &mut dyn BufRead) -> io::Result<()> {
    loop {
        let mut line = Vec::new();
        let bytes_read = reader.read_until(b'\n', &mut line)?;
        if bytes_read == 0 || is_header_terminator(&line) {
            return Ok(());
        }
    }
}

fn looks_like_content_length_header(bytes: &[u8]) -> bool {
    const SENTINEL: &[u8] = b"content-length:";
    if bytes.is_empty() {
        return false;
    }
    let prefix_len = bytes.len().min(SENTINEL.len());
    match (bytes.get(..prefix_len), SENTINEL.get(..prefix_len)) {
        (Some(prefix), Some(expected)) => prefix.eq_ignore_ascii_case(expected),
        _ => false,
    }
}

/// Recover a following frame after an invalid header without scanning payload
/// bytes for a `Content-Length` sentinel.
///
/// If the next buffered bytes already look like a header (including a prefix
/// split across a small `BufRead` buffer), the claimed body was omitted and
/// must not be consumed. Otherwise consume an in-limit claimed body so a
/// leftover payload cannot be reread as headers.
fn recover_after_malformed_header(
    reader: &mut dyn BufRead,
    claimed_length: Option<usize>,
) -> io::Result<()> {
    let Some(length) = claimed_length.filter(|&len| len > 0 && len <= MAX_FRAME_SIZE) else {
        return Ok(());
    };
    let available = reader.fill_buf()?;
    if available.is_empty() || looks_like_content_length_header(available) {
        return Ok(());
    }
    let mut limited = reader.take(length as u64);
    io::copy(&mut limited, &mut io::sink()).map(|_| ())
}

/// Read one LSP message from a buffered reader as a typed one-frame outcome.
///
/// This helper consumes at most one frame from `reader` so a following frame
/// remains available to the next call. For long-running loops, prefer
/// [`ContentLengthMessageReader`].
pub fn read_message_outcome(
    reader: &mut dyn BufRead,
) -> io::Result<Option<Result<JsonRpcRequest, IncomingMessageError>>> {
    let mut content_length = None;

    loop {
        let mut line = Vec::new();
        let bytes_read = reader.read_until(b'\n', &mut line)?;
        if bytes_read == 0 {
            return Ok(None);
        }

        if is_header_terminator(&line) {
            break;
        }

        let header = match std::str::from_utf8(trim_header_crlf(&line)) {
            Ok(header) => header,
            Err(_) => {
                drain_to_header_end(reader)?;
                recover_after_malformed_header(reader, content_length)?;
                return Ok(Some(Err(IncomingMessageError::Framing(
                    FramingError::InvalidHeaderUtf8,
                ))));
            }
        };
        if let Some((name, value)) = header.split_once(':')
            && name.trim().eq_ignore_ascii_case("Content-Length")
        {
            match value.trim().parse::<usize>() {
                Ok(length) => content_length = Some(length),
                Err(_) => {
                    return Ok(Some(Err(IncomingMessageError::Framing(
                        FramingError::InvalidContentLength,
                    ))));
                }
            }
        }
    }

    let length = match content_length {
        Some(length) => length,
        None => {
            return Ok(Some(Err(IncomingMessageError::Framing(
                FramingError::MissingContentLength,
            ))));
        }
    };

    if length > MAX_FRAME_SIZE {
        return Ok(Some(Err(IncomingMessageError::Framing(FramingError::FrameTooLarge {
            len: length,
        }))));
    }

    let mut body = vec![0u8; length];
    if let Err(error) = reader.read_exact(&mut body) {
        if error.kind() == io::ErrorKind::UnexpectedEof {
            return Ok(None);
        }
        return Err(error);
    }

    Ok(Some(decode_incoming_body(&body)))
}

/// Read an LSP message from a buffered reader.
///
/// This is a compatibility helper for one-shot reads. For long-running loops,
/// prefer [`ContentLengthMessageReader`] to preserve parser state across calls.
///
/// Malformed complete frames return `Ok(None)` after a payload-private log
/// so the next sequential call can still read a following valid frame.
pub fn read_message(reader: &mut dyn BufRead) -> io::Result<Option<JsonRpcRequest>> {
    match read_message_outcome(reader)? {
        Some(Ok(request)) => Ok(Some(request)),
        Some(Err(error)) => {
            log_incoming_rejection(&error);
            Ok(None)
        }
        None => Ok(None),
    }
}
