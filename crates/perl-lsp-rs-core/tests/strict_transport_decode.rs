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
fn valid_utf8_replacement_char_is_not_an_encoding_failure() {
    let body = r#"{"jsonrpc":"2.0","id":1,"method":"test","params":{"text":"abc�"}}"#;
    let request = match decode_incoming_body(body.as_bytes()) {
        Ok(request) => request,
        Err(error) => panic!("valid UTF-8 U+FFFD must decode: {error:?}"),
    };
    let params = match request.params {
        Some(params) => params,
        None => panic!("expected params"),
    };
    assert_eq!(params["text"], "abc\u{FFFD}");
}

#[test]
fn invalid_utf8_is_typed_encoding_failure_not_lossy_json() {
    let body = invalid_utf8_json_body();
    match decode_incoming_body(&body) {
        Err(error @ IncomingMessageError::InvalidUtf8 { payload_bytes, valid_up_to }) => {
            assert_eq!(payload_bytes, body.len());
            assert!(valid_up_to < body.len());
            assert_eq!(error.stage(), IncomingMessageStage::Encoding);
            assert_payload_private(&error);
        }
        other => panic!("expected InvalidUtf8, got {other:?}"),
    }
}

#[test]
fn truncated_utf8_is_typed_encoding_failure() {
    let body = truncated_utf8_json_body();
    match decode_incoming_body(&body) {
        Err(error @ IncomingMessageError::InvalidUtf8 { .. }) => {
            assert_eq!(error.stage(), IncomingMessageStage::Encoding);
            assert_payload_private(&error);
        }
        other => panic!("expected InvalidUtf8 for truncated sequence, got {other:?}"),
    }
}

#[test]
fn malformed_json_is_distinct_from_message_shape() {
    let body = format!(r#"{{"jsonrpc":"2.0","id":1,"method":"{SECRET}""#);
    match decode_incoming_body(body.as_bytes()) {
        Err(error @ IncomingMessageError::MalformedJson { payload_bytes, line, column }) => {
            assert_eq!(payload_bytes, body.len());
            assert_eq!(line, 1);
            assert!(column > 1);
            assert_eq!(error.stage(), IncomingMessageStage::Json);
            assert_payload_private(&error);
        }
        other => panic!("expected MalformedJson, got {other:?}"),
    }
}

#[test]
fn json_scalar_is_message_shape_failure() {
    match decode_incoming_body(b"42") {
        Err(error @ IncomingMessageError::InvalidMessageShape { recoverable_id: None, .. }) => {
            assert_eq!(error.stage(), IncomingMessageStage::JsonRpcShape);
            assert_ne!(
                error.to_string(),
                IncomingMessageError::MalformedJson { payload_bytes: 2, line: 1, column: 1 }
                    .to_string()
            );
        }
        other => panic!("expected InvalidMessageShape for scalar, got {other:?}"),
    }
}

#[test]
fn json_rpc_batch_array_is_distinct_from_scalar_shape() {
    let body = br#"[{"jsonrpc":"2.0","id":1,"method":"test"}]"#;
    let batch = match decode_incoming_body(body) {
        Err(error) => error,
        Ok(request) => panic!("batch must not decode as {request:?}"),
    };
    let scalar = match decode_incoming_body(b"42") {
        Err(error) => error,
        Ok(request) => panic!("scalar must not decode as {request:?}"),
    };
    assert!(matches!(batch, IncomingMessageError::UnsupportedBatch { .. }));
    assert!(matches!(scalar, IncomingMessageError::InvalidMessageShape { .. }));
    assert_ne!(batch.to_string(), scalar.to_string());
    assert_eq!(batch.stage(), IncomingMessageStage::JsonRpcShape);
    assert_eq!(scalar.stage(), IncomingMessageStage::JsonRpcShape);
}

#[test]
fn missing_jsonrpc_on_method_object_preserves_recoverable_id() {
    let body = br#"{"id":4,"method":"test","params":{}}"#;
    match decode_incoming_body(body) {
        Err(IncomingMessageError::InvalidJsonRpc { recoverable_id, .. }) => {
            assert_eq!(recoverable_id, Some(JsonRpcId::Integer(4)));
        }
        other => panic!("expected InvalidJsonRpc, got {other:?}"),
    }
}

#[test]
fn numeric_jsonrpc_is_invalid_jsonrpc_not_malformed_json() {
    let body = br#"{"jsonrpc":2.0,"id":5,"method":"test"}"#;
    match decode_incoming_body(body) {
        Err(IncomingMessageError::InvalidJsonRpc { recoverable_id, .. }) => {
            assert_eq!(recoverable_id, Some(JsonRpcId::Integer(5)));
        }
        other => panic!("expected InvalidJsonRpc for numeric jsonrpc, got {other:?}"),
    }
}

#[test]
fn jsonrpc_1_0_string_still_decodes_as_current_request() {
    // Exact "2.0" enforcement is #9636. Rejecting "1.0" here would change the
    // current shipped Invalid Request response path owned by #6720.
    let body = br#"{"jsonrpc":"1.0","id":1,"method":"initialize","params":{}}"#;
    match decode_incoming_body(body) {
        Ok(request) => assert_eq!(request.method, "initialize"),
        Err(error) => panic!("jsonrpc 1.0 string must remain a current request: {error:?}"),
    }
}

#[test]
fn malformed_object_without_id_does_not_invent_one() {
    let body = br#"{"jsonrpc":"2.0"}"#;
    match decode_incoming_body(body) {
        Err(IncomingMessageError::InvalidMessageShape { recoverable_id: None, .. }) => {}
        other => panic!("expected shape failure with no invented id, got {other:?}"),
    }
}

#[test]
fn method_with_non_string_value_preserves_id() {
    let body = br#"{"jsonrpc":"2.0","id":8,"method":1}"#;
    match decode_incoming_body(body) {
        Err(IncomingMessageError::InvalidMessageShape { recoverable_id, .. }) => {
            assert_eq!(recoverable_id, Some(JsonRpcId::Integer(8)));
        }
        other => panic!("expected InvalidMessageShape, got {other:?}"),
    }
}

#[test]
fn client_response_without_method_still_routes_internally() {
    let body = br#"{"jsonrpc":"2.0","id":7,"result":{}}"#;
    match decode_incoming_body(body) {
        Ok(request) => {
            assert_eq!(request.method, "$/perl-lsp/clientResponse");
            assert!(request.id.is_none());
        }
        Err(error) => panic!("response conversion must remain #7626's mapping: {error:?}"),
    }
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
