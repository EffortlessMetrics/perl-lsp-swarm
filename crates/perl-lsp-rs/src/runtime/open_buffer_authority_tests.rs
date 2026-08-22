//! Open-buffer authority across external change, delete, and rename (#8041).
//!
//! Internal proof for the backing-file transition policy: while a document
//! is open its editor buffer is the only authoritative source, external
//! filesystem events must not evict it or re-derive workspace facts from
//! contradicting disk bytes, and `didSave`/`didClose` complete the handoff
//! deterministically.
#![expect(
    clippy::unwrap_used,
    reason = "test-only policy proof: https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/3021"
)]
#![expect(
    clippy::expect_used,
    reason = "test-only policy proof: https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/3021"
)]
#![expect(
    clippy::panic,
    reason = "test-only barrier failure is a hard test error, not a production path"
)]

use super::{BackingFileTransition, LspServer};
use serde_json::json;
use tempfile::TempDir;

fn file_uri(dir: &TempDir, name: &str) -> String {
    url::Url::from_file_path(dir.path().join(name))
        .expect("temp path must convert to file URI")
        .to_string()
}

fn write_file(dir: &TempDir, name: &str, content: &str) {
    std::fs::write(dir.path().join(name), content).expect("write temp workspace file");
}

fn delete_file(dir: &TempDir, name: &str) {
    let _ = std::fs::remove_file(dir.path().join(name));
}

fn watched_changes(server: &LspServer, uri: &str, change_type: i32) {
    server
        .handle_did_change_watched_files(Some(json!({
            "changes": [{ "uri": uri, "type": change_type }]
        })))
        .expect("watched-files notification must parse");
}

/// Wait until the synchronous-fallback index tasks have settled.
fn wait_for_index_tasks(server: &LspServer) {
    for _ in 0..200 {
        if server.pending_index_task_count.load(std::sync::atomic::Ordering::SeqCst) == 0 {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    panic!("background index tasks did not drain");
}

fn index_symbols(server: &LspServer, query: &str) -> Vec<(String, String)> {
    server
        .coordinator()
        .map(|coordinator| {
            coordinator
                .index()
                .find_symbols(query)
                .into_iter()
                .map(|symbol| (symbol.name, symbol.uri))
                .collect()
        })
        .unwrap_or_default()
}

fn document_text(server: &LspServer, uri: &str) -> Option<String> {
    let documents = server.documents.lock();
    server.get_document(&documents, uri).map(|doc| doc.text_str().to_string())
}

fn did_open(server: &LspServer, uri: &str, text: &str) {
    server
        .handle_did_open(Some(json!({
            "textDocument": {
                "uri": uri,
                "languageId": "perl",
                "version": 1,
                "text": text
            }
        })))
        .expect("didOpen params are valid");
    wait_for_index_tasks(server);
}

fn did_change_full(server: &LspServer, uri: &str, version: i32, text: &str) {
    server
        .handle_did_change(Some(json!({
            "textDocument": { "uri": uri, "version": version },
            "contentChanges": [{ "text": text }]
        })))
        .expect("didChange params are valid");
    wait_for_index_tasks(server);
}

fn did_close(server: &LspServer, uri: &str) {
    server
        .handle_did_close(Some(json!({
            "textDocument": { "uri": uri }
        })))
        .expect("didClose params are valid");
    wait_for_index_tasks(server);
}

#[test]
fn watched_change_on_open_document_never_indexes_disk_over_buffer() {
    let dir = TempDir::new().expect("tempdir");
    let uri = file_uri(&dir, "diverged.pl");

    write_file(&dir, "diverged.pl", "package Diverged;\nsub v1_only { }\n1;\n");
    let server = LspServer::new();
    did_open(&server, &uri, "package Diverged;\nsub v1_only { }\n1;\n");
    assert_eq!(index_symbols(&server, "v1_only").len(), 1, "v1 indexed from open baseline");

    // Unsaved editor edit to v2; the inline commit binds the index to the buffer.
    did_change_full(&server, &uri, 2, "package Diverged;\nsub v2_only { }\n1;\n");
    assert_eq!(
        index_symbols(&server, "v2_only").len(),
        1,
        "buffer snapshot committed at the edited generation"
    );

    // External disk write to v3 followed by the watcher observation.
    write_file(&dir, "diverged.pl", "package Diverged;\nsub v3_only { }\n1;\n");
    watched_changes(&server, &uri, 2);

    wait_for_index_tasks(&server);
    assert!(
        index_symbols(&server, "v3_only").is_empty(),
        "disk bytes behind an authoritative open buffer must never be indexed (#8041)"
    );
    assert_eq!(
        index_symbols(&server, "v2_only").len(),
        1,
        "the last accepted buffer snapshot remains the cross-file authority"
    );
    assert_eq!(
        document_text(&server, &uri).as_deref(),
        Some("package Diverged;\nsub v2_only { }\n1;\n"),
        "the open buffer text is untouched"
    );
    assert_eq!(
        server.take_backing_file_transition(&uri),
        Some(BackingFileTransition::Changed),
        "the divergence must be recorded for the save/close handoff"
    );
}

#[test]
fn watched_delete_preserves_open_unsaved_document_and_generation() {
    let dir = TempDir::new().expect("tempdir");
    let uri = file_uri(&dir, "doomed.pl");
    let v2 = "package Doomed;\nsub unsaved_work { }\n1;\n";

    write_file(&dir, "doomed.pl", "package Doomed;\nsub original { }\n1;\n");
    let server = LspServer::new();
    did_open(&server, &uri, "package Doomed;\nsub original { }\n1;\n");
    did_change_full(&server, &uri, 2, v2);

    let generation_arc = server.document_freshness(&uri).expect("open document").2;

    delete_file(&dir, "doomed.pl");
    watched_changes(&server, &uri, 3);

    assert_eq!(
        document_text(&server, &uri).as_deref(),
        Some(v2),
        "a watched disk deletion must not evict the open buffer"
    );
    assert_ne!(
        generation_arc.load(std::sync::atomic::Ordering::SeqCst),
        u32::MAX,
        "the document generation must survive the external delete"
    );
    assert!(
        server.coordinator().is_none_or(|c| c.index().indexed_generation(&uri).is_none()),
        "stale disk-backed facts must be unavailable after the delete"
    );
    assert_eq!(server.take_backing_file_transition(&uri), Some(BackingFileTransition::Deleted),);
}

#[test]
fn close_after_external_delete_removes_subject_without_resurrection() {
    let dir = TempDir::new().expect("tempdir");
    let uri = file_uri(&dir, "gone_on_close.pl");

    write_file(&dir, "gone_on_close.pl", "package GoneOnClose;\nsub pre_delete { }\n1;\n");
    let server = LspServer::new();
    did_open(&server, &uri, "package GoneOnClose;\nsub pre_delete { }\n1;\n");
    did_change_full(&server, &uri, 2, "package GoneOnClose;\nsub unsaved { }\n1;\n");

    delete_file(&dir, "gone_on_close.pl");
    watched_changes(&server, &uri, 3);
    did_close(&server, &uri);

    assert!(
        document_text(&server, &uri).is_none(),
        "close with the backing file absent removes the source subject"
    );
    assert!(
        server.coordinator().is_none_or(|c| c.index().indexed_generation(&uri).is_none()),
        "no disk-backed facts may be resurrected after close-after-delete"
    );
    assert!(index_symbols(&server, "unsaved").is_empty());
    assert!(index_symbols(&server, "pre_delete").is_empty());
    assert_eq!(
        server.take_backing_file_transition(&uri),
        None,
        "the transition marker must be consumed exactly once by close"
    );
}

#[test]
fn close_after_external_divergence_reloads_current_disk_bytes() {
    let dir = TempDir::new().expect("tempdir");
    let uri = file_uri(&dir, "reload_on_close.pl");

    write_file(&dir, "reload_on_close.pl", "package ReloadOnClose;\nsub v1_only { }\n1;\n");
    let server = LspServer::new();
    did_open(&server, &uri, "package ReloadOnClose;\nsub v1_only { }\n1;\n");
    did_change_full(&server, &uri, 2, "package ReloadOnClose;\nsub buffer_only { }\n1;\n");

    // External change observed while open: skipped under buffer authority.
    write_file(&dir, "reload_on_close.pl", "package ReloadOnClose;\nsub fresh_disk_only { }\n1;\n");
    watched_changes(&server, &uri, 2);
    did_close(&server, &uri);

    let symbols = index_symbols(&server, "_only");
    assert!(
        symbols.iter().any(|(name, _)| name == "fresh_disk_only"),
        "closed-file authority reloads stable current disk source, got {symbols:?}"
    );
    assert!(
        !symbols.iter().any(|(name, _)| name == "v1_only"),
        "pre-divergence bytes must not survive close"
    );
    assert!(
        !symbols.iter().any(|(name, _)| name == "buffer_only"),
        "the discarded buffer must not leak into closed-file facts"
    );
}

#[test]
fn save_after_external_delete_recoheres_index_from_authoritative_buffer() {
    let dir = TempDir::new().expect("tempdir");
    let uri = file_uri(&dir, "saved_back.pl");
    let saved = "package SavedBack;\nsub recreated_by_save { }\n1;\n";

    write_file(&dir, "saved_back.pl", "package SavedBack;\nsub original { }\n1;\n");
    let server = LspServer::new();
    did_open(&server, &uri, "package SavedBack;\nsub original { }\n1;\n");
    did_change_full(&server, &uri, 2, saved);

    // Delete observed externally, then the editor recreates the file on save.
    delete_file(&dir, "saved_back.pl");
    watched_changes(&server, &uri, 3);

    write_file(&dir, "saved_back.pl", saved);
    let doc_generation = server.document_generation(&uri).expect("still open");
    server
        .handle_did_save(Some(json!({ "textDocument": { "uri": uri } })))
        .expect("didSave params are valid");
    wait_for_index_tasks(&server);

    let indexed = server.coordinator().and_then(|c| c.index().indexed_generation(&uri));
    assert_eq!(
        indexed,
        Some(doc_generation),
        "save must commit the authoritative buffer snapshot at its own generation"
    );
    assert_eq!(
        index_symbols(&server, "recreated_by_save").len(),
        1,
        "workspace facts re-cohere with the saved buffer"
    );
    assert_eq!(
        server.take_backing_file_transition(&uri),
        None,
        "save consumes the divergence marker"
    );
}

#[test]
fn rename_of_open_document_keeps_instance_and_prevents_duplicate_facts() {
    let dir = TempDir::new().expect("tempdir");
    let old_uri = file_uri(&dir, "renamed_clean.pm");
    let new_uri = file_uri(&dir, "renamed_new.pm");

    write_file(&dir, "renamed_clean.pm", "package RenamedClean;\nsub clean_body { }\n1;\n");
    write_file(&dir, "renamed_new.pm", "package RenamedClean;\nsub clean_body { }\n1;\n");
    let server = LspServer::new();
    did_open(&server, &old_uri, "package RenamedClean;\nsub clean_body { }\n1;\n");

    server
        .handle_did_rename_files(Some(json!({
            "files": [{ "oldUri": old_uri, "newUri": new_uri }]
        })))
        .expect("didRenameFiles params are valid");
    wait_for_index_tasks(&server);

    assert!(
        document_text(&server, &old_uri).is_some(),
        "file-operation authority does not retarget an open buffer's identity"
    );
    assert!(
        document_text(&server, &new_uri).is_none(),
        "the buffer instance must not silently move to the new URI"
    );
    assert!(
        server.coordinator().is_none_or(|c| c.index().indexed_generation(&old_uri).is_none()),
        "old rename identity must not remain a current workspace fact"
    );
    assert_eq!(
        index_symbols(&server, "clean_body").iter().map(|(_, uri)| uri.clone()).collect::<Vec<_>>(),
        vec![normalize_like_index(&new_uri)],
        "exactly one current fact for the renamed subject, bound to the new path"
    );
    assert_eq!(
        server.take_backing_file_transition(&old_uri),
        Some(BackingFileTransition::RenamedOrMoved { new_uri: normalize_like_index(&new_uri) }),
    );
}

fn normalize_like_index(uri: &str) -> String {
    perl_uri::uri_key(uri)
}

#[test]
fn rename_of_open_unsaved_document_indexes_disk_not_buffer() {
    let dir = TempDir::new().expect("tempdir");
    let old_uri = file_uri(&dir, "dirty_old.pm");
    let new_uri = file_uri(&dir, "dirty_new.pm");

    write_file(&dir, "dirty_old.pm", "package DirtyOld;\nsub old_disk_leaf { }\n1;\n");
    write_file(&dir, "dirty_new.pm", "package DirtyNew;\nsub new_disk_leaf { }\n1;\n");
    let server = LspServer::new();
    did_open(&server, &old_uri, "package DirtyOld;\nsub old_disk_leaf { }\n1;\n");
    did_change_full(&server, &old_uri, 2, "package DirtyOld;\nsub buffer_only_leaf { }\n1;\n");

    server
        .handle_did_rename_files(Some(json!({
            "files": [{ "oldUri": old_uri, "newUri": new_uri }]
        })))
        .expect("didRenameFiles params are valid");
    wait_for_index_tasks(&server);

    assert_eq!(
        document_text(&server, &old_uri).as_deref(),
        Some("package DirtyOld;\nsub buffer_only_leaf { }\n1;\n"),
        "unsaved source survives the rename observation untouched"
    );
    assert_eq!(
        index_symbols(&server, "new_disk_leaf").len(),
        1,
        "the new path is indexed from disk truth for the closed successor subject"
    );
    assert!(
        index_symbols(&server, "buffer_only_leaf").is_empty(),
        "cross-file facts must not derive from the old URI's diverged buffer via the rename"
    );
    assert!(
        index_symbols(&server, "old_disk_leaf").is_empty(),
        "duplicate old/new identities must not coexist as current workspace facts"
    );
}

#[test]
fn watcher_observed_rename_pair_preserves_open_buffer() {
    let dir = TempDir::new().expect("tempdir");
    let old_uri = file_uri(&dir, "watch_renamed.pm");
    let new_uri = file_uri(&dir, "watch_renamed_new.pm");

    write_file(&dir, "watch_renamed.pm", "package WatchRenamed;\nsub before_move { }\n1;\n");
    write_file(&dir, "watch_renamed_new.pm", "package WatchRenamed;\nsub before_move { }\n1;\n");
    let server = LspServer::new();
    did_open(&server, &old_uri, "package WatchRenamed;\nsub before_move { }\n1;\n");
    did_change_full(&server, &old_uri, 2, "package WatchRenamed;\nsub unsaved_keep { }\n1;\n");

    // A watcher-only rename surfaces as DELETED(old) + CREATED(new).
    watched_changes(&server, &old_uri, 3);
    watched_changes(&server, &new_uri, 1);

    assert_eq!(
        document_text(&server, &old_uri).as_deref(),
        Some("package WatchRenamed;\nsub unsaved_keep { }\n1;\n"),
        "the watcher pair must not destroy or retarget the open buffer"
    );
    assert!(
        server.coordinator().is_none_or(|c| c.index().indexed_generation(&old_uri).is_none()),
        "facts for the abandoned old path are gone"
    );
    assert_eq!(
        index_symbols(&server, "before_move").len(),
        1,
        "the successor path carries exactly one disk-backed fact"
    );
}

#[test]
fn late_watcher_batch_resolves_authority_at_execution_time() {
    let dir = TempDir::new().expect("tempdir");
    let uri = file_uri(&dir, "late_batch.pl");

    write_file(&dir, "late_batch.pl", "package LateBatch;\nsub on_disk_late { }\n1;\n");
    let server = LspServer::new();
    did_open(&server, &uri, "package LateBatch;\nsub on_disk_late { }\n1;\n");
    did_change_full(&server, &uri, 2, "package LateBatch;\nsub buffer_late { }\n1;\n");

    // A debounced batch firing while the document is still open must not
    // pull disk bytes in behind the buffer's back.
    server.handle_watched_file_batch(vec![uri.clone()]);
    wait_for_index_tasks(&server);
    assert!(
        index_symbols(&server, "on_disk_late").is_empty(),
        "late work cannot re-derive the open subject from stale disk bytes"
    );
    assert_eq!(
        index_symbols(&server, "buffer_late").len(),
        1,
        "the buffer-bound snapshot stays the current workspace fact"
    );

    // After close the same late batch observes a closed disk-backed subject.
    did_close(&server, &uri);
    server.handle_watched_file_batch(vec![uri.clone()]);
    wait_for_index_tasks(&server);
    assert_eq!(
        index_symbols(&server, "on_disk_late").len(),
        1,
        "once closed, the watcher path indexes disk truth again"
    );
}
