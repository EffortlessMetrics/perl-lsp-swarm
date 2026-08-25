//! Falsifiers and preservation proofs for accepted-ticket active-document
//! parser readiness (#11675).

#[cfg(test)]
mod tests {
    use super::super::LspServer;
    use parking_lot::Mutex;
    use serde_json::json;
    use std::io::Write;
    use std::sync::Arc as StdArc;
    use std::time::Instant;

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

    fn open(server: &LspServer, uri: &str, text: &str) {
        crate::must_with(
            server.test_handle_did_open(Some(json!({
                "textDocument": {
                    "uri": uri,
                    "languageId": "perl",
                    "version": 1,
                    "text": text
                }
            }))),
            "didOpen should succeed",
        );
    }

    fn change(server: &LspServer, uri: &str, version: i32, text: &str) {
        crate::must_with(
            server.test_handle_did_change(Some(json!({
                "textDocument": { "uri": uri, "version": version },
                "contentChanges": [{ "text": text }]
            }))),
            "didChange should succeed",
        );
    }

    fn wait_for_frames(buf: &StdArc<Mutex<Vec<u8>>>, minimum: usize, needle: &str) -> bool {
        let deadline = Instant::now() + std::time::Duration::from_secs(2);
        while Instant::now() < deadline {
            if String::from_utf8_lossy(&buf.lock()).matches(needle).count() >= minimum {
                return true;
            }
            std::thread::yield_now();
        }
        false
    }
    /// Falsifier (#11675 shift-left item 2 + notification rule): the
    /// `perl-lsp/active-document-ready` frame must be a projection of an
    /// already-current readiness state -- it may not precede the required
    /// core-effect publications it claims. On pre-#11675 main the didOpen
    /// background index task emitted the frame before any diagnostics
    /// publication ran.
    #[test]
    fn ready_notification_is_not_emitted_before_required_effects() {
        let (server, buf) = make_server_with_capture();
        let uri = "file:///readiness_order_test.pl";
        open(&server, uri, "sub order_check {}\nmy $x = 1;\n");
        assert!(
            wait_for_frames(&buf, 1, "active-document-ready"),
            "opening a clean document must mint parser-core readiness"
        );
        assert!(
            wait_for_frames(&buf, 1, "publishDiagnostics"),
            "required-effect publication must also land"
        );

        let text = String::from_utf8_lossy(&buf.lock()).into_owned();
        let ready_pos = text.find("active-document-ready");
        assert!(
            ready_pos.is_some(),
            "opening a clean document must mint parser-core readiness; got: {text:?}"
        );
        let diag_pos = text.find("publishDiagnostics");
        match (ready_pos, diag_pos) {
            (Some(ready), Some(diag)) => assert!(
                ready > diag,
                "active-document-ready must be a projection of current effects, \
                 never precede them; ready@{ready} < publishDiagnostics@{diag}"
            ),
            (Some(_), None) => {}
            _ => unreachable!("ready_pos checked Some above"),
        }
        let key = server.normalize_uri_key(uri);
        let (state, generation, _seq) = crate::must_some_with(
            server.test_active_document_readiness(&key),
            "open document must have a readiness entry",
        );
        assert_eq!(generation, 1);
        assert_eq!(state, "parser_core_ready", "clean open with committed effects is ready");
    }

    /// A stale effect attachment from an older generation can neither
    /// satisfy nor disturb the newer generation's readiness entry.
    #[test]
    fn stale_effect_attachment_cannot_satisfy_newer_generation() {
        let server = LspServer::with_io(
            Box::new(std::io::Cursor::new(Vec::<u8>::new())),
            Box::new(std::io::sink()),
        );
        let uri = "file:///readiness_stale_attach.pl";
        open(&server, uri, "sub gen_one {};\n");
        let key = server.normalize_uri_key(uri);
        // Capture the generation-1 identity arc.
        let stale_instance = {
            let docs = server.documents.lock();
            std::sync::Arc::clone(&crate::must_some_with(docs.get(&key), "open").generation.clone())
        };
        change(&server, uri, 2, "sub gen_two {};\n");

        // Late generation-1 effect attachments must be rejected.
        server.attach_active_document_effect(
            &key,
            &stale_instance,
            1,
            super::super::readiness::CoreEffectKind::ParserDiagnosticsPublication,
        );
        let (_, generation, _) =
            crate::must_some_with(server.test_active_document_readiness(&key), "entry survives");
        assert_eq!(generation, 2, "entry tracks the newer generation");

        // The newer generation's own effects complete -> ready at gen 2.
        let live_instance = {
            let docs = server.documents.lock();
            std::sync::Arc::clone(&crate::must_some_with(docs.get(&key), "open").generation.clone())
        };
        server.attach_active_document_effect(
            &key,
            &live_instance,
            2,
            super::super::readiness::CoreEffectKind::ParserDiagnosticsPublication,
        );
        server.attach_active_document_effect(
            &key,
            &live_instance,
            2,
            super::super::readiness::CoreEffectKind::DocumentSymbols,
        );
        let (state, generation, _) =
            crate::must_some_with(server.test_active_document_readiness(&key), "entry present");
        assert_eq!(state, "parser_core_ready");
        assert_eq!(generation, 2);
    }

    /// A guarded no-parse edit supersedes prior clean readiness and never
    /// projects a fresh ready notification.
    #[test]
    fn guard_supersedes_prior_readiness_without_ready_projection() {
        let (server, buf) = make_server_with_capture();
        let uri = "file:///readiness_guard_test.pl";
        open(&server, uri, "sub clean_before {};\n");
        assert!(
            wait_for_frames(&buf, 1, "active-document-ready"),
            "initial clean open must mint and project readiness"
        );
        let key = server.normalize_uri_key(uri);
        let (state_before, _, _) =
            crate::must_some_with(server.test_active_document_readiness(&key), "entry present");
        assert_eq!(state_before, "parser_core_ready");
        let ready_frames_before = count_ready_frames(&buf);

        // Oversized replacement text trips the large-file guard while each
        // line stays inside the per-line length bound.
        let line_budget = crate::state::max_file_size_bytes() + 1024;
        let oversized: String = "my $filler = 1;\n".repeat(line_budget / 16);
        change(&server, uri, 2, &oversized);
        std::thread::sleep(std::time::Duration::from_millis(50));

        let (state_after, generation_after, _) =
            crate::must_some_with(server.test_active_document_readiness(&key), "entry present");
        assert_eq!(state_after, "guarded", "guarded no-parse state supersedes readiness");
        assert_eq!(generation_after, 2);
        assert_eq!(
            count_ready_frames(&buf),
            ready_frames_before,
            "guarded terminal must not project a new active-document-ready frame"
        );
    }

    /// Pull-diagnostic clients: push publication drops out of the required
    /// profile, but symbol commitment still gates readiness.
    #[test]
    fn pull_client_profile_marks_push_publication_not_applicable() {
        let server = LspServer::with_io(
            Box::new(std::io::Cursor::new(Vec::<u8>::new())),
            Box::new(std::io::sink()),
        );
        server.client_supports_pull_diags.store(true, std::sync::atomic::Ordering::Relaxed);
        let uri = "file:///readiness_pull_client.pl";
        open(&server, uri, "sub pull_profile {};\n");
        let key = server.normalize_uri_key(uri);
        let (state, _, _) =
            crate::must_some_with(server.test_active_document_readiness(&key), "entry present");
        assert_eq!(
            state, "parser_core_ready",
            "profile v1 marks push publication not_applicable for pull clients"
        );
    }

    /// Close removes the live readiness claim entirely.
    #[test]
    fn close_removes_readiness_entry() {
        let server = LspServer::with_io(
            Box::new(std::io::Cursor::new(Vec::<u8>::new())),
            Box::new(std::io::sink()),
        );
        let uri = "file:///readiness_close_test.pl";
        open(&server, uri, "sub close_me {};\n");
        let key = server.normalize_uri_key(uri);
        assert!(server.test_active_document_readiness(&key).is_some());
        crate::must_with(
            server.handle_did_close(Some(json!({ "textDocument": { "uri": uri } }))),
            "didClose should succeed",
        );
        assert!(server.test_active_document_readiness(&key).is_none());
    }

    fn count_ready_frames(buf: &StdArc<Mutex<Vec<u8>>>) -> usize {
        String::from_utf8_lossy(&buf.lock()).matches("active-document-ready").count()
    }
}
