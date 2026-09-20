//! Redaction against a hostile fixture, exercised through `serde_json`, not
//! only `Debug`.
//!
//! Builds one event carrying an absolute host path, an API-key-looking
//! secret, and a line of real Perl source, and asserts none of those exact
//! byte sequences — nor a byte length for either non-`Private` value —
//! survive `serde_json::to_string` of the full recorded trace.
//!
//! This crate's fallible test convention: as an integration test file (not
//! an in-module `src/*.rs` unit test), this file does not carry a file-level
//! `#![allow(clippy::unwrap_used, clippy::expect_used)]`. Every test returns
//! `Result<(), Box<dyn std::error::Error>>` and uses `?` (or a `map_err` that
//! preserves the original diagnostic context, matching #14825's "without
//! losing context" principle) instead of `.unwrap()`/`.expect()`.
//! `assert!`/`assert_eq!` remain: they are the actual property checks, not
//! fallible setup, and the tests would be strictly weaker without them.

use perl_operation_trace::{
    EventFieldValue, OperationContext, OperationEvent, OperationEventKind, OperationIdAllocator,
    OperationKind, OperationRecorder, PrivateValue, RecorderBounds, SecretField, SessionId,
};

const HOSTILE_HOST_PATH: &str = "/home/alice/.config/perl-lsp/secrets/prod.env";
const HOSTILE_SECRET: &str = "sk-live-51H8x7KJ3mN9pQrStUvWxYz0123456789abcdef";
const HOSTILE_SOURCE_LINE: &str =
    "my $api_key = 'sk-live-51H8x7KJ3mN9pQrStUvWxYz0123456789abcdef';";

fn hostile_event() -> OperationEvent {
    OperationEvent::new(OperationEventKind::StageStarted)
        .with_field("stage", EventFieldValue::PublicString("compile".into()))
        .with_field("host_path", EventFieldValue::Private(PrivateValue::new(HOSTILE_HOST_PATH)))
        .with_field("source_line", EventFieldValue::Secret(SecretField::new(HOSTILE_SOURCE_LINE)))
        .with_field("api_key_hint", EventFieldValue::Secret(SecretField::new(HOSTILE_SECRET)))
}

#[test]
fn hostile_event_fixture_has_the_expected_kind() {
    // Pins the fixture builder directly: the sole reason `hostile_event()`
    // uses `StageStarted` is that it is the one event kind whose registry
    // entry declares a field at every privacy tier (see `src/registry.rs`).
    // If this constructor's kind ever drifted to one that does not declare
    // `host_path`/`source_line`/`api_key_hint`, `EventRegistry::validate`
    // would reject the event and `hostile_fixture_never_appears_in_the_serialized_trace`
    // would fail via its `record(...)?` — but only as an incidental side
    // effect, not because anything named the expected kind directly.
    assert_eq!(hostile_event().kind(), OperationEventKind::StageStarted);
}

#[test]
fn hostile_fixture_never_appears_in_the_serialized_trace() -> Result<(), Box<dyn std::error::Error>>
{
    let mut allocator = OperationIdAllocator::new(
        SessionId::new("redaction-fixture")
            .map_err(|e| format!("build fixture session id: {e}"))?,
    );
    let context = OperationContext::root(
        allocator.next().ok_or("fresh allocator unexpectedly exhausted its sequence space")?,
        OperationKind::CompilerBuild,
    );

    let mut recorder = OperationRecorder::new(RecorderBounds::new(100, 100_000, 100));
    recorder
        .record(&context, hostile_event())
        .map_err(|e| format!("record registry-valid hostile event: {e}"))?;

    let json = serde_json::to_string(&recorder.snapshot())
        .map_err(|e| format!("serialize the whole trace: {e}"))?;

    // (a) The absolute host path and the Perl source line, supplied as
    // `Private`/`Secret` respectively, must not appear.
    assert!(!json.contains(HOSTILE_HOST_PATH), "host path leaked into the trace:\n{json}");
    assert!(!json.contains(HOSTILE_SOURCE_LINE), "Perl source leaked into the trace:\n{json}");
    assert!(!json.contains("alice"), "host path fragment leaked into the trace:\n{json}");
    assert!(!json.contains("api_key ="), "Perl source fragment leaked into the trace:\n{json}");

    // (b) The API-key-looking secret, supplied as `SecretField`, must not
    // appear (full value or a distinctive prefix).
    assert!(!json.contains(HOSTILE_SECRET), "secret leaked into the trace:\n{json}");
    assert!(
        !json.contains("sk-live-51H8x7KJ3mN9pQrStUvWxYz"),
        "secret prefix leaked into the trace:\n{json}"
    );

    // (c) The load-bearing assertion: the serialized output must not
    // disclose a byte length for either `Secret` value either. `PrivateValue`
    // legitimately does disclose a length (`<redacted:N bytes>`), so a naive
    // implementation that treated `Secret` as "just another Private" would
    // still pass (a) and (b) above but fail here.
    let secret_len = HOSTILE_SECRET.len();
    assert!(
        !json.contains(&format!("<redacted:{secret_len} bytes>")),
        "the secret must not disclose its byte length via the Private-tier redaction shape:\n{json}"
    );
    let source_line_len = HOSTILE_SOURCE_LINE.len();
    assert!(
        !json.contains(&format!("<redacted:{source_line_len} bytes>")),
        "the source line is Secret and must not disclose its byte length either \
         (real Perl source is frequently low-entropy, which is why it is classified \
         Secret rather than Private):\n{json}"
    );
    assert!(
        json.contains("\"<redacted>\""),
        "the secret field must serialize to the fixed, length-free placeholder:\n{json}"
    );

    // Sanity: the trace really did retain the *private* host path's length
    // (proving the two tiers behave differently, not that both redact to
    // nothing observable).
    let host_path_len = HOSTILE_HOST_PATH.len();
    assert!(
        json.contains(&format!("<redacted:{host_path_len} bytes>")),
        "sanity: the Private tier must still disclose its own byte length:\n{json}"
    );

    Ok(())
}
