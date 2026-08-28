//! Regression coverage for strict incoming transport decoding (#7596).

use perl_lsp_rs_core::protocol::JsonRpcRequest;
use perl_lsp_rs_core::transport::{
    ContentLengthMessageReader, FramingError, IncomingMessageError, frame,
};
use serde_json::Value;
use std::error::Error;
use std::io::{self, Cursor};

type TestResult = Result<(), Box<dyn Error>>;

fn request_body(id: i64, method: &str, params: &str) -> Vec<u8> {
    format!(
        r#"{{"jsonrpc":"2.0","id":{id},"method":"{method}","params":{params}}}"#
    )
    .into_bytes()
}

fn required_request(
    reader: &mut ContentLengthMessageReader,
    input: &mut Cursor<Vec<u8>>,
) -> Result<JsonRpcRequest, Box<dyn Error>> {
    match reader.read_next_outcome(input)? {
        Some(Ok(request)) => Ok(request),
        Some(Err(error)) => Err(error.into()),
        None => Err(io::Error::new(io::ErrorKind::UnexpectedEof, "expected request").into()),
    }
}

fn required_error(
    reader: &mut ContentLengthMessageReader,
    input: &mut Cursor<Vec<u8>>,
) -> Result<IncomingMessageError, Box<dyn Error>> {
    match reader.read_next_outcome(input)? {
        Some(Err(error)) => Ok(error),
        Some(Ok(request)) => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("expected decode failure, got method {}", request.method),
        )
        .into()),
        None => Err(io::Error::new(io::ErrorKind::UnexpectedEof, "expected decode failure").into()),
    }
}

#[test]
fn accepts_multibyte_utf8_with_byte_content_length() -> TestResult {
    let body = request_body(1, "textDocument/hover", r#"{"label":"café 🦀"}"#);
    let mut input = Cursor::new(frame(&body));
    let mut reader = ContentLengthMessageReader::new();

    let request = required_request(&mut reader, &mut input)?;
    let label = request
        .params
        .as_ref()
        .and_then(|params| params.get("label"))
        .and_then(Value::as_str);

    assert_eq!(label, Some("café 🦀"));
    Ok(())
}

#[test]
fn invalid_utf8_is_typed_and_payload_private() -> TestResult {
    const SECRET: &str = "private-document-token";
    let mut body = request_body(
        2,
        "textDocument/didOpen",
        r#"{"text":"private-document-token"}"#,
    );
    let valid_up_to = body.len();
    body.push(0xff);

    let mut input = Cursor::new(frame(&body));
    let mut reader = ContentLengthMessageReader::new();
    let error = required_error(&mut reader, &mut input)?;
    let rendered = error.to_string();
    let debug = format!("{error:?}");

    match error {
        IncomingMessageError::InvalidUtf8 {
            payload_bytes,
            valid_up_to: actual_valid_up_to,
            error_len,
            ..
        } => {
            assert_eq!(payload_bytes, body.len());
            assert_eq!(actual_valid_up_to, valid_up_to);
            assert_eq!(error_len, Some(1));
        }
        other => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("expected invalid UTF-8, got {other}"),
            )
            .into());
        }
    }

    assert!(!rendered.contains(SECRET));
    assert!(!debug.contains(SECRET));
    assert!(!rendered.contains('\u{fffd}'));
    Ok(())
}

#[test]
fn truncated_utf8_reports_a_truncated_sequence() -> TestResult {
    let mut body = request_body(3, "workspace/symbol", r#"{"query":"safe"}"#);
    let valid_up_to = body.len();
    body.extend_from_slice(&[0xe2, 0x82]);

    let mut input = Cursor::new(frame(&body));
    let mut reader = ContentLengthMessageReader::new();
    let error = required_error(&mut reader, &mut input)?;

    match error {
        IncomingMessageError::InvalidUtf8 {
            valid_up_to: actual_valid_up_to,
            error_len,
            ..
        } => {
            assert_eq!(actual_valid_up_to, valid_up_to);
            assert_eq!(error_len, None);
        }
        other => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("expected truncated UTF-8, got {other}"),
            )
            .into());
        }
    }

    Ok(())
}

#[test]
fn malformed_json_and_invalid_message_shape_remain_distinct() -> TestResult {
    let malformed =
        br#"{"jsonrpc":"2.0","method":"private-method","params":{"token":"private-token"}"#;
    let mut malformed_input = Cursor::new(frame(malformed));
    let mut malformed_reader = ContentLengthMessageReader::new();
    let malformed_error = required_error(&mut malformed_reader, &mut malformed_input)?;

    assert!(!malformed_error.to_string().contains("private-token"));
    assert!(!format!("{malformed_error:?}").contains("private-token"));
    assert!(matches!(
        &malformed_error,
        IncomingMessageError::MalformedJson { .. }
    ));

    let invalid_shape =
        br#"{"jsonrpc":"2.0","id":1,"method":{"private-token":true},"params":{"token":"private-token"}}"#;
    let mut shape_input = Cursor::new(frame(invalid_shape));
    let mut shape_reader = ContentLengthMessageReader::new();
    let shape_error = required_error(&mut shape_reader, &mut shape_input)?;

    assert!(!shape_error.to_string().contains("private-token"));
    assert!(!format!("{shape_error:?}").contains("private-token"));
    assert!(matches!(
        shape_error,
        IncomingMessageError::InvalidMessageShape { .. }
    ));
    Ok(())
}

#[test]
fn invalid_utf8_frame_does_not_consume_following_valid_frame() -> TestResult {
    let mut invalid_body = request_body(4, "textDocument/didChange", r#"{"text":"safe"}"#);
    invalid_body.push(0xff);
    let valid_body = request_body(5, "shutdown", "{}");

    let mut stream = frame(&invalid_body);
    stream.extend(frame(&valid_body));
    let mut input = Cursor::new(stream);
    let mut reader = ContentLengthMessageReader::new();

    assert!(matches!(
        required_error(&mut reader, &mut input)?,
        IncomingMessageError::InvalidUtf8 { .. }
    ));
    assert_eq!(required_request(&mut reader, &mut input)?.method, "shutdown");
    Ok(())
}

#[test]
fn malformed_json_frame_does_not_consume_following_valid_frame() -> TestResult {
    let malformed_body = br#"{"jsonrpc":"2.0","method":"textDocument/hover""#;
    let valid_body = request_body(9, "shutdown", "{}");

    let mut stream = frame(malformed_body);
    stream.extend(frame(&valid_body));
    let mut input = Cursor::new(stream);
    let mut reader = ContentLengthMessageReader::new();

    assert!(matches!(
        required_error(&mut reader, &mut input)?,
        IncomingMessageError::MalformedJson { .. }
    ));
    assert_eq!(required_request(&mut reader, &mut input)?.method, "shutdown");
    Ok(())
}

#[test]
fn framing_failure_does_not_consume_following_valid_frame() -> TestResult {
    let valid_body = request_body(6, "initialize", "{}");
    let mut stream = b"Content-Length: nope\r\n\r\n".to_vec();
    stream.extend(frame(&valid_body));
    let mut input = Cursor::new(stream);
    let mut reader = ContentLengthMessageReader::new();

    assert!(matches!(
        required_error(&mut reader, &mut input)?,
        IncomingMessageError::Framing(FramingError::InvalidContentLength)
    ));
    assert_eq!(
        required_request(&mut reader, &mut input)?.method,
        "initialize"
    );
    Ok(())
}

#[test]
fn compatibility_reader_strictly_rejects_then_continues() -> TestResult {
    let mut invalid_body = request_body(7, "textDocument/didOpen", r#"{"text":"safe"}"#);
    invalid_body.push(0xff);
    let valid_body = request_body(8, "exit", "{}");

    let mut stream = frame(&invalid_body);
    stream.extend(frame(&valid_body));
    let mut input = Cursor::new(stream);
    let mut reader = ContentLengthMessageReader::new();

    let request = reader
        .read_next(&mut input)?
        .ok_or_else(|| io::Error::new(io::ErrorKind::UnexpectedEof, "expected valid request"))?;

    assert_eq!(request.method, "exit");
    Ok(())
}

#[test]
fn strict_reader_source_bans_lossy_decode_and_payload_previews() {
    let source = include_str!("../src/transport/incoming.rs");

    for forbidden in ["from_utf8_lossy", "body_preview", "preview ="] {
        assert!(
            !source.contains(forbidden),
            "strict incoming reader must not contain {forbidden}"
        );
    }
}
