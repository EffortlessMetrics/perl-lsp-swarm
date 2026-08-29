// Test infrastructure — allow test-friendly patterns.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use perl_lsp_ux_tests::active_document_readiness::{
    ACTIVE_DOCUMENT_READY_METHOD, active_document_ready_event,
    active_document_ready_event_count, has_active_document_ready_generation_after,
};
use perl_lsp_ux_tests::LspEvent;
use serde_json::json;

fn ready(uri: &str, generation: serde_json::Value) -> LspEvent {
    LspEvent::Other {
        method: ACTIVE_DOCUMENT_READY_METHOD.to_string(),
        params: json!({"uri": uri, "generation": generation}),
    }
}

#[test]
fn readiness_cursor_rejects_historical_delayed_and_cross_uri_evidence() {
    let uri = "file:///workspace/lib/App.pm";
    let mut events = vec![ready(uri, json!(1)), ready(uri, json!(2))];
    let cursor = active_document_ready_event_count(&events, uri);

    assert!(!has_active_document_ready_generation_after(
        &events, uri, cursor, 1
    ));

    events.push(ready(uri, json!(2)));
    assert!(
        !has_active_document_ready_generation_after(&events, uri, cursor, 1),
        "a delayed pre-close generation must not release a generation-1 reopen wait"
    );

    events.push(ready("file:///workspace/lib/Other.pm", json!(1)));
    assert!(!has_active_document_ready_generation_after(
        &events, uri, cursor, 1
    ));

    events.push(ready(uri, json!(1)));
    assert!(has_active_document_ready_generation_after(
        &events, uri, cursor, 1
    ));
}

#[test]
fn readiness_decoder_rejects_wrong_method_missing_uri_and_string_generation() {
    let uri = "file:///workspace/lib/App.pm";
    assert_eq!(
        active_document_ready_event(&ready(uri, json!(2)))
            .map(|event| (event.uri, event.generation)),
        Some((uri, 2))
    );

    for invalid in [
        LspEvent::Other {
            method: "perl-lsp/other".to_string(),
            params: json!({"uri": uri, "generation": 2}),
        },
        LspEvent::Other {
            method: ACTIVE_DOCUMENT_READY_METHOD.to_string(),
            params: json!({"generation": 2}),
        },
        ready(uri, json!("2")),
    ] {
        assert!(active_document_ready_event(&invalid).is_none());
    }
}
