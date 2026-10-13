//! Direct oracles for the production callback used by the didSave registry row.
//! Public validator and real-process tests separately prove consumer routing.

use super::{SchemaError, did_save_params};
use serde_json::json;

const METHOD: &str = "textDocument/didSave";

#[test]
fn save_success_is_observed_with_absent_empty_and_nonempty_text() {
    let params = json!({"textDocument": {"uri": "untitled:save"}});
    assert_eq!(did_save_params(METHOD, &params), Ok(()));
    let mut with_text = params;
    with_text["text"] = json!("");
    assert_eq!(did_save_params(METHOD, &with_text), Ok(()));
    with_text["text"] = json!("my $saved = 1;\n");
    assert_eq!(did_save_params(METHOD, &with_text), Ok(()));
}

#[test]
fn present_null_text_preserves_the_exact_failure() {
    assert_eq!(
        did_save_params(METHOD, &json!({"textDocument": {"uri": "untitled:save"}, "text": null})),
        Err(SchemaError {
            method: Some(METHOD.to_string()),
            path: "$.params.text".to_string(),
            expected: "string".to_string(),
            observed: "null".to_string(),
        })
    );
}
