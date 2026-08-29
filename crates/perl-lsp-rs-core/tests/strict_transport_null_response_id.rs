//! Regression coverage for JSON-RPC null response ids (#7596).

use perl_lsp_rs_core::transport::{ContentLengthMessageReader, IncomingMessageError, frame};
use std::error::Error;
use std::io::{self, Cursor};

#[test]
fn null_response_id_is_an_invalid_shape_not_a_mixed_envelope() -> Result<(), Box<dyn Error>> {
    let body = br#"{"jsonrpc":"2.0","id":null,"result":{}}"#;
    let payload_bytes = body.len();
    let mut input = Cursor::new(frame(body));
    let mut reader = ContentLengthMessageReader::new();

    let Some(outcome) = reader.read_next_outcome(&mut input)? else {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "expected one decoded frame outcome",
        )
        .into());
    };
    let error = match outcome {
        Ok(_) => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "JSON-RPC response id must not be null",
            )
            .into());
        }
        Err(error) => error,
    };

    match error {
        IncomingMessageError::InvalidMessageShape { payload_bytes: actual, .. }
            if actual == payload_bytes =>
        {
            Ok(())
        }
        IncomingMessageError::MixedRequestResponse { .. } => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "null response id was misclassified as a mixed request/response envelope",
        )
        .into()),
        other => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("expected invalid message shape for null response id, got {other}"),
        )
        .into()),
    }
}
