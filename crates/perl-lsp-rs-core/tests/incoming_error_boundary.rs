//! Incoming failure facts stay separate from Perl error classification (#19395).
//!
//! These tests exercise the real exported types and reader. They do not claim
//! isolated dependency closure: FramingError neutrality remains #13996 and the
//! complete exact-source error graph is proved by #13998.

use perl_lsp_rs_core::protocol::JsonRpcId;
use perl_lsp_rs_core::transport::{
    ContentLengthMessageReader, FramingError, IncomingMessageError, IncomingMessageStage,
    decode_incoming_body, frame,
};
use std::{error::Error, io::Cursor, marker::PhantomData};

// Match the existing inventory's compile-time inherent-vs-blanket probe.
// Testing the actual trait relationship also catches an impl written through
// a renamed import, which a source-token guard alone cannot detect.
struct ErrorClassProbe<T>(PhantomData<T>);

trait ErrorClassProbeFallback {
    const IMPLEMENTS_ERROR_CLASS: bool = false;
}

impl<T> ErrorClassProbeFallback for ErrorClassProbe<T> {}

impl<T: perl_parser_core::ErrorClass> ErrorClassProbe<T> {
    const IMPLEMENTS_ERROR_CLASS: bool = true;
}

struct ClassifiedControl;

impl perl_parser_core::ErrorClass for ClassifiedControl {
    fn error_class(&self) -> perl_parser_core::ErrorCategory {
        perl_parser_core::ErrorCategory::Protocol
    }
}

#[test]
fn incoming_failure_type_has_no_parser_taxonomy_impl() {
    const _: () = assert!(
        !ErrorClassProbe::<IncomingMessageError>::IMPLEMENTS_ERROR_CLASS,
        "IncomingMessageError must not implement the parser taxonomy"
    );
    const _: () = assert!(
        ErrorClassProbe::<ClassifiedControl>::IMPLEMENTS_ERROR_CLASS,
        "the negative-impl probe must detect an actual implementation"
    );
}

#[test]
fn compatibility_exports_keep_one_type_identity() {
    let old_path: fn(
        perl_lsp_rs_core::transport::incoming::IncomingMessageError,
    ) -> IncomingMessageError = |error| error;
    let error = IncomingMessageError::UnsupportedBatch { payload_bytes: 2 };
    assert_eq!(old_path(error.clone()), error);

    let old_stage: perl_lsp_rs_core::transport::incoming::IncomingMessageStage =
        IncomingMessageStage::Json;
    assert_eq!(old_stage, IncomingMessageStage::Json);
}

#[test]
fn every_failure_preserves_stage_size_id_and_source() {
    let cases = [
        (
            IncomingMessageError::Framing(FramingError::InvalidHeader),
            IncomingMessageStage::Framing,
            None,
            None,
        ),
        (
            IncomingMessageError::Framing(FramingError::FrameTooLarge { len: 99 }),
            IncomingMessageStage::Framing,
            Some(99),
            None,
        ),
        (
            IncomingMessageError::InvalidUtf8 { payload_bytes: 3, valid_up_to: 1 },
            IncomingMessageStage::Encoding,
            Some(3),
            None,
        ),
        (
            IncomingMessageError::MalformedJson { payload_bytes: 7, line: 1, column: 2 },
            IncomingMessageStage::Json,
            Some(7),
            None,
        ),
        (
            IncomingMessageError::InvalidMessageShape {
                payload_bytes: 8,
                recoverable_id: Some(JsonRpcId::Integer(1)),
            },
            IncomingMessageStage::JsonRpcShape,
            Some(8),
            Some(JsonRpcId::Integer(1)),
        ),
        (
            IncomingMessageError::UnsupportedBatch { payload_bytes: 2 },
            IncomingMessageStage::JsonRpcShape,
            Some(2),
            None,
        ),
        (
            IncomingMessageError::InvalidJsonRpc {
                payload_bytes: 9,
                recoverable_id: Some(JsonRpcId::String("1".to_owned())),
            },
            IncomingMessageStage::JsonRpcShape,
            Some(9),
            Some(JsonRpcId::String("1".to_owned())),
        ),
    ];

    for (error, stage, bytes, id) in cases {
        assert_eq!(error.stage(), stage);
        assert_eq!(error.payload_bytes(), bytes);
        assert_eq!(error.recoverable_id(), id.as_ref());
        assert_eq!(error.clone(), error);
        match &error {
            IncomingMessageError::Framing(expected) => {
                let actual =
                    error.source().and_then(|source| source.downcast_ref::<FramingError>());
                assert_eq!(actual, Some(expected), "retain the typed framing cause");
            }
            _ => assert!(error.source().is_none()),
        }
    }
}

#[test]
fn stage_tokens_and_payload_private_diagnostics_remain_stable() -> Result<(), Box<dyn Error>> {
    for (stage, token) in [
        (IncomingMessageStage::Framing, "framing"),
        (IncomingMessageStage::Encoding, "encoding"),
        (IncomingMessageStage::Json, "json"),
        (IncomingMessageStage::JsonRpcShape, "jsonrpc-shape"),
    ] {
        assert_eq!(stage.as_str(), token);
        assert_eq!(stage.to_string(), token);
    }

    let error = match decode_incoming_body(br#"{"secret":"private-payload-19395""#) {
        Err(error) => error,
        Ok(_) => return Err(std::io::Error::other("malformed JSON was admitted").into()),
    };
    assert_eq!(error.stage(), IncomingMessageStage::Json);
    assert!(!error.to_string().contains("private-payload-19395"));
    assert!(!format!("{error:?}").contains("private-payload-19395"));
    Ok(())
}

#[test]
fn real_reader_preserves_typed_failure_then_following_message() -> Result<(), Box<dyn Error>> {
    let mut wire = frame(&[0xff]);
    wire.extend(frame(br#"{"jsonrpc":"2.0","id":"next","method":"shutdown"}"#));
    let mut stream = Cursor::new(wire);
    let mut reader = ContentLengthMessageReader::new();

    let Some(Err(error)) = reader.read_next_outcome(&mut stream)? else {
        return Err(std::io::Error::other("invalid UTF-8 did not produce one failure").into());
    };
    assert_eq!(error.stage(), IncomingMessageStage::Encoding);
    assert_eq!(error.payload_bytes(), Some(1));

    let Some(Ok(request)) = reader.read_next_outcome(&mut stream)? else {
        return Err(std::io::Error::other("following valid frame was lost").into());
    };
    assert_eq!(request.id, Some(JsonRpcId::String("next".to_owned())));
    assert_eq!(request.method, "shutdown");
    assert!(reader.read_next_outcome(&mut stream)?.is_none());
    Ok(())
}

#[test]
fn definitions_have_one_owner_without_direct_taxonomy_imports() {
    let facts = include_str!("../src/transport/incoming_error.rs");
    let reader = include_str!("../src/transport/incoming.rs");
    for declaration in ["pub enum IncomingMessageError", "pub enum IncomingMessageStage"] {
        assert_eq!(facts.matches(declaration).count(), 1, "one canonical definition");
        assert_eq!(reader.matches(declaration).count(), 0, "reader must forward, not copy");
    }
    for forbidden in ["perl_parser_core", "ErrorClass", "ErrorCategory"] {
        assert!(!facts.contains(forbidden), "failure facts import {forbidden}");
        assert!(!reader.contains(forbidden), "reader imports {forbidden}");
    }
}
