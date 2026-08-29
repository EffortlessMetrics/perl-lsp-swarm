//! Active-document readiness observation helpers for UX scenarios.

use crate::LspEvent;
use serde_json::Value;

const ACTIVE_DOCUMENT_READY_METHOD: &str = "perl-lsp/active-document-ready";

pub(crate) fn active_document_ready_generation(event: &LspEvent, uri: &str) -> Option<u64> {
    let LspEvent::Other { method, params } = event else {
        return None;
    };
    if method != ACTIVE_DOCUMENT_READY_METHOD
        || params.get("uri").and_then(Value::as_str) != Some(uri)
    {
        return None;
    }
    params.get("generation").and_then(Value::as_u64)
}

pub(crate) fn has_generation_after(
    generations: &[u64],
    already_seen: usize,
    expected_generation: u64,
) -> bool {
    generations
        .get(already_seen..)
        .is_some_and(|new_generations| new_generations.contains(&expected_generation))
}

#[cfg(test)]
mod tests {
    use super::{
        ACTIVE_DOCUMENT_READY_METHOD, active_document_ready_generation, has_generation_after,
    };
    use crate::LspEvent;
    use serde_json::json;

    #[test]
    fn readiness_filter_requires_matching_method_uri_and_numeric_generation() {
        let wanted_uri = "file:///workspace/current.pl";
        let matching = LspEvent::Other {
            method: ACTIVE_DOCUMENT_READY_METHOD.to_string(),
            params: json!({"uri": wanted_uri, "generation": 2}),
        };
        assert_eq!(active_document_ready_generation(&matching, wanted_uri), Some(2));

        for rejected in [
            LspEvent::Other {
                method: ACTIVE_DOCUMENT_READY_METHOD.to_string(),
                params: json!({"uri": "file:///workspace/other.pl", "generation": 2}),
            },
            LspEvent::Other {
                method: "perl-lsp/other".to_string(),
                params: json!({"uri": wanted_uri, "generation": 2}),
            },
            LspEvent::Other {
                method: ACTIVE_DOCUMENT_READY_METHOD.to_string(),
                params: json!({"uri": wanted_uri, "generation": "2"}),
            },
            LspEvent::Other {
                method: ACTIVE_DOCUMENT_READY_METHOD.to_string(),
                params: json!({"uri": wanted_uri}),
            },
        ] {
            assert_eq!(active_document_ready_generation(&rejected, wanted_uri), None);
        }
    }

    #[test]
    fn historical_and_wrong_generation_events_do_not_release_checkpoint() {
        let mut generations = vec![1, 2];
        let checkpoint = generations.len();

        assert!(!has_generation_after(&generations, checkpoint, 1));

        generations.push(2);
        assert!(
            !has_generation_after(&generations, checkpoint, 1),
            "a delayed pre-checkpoint generation must not release a generation-1 barrier"
        );

        generations.push(1);
        assert!(has_generation_after(&generations, checkpoint, 1));
    }
}
