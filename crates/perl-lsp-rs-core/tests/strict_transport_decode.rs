//! Regression coverage for strict incoming transport decoding (#7596).

use perl_lsp_rs_core::protocol::JsonRpcRequest;
use perl_lsp_rs_core::transport::{
    frame, ContentLengthMessageReader, FramingError, IncomingMessageError,
};
use serde_json::Value;
use std::error::Error;
use std::io::{self, Cursor, Read};

type TestResult = Result<(), Box<dyn Error>>;

fn request_body(id: i64, method: &str, params: &str) -> Vec<u8> {
    format!(r#"{{"jsonrpc":"2.0","id":{id},"method":"{method}","params":{params}}}"#).into_bytes()
}

struct ChunkedReader {
    bytes: Vec<u8>,
    offset: usize,
    chunk_size: usize,
}

impl ChunkedReader {
    fn new(bytes: Vec<u8>, chunk_size: usize) -> Self {
        Self { bytes, offset: 0, chunk_size }
    }
}

impl Read for ChunkedReader {
    fn read(&mut self, destination: &mut [u8]) -> io::Result<usize> {
        if self.offset >= self.bytes.len() {
            return Ok(0);
        }
        let remaining = self.bytes.len() - self.offset;
        let count = self.chunk_size.min(remaining).min(destination.len());
        let source = self.bytes.get(self.offset..self.offset + count).ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "chunk source out of bounds")
        })?;
        let target = destination
            .get_mut(..count)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "chunk target too small"))?;
        target.copy_from_slice(source);
        self.offset += count;
        Ok(count)
    }
}

fn required_request(
    reader: &mut ContentLengthMessageReader,
    input: &mut dyn Read,
) -> Result<JsonRpcRequest, Box<dyn Error>> {
    match reader.read_next_outcome(input)? {
        Some(Ok(request)) => Ok(request),
        Some(Err(error)) => Err(error.into()),
        None => Err(io::Error::new(io::ErrorKind::UnexpectedEof, "expected request").into()),
    }
}

fn required_error(
    reader: &mut ContentLengthMessageReader,
    input: &mut dyn Read,
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
    let label =
        request.params.as_ref().and_then(|params| params.get("label")).and_then(Value::as_str);

    assert_eq!(label, Some("café 🦀"));
    Ok(())
}

#[test]
fn invalid_utf8_is_typed_and_payload_private() -> TestResult {
    const SECRET: &str = "private-document-token";
    let mut body =
        br#"{"jsonrpc":"2.0","id":2,"method":"textDocument/didOpen","params":{"text":"private-document-token"}}"#
            .to_vec();
    let valid_up_to = body
        .iter()
        .rposition(|byte| *byte == b'"')
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing string terminator"))?;
    body.insert(valid_up_to, 0xff);

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
            valid_up_to: actual_valid_up_to, error_len, ..
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
    assert!(matches!(&malformed_error, IncomingMessageError::MalformedJson { .. }));

    let invalid_shape =
        br#"{"jsonrpc":"2.0","id":1,"method":{"private-token":true},"params":{"token":"private-token"}}"#;
    let mut shape_input = Cursor::new(frame(invalid_shape));
    let mut shape_reader = ContentLengthMessageReader::new();
    let shape_error = required_error(&mut shape_reader, &mut shape_input)?;

    assert!(!shape_error.to_string().contains("private-token"));
    assert!(!format!("{shape_error:?}").contains("private-token"));
    assert!(matches!(shape_error, IncomingMessageError::InvalidMessageShape { .. }));
    Ok(())
}

#[test]
fn invalid_jsonrpc_version_and_batch_are_distinct_outcomes() -> TestResult {
    let invalid_version = br#"{"jsonrpc":"1.0","id":1,"method":"shutdown","params":{}}"#;
    let mut version_input = Cursor::new(frame(invalid_version));
    let mut version_reader = ContentLengthMessageReader::new();
    let version_error = required_error(&mut version_reader, &mut version_input)?;
    if !matches!(version_error, IncomingMessageError::InvalidJsonRpcVersion { .. }) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("expected invalid JSON-RPC version, got {version_error}"),
        )
        .into());
    }

    let batch = br#"[{"jsonrpc":"2.0","id":1,"method":"shutdown","params":{}}]"#;
    let mut batch_input = Cursor::new(frame(batch));
    let mut batch_reader = ContentLengthMessageReader::new();
    let batch_error = required_error(&mut batch_reader, &mut batch_input)?;
    if !matches!(batch_error, IncomingMessageError::UnsupportedBatch { .. }) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("expected unsupported batch, got {batch_error}"),
        )
        .into());
    }
    Ok(())
}

#[test]
fn malformed_response_envelopes_are_rejected_and_recovery_preserves_valid_input() -> TestResult {
    let malformed_responses: [&[u8]; 8] = [
        br#"{"jsonrpc":"2.0","id":true,"result":{}}"#,
        br#"{"jsonrpc":"2.0","id":1}"#,
        br#"{"jsonrpc":"2.0","id":1,"result":{},"error":{"code":-1,"message":"bad"}}"#,
        br#"{"jsonrpc":"2.0","id":1,"error":null}"#,
        br#"{"jsonrpc":"2.0","id":1,"error":7}"#,
        br#"{"jsonrpc":"2.0","id":1,"error":{}}"#,
        br#"{"jsonrpc":"2.0","id":1,"method":"shutdown","result":{}}"#,
        br#"{"jsonrpc":"2.0","id":1,"method":"shutdown","error":{"code":-1,"message":"bad"}}"#,
    ];
    let valid_request = request_body(11, "shutdown", "{}");
    let mut stream = Vec::new();
    for response in &malformed_responses {
        stream.extend(frame(response));
    }
    stream.extend(frame(&valid_request));

    let mut input = Cursor::new(stream);
    let mut reader = ContentLengthMessageReader::new();
    for _ in &malformed_responses {
        let error = required_error(&mut reader, &mut input)?;
        if !matches!(&error, IncomingMessageError::InvalidMessageShape { .. }) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("expected malformed response rejection, got {error}"),
            )
            .into());
        }
        if error.is_terminal_at_eof() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "recoverable malformed response was classified as terminal",
            )
            .into());
        }
    }

    let request = required_request(&mut reader, &mut input)?;
    if request.method != "shutdown" {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("expected valid request after malformed responses, got {}", request.method),
        )
        .into());
    }
    Ok(())
}

#[test]
fn split_reads_recover_and_then_report_clean_eof() -> TestResult {
    let mut stream = frame(&request_body(12, "initialize", "{}"));
    stream.extend(frame(&request_body(13, "shutdown", "{}")));
    let mut input = ChunkedReader::new(stream, 1);
    let mut reader = ContentLengthMessageReader::new();

    if required_request(&mut reader, &mut input)?.method != "initialize" {
        return Err(
            io::Error::new(io::ErrorKind::InvalidData, "expected initialize request").into()
        );
    }
    if required_request(&mut reader, &mut input)?.method != "shutdown" {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "expected shutdown request").into());
    }
    if reader.read_next_outcome(&mut input)?.is_some() {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "expected clean EOF").into());
    }
    Ok(())
}

#[test]
fn incomplete_frame_is_typed_before_clean_eof() -> TestResult {
    let body = request_body(10, "shutdown", "{}");
    let full_frame = frame(&body);
    let truncated_frame = full_frame
        .get(..full_frame.len().saturating_sub(1))
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "frame unexpectedly empty"))?
        .to_vec();
    let mut input = Cursor::new(truncated_frame);
    let mut reader = ContentLengthMessageReader::new();

    let error = required_error(&mut reader, &mut input)?;
    if !matches!(error, IncomingMessageError::TruncatedFrame { .. }) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("expected truncated frame, got {error}"),
        )
        .into());
    }
    if reader.read_next_outcome(&mut input)?.is_some() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "terminal truncation must not repeat after the buffered frame is cleared",
        )
        .into());
    }

    let mut clean_input = Cursor::new(Vec::new());
    let mut clean_reader = ContentLengthMessageReader::new();
    if clean_reader.read_next_outcome(&mut clean_input)?.is_some() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "clean EOF must not produce an incoming outcome",
        )
        .into());
    }
    Ok(())
}

#[test]
fn invalid_utf8_frame_does_not_consume_following_valid_frame() -> TestResult {
    let mut invalid_body =
        br#"{"jsonrpc":"2.0","id":4,"method":"textDocument/didChange","params":{"text":"safe"}}"#
            .to_vec();
    let string_end = invalid_body
        .iter()
        .rposition(|byte| *byte == b'"')
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing string terminator"))?;
    invalid_body.insert(string_end, 0xff);
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
    assert_eq!(required_request(&mut reader, &mut input)?.method, "initialize");
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
        assert!(!source.contains(forbidden), "strict incoming reader must not contain {forbidden}");
    }
}

#[test]
fn shipped_ingress_uses_typed_reader_not_compatibility_loop() -> TestResult {
    let cli = include_str!("../../perl-lsp-rs/src/cli.rs");
    let serving = include_str!("../../perl-lsp-rs/src/runtime/serving.rs");
    if cli.contains("msg_reader.read_next(&mut buf_reader)")
        || serving.contains("message_reader.read_next(reader)")
        || serving.contains("message_reader.read_next(&mut buf_reader)")
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "shipped ingress must not call the lossy compatibility reader",
        )
        .into());
    }
    Ok(())
}
