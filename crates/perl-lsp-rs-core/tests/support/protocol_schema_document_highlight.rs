//! Direct oracles for the production callbacks used by the checked registry.
//! Public validator and real-process tests separately prove consumer routing.

use super::{SchemaError, document_highlight_params, document_highlight_result};
use serde_json::{Value, json};

const METHOD: &str = "textDocument/documentHighlight";

fn range() -> Value {
    json!({"start": {"line": 0, "character": 3}, "end": {"line": 0, "character": 9}})
}

#[test]
fn params_success_is_observed_for_absent_string_and_integer_progress_tokens() {
    let params = json!({
        "textDocument": {"uri": "untitled:highlight"},
        "position": {"line": 0, "character": 3}
    });
    assert_eq!(document_highlight_params(METHOD, &params), Ok(()));
    let mut with_tokens = params;
    with_tokens["workDoneToken"] = json!("work");
    with_tokens["partialResultToken"] = json!(-7);
    assert_eq!(document_highlight_params(METHOD, &with_tokens), Ok(()));
}

#[test]
fn result_success_is_observed_for_null_empty_and_known_highlight_kinds() {
    assert_eq!(document_highlight_result(METHOD, &Value::Null), Ok(()));
    assert_eq!(document_highlight_result(METHOD, &json!([])), Ok(()));
    let highlights = json!([
        {"range": range()},
        {"range": range(), "kind": 1},
        {"range": range(), "kind": 2},
        {"range": range(), "kind": 3}
    ]);
    assert_eq!(document_highlight_result(METHOD, &highlights), Ok(()));
}

#[test]
fn non_array_result_reports_the_exact_payload_error() {
    assert_eq!(
        document_highlight_result(METHOD, &json!(false)),
        Err(SchemaError {
            method: Some(METHOD.to_string()),
            path: "$.result".to_string(),
            expected: "DocumentHighlight array or null".to_string(),
            observed: "boolean:false".to_string(),
        })
    );
}

#[test]
fn out_of_range_kind_reports_the_exact_kind_error() {
    assert_eq!(
        document_highlight_result(METHOD, &json!([{"range": range(), "kind": 4}])),
        Err(SchemaError {
            method: Some(METHOD.to_string()),
            path: "$.result[0].kind".to_string(),
            expected: "DocumentHighlightKind 1, 2, or 3".to_string(),
            observed: "4".to_string(),
        })
    );
}
