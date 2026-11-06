//! #7301: deterministic work-volume receipt for didOpen + async didChange (first slice).

#![cfg(feature = "expose_lsp_test_api")]
use perl_lsp::LspServer;
use perl_lsp::runtime::{InstrumentStatus, RECEIPT_SCHEMA_VERSION};
use serde_json::json;
use std::sync::Arc;

type TestResult = Result<(), Box<dyn std::error::Error>>;

const URI: &str = "file:///7301_work_receipt.pl";
const OPEN_TEXT: &str = "my $x = 1;\n";
const EDIT_TEXT: &str = "my $x = 2;\n";

#[test]
#[serial_test::serial]
fn did_open_and_async_edit_emit_versioned_work_receipt() -> TestResult {
    let server = Arc::new(LspServer::new());
    server.test_text_sync_work_capture_start();
    server.test_install_parse_worker();
    server.test_handle_initialize_dispatch(Some(json!({
        "capabilities": {},
        "rootUri": null,
        "workspaceFolders": null
    })))?;

    server.test_apply_did_open(URI, OPEN_TEXT, 1)?;
    server.test_apply_did_change(URI, EDIT_TEXT, 2)?;

    let receipt = server.test_text_sync_work_capture_receipt(
        "did_open_plus_one_full_replacement_edit",
        "text_sync_work_receipt_7301::did_open_and_async_edit_emit_versioned_work_receipt",
    );

    assert_eq!(receipt.schema_version, RECEIPT_SCHEMA_VERSION);
    assert_eq!(receipt.instrument_status, InstrumentStatus::Ok);
    assert!(
        receipt.totals.full_source_materializations >= 1,
        "expected at least one materialization on edit path; totals={:?}",
        receipt.totals
    );
    assert!(
        receipt.totals.source_buffer_shares >= 1,
        "parse worker enqueue must record Arc share, not copied bytes; totals={:?}",
        receipt.totals
    );
    assert!(
        receipt.totals.full_source_bytes_copied >= u64::try_from(EDIT_TEXT.len())?,
        "replace_text_state copy must be counted; totals={:?}",
        receipt.totals
    );
    assert!(
        receipt.totals.canonical_digest_computed >= 1,
        "didOpen snapshot hash must be recorded; totals={:?}",
        receipt.totals
    );
    assert!(
        receipt.totals.documents_lock_acquire_release_pairs >= 1,
        "documents lock hold must be observable; totals={:?}",
        receipt.totals
    );
    assert!(
        receipt.parse_worker.as_ref().is_some_and(|m| m.jobs_enqueued >= 1),
        "worker metrics must compose into receipt; receipt={receipt:?}"
    );

    for event in &receipt.events {
        assert!(
            !event.uri_tail.contains('\\') || event.uri_tail.ends_with(".pl"),
            "receipt must not carry host paths; event={event:?}"
        );
    }

    Ok(())
}
