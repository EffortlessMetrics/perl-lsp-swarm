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
//! → IncomingMessage
//! → legacy method carrier only for callers not yet migrated under #7010
//! ```
//!
//! The reader reports one typed outcome per completed frame. Callers own
//! continue / respond / close disposition. Exact shipped-process wire and
//! exit behavior remain #6720 / #7004.

use super::framing::{ContentLengthFramer, FramingError, MAX_FRAME_SIZE};
use super::incoming_message::{IncomingMessage, IncomingResponsePayload, decode_incoming_message};
use crate::protocol::{JSONRPC_VERSION, JsonRpcRequest};
use serde_json::{Value, json};
use std::io::{self, BufRead, Read};

pub use super::incoming_error::{IncomingMessageError, IncomingMessageStage};

const CLIENT_RESPONSE_METHOD: &str = "$/perl-lsp/clientResponse";
const READ_CHUNK_BYTES: usize = 8 * 1024;

/// Decode one frame through the legacy method-shaped compatibility surface.
///
/// New ingress consumers should use [`decode_incoming_message`] and retain
/// response direction. This adapter remains only until #7010 switches every
/// shipped reader to the connection-owned response registry; #7626 then removes
/// the synthetic carrier and its dispatch/catalog entries together.
pub fn decode_incoming_body(body: &[u8]) -> Result<JsonRpcRequest, IncomingMessageError> {
    decode_incoming_message(body).map(legacy_method_carrier)
}

// Migration-only projection. Never move this function into the generic runtime.
// A valid response is known before this adapter is entered; malformed response
// objects must not be promoted into successful null-result carriers.
fn legacy_method_carrier(message: IncomingMessage) -> JsonRpcRequest {
    match message {
        IncomingMessage::Request(request) => request.into_method_call(),
        IncomingMessage::Notification(notification) => notification.into_method_call(),
        IncomingMessage::Response(response) => {
            let (result, error) = match response.payload {
                IncomingResponsePayload::Result(result) => (result, Value::Null),
                IncomingResponsePayload::Error(error) => {
                    let mut object = serde_json::Map::new();
                    object.insert("code".into(), error.code.into());
                    object.insert("message".into(), error.message.into());
                    if let Some(data) = error.data {
                        object.insert("data".into(), data);
                    }
                    (Value::Null, Value::Object(object))
                }
            };
            JsonRpcRequest {
                _jsonrpc: JSONRPC_VERSION.to_string(),
                id: None,
                method: CLIENT_RESPONSE_METHOD.to_string(),
                params: Some(json!({"id": response.id, "result": result, "error": error})),
            }
        }
    }
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
        self.read_next_message_outcome(reader)
            .map(|outcome| outcome.map(|decoded| decoded.map(legacy_method_carrier)))
    }

    /// Read exactly one typed request, notification, response, or decode failure.
    ///
    /// Responses remain responses here. EOF and I/O failures retain their
    /// existing outer representation; no routing or response policy runs here.
    pub fn read_next_message_outcome(
        &mut self,
        reader: &mut dyn Read,
    ) -> io::Result<Option<Result<IncomingMessage, IncomingMessageError>>> {
        let mut chunk = [0u8; READ_CHUNK_BYTES];

        loop {
            match self.framer.try_next() {
                Ok(Some(body)) => return Ok(Some(decode_incoming_message(&body))),
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
    read_incoming_message_outcome(reader)
        .map(|outcome| outcome.map(|decoded| decoded.map(legacy_method_carrier)))
}

/// Read one complete incoming envelope without rewriting its direction.
///
/// This shares the legacy one-shot reader's framing and recovery implementation.
/// A following buffered frame remains available for the next call.
pub fn read_incoming_message_outcome(
    reader: &mut dyn BufRead,
) -> io::Result<Option<Result<IncomingMessage, IncomingMessageError>>> {
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

    Ok(Some(decode_incoming_message(&body)))
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
