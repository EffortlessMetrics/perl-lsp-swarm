//! Selected error-family behavior, using only the exact source-linked library.
//! No parser taxonomy, copied error, or replacement decoder belongs here.

// Use the same selected production library under the application crate name
// expected by the real integration test. No test source or API is copied.
extern crate neutral_lsp_error_probe as perl_lsp_rs_core;

#[path = "../../../crates/perl-lsp-rs-core/tests/strict_transport_decode.rs"]
mod strict_transport_decode;

use neutral_lsp_error_probe::protocol::{JsonRpcError, JsonRpcId};
use neutral_lsp_error_probe::runtime::cancellation::{
    CancellationError, CancellationRegistry, PerlLspCancellationToken,
};
use neutral_lsp_error_probe::transport::{
    ContentLengthMessageReader, FramingError, IncomingMessageError, IncomingMessageStage,
    decode_incoming_body, frame,
};
use std::{error::Error, io::Cursor, time::Duration};

type TestResult = Result<(), Box<dyn Error>>;

#[test]
fn all_four_error_families_are_standard_errors() {
    fn standard_error<T: Error>() {}
    standard_error::<JsonRpcError>();
    standard_error::<FramingError>();
    standard_error::<IncomingMessageError>();
    standard_error::<CancellationError>();
}

#[test]
fn jsonrpc_error_preserves_code_message_data_and_display() -> TestResult {
    let error = JsonRpcError::with_data(-32602, "bad parameter", serde_json::json!({"at": 3}));
    assert_eq!(error.code, -32602);
    assert_eq!(error.message, "bad parameter");
    assert_eq!(error.data, Some(serde_json::json!({"at": 3})));
    assert_eq!(error.to_string(), "-32602: bad parameter");
    assert!(error.source().is_none());
    let encoded = serde_json::to_value(&error)?;
    assert_eq!(encoded["code"], -32602);
    assert_eq!(encoded["message"], "bad parameter");
    assert_eq!(encoded["data"]["at"], 3);
    assert!(JsonRpcError::new(-32601, "missing").data.is_none());
    Ok(())
}

#[test]
fn all_framing_variants_preserve_typed_facts() {
    for (error, expected) in [
        (FramingError::InvalidHeader, "invalid Content-Length header"),
        (FramingError::InvalidHeaderUtf8, "header contains invalid UTF-8"),
        (FramingError::MissingContentLength, "missing Content-Length header"),
        (FramingError::InvalidContentLength, "invalid Content-Length value"),
        (FramingError::FrameTooLarge { len: 99 }, "frame too large: 99 bytes"),
    ] {
        assert_eq!(error.to_string(), expected);
        assert_eq!(error.clone(), error);
        assert!(error.source().is_none());
        let incoming = IncomingMessageError::Framing(error.clone());
        assert_eq!(incoming.stage(), IncomingMessageStage::Framing);
        assert_eq!(
            incoming.source().and_then(|cause| cause.downcast_ref::<FramingError>()),
            Some(&error)
        );
    }
}

#[test]
fn incoming_stages_metadata_and_id_types_remain_distinct() {
    let cases = [
        (IncomingMessageError::InvalidUtf8 { payload_bytes: 2, valid_up_to: 1 },
            IncomingMessageStage::Encoding, 2, None),
        (IncomingMessageError::MalformedJson { payload_bytes: 3, line: 1, column: 2 },
            IncomingMessageStage::Json, 3, None),
        (IncomingMessageError::InvalidMessageShape {
            payload_bytes: 4, recoverable_id: Some(JsonRpcId::Integer(7)),
        }, IncomingMessageStage::JsonRpcShape, 4, Some(JsonRpcId::Integer(7))),
        (IncomingMessageError::UnsupportedBatch { payload_bytes: 2 },
            IncomingMessageStage::JsonRpcShape, 2, None),
        (IncomingMessageError::InvalidJsonRpc {
            payload_bytes: 5, recoverable_id: Some(JsonRpcId::String("7".into())),
        }, IncomingMessageStage::JsonRpcShape, 5, Some(JsonRpcId::String("7".into()))),
    ];
    for (error, stage, bytes, id) in cases {
        assert_eq!(error.stage(), stage);
        assert_eq!(error.payload_bytes(), Some(bytes));
        assert_eq!(error.recoverable_id(), id.as_ref());
        assert!(error.source().is_none());
    }
    assert_eq!(IncomingMessageError::Framing(FramingError::InvalidHeader).payload_bytes(), None);
    assert_eq!(
        IncomingMessageError::Framing(FramingError::FrameTooLarge { len: 99 }).payload_bytes(),
        Some(99)
    );
    for (stage, token) in [
        (IncomingMessageStage::Framing, "framing"),
        (IncomingMessageStage::Encoding, "encoding"),
        (IncomingMessageStage::Json, "json"),
        (IncomingMessageStage::JsonRpcShape, "jsonrpc-shape"),
    ] {
        assert_eq!(stage.as_str(), token);
        assert_eq!(stage.to_string(), token);
    }
}

#[test]
fn all_cancellation_variants_retain_payloads_and_display() {
    for (error, expected) in [
        (CancellationError::LockError("same".into()), "Lock error: same"),
        (CancellationError::InvalidRequest("same".into()), "Invalid request: same"),
        (CancellationError::ProviderNotFound("same".into()), "Provider not found: same"),
        (CancellationError::Timeout(Duration::from_millis(1)), "Operation timeout: 1ms"),
    ] {
        assert_eq!(error.to_string(), expected);
        assert!(error.source().is_none());
    }
}

#[test]
fn actual_decoder_is_strict_and_payload_private() -> TestResult {
    let encoding = decode_incoming_body(&[0xff]).err().ok_or("UTF-8 failure was admitted")?;
    assert_eq!(encoding.stage(), IncomingMessageStage::Encoding);
    let json = decode_incoming_body(br#"{"secret":"secret-13998""#)
        .err().ok_or("malformed JSON was admitted")?;
    assert_eq!(json.stage(), IncomingMessageStage::Json);
    assert!(!json.to_string().contains("secret-13998"));
    assert!(!format!("{json:?}").contains("secret-13998"));
    let batch = decode_incoming_body(b"[]").err().ok_or("batch was admitted")?;
    assert!(matches!(batch, IncomingMessageError::UnsupportedBatch { payload_bytes: 2 }));
    Ok(())
}

#[test]
fn actual_reader_reports_failure_then_the_following_frame() -> TestResult {
    let mut bytes = frame(&[0xff]);
    bytes.extend(frame(br#"{"jsonrpc":"2.0","id":"next","method":"shutdown"}"#));
    let mut source = Cursor::new(bytes);
    let mut reader = ContentLengthMessageReader::new();
    let first = reader.read_next_outcome(&mut source)?.ok_or("missing failed-frame outcome")?;
    assert!(matches!(first, Err(IncomingMessageError::InvalidUtf8 { payload_bytes: 1, .. })));
    let next = reader.read_next_outcome(&mut source)?.ok_or("following frame was lost")??;
    assert_eq!(next.id, Some(JsonRpcId::String("next".into())));
    assert_eq!(next.method, "shutdown");
    assert!(reader.read_next_outcome(&mut source)?.is_none());
    Ok(())
}

#[test]
fn actual_registry_keeps_integer_and_string_identifiers_separate() -> TestResult {
    let registry = CancellationRegistry::new();
    let integer = JsonRpcId::Integer(7);
    let string = JsonRpcId::String("7".into());
    for id in [&integer, &string] {
        registry.register_token(PerlLspCancellationToken::new(id.clone(), "probe".into()))?;
    }
    assert_eq!(registry.active_count(), 2);
    registry.cancel_request(&integer)?;
    assert!(registry.is_cancelled(&integer));
    assert!(!registry.is_cancelled(&string));
    registry.remove_request(&integer);
    registry.remove_request(&string);
    assert_eq!(registry.active_count(), 0);
    Ok(())
}

#[test]
fn actual_id_helpers_reject_non_ids_and_preserve_extremes() -> TestResult {
    for value in [serde_json::Value::Null, serde_json::json!(true), serde_json::json!(1.5),
        serde_json::json!([]), serde_json::json!({}), serde_json::json!(u64::MAX)] {
        assert!(JsonRpcId::from_value(&value).is_none());
        assert!(serde_json::from_value::<JsonRpcId>(value).is_err());
    }
    for id in [JsonRpcId::Integer(i64::MIN), JsonRpcId::Integer(i64::MAX),
        JsonRpcId::String("λ/7".into())] {
        assert_eq!(JsonRpcId::from_value(&id.to_value()), Some(id.clone()));
        assert_eq!(serde_json::from_value::<JsonRpcId>(serde_json::to_value(&id)?)?, id);
    }
    Ok(())
}
