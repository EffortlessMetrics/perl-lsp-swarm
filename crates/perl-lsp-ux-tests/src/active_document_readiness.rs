//! Generation-aware observations for `perl-lsp/active-document-ready`.
//!
//! The server can emit more than one readiness notification for the same URI
//! across edits and document sessions. Consumers that wait only by URI can
//! therefore reuse stale buffered evidence. This module keeps the event cursor
//! and expected generation load-bearing without introducing another readiness
//! state machine.

use crate::LspEvent;
use serde_json::Value;

/// Server notification method used for active-document parser-core readiness.
pub const ACTIVE_DOCUMENT_READY_METHOD: &str = "perl-lsp/active-document-ready";

/// One decoded active-document readiness notification.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActiveDocumentReadyEvent<'a> {
    /// Exact document URI carried by the server notification.
    pub uri: &'a str,
    /// Exact accepted document generation carried by the notification.
    pub generation: u64,
}

/// Decode a numeric active-document readiness notification.
///
/// Wrong methods, missing URIs, and missing or non-numeric generations are not
/// valid readiness evidence and return `None`.
pub fn active_document_ready_event(event: &LspEvent) -> Option<ActiveDocumentReadyEvent<'_>> {
    let LspEvent::Other { method, params } = event else {
        return None;
    };
    if method != ACTIVE_DOCUMENT_READY_METHOD {
        return None;
    }
    Some(ActiveDocumentReadyEvent {
        uri: params.get("uri").and_then(Value::as_str)?,
        generation: params.get("generation").and_then(Value::as_u64)?,
    })
}

/// Count valid readiness events already observed for one exact URI.
pub fn active_document_ready_event_count(events: &[LspEvent], uri: &str) -> usize {
    events
        .iter()
        .filter_map(active_document_ready_event)
        .filter(|event| event.uri == uri)
        .count()
}

/// Return whether `expected_generation` appears after a caller-owned event cursor.
pub fn has_active_document_ready_generation_after(
    events: &[LspEvent],
    uri: &str,
    already_seen: usize,
    expected_generation: u64,
) -> bool {
    events
        .iter()
        .filter_map(active_document_ready_event)
        .filter(|event| event.uri == uri)
        .skip(already_seen)
        .any(|event| event.generation == expected_generation)
}

#[cfg(test)]
mod tests {
    use super::{
        ACTIVE_DOCUMENT_READY_METHOD, active_document_ready_event,
        active_document_ready_event_count, has_active_document_ready_generation_after,
    };
    use crate::LspEvent;
    use serde_json::json;

    fn ready(uri: &str, generation: serde_json::Value) -> LspEvent {
        LspEvent::Other {
            method: ACTIVE_DOCUMENT_READY_METHOD.to_string(),
            params: json!({"uri": uri, "generation": generation}),
        }
    }

    #[test]
    fn decoder_requires_exact_method_uri_and_numeric_generation() {
        let wanted_uri = "file:///workspace/lib/App.pm";
        assert_eq!(
            active_document_ready_event(&ready(wanted_uri, json!(2))),
            Some(super::ActiveDocumentReadyEvent {
                uri: wanted_uri,
                generation: 2,
            })
        );

        for invalid in [
            LspEvent::Other {
                method: "perl-lsp/other".to_string(),
                params: json!({"uri": wanted_uri, "generation": 2}),
            },
            LspEvent::Other {
                method: ACTIVE_DOCUMENT_READY_METHOD.to_string(),
                params: json!({"generation": 2}),
            },
            ready(wanted_uri, json!("2")),
        ] {
            assert_eq!(active_document_ready_event(&invalid), None);
        }
    }

    #[test]
    fn old_and_delayed_generations_do_not_release_post_snapshot_wait() {
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
}
