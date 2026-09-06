//! Discriminating proof for #7596 strict incoming decode outcomes.
//!
//! These tests fail if invalid bytes are rewritten with U+FFFD, if a malformed
//! frame is silently dropped so the caller only sees the next request, if
//! framing/encoding/JSON/shape failures collapse to one string, if body bytes
//! leak through ordinary Display/Debug, or if a one-shot `read_message`
//! over-reads a following valid frame.

use perl_lsp_rs_core::protocol::JsonRpcId;
use perl_lsp_rs_core::transport::{
    ContentLengthMessageReader, FramingError, IncomingMessageError, IncomingMessageStage,
    MAX_FRAME_SIZE, decode_incoming_body, read_message, read_message_outcome,
};
use perl_parser_core::{ErrorCategory, ErrorClass};
use std::io::{self, BufReader, Cursor, Write};

const SECRET: &str = "SECRET_BODY_TOKEN_7596";

fn framed(body: &[u8]) -> Vec<u8> {
    let mut frame = format!("Content-Length: {}\r\n\r\n", body.len()).into_bytes();
    frame.extend_from_slice(body);
    frame
}

fn framed_request(id: u64, method: &str) -> Vec<u8> {
    let body = format!(r#"{{"jsonrpc":"2.0","id":{id},"method":"{method}","params":{{}}}}"#);
    framed(body.as_bytes())
}

fn invalid_utf8_json_body() -> Vec<u8> {
    let mut body =
        format!(r#"{{"jsonrpc":"2.0","id":1,"method":"test","params":{{"text":"abc{SECRET}"#)
            .into_bytes();
    body.push(0xFF);
    body.extend_from_slice(br#""}}"#);
    body
}

fn truncated_utf8_json_body() -> Vec<u8> {
    // "é" is C3 A9. Truncating after C3 is invalid UTF-8 that lossy decode
    // would turn into U+FFFD and still parse as JSON.
    let mut body = br#"{"jsonrpc":"2.0","id":1,"method":"test","params":{"text":""#.to_vec();
    body.push(0xC3);
    body.extend_from_slice(br#""}}"#);
    body
}

fn require_err(body: &[u8]) -> Result<IncomingMessageError, String> {
    match decode_incoming_body(body) {
        Err(error) => Ok(error),
        Ok(request) => Err(format!("expected decode error, got method {}", request.method)),
    }
}

fn require_ok(body: &[u8]) -> Result<perl_lsp_rs_core::protocol::JsonRpcRequest, String> {
    decode_incoming_body(body).map_err(|error| format!("expected request, got {error:?}"))
}

fn assert_payload_private(error: &IncomingMessageError) {
    let display = error.to_string();
    let debug = format!("{error:?}");
    assert!(
        !display.contains(SECRET) && !debug.contains(SECRET),
        "ordinary diagnostics must not include body bytes: display={display:?} debug={debug:?}"
    );
    assert!(
        !display.contains('\u{FFFD}') && !debug.contains('\u{FFFD}'),
        "diagnostics must not mention U+FFFD replacement text: display={display:?} debug={debug:?}"
    );
}

#[test]
fn valid_utf8_json_request_decodes() -> io::Result<()> {
    let mut cursor = Cursor::new(framed_request(1, "initialize"));
    let mut reader = ContentLengthMessageReader::new();
    let request = match reader.read_next_outcome(&mut cursor)? {
        Some(Ok(request)) => request,
        other => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("expected request, got {other:?}"),
            ));
        }
    };
    assert_eq!(request.method, "initialize");
    assert_eq!(request.id, Some(JsonRpcId::Integer(1)));
    Ok(())
}

#[test]
fn multibyte_unicode_uses_byte_accurate_content_length() -> io::Result<()> {
    let body = r#"{"jsonrpc":"2.0","id":1,"method":"test","params":{"text":"héllo wörld 🦀"}}"#;
    assert_ne!(body.len(), body.chars().count());
    let mut header = Vec::new();
    write!(&mut header, "Content-Length: {}\r\n\r\n", body.len())?;
    let mut frame = header;
    frame.extend_from_slice(body.as_bytes());

    let mut reader = BufReader::new(Cursor::new(frame));
    let request = read_message(&mut reader)?
        .ok_or_else(|| io::Error::new(io::ErrorKind::UnexpectedEof, "expected unicode request"))?;
    let params = request
        .params
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "expected params"))?;
    assert_eq!(params["text"], "héllo wörld 🦀");
    Ok(())
}

#[test]
fn valid_utf8_replacement_char_is_not_an_encoding_failure() -> Result<(), String> {
    let body = r#"{"jsonrpc":"2.0","id":1,"method":"test","params":{"text":"abc�"}}"#;
    let request = require_ok(body.as_bytes())?;
    let params = request.params.ok_or_else(|| "expected params".to_string())?;
    assert_eq!(params["text"], "abc\u{FFFD}");
    Ok(())
}

#[test]
fn invalid_utf8_is_typed_encoding_failure_not_lossy_json() -> Result<(), String> {
    let body = invalid_utf8_json_body();
    let error = require_err(&body)?;
    if !matches!(
        error,
        IncomingMessageError::InvalidUtf8 { payload_bytes, valid_up_to }
            if payload_bytes == body.len() && valid_up_to < body.len()
    ) {
        return Err(format!("expected InvalidUtf8, got {error:?}"));
    }
    assert_eq!(error.stage(), IncomingMessageStage::Encoding);
    assert_payload_private(&error);
    Ok(())
}

#[test]
fn truncated_utf8_is_typed_encoding_failure() -> Result<(), String> {
    let error = require_err(&truncated_utf8_json_body())?;
    if !matches!(error, IncomingMessageError::InvalidUtf8 { .. }) {
        return Err(format!("expected InvalidUtf8 for truncated sequence, got {error:?}"));
    }
    assert_eq!(error.stage(), IncomingMessageStage::Encoding);
    assert_payload_private(&error);
    Ok(())
}

#[test]
fn malformed_json_is_distinct_from_message_shape() -> Result<(), String> {
    let body = format!(r#"{{"jsonrpc":"2.0","id":1,"method":"{SECRET}""#);
    let error = require_err(body.as_bytes())?;
    if !matches!(
        error,
        IncomingMessageError::MalformedJson { payload_bytes, line, column }
            if payload_bytes == body.len() && line == 1 && column > 1
    ) {
        return Err(format!("expected MalformedJson, got {error:?}"));
    }
    assert_eq!(error.stage(), IncomingMessageStage::Json);
    assert_payload_private(&error);
    Ok(())
}

#[test]
fn json_scalar_is_message_shape_failure() -> Result<(), String> {
    let error = require_err(b"42")?;
    if !matches!(error, IncomingMessageError::InvalidMessageShape { recoverable_id: None, .. }) {
        return Err(format!("expected InvalidMessageShape for scalar, got {error:?}"));
    }
    assert_eq!(error.stage(), IncomingMessageStage::JsonRpcShape);
    assert_ne!(
        error.to_string(),
        IncomingMessageError::MalformedJson { payload_bytes: 2, line: 1, column: 1 }.to_string()
    );
    Ok(())
}

#[test]
fn json_rpc_batch_array_is_distinct_from_scalar_shape() -> Result<(), String> {
    let batch = require_err(br#"[{"jsonrpc":"2.0","id":1,"method":"test"}]"#)?;
    let scalar = require_err(b"42")?;
    if !matches!(batch, IncomingMessageError::UnsupportedBatch { .. }) {
        return Err(format!("expected UnsupportedBatch, got {batch:?}"));
    }
    if !matches!(scalar, IncomingMessageError::InvalidMessageShape { .. }) {
        return Err(format!("expected InvalidMessageShape, got {scalar:?}"));
    }
    assert_ne!(batch.to_string(), scalar.to_string());
    assert_eq!(batch.stage(), IncomingMessageStage::JsonRpcShape);
    assert_eq!(scalar.stage(), IncomingMessageStage::JsonRpcShape);
    Ok(())
}

#[test]
fn missing_jsonrpc_on_method_object_preserves_recoverable_id() -> Result<(), String> {
    let error = require_err(br#"{"id":4,"method":"test","params":{}}"#)?;
    if !matches!(
        error,
        IncomingMessageError::InvalidJsonRpc { recoverable_id: Some(JsonRpcId::Integer(4)), .. }
    ) {
        return Err(format!("expected InvalidJsonRpc, got {error:?}"));
    }
    Ok(())
}

#[test]
fn numeric_jsonrpc_is_invalid_jsonrpc_not_malformed_json() -> Result<(), String> {
    let error = require_err(br#"{"jsonrpc":2.0,"id":5,"method":"test"}"#)?;
    if !matches!(
        error,
        IncomingMessageError::InvalidJsonRpc { recoverable_id: Some(JsonRpcId::Integer(5)), .. }
    ) {
        return Err(format!("expected InvalidJsonRpc for numeric jsonrpc, got {error:?}"));
    }
    Ok(())
}

#[test]
fn jsonrpc_1_0_string_still_decodes_as_current_request() -> Result<(), String> {
    // Exact "2.0" enforcement is #9636. Rejecting "1.0" here would change the
    // current shipped Invalid Request response path owned by #6720.
    let request = require_ok(br#"{"jsonrpc":"1.0","id":1,"method":"initialize","params":{}}"#)?;
    assert_eq!(request.method, "initialize");
    Ok(())
}

#[test]
fn malformed_object_without_id_does_not_invent_one() -> Result<(), String> {
    let error = require_err(br#"{"jsonrpc":"2.0"}"#)?;
    if !matches!(error, IncomingMessageError::InvalidMessageShape { recoverable_id: None, .. }) {
        return Err(format!("expected shape failure with no invented id, got {error:?}"));
    }
    Ok(())
}

#[test]
fn method_with_non_string_value_preserves_id() -> Result<(), String> {
    let error = require_err(br#"{"jsonrpc":"2.0","id":8,"method":1}"#)?;
    if !matches!(
        error,
        IncomingMessageError::InvalidMessageShape {
            recoverable_id: Some(JsonRpcId::Integer(8)),
            ..
        }
    ) {
        return Err(format!("expected InvalidMessageShape, got {error:?}"));
    }
    Ok(())
}

#[test]
fn client_response_without_method_still_routes_internally() -> Result<(), String> {
    let request = require_ok(br#"{"jsonrpc":"2.0","id":7,"result":{}}"#)?;
    assert_eq!(request.method, "$/perl-lsp/clientResponse");
    assert!(request.id.is_none());
    Ok(())
}

#[test]
fn invalid_utf8_frame_then_valid_frame_is_observable_then_recoverable() -> io::Result<()> {
    let mut payload = framed(&invalid_utf8_json_body());
    payload.extend(framed_request(2, "recovered"));
    let mut cursor = Cursor::new(payload);
    let mut reader = ContentLengthMessageReader::new();

    match reader.read_next_outcome(&mut cursor)? {
        Some(Err(error @ IncomingMessageError::InvalidUtf8 { .. })) => {
            assert_payload_private(&error);
        }
        other => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("expected InvalidUtf8, got {other:?}"),
            ));
        }
    }

    let recovered = match reader.read_next_outcome(&mut cursor)? {
        Some(Ok(request)) => request,
        other => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("expected recovered request, got {other:?}"),
            ));
        }
    };
    assert_eq!(recovered.method, "recovered");
    Ok(())
}

#[test]
fn malformed_json_frame_then_valid_frame_is_observable_then_recoverable() -> io::Result<()> {
    let mut payload = framed(format!(r#"{{"jsonrpc":"2.0","method":"{SECRET}""#).as_bytes());
    payload.extend(framed_request(3, "after-json"));
    let mut cursor = Cursor::new(payload);
    let mut reader = ContentLengthMessageReader::new();

    match reader.read_next_outcome(&mut cursor)? {
        Some(Err(error @ IncomingMessageError::MalformedJson { .. })) => {
            assert_payload_private(&error);
        }
        other => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("expected MalformedJson, got {other:?}"),
            ));
        }
    }

    match reader.read_next_outcome(&mut cursor)? {
        Some(Ok(request)) => assert_eq!(request.method, "after-json"),
        other => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("expected recovered request, got {other:?}"),
            ));
        }
    }
    Ok(())
}

#[test]
fn malformed_header_then_valid_frame_is_observable_then_recoverable() -> io::Result<()> {
    let mut payload = b"Content-Length: not-a-number\r\n\r\n".to_vec();
    payload.extend(framed_request(4, "after-frame"));
    let mut cursor = Cursor::new(payload);
    let mut reader = ContentLengthMessageReader::new();

    match reader.read_next_outcome(&mut cursor)? {
        Some(Err(IncomingMessageError::Framing(FramingError::InvalidContentLength))) => {}
        other => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("expected InvalidContentLength, got {other:?}"),
            ));
        }
    }

    match reader.read_next_outcome(&mut cursor)? {
        Some(Ok(request)) => assert_eq!(request.method, "after-frame"),
        other => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("expected recovered request, got {other:?}"),
            ));
        }
    }
    Ok(())
}

#[test]
fn compatibility_read_next_skips_malformed_json_without_returning_it_as_success() -> io::Result<()>
{
    let mut payload = framed(b"not json at all!");
    payload.extend(framed_request(2, "valid"));
    let mut cursor = Cursor::new(payload);
    let mut reader = ContentLengthMessageReader::new();
    let request = reader.read_next(&mut cursor)?.ok_or_else(|| {
        io::Error::new(io::ErrorKind::UnexpectedEof, "expected request after skip")
    })?;
    assert_eq!(request.method, "valid");
    Ok(())
}

#[test]
fn read_message_rejects_invalid_utf8_header_without_losing_next_frame() -> io::Result<()> {
    let mut payload = vec![0xFF];
    payload.extend_from_slice(b"\r\n\r\n");
    payload.extend(framed_request(6, "after-header"));
    let mut reader = BufReader::with_capacity(4096, Cursor::new(payload));

    match read_message_outcome(&mut reader)? {
        Some(Err(IncomingMessageError::Framing(FramingError::InvalidHeaderUtf8))) => {}
        other => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("expected InvalidHeaderUtf8, got {other:?}"),
            ));
        }
    }
    let recovered = read_message(&mut reader)?.ok_or_else(|| {
        io::Error::new(io::ErrorKind::UnexpectedEof, "expected recovered request")
    })?;
    assert_eq!(recovered.method, "after-header");
    Ok(())
}

#[test]
fn read_message_discards_known_body_after_invalid_utf8_secondary_header() -> io::Result<()> {
    // Content-Length is trustworthy before a later header line fails UTF-8.
    // Leaving that body unread makes the next call swallow the following frame.
    let body = br#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#;
    let mut payload = format!("Content-Length: {}\r\n", body.len()).into_bytes();
    payload.push(0xFF);
    payload.extend_from_slice(b"\r\n\r\n");
    payload.extend_from_slice(body);
    payload.extend(framed_request(7, "after-known-length"));

    let mut reader = BufReader::with_capacity(4096, Cursor::new(payload.clone()));
    match read_message_outcome(&mut reader)? {
        Some(Err(IncomingMessageError::Framing(FramingError::InvalidHeaderUtf8))) => {}
        other => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("expected InvalidHeaderUtf8, got {other:?}"),
            ));
        }
    }
    let recovered = read_message(&mut reader)?.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "expected recovered request after known-length body discard",
        )
    })?;
    assert_eq!(recovered.method, "after-known-length");

    // The stateful production reader recovers the same bytes via sentinel resync.
    let mut cursor = Cursor::new(payload);
    let mut stateful = ContentLengthMessageReader::new();
    match stateful.read_next_outcome(&mut cursor)? {
        Some(Err(IncomingMessageError::Framing(FramingError::InvalidHeaderUtf8))) => {}
        other => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("expected stateful InvalidHeaderUtf8, got {other:?}"),
            ));
        }
    }
    match stateful.read_next_outcome(&mut cursor)? {
        Some(Ok(request)) => assert_eq!(request.method, "after-known-length"),
        other => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("expected recovered stateful request, got {other:?}"),
            ));
        }
    }
    Ok(())
}

#[test]
fn read_message_does_not_consume_next_frame_when_malformed_body_is_missing() -> io::Result<()> {
    // Claimed Content-Length with no body: blindly discarding N bytes would
    // eat the following valid frame. Resync must stop at the next sentinel.
    let mut payload = b"Content-Length: 64\r\n".to_vec();
    payload.push(0xFF);
    payload.extend_from_slice(b"\r\n\r\n");
    payload.extend(framed_request(8, "after-missing-body"));

    let mut reader = BufReader::with_capacity(4096, Cursor::new(payload.clone()));
    match read_message_outcome(&mut reader)? {
        Some(Err(IncomingMessageError::Framing(FramingError::InvalidHeaderUtf8))) => {}
        other => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("expected InvalidHeaderUtf8, got {other:?}"),
            ));
        }
    }
    let recovered = read_message(&mut reader)?.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "expected recovered request after omitted malformed body",
        )
    })?;
    assert_eq!(recovered.method, "after-missing-body");

    let mut cursor = Cursor::new(payload);
    let mut stateful = ContentLengthMessageReader::new();
    match stateful.read_next_outcome(&mut cursor)? {
        Some(Err(IncomingMessageError::Framing(FramingError::InvalidHeaderUtf8))) => {}
        other => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("expected stateful InvalidHeaderUtf8, got {other:?}"),
            ));
        }
    }
    match stateful.read_next_outcome(&mut cursor)? {
        Some(Ok(request)) => assert_eq!(request.method, "after-missing-body"),
        other => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("expected recovered stateful request, got {other:?}"),
            ));
        }
    }
    Ok(())
}

#[test]
fn read_message_rejects_invalid_utf8_without_losing_next_frame() -> io::Result<()> {
    let mut payload = framed(&invalid_utf8_json_body());
    payload.extend(framed_request(9, "second"));
    let mut reader = BufReader::with_capacity(4096, Cursor::new(payload));

    match read_message_outcome(&mut reader)? {
        Some(Err(IncomingMessageError::InvalidUtf8 { .. })) => {}
        other => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("expected InvalidUtf8 outcome, got {other:?}"),
            ));
        }
    }
    let second = read_message(&mut reader)?
        .ok_or_else(|| io::Error::new(io::ErrorKind::UnexpectedEof, "expected second request"))?;
    assert_eq!(second.method, "second");
    Ok(())
}

#[test]
fn read_message_keeps_two_valid_frames_sequential() -> io::Result<()> {
    let mut payload = framed_request(1, "initialize");
    payload.extend(framed_request(2, "shutdown"));
    let mut reader = BufReader::with_capacity(4096, Cursor::new(payload));
    let first = read_message(&mut reader)?
        .ok_or_else(|| io::Error::new(io::ErrorKind::UnexpectedEof, "expected first"))?;
    let second = read_message(&mut reader)?
        .ok_or_else(|| io::Error::new(io::ErrorKind::UnexpectedEof, "expected second"))?;
    assert_eq!(first.method, "initialize");
    assert_eq!(second.method, "shutdown");
    Ok(())
}

#[test]
fn oversized_frame_then_eof_preserves_typed_error_before_eof() -> io::Result<()> {
    let header = format!("Content-Length: {}\r\n\r\n", MAX_FRAME_SIZE + 1);
    let mut cursor = Cursor::new(header.into_bytes());
    let mut reader = ContentLengthMessageReader::new();

    match reader.read_next_outcome(&mut cursor)? {
        Some(Err(IncomingMessageError::Framing(FramingError::FrameTooLarge { len }))) => {
            assert_eq!(len, MAX_FRAME_SIZE + 1);
        }
        other => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("expected FrameTooLarge, got {other:?}"),
            ));
        }
    }
    assert!(reader.read_next_outcome(&mut cursor)?.is_none());
    Ok(())
}

#[test]
fn incoming_error_classes_stay_protocol_except_frame_too_large() {
    assert_eq!(
        IncomingMessageError::InvalidUtf8 { payload_bytes: 3, valid_up_to: 1 }.error_class(),
        ErrorCategory::Protocol
    );
    assert_eq!(
        IncomingMessageError::Framing(FramingError::FrameTooLarge { len: MAX_FRAME_SIZE + 1 })
            .error_class(),
        ErrorCategory::ResourceLimit
    );
}

#[test]
fn incoming_source_does_not_use_lossy_utf8_or_body_preview() {
    let incoming = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/transport/incoming.rs"));
    assert!(
        !incoming.contains("from_utf8_lossy"),
        "incoming decode must not use String::from_utf8_lossy"
    );
    assert!(!incoming.contains("body_preview"), "incoming decode must not log a body preview");

    let framing = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/transport/framing.rs"));
    let production = framing.split("#[cfg(test)]").next().unwrap_or(framing);
    assert!(
        !production.contains("from_utf8_lossy"),
        "framing production path must not use String::from_utf8_lossy"
    );
    assert!(
        !production.contains("body_preview"),
        "framing production path must not log a body preview"
    );
}
