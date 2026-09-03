//! Red-first falsifiers for the accepted-ticket push-diagnostics sink (#11673).
//!
//! These tests deliberately avoid the new sink API so they compile against
//! both pre-fix and post-fix push paths: against the pre-fix behavior they
//! fail (stale frame reaches the client); against the post-fix paths they
//! pass (the sink rejects the candidate at the enqueue boundary).

#[cfg(test)]
mod tests {
    use super::super::parse_worker::DocumentsHandle;
    use super::super::{DocumentState, LspServer};
    use crate::state::FIRST_ACCEPTED_DOCUMENT_GENERATION;
    use parking_lot::Mutex;
    use serde_json::json;
    use std::io::Write;
    use std::sync::Arc as StdArc;
    use std::sync::atomic::AtomicU32;
    use std::time::{Duration, Instant};

    /// Shared-buffer writer for capturing outbound LSP notifications in tests.
    struct SharedVecWriter {
        inner: StdArc<Mutex<Vec<u8>>>,
    }
    impl Write for SharedVecWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.inner.lock().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    fn make_server_with_capture() -> (LspServer, StdArc<Mutex<Vec<u8>>>) {
        let buf = StdArc::new(Mutex::new(Vec::<u8>::new()));
        let writer = SharedVecWriter { inner: StdArc::clone(&buf) };
        let server =
            LspServer::with_io(Box::new(std::io::Cursor::new(Vec::<u8>::new())), Box::new(writer));
        (server, buf)
    }

    fn wait_for_frames(buf: &StdArc<parking_lot::Mutex<Vec<u8>>>, minimum: usize) -> bool {
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            if String::from_utf8_lossy(&buf.lock()).matches("publishDiagnostics").count() >= minimum
            {
                return true;
            }
            std::thread::yield_now();
        }
        false
    }

    #[test]
    #[expect(
        clippy::expect_used,
        reason = "didOpen dispatch must fail the test loudly when the fallback path cannot run"
    )]
    fn unavailable_diagnostic_debouncer_falls_back_to_immediate_publish() {
        let (server, buf) = make_server_with_capture();
        let uri = "file:///diagnostic-debounce-fallback.pl";
        // didOpen is the sanctioned setup here: it routes through
        // `handle_did_open_with_cancellation`, which publishes a parsed
        // snapshot before the document is visible, so `current_parsed()` is
        // `Some` and the publish path below actually runs. A direct
        // `DocumentState::from_parts` insert leaves `parsed: None`, and
        // `publish_diagnostics` then silently withholds (#3396 PR4
        // pending-parse guard) -- the fallback could never be exercised.
        server
            .test_handle_did_open(Some(json!({
                "textDocument": {
                    "uri": uri,
                    "languageId": "perl",
                    "version": 1,
                    "text": "my $value = 1;\n"
                }
            })))
            .expect("didOpen should succeed");
        assert!(wait_for_frames(&buf, 1), "initial open should publish diagnostics");
        buf.lock().clear();

        server.install_diagnostic_debouncer(
            super::super::diagnostic_debounce::DiagnosticDebouncer::unavailable_for_test(),
        );
        server.publish_diagnostics_debounced(uri);

        assert!(
            wait_for_frames(&buf, 1),
            "an unavailable diagnostic debouncer must fall back to immediate publication"
        );
        let output = String::from_utf8_lossy(&buf.lock()).into_owned();
        assert_eq!(
            output.matches("\"method\":\"textDocument/publishDiagnostics\"").count(),
            1,
            "fallback must emit exactly one diagnostic notification: {output}"
        );
        assert!(
            output.contains(uri),
            "fallback must publish diagnostics for the requested document: {output}"
        );
        assert!(
            server.diagnostic_debouncer.lock().is_none(),
            "the permanently unavailable worker must be evicted after its first rejected admission"
        );
    }

    /// didClose + didOpen of the SAME URI installs a brand-new document
    /// instance whose numeric generation can equal the removed one. Performed
    /// directly on the documents map so no handler reentrancy is needed.
    /// `DocumentsHandle` carries the crate's sanctioned `Send`/`Sync`
    /// justification for the raw-pointer-bearing document map (see its doc
    /// comment) so it can cross into the test hook closure.
    fn replace_with_new_document_instance(
        documents: &DocumentsHandle,
        uri_key: String,
        text: &str,
        version: i32,
    ) {
        let mut docs = documents.lock();
        docs.remove(&uri_key); // didClose
        let rope = ropey::Rope::from_str(text);
        // didOpen: fresh instance at its first accepted generation. The
        // numeric value can coincide with the removed instance's counter --
        // that coincidence is exactly the ABA hazard under test.
        docs.insert(
            uri_key.clone(),
            DocumentState::from_parts(
                rope,
                text.to_string(),
                version,
                StdArc::new(AtomicU32::new(FIRST_ACCEPTED_DOCUMENT_GENERATION.get())),
            ),
        );
    }

    /// Falsifier (#11673 shift-left list item 2): a full-push candidate
    /// computed for a closed-and-reopened document must not reach the client.
    /// The pre-fix guard compares the REMOVED instance's counter with itself
    /// and passes; only an instance-identity check at the enqueue boundary
    /// rejects the candidate.
    #[test]
    fn full_push_publish_rejected_for_wrong_document_instance_after_close_reopen() {
        let (server, buf) = make_server_with_capture();
        let uri = "file:///full_aba_test.pl";
        let uri_key = server.normalize_uri_key(uri);
        crate::must_with(
            server.test_handle_did_open(Some(json!({
                "textDocument": {
                    "uri": uri,
                    "languageId": "perl",
                    "version": 1,
                    "text": "my $aba = 1;\n"
                }
            }))),
            "didOpen should succeed",
        );
        assert!(wait_for_frames(&buf, 1), "initial open should publish before the falsifier runs");
        buf.lock().clear();

        let documents = DocumentsHandle(StdArc::clone(&server.documents));
        let hook_key = uri_key.clone();
        *server.diagnostic_after_snapshot_hook.lock() = Some(Box::new(move || {
            replace_with_new_document_instance(&documents, hook_key.clone(), "my $aba = 2;\n", 2);
        }));

        server.publish_diagnostics(uri);
        drop(server);
        std::thread::sleep(Duration::from_millis(50));

        let captured = buf.lock();
        let text = String::from_utf8_lossy(&captured);
        assert!(
            !text.contains("publishDiagnostics"),
            "stale full-push candidate derived from a removed document instance \
             must be rejected at the publication boundary; got: {text:?}"
        );
    }

    /// Falsifier (#11673 shift-left list item 1): N diagnostics must not be
    /// enqueued after N+1 acceptance. The pre-fix fast path enqueued whatever
    /// it had snapshotted with no commit-time currency check -- any wait
    /// between snapshot and send published stale-N errors over newer state.
    #[test]
    fn fast_path_stale_publish_visible_without_accepted_ticket_boundary() {
        let (server, buf) = make_server_with_capture();
        let uri = "file:///fast_stale_test.pl";
        let uri_key = server.normalize_uri_key(uri);
        crate::must_with(
            server.test_handle_did_open(Some(json!({
                "textDocument": {
                    "uri": uri,
                    "languageId": "perl",
                    "version": 1,
                    "text": "sub { SYNTAX ERROR HERE }\n"
                }
            }))),
            "didOpen should succeed",
        );
        assert!(wait_for_frames(&buf, 1), "initial open should publish before the falsifier runs");
        buf.lock().clear();

        // Between the fast path's snapshot and its enqueue, a newer accepted
        // document state lands (simulated N+1 edit / reopen).
        let documents = DocumentsHandle(StdArc::clone(&server.documents));
        let hook_key = uri_key.clone();
        *server.diagnostic_after_snapshot_hook.lock() = Some(Box::new(move || {
            replace_with_new_document_instance(&documents, hook_key.clone(), "print 42;\n", 2);
        }));

        server.publish_parse_errors_fast(uri);
        drop(server);
        std::thread::sleep(Duration::from_millis(50));

        let captured = buf.lock();
        let text = String::from_utf8_lossy(&captured);
        assert!(
            !text.contains("publishDiagnostics"),
            "fast-path candidate must not enqueue after newer document state was \
             accepted; got: {text:?}"
        );
    }
}
