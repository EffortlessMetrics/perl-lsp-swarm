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
const SECOND_EDIT_TEXT: &str = "my $x = 3;\n";
const URI_TAIL: &str = "7301_work_receipt.pl";

#[test]
#[serial_test::serial]
fn did_open_and_async_edit_emit_versioned_work_receipt() -> TestResult {
    let server = Arc::new(LspServer::new());
    let _capture = server.test_text_sync_work_capture_start();
    server.test_install_parse_worker();
    server.test_handle_initialize_dispatch(Some(json!({
        "capabilities": {},
        "rootUri": null,
        "workspaceFolders": null
    })))?;

    server.test_apply_did_open(URI, OPEN_TEXT, 1)?;
    server.test_apply_did_change(URI, EDIT_TEXT, 2)?;

    let receipt = server.test_text_sync_work_capture_receipt(
        URI,
        "did_open_plus_one_full_replacement_edit",
        "text_sync_work_receipt_7301::did_open_and_async_edit_emit_versioned_work_receipt",
    );

    assert_eq!(receipt.schema_version, RECEIPT_SCHEMA_VERSION);
    assert_eq!(receipt.instrument_status, InstrumentStatus::Ok);
    assert_eq!(receipt.accepted_generations, 2);
    assert_eq!(receipt.totals.full_source_materializations, 1);
    assert_eq!(receipt.totals.source_buffer_shares, 1);
    assert_eq!(receipt.totals.canonical_digest_computed, 2);
    assert_eq!(receipt.totals.documents_lock_acquire_release_pairs, 1);
    let expected_copied = 2 * u64::try_from(OPEN_TEXT.len())? + 4 * u64::try_from(EDIT_TEXT.len())?;
    assert_eq!(
        receipt.totals.full_source_bytes_copied, expected_copied,
        "owned open strings, edit materialization, edit Arc transport, edit state copy, and both snapshot Arc<str> copies"
    );

    let worker = receipt.parse_worker.as_ref().ok_or("worker metrics missing")?;
    assert_eq!(worker.jobs_enqueued, 1);
    assert_eq!(worker.jobs_published, 1);
    let accepted_paths: Vec<&str> = receipt
        .events
        .iter()
        .filter(|event| event.kind == "parsed_snapshot_source_identity")
        .map(|event| event.sync_path.as_str())
        .collect();
    assert_eq!(accepted_paths, ["sync", "async"]);
    assert_eq!(
        receipt.events.iter().filter(|event| event.kind == "rope_rebuilt").count(),
        1,
        "full replacement is reconstruction, not an incremental rope edit"
    );
    assert!(receipt.events.iter().all(|event| event.kind != "rope_edit_applied"));
    for event in &receipt.events {
        assert_eq!(event.uri_tail, URI_TAIL, "bounded document identity must not be fabricated");
    }

    drop(_capture);

    // A second window must use worker deltas and count only its own accepted
    // snapshot, not lifetime generations or counters from the first workload.
    let _capture = server.test_text_sync_work_capture_start();
    server.test_apply_did_change(URI, SECOND_EDIT_TEXT, 3)?;
    let second = server.test_text_sync_work_capture_receipt(
        URI,
        "one_full_replacement_after_an_earlier_capture",
        "text_sync_work_receipt_7301::second_capture_uses_worker_delta",
    );
    assert_eq!(second.instrument_status, InstrumentStatus::Ok);
    assert_eq!(second.accepted_generations, 1);
    let second_worker = second.parse_worker.as_ref().ok_or("second worker metrics missing")?;
    assert_eq!(second_worker.jobs_enqueued, 1);
    assert_eq!(second_worker.jobs_published, 1);
    assert!(second.events.iter().all(|event| event.uri_tail == URI_TAIL));

    Ok(())
}

#[test]
#[serial_test::serial]
fn capture_sessions_are_exclusive_across_servers() -> TestResult {
    let first = Arc::new(LspServer::new());
    let second = Arc::new(LspServer::new());
    let (first_started_tx, first_started_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let holder = std::thread::spawn(move || {
        let _capture = first.test_text_sync_work_capture_start();
        let _ = first_started_tx.send(());
        let _ = release_rx.recv();
    });
    first_started_rx.recv_timeout(std::time::Duration::from_secs(5))?;

    let (second_started_tx, second_started_rx) = std::sync::mpsc::channel();
    let contender = std::thread::spawn(move || {
        let _capture = second.test_text_sync_work_capture_start();
        let _ = second_started_tx.send(());
    });
    assert!(
        second_started_rx.recv_timeout(std::time::Duration::from_millis(50)).is_err(),
        "second server entered capture while the first session was live"
    );

    release_tx.send(())?;
    second_started_rx.recv_timeout(std::time::Duration::from_secs(5))?;
    holder.join().map_err(|_| std::io::Error::other("first capture thread panicked"))?;
    contender.join().map_err(|_| std::io::Error::other("second capture thread panicked"))?;
    Ok(())
}
