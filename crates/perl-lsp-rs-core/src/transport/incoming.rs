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

use super::document_symbol_probe;
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
    probe_read_offset: u64,
}

impl ContentLengthMessageReader {
    /// Create a new reader with empty frame state.
    #[must_use]
    pub fn new() -> Self {
        Self { framer: ContentLengthFramer::new(), probe_read_offset: 0 }
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
                Ok(Some(body)) => {
                    let outcome = decode_incoming_body(&body);
                    document_symbol_probe::emit("decoded_body", || match &outcome {
                        Ok(request) => {
                            let method_class = match request.method.as_str() {
                                "textDocument/didOpen" => "did_open",
                                "textDocument/documentSymbol" => "document_symbol",
                                CLIENT_RESPONSE_METHOD => "client_response",
                                _ => "other",
                            };
                            let numeric_id = request.id.as_ref().and_then(|id| match id {
                                JsonRpcId::Integer(id) => Some(*id),
                                _ => None,
                            });
                            json!({ "frame_sequence": self.framer.probe_frame_sequence(),
                                    "body_bytes": body.len(), "outcome": "accepted",
                                    "method_class": method_class, "numeric_id": numeric_id,
                                    "id_omitted": request.id.is_some() && numeric_id.is_none() })
                        }
                        Err(error) => json!({
                            "frame_sequence": self.framer.probe_frame_sequence(),
                            "body_bytes": body.len(), "outcome": "rejected",
                            "rejection_stage": error.stage().as_str(),
                        }),
                    });
                    return Ok(Some(outcome));
                }
                Ok(None) => {}
                Err(error) => {
                    document_symbol_probe::emit(
                        "framing_rejected",
                        || json!({ "rejection_stage": "framing" }),
                    );
                    return Ok(Some(Err(IncomingMessageError::Framing(error))));
                }
            }

            let bytes_read = match reader.read(&mut chunk) {
                Ok(bytes) => bytes,
                Err(error) => {
                    document_symbol_probe::emit(
                        "read_failed",
                        || json!({ "io_error_kind": format!("{:?}", error.kind()) }),
                    );
                    return Err(error);
                }
            };
            if document_symbol_probe::active() {
                let start = self.probe_read_offset;
                self.probe_read_offset = start.saturating_add(bytes_read as u64);
                document_symbol_probe::emit("stream_read", || {
                    json!({
                        "stream_start": start, "stream_end": self.probe_read_offset,
                        "accepted_bytes": bytes_read,
                    })
                });
            }
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

fn starts_with_ascii_prefix(bytes: &[u8], sentinel: &[u8]) -> bool {
    if bytes.is_empty() {
        return false;
    }
    let prefix_len = bytes.len().min(sentinel.len());
    match (bytes.get(..prefix_len), sentinel.get(..prefix_len)) {
        (Some(prefix), Some(expected)) => prefix.eq_ignore_ascii_case(expected),
        _ => false,
    }
}

fn looks_like_lsp_header_prefix(bytes: &[u8]) -> bool {
    starts_with_ascii_prefix(bytes, b"content-length:")
        || starts_with_ascii_prefix(bytes, b"content-type:")
}

/// Recover a following frame after an invalid header.
///
/// LSP JSON-RPC bodies start with `{` or `[`. A following frame's header block
/// starts with `Content-Length` or optional `Content-Type`, including a prefix
/// split across a small `BufRead`. Those two starts are the protocol
/// discriminator. A claimed payload that itself begins with those header names
/// is not distinguishable from a following frame on a stateless `BufRead`.
fn recover_after_malformed_header(
    reader: &mut dyn BufRead,
    claimed_length: Option<usize>,
) -> io::Result<()> {
    let Some(length) = claimed_length.filter(|&len| len > 0 && len <= MAX_FRAME_SIZE) else {
        return Ok(());
    };
    let available = reader.fill_buf()?;
    if available.is_empty() || looks_like_lsp_header_prefix(available) {
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

#[cfg(test)]
mod document_symbol_transport_controls {
    use super::super::framing::frame;
    use super::*;
    use std::io::Cursor;

    struct Fragmented(Cursor<Vec<u8>>);
    impl io::Read for Fragmented {
        fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
            let limit = bytes.len().min(7);
            io::Read::read(&mut self.0, &mut bytes[..limit])
        }
    }

    fn assert_fragmented_read_ranges(records: &[Value], input_bytes: usize) {
        let reads: Vec<_> =
            records.iter().filter(|record| record["stage"] == "stream_read").collect();
        assert_eq!(reads.len(), input_bytes.div_ceil(7) + 1);
        for (index, read) in reads.iter().enumerate() {
            let start = (index * 7).min(input_bytes);
            let accepted = (input_bytes - start).min(7);
            assert_eq!(read["metadata"]["stream_start"], start);
            assert_eq!(read["metadata"]["accepted_bytes"], accepted);
            assert_eq!(read["metadata"]["stream_end"], start + accepted);
        }
    }

    #[test]
    fn interleaved_reply_header_loses_open_while_atomic_frames_keep_it() -> io::Result<()> {
        let reply =
            json!({ "jsonrpc": "2.0", "id": "synthetic-server-id", "result": [] }).to_string();
        let open = json!({ "jsonrpc": "2.0", "method": "textDocument/didOpen", "params": {
            "textDocument": { "uri": "file:///fixture.pm", "version": 1, "languageId": "perl", "text": "sub alpha {}\n" }
        }}).to_string();
        let query = json!({ "jsonrpc": "2.0", "id": 116, "method": "textDocument/documentSymbol",
            "params": { "textDocument": { "uri": "file:///fixture.pm" } }})
        .to_string();
        let mut interleaved = format!("Content-Length: {}\r\n\r\n", reply.len()).into_bytes();
        let reply_header_bytes = interleaved.len();
        interleaved.extend(frame(open.as_bytes()));
        interleaved.extend(reply.as_bytes());
        interleaved.extend(frame(query.as_bytes()));
        let query_start = interleaved.len() - frame(query.as_bytes()).len();
        let stream_bytes = interleaved.len();
        let (result, records) = document_symbol_probe::capture(|| -> io::Result<()> {
            let mut reader = ContentLengthMessageReader::new();
            let mut stream = Fragmented(Cursor::new(interleaved));
            assert!(matches!(
                reader.read_next_outcome(&mut stream)?,
                Some(Err(IncomingMessageError::MalformedJson { .. }))
            ));
            assert!(matches!(reader.read_next_outcome(&mut stream)?, Some(Ok(request))
            if request.method == "textDocument/documentSymbol" && request.id == Some(JsonRpcId::Integer(116))));
            assert!(
                reader.read_next_outcome(&mut stream)?.is_none(),
                "original didOpen was lost during resynchronization"
            );
            Ok(())
        });
        result?;
        assert_fragmented_read_ranges(&records, stream_bytes);
        for (index, record) in records.iter().enumerate() {
            assert_eq!(record["event_sequence"], index + 1);
        }
        let extractions: Vec<_> =
            records.iter().filter(|record| record["stage"] == "body_extracted").collect();
        assert_eq!(extractions.len(), 2);
        assert_eq!(extractions[0]["metadata"]["stream_start"], 0);
        assert_eq!(extractions[0]["metadata"]["stream_end"], reply_header_bytes + reply.len());
        assert_eq!(extractions[1]["metadata"]["stream_start"], query_start);
        assert_eq!(extractions[1]["metadata"]["stream_end"], stream_bytes);
        let discarded: Vec<_> =
            records.iter().filter(|record| record["stage"] == "prefix_discarded").collect();
        assert_eq!(discarded.len(), 1);
        assert_eq!(discarded[0]["metadata"]["stream_start"], reply_header_bytes + reply.len());
        assert_eq!(discarded[0]["metadata"]["stream_end"], query_start);
        let decoded: Vec<_> =
            records.iter().filter(|record| record["stage"] == "decoded_body").collect();
        assert_eq!(decoded.len(), 2);
        assert_eq!(decoded[0]["metadata"]["outcome"], "rejected");
        assert_eq!(decoded[1]["metadata"]["outcome"], "accepted");
        assert_eq!(decoded[1]["metadata"]["method_class"], "document_symbol");
        assert_eq!(decoded[1]["metadata"]["numeric_id"], 116);
        assert!(!serde_json::to_string(&records)?.contains("synthetic-server-id"));

        let mut atomic = frame(reply.as_bytes());
        atomic.extend(frame(open.as_bytes()));
        atomic.extend(frame(query.as_bytes()));
        let atomic_bytes = atomic.len();
        let (result, records) = document_symbol_probe::capture(|| -> io::Result<()> {
            let mut reader = ContentLengthMessageReader::new();
            let mut stream = Fragmented(Cursor::new(atomic));
            for expected in
                [CLIENT_RESPONSE_METHOD, "textDocument/didOpen", "textDocument/documentSymbol"]
            {
                assert!(
                    matches!(reader.read_next_outcome(&mut stream)?, Some(Ok(request)) if request.method == expected)
                );
            }
            assert!(reader.read_next_outcome(&mut stream)?.is_none());
            Ok(())
        });
        result?;
        assert_fragmented_read_ranges(&records, atomic_bytes);
        assert_eq!(records.iter().filter(|record| record["stage"] == "body_extracted").count(), 3);
        assert_eq!(
            records
                .iter()
                .filter(|record| record["stage"] == "decoded_body"
                    && record["metadata"]["outcome"] == "accepted")
                .count(),
            3
        );
        assert!(!records.iter().any(|record| record["stage"] == "prefix_discarded"
            || record["metadata"]["outcome"] == "rejected"));
        Ok(())
    }
}
