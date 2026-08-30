//! Focused regression coverage for scalar JSON message-shape classification (#7596).

use perl_lsp_rs_core::protocol::JsonRpcRequest;
use perl_lsp_rs_core::transport::{ContentLengthMessageReader, IncomingMessageError, frame};
use std::error::Error;
use std::io::{self, Cursor, Read};

type TestResult = Result<(), Box<dyn Error>>;

fn required_error(
    reader: &mut ContentLengthMessageReader,
    input: &mut dyn Read,
) -> Result<IncomingMessageError, Box<dyn Error>> {
    match reader.read_next_outcome(input)? {
        Some(Err(error)) => Ok(error),
        Some(Ok(request)) => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("expected scalar rejection, got request method {}", request.method),
        )
        .into()),
        None => {
            Err(io::Error::new(io::ErrorKind::UnexpectedEof, "expected scalar rejection").into())
        }
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

#[test]
fn scalar_json_fails_message_shape_before_jsonrpc_member_lookup() -> TestResult {
    let scalar_bodies: &[&[u8]] = &[b"true", b"42", br#""private-scalar-token""#, b"null"];

    for body in scalar_bodies {
        let mut input = Cursor::new(frame(body));
        let mut reader = ContentLengthMessageReader::new();
        let error = required_error(&mut reader, &mut input)?;

        if !matches!(&error, IncomingMessageError::InvalidMessageShape { .. }) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("scalar JSON must fail the top-level message-shape stage, got {error:?}"),
            )
            .into());
        }
        if error.kind() != "invalid_message_shape" {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("unexpected scalar error kind: {}", error.kind()),
            )
            .into());
        }
        if error.is_terminal_at_eof() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "a complete scalar frame must remain recoverable",
            )
            .into());
        }
        if error.to_string().contains("private-scalar-token")
            || format!("{error:?}").contains("private-scalar-token")
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "scalar rejection leaked payload content",
            )
            .into());
        }
    }

    Ok(())
}

#[test]
fn scalar_rejection_consumes_only_its_frame() -> TestResult {
    let valid = br#"{"jsonrpc":"2.0","id":9,"method":"shutdown","params":{}}"#;
    let mut stream = frame(b"true");
    stream.extend(frame(valid));

    let mut input = Cursor::new(stream);
    let mut reader = ContentLengthMessageReader::new();

    let error = required_error(&mut reader, &mut input)?;
    if !matches!(error, IncomingMessageError::InvalidMessageShape { .. }) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "scalar frame did not produce InvalidMessageShape",
        )
        .into());
    }

    let request = required_request(&mut reader, &mut input)?;
    if request.method != "shutdown" {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("following frame changed method: {}", request.method),
        )
        .into());
    }

    if reader.read_next_outcome(&mut input)?.is_some() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "expected clean EOF after following valid frame",
        )
        .into());
    }

    Ok(())
}
