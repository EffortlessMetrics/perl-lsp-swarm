//! Exact wire-shape contract for the LSP `textDocumentSync` capability.

use serde_json::{Value, json};
use std::collections::BTreeSet;

mod support;
use support::lsp_harness::LspHarness;

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[test]
fn text_document_sync_options_have_exact_lsp_wire_shape() -> TestResult {
    let mut harness = LspHarness::new();
    let response = harness.initialize(Some(json!({
        "textDocument": {},
        "workspace": {}
    })))?;

    let sync = response
        .pointer("/capabilities/textDocumentSync")
        .and_then(Value::as_object)
        .ok_or("textDocumentSync must be an object")?;

    let observed_keys: BTreeSet<_> = sync.keys().map(String::as_str).collect();
    let expected_keys = BTreeSet::from([
        "change",
        "openClose",
        "save",
        "willSave",
        "willSaveWaitUntil",
    ]);
    assert_eq!(
        observed_keys, expected_keys,
        "textDocumentSync must change only through an intentional wire-contract update"
    );

    assert_eq!(sync.get("openClose"), Some(&Value::Bool(true)));
    assert_eq!(sync.get("change").and_then(Value::as_u64), Some(1));
    assert_eq!(sync.get("willSave"), Some(&Value::Bool(true)));
    assert_eq!(sync.get("willSaveWaitUntil"), Some(&Value::Bool(true)));

    let save = sync
        .get("save")
        .and_then(Value::as_object)
        .ok_or("textDocumentSync.save must be an object")?;
    let observed_save_keys: BTreeSet<_> = save.keys().map(String::as_str).collect();
    assert_eq!(
        observed_save_keys,
        BTreeSet::from(["includeText"]),
        "textDocumentSync.save must preserve its exact LSP wire shape"
    );
    assert_eq!(save.get("includeText"), Some(&Value::Bool(true)));

    Ok(())
}
