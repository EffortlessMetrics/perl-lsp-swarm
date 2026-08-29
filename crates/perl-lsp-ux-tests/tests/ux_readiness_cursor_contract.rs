// Test infrastructure — allow test-friendly patterns.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

#[path = "support/active_document_readiness.rs"]
mod active_document_readiness;

use active_document_readiness::{
    ACTIVE_DOCUMENT_READY_METHOD, has_generation_after, ready_generation, ready_generations,
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
    let other_uri = "file:///workspace/lib/Other.pm";
    let mut events = vec![ready(uri, json!(1)), ready(uri, json!(2))];
    let cursor = ready_generations(&events, uri).len();

    assert!(!has_generation_after(
        &ready_generations(&events, uri),
        cursor,
        1,
    ));

    events.push(ready(uri, json!(2)));
    assert!(
        !has_generation_after(&ready_generations(&events, uri), cursor, 1),
        "a delayed pre-close generation must not release a generation-1 reopen wait"
    );

    events.push(ready(other_uri, json!(1)));
    assert!(
        !has_generation_after(&ready_generations(&events, uri), cursor, 1),
        "readiness for another URI must not release this document's wait"
    );

    events.push(ready(uri, json!(1)));
    assert!(has_generation_after(
        &ready_generations(&events, uri),
        cursor,
        1,
    ));
}

#[test]
fn readiness_decoder_requires_exact_method_uri_and_numeric_generation() {
    let uri = "file:///workspace/lib/App.pm";
    assert_eq!(ready_generation(&ready(uri, json!(2)), uri), Some(2));

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
        ready("file:///workspace/lib/Other.pm", json!(2)),
    ] {
        assert_eq!(ready_generation(&invalid, uri), None);
    }
}

#[test]
fn cursor_beyond_observed_events_is_not_silently_clamped() {
    let generations = vec![1, 2];
    assert!(!has_generation_after(&generations, 3, 1));
}
