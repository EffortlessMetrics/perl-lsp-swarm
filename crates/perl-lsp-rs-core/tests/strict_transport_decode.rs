//! Regression coverage for strict incoming transport decoding (#7596).

use perl_lsp_rs_core::protocol::JsonRpcRequest;
use perl_lsp_rs_core::transport::framing::{MAX_FRAME_SIZE, read_message};
use perl_lsp_rs_core::transport::{
    ContentLengthMessageReader, FramingError, IncomingMessageError, frame,
};
use serde_json::Value;
use std::error::Error;
use std::io::{self, BufReader, Cursor, Read};

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

struct FailingReader {
    bytes: Vec<u8>,
    offset: usize,
    fail_after: usize,
}

impl FailingReader {
    fn new(bytes: Vec<u8>, fail_after: usize) -> Self {
        Self { bytes, offset: 0, fail_after }
    }
}

impl Read for FailingReader {
    fn read(&mut self, destination: &mut [u8]) -> io::Result<usize> {
        if self.offset >= self.fail_after {
            return Err(io::Error::new(io::ErrorKind::Other, "injected transport read failure"));
        }

        let remaining = self.bytes.len().saturating_sub(self.offset);
        let before_failure = self.fail_after - self.offset;
        let count = remaining.min(before_failure).min(destination.len());
        let source = self
            .bytes
            .get(self.offset..self.offset + count)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "source out of bounds"))?;
        let target = destination
            .get_mut(..count)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "destination too small"))?;
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
fn mixed_request_response_envelope_is_a_dedicated_outcome() -> TestResult {
    let mixed = br#"{"jsonrpc":"2.0","id":7,"method":"shutdown","result":{"secret-token":true}}"#;
    let mixed_len = mixed.len();
    let mut input = Cursor::new(frame(mixed));
    let mut reader = ContentLengthMessageReader::new();
    let error = required_error(&mut reader, &mut input)?;

    let IncomingMessageError::MixedRequestResponse { payload_bytes } = &error else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("expected mixed request/response outcome, got {error}"),
        )
        .into());
    };
    if *payload_bytes != mixed_len {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("expected {mixed_len} payload bytes, got {payload_bytes}"),
        )
        .into());
    }
    if error.source().is_some() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "mixed request/response outcome must not expose a synthetic cause",
        )
        .into());
    }
    if error.is_terminal_at_eof() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "mixed request/response outcome must stay recoverable",
        )
        .into());
    }
    if error.to_string().contains("secret-token") || format!("{error:?}").contains("secret-token") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "mixed request/response outcome leaked payload contents",
        )
        .into());
    }
    Ok(())
}

#[test]
fn malformed_response_envelopes_are_rejected_and_recovery_preserves_valid_input() -> TestResult {
    // (payload, mixed) — mixed entries declare request and response members
    // together and must surface the dedicated `MixedRequestResponse` outcome.
    let malformed_responses: [(&[u8], bool); 8] = [
        (br#"{"jsonrpc":"2.0","id":true,"result":{}}"#, false),
        (br#"{"jsonrpc":"2.0","id":1}"#, false),
        (br#"{"jsonrpc":"2.0","id":1,"result":{},"error":{"code":-1,"message":"bad"}}"#, false),
        (br#"{"jsonrpc":"2.0","id":1,"error":null}"#, false),
        (br#"{"jsonrpc":"2.0","id":1,"error":7}"#, false),
        (br#"{"jsonrpc":"2.0","id":1,"error":{}}"#, false),
        (br#"{"jsonrpc":"2.0","id":1,"method":"shutdown","result":{}}"#, true),
        (
            br#"{"jsonrpc":"2.0","id":1,"method":"shutdown","error":{"code":-1,"message":"bad"}}"#,
            true,
        ),
    ];
    let valid_request = request_body(11, "shutdown", "{}");
    let mut stream = Vec::new();
    for (response, _) in &malformed_responses {
        stream.extend(frame(response));
    }
    stream.extend(frame(&valid_request));

    let mut input = Cursor::new(stream);
    let mut reader = ContentLengthMessageReader::new();
    for (_, mixed) in &malformed_responses {
        let error = required_error(&mut reader, &mut input)?;
        let expected = if *mixed {
            matches!(&error, IncomingMessageError::MixedRequestResponse { .. })
        } else {
            matches!(&error, IncomingMessageError::InvalidMessageShape { .. })
        };
        if !expected {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("expected typed malformed response rejection, got {error}"),
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
    let string_end = invalid_body
        .iter()
        .rposition(|byte| *byte == b'"')
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing string terminator"))?;
    invalid_body.insert(string_end, 0xff);
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
fn compatibility_read_message_skips_rejected_frame_and_returns_following_valid() -> TestResult {
    let mut malformed = request_body(7, "broken", r#"{"text":"private"}"#);
    let string_end = malformed
        .iter()
        .rposition(|byte| *byte == b'"')
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing string terminator"))?;
    malformed.insert(string_end, 0xff);
    let valid = request_body(8, "exit", "{}");
    let mut stream = frame(&malformed);
    stream.extend(frame(&valid));
    let mut input = BufReader::with_capacity(4096, Cursor::new(stream));

    let request = read_message(&mut input)?
        .ok_or_else(|| io::Error::new(io::ErrorKind::UnexpectedEof, "expected valid request"))?;
    if request.method != "exit" {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("compatibility reader returned unexpected method {}", request.method),
        )
        .into());
    }
    if read_message(&mut input)?.is_some() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "compatibility reader should reach EOF after the valid frame",
        )
        .into());
    }
    Ok(())
}

#[test]
fn strict_reader_propagates_underlying_io_failure() -> TestResult {
    let bytes = frame(&request_body(15, "initialize", "{}"));
    let fail_after = bytes.len().saturating_sub(1);
    let mut input = FailingReader::new(bytes, fail_after);
    let mut reader = ContentLengthMessageReader::new();
    let error = match reader.read_next_outcome(&mut input) {
        Err(error) => error,
        Ok(other) => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("injected I/O failure was not propagated: {other:?}"),
            )
            .into());
        }
    };
    if error.kind() != io::ErrorKind::Other {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("unexpected injected I/O error kind: {:?}", error.kind()),
        )
        .into());
    }
    Ok(())
}

#[test]
fn oversized_frame_is_reported_and_following_frame_is_recoverable() -> TestResult {
    let oversized_body = vec![b'x'; MAX_FRAME_SIZE + 1];
    let mut stream = format!("Content-Length: {}\r\n\r\n", oversized_body.len()).into_bytes();
    stream.extend_from_slice(&oversized_body);
    stream.extend(frame(&request_body(16, "shutdown", "{}")));
    let mut input = Cursor::new(stream);
    let mut reader = ContentLengthMessageReader::new();

    let error = required_error(&mut reader, &mut input)?;
    if !matches!(error, IncomingMessageError::Framing(FramingError::FrameTooLarge { .. })) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("expected oversized-frame error, got {error}"),
        )
        .into());
    }
    if required_request(&mut reader, &mut input)?.method != "shutdown" {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "valid frame after oversized header was not recovered",
        )
        .into());
    }
    Ok(())
}

#[test]
fn leading_desynchronization_is_discarded_before_valid_frame() -> TestResult {
    let mut stream = b"garbage-before-frame\0\xff".to_vec();
    stream.extend(frame(&request_body(17, "initialized", "{}")));
    let mut input = Cursor::new(stream);
    let mut reader = ContentLengthMessageReader::new();

    if required_request(&mut reader, &mut input)?.method != "initialized" {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "valid frame after leading desynchronization was not recovered",
        )
        .into());
    }
    Ok(())
}

#[test]
fn embedded_content_length_text_remains_inside_payload() -> TestResult {
    let body = request_body(
        18,
        "textDocument/hover",
        r#"{"text":"prefix Content-Length: 0\r\n\r\nsuffix"}"#,
    );
    let mut stream = frame(&body);
    stream.extend(frame(&request_body(19, "shutdown", "{}")));
    let mut input = Cursor::new(stream);
    let mut reader = ContentLengthMessageReader::new();

    let request = required_request(&mut reader, &mut input)?;
    let text = request
        .params
        .as_ref()
        .and_then(|params| params.get("text"))
        .and_then(Value::as_str)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing embedded text"))?;
    if text != "prefix Content-Length: 0\r\n\r\nsuffix" {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "embedded Content-Length text was altered",
        )
        .into());
    }
    if required_request(&mut reader, &mut input)?.method != "shutdown" {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "frame after embedded Content-Length text was not recovered",
        )
        .into());
    }
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
