//! Falsifiers and preservation proofs for the accepted-ticket document-symbol
//! sink (#11674).

#[cfg(test)]
mod tests {
    use super::super::LspServer;
    use crate::runtime::document_symbols_sink::DocumentSymbolIdentity;
    use crate::runtime::parse_effect_contract::ParseEffectCommitOutcomeV1;
    use crate::{must_some_with, must_with};
    use serde_json::json;

    fn make_server() -> LspServer {
        LspServer::with_io(
            Box::new(std::io::Cursor::new(Vec::<u8>::new())),
            Box::new(std::io::sink()),
        )
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

    fn has_symbol(server: &LspServer, prefix: &str) -> bool {
        !server.symbol_index.lock().search_prefix(prefix).is_empty()
    }

    fn identity_for_current(server: &LspServer, uri: &str) -> DocumentSymbolIdentity {
        let key = server.normalize_uri_key(uri);
        let docs = server.documents.lock();
        let doc = crate::must_some_with(docs.get(&key), "document must be open");
        DocumentSymbolIdentity::for_document(&key, &doc.generation, doc.current_generation())
    }

    /// A stale candidate whose instance was replaced between extraction and
    /// the commit attempt must be rejected without touching the newer row.
    /// The hook deterministically opens the exact window the pre-fix code
    /// left unguarded (extraction -> unconditional store write).
    #[test]
    fn wrong_instance_candidate_rejected_when_instance_replaced_before_commit() {
        let server = make_server();
        let uri = "file:///sym_aba_test.pl";
        open(&server, uri, "sub alpha_keep {};\n");

        // Capture the pre-edit ticket identity (generation 1 of instance A).
        let stale_identity = identity_for_current(&server, uri);

        // Between extraction and commit, didClose+didOpen installs a fresh
        // instance of the same URI.
        let documents =
            crate::runtime::parse_worker::DocumentsHandle(std::sync::Arc::clone(&server.documents));
        let key = server.normalize_uri_key(uri);
        *server.document_symbols_before_commit_hook.lock() = Some(Box::new(move || {
            let mut docs = documents.lock();
            docs.remove(&key);
            let rope = ropey::Rope::from_str("print 42;\n");
            docs.insert(
                key.clone(),
                super::super::DocumentState::from_parts(
                    rope,
                    "print 42;\n".to_string(),
                    2,
                    std::sync::Arc::new(std::sync::atomic::AtomicU32::new(1)),
                ),
            );
        }));

        // The stale N=1 replacement now attempts to commit.
        let ast = crate::must_with(
            perl_parser::Parser::new("sub alpha_stale {};").parse(),
            "parse should succeed",
        );
        let outcome =
            server.commit_document_symbols_from_ast(&stale_identity, &ast, "sub alpha_stale {};");

        assert_eq!(
            outcome,
            ParseEffectCommitOutcomeV1::RejectedWrongDocumentInstance,
            "a candidate derived from a removed document instance must be rejected"
        );
        assert!(
            has_symbol(&server, "alpha_keep"),
            "the row belonging to the superseding state must be untouched"
        );
        assert!(
            !has_symbol(&server, "alpha_stale"),
            "the rejected stale symbols must not enter the store"
        );
    }

    /// A parse-derived clear from an older generation cannot remove a newer
    /// generation's committed symbols.
    #[test]
    fn superseded_generation_clear_cannot_remove_newer_row() {
        let server = make_server();
        let uri = "file:///sym_superseded_test.pl";
        open(&server, uri, "sub alpha_gen1 {};\n");
        let stale_identity = identity_for_current(&server, uri);

        change(&server, uri, 2, "sub beta_gen2 {};\n");
        assert!(has_symbol(&server, "beta_gen2"), "v2 symbols must be committed");

        assert_eq!(
            server.clear_document_symbols_for_identity(&stale_identity),
            ParseEffectCommitOutcomeV1::RejectedStaleTicket
        );
        assert!(
            has_symbol(&server, "beta_gen2"),
            "the newer generation's symbols must survive a late N clear"
        );
    }

    /// An exact empty result for the current generation supersedes prior
    /// symbols (empty is not "keep whatever was there").
    #[test]
    fn current_empty_result_supersedes_prior_symbols() {
        let server = make_server();
        let uri = "file:///sym_empty_test.pl";
        open(&server, uri, "sub alpha_then {};\n");
        assert!(has_symbol(&server, "alpha_then"));

        change(&server, uri, 2, "# only a comment, no symbols\n");
        assert!(
            !has_symbol(&server, "alpha_then"),
            "current exact-empty result must clear prior symbols"
        );
    }

    /// Close/reopen keeps instances distinct: eviction clears the old row and
    /// the reopened document commits its own.
    #[test]
    fn close_reopen_does_not_inherit_prior_row() {
        let server = make_server();
        let uri = "file:///sym_reopen_test.pl";
        open(&server, uri, "sub alpha_old {};\n");
        assert!(has_symbol(&server, "alpha_old"));

        crate::must_with(
            server.handle_did_close(Some(json!({ "textDocument": { "uri": uri } }))),
            "didClose should succeed",
        );
        assert!(
            !has_symbol(&server, "alpha_old"),
            "lifecycle eviction must clear the closed document's row"
        );

        open(&server, uri, "sub beta_new {};\n");
        assert!(has_symbol(&server, "beta_new"));
        assert!(!has_symbol(&server, "alpha_old"));
    }

    /// The committed ledger advances monotonically per URI and records the
    /// accepted generation -- the anchor #6729's result-ID row consumes.
    #[test]
    fn committed_ledger_records_ticket_and_monotonic_sequence() {
        let server = make_server();
        let uri = "file:///sym_ledger_test.pl";
        open(&server, uri, "sub ledger_one {};\n");
        let key = server.normalize_uri_key(uri);
        let baseline = crate::must_some_with(
            server.test_last_committed_document_symbols(&key),
            "didOpen must record",
        );

        change(&server, uri, 2, "sub ledger_two {};\n");
        let after_edit = crate::must_some_with(
            server.test_last_committed_document_symbols(&key),
            "edit commit must be recorded",
        );
        assert!(
            after_edit.0 > baseline.0 || (after_edit.0 == baseline.0 && after_edit.1 > baseline.1),
            "ledger must advance in (generation, sequence): {baseline:?} -> {after_edit:?}"
        );
    }

    /// Barrier test (#11674 review): a didClose eviction completing after the
    /// boundary validated the ticket but before the store mutation must win
    /// the serialization. The final index row stays empty, the late callback
    /// reports a lifecycle rejection instead of a current commit, and the
    /// closed URI gains no committed sink record.
    #[test]
    fn did_close_between_validation_and_install_keeps_index_empty() {
        let server = make_server();
        let uri = "file:///sym_close_barrier.pl";
        open(&server, uri, "sub alpha_barrier {};\n");
        assert!(has_symbol(&server, "alpha_barrier"), "didOpen must commit its row");

        let identity = identity_for_current(&server, uri);
        let key = server.normalize_uri_key(uri);
        let ledger_before_install = must_some_with(
            server.test_last_committed_document_symbols(&key),
            "didOpen must have recorded",
        );

        // Interpose exactly between boundary validation and the serialized
        // install, then run the lifecycle-owned eviction: remove the document
        // map entry first, then clear through the index-only sweep path --
        // the same lock-free-against-sink order didClose uses.
        let documents =
            crate::runtime::parse_worker::DocumentsHandle(std::sync::Arc::clone(&server.documents));
        let symbol_index = std::sync::Arc::clone(&server.symbol_index);
        *server.document_symbols_before_install_hook.lock() = Some(Box::new(move || {
            documents.lock().remove(&key);
            symbol_index.lock().remove_document(&key);
        }));

        // The pre-validated candidate now attempts its commit against a
        // document the lifecycle already evicted.
        let ast = must_with(
            perl_parser::Parser::new("sub alpha_barrier {};").parse(),
            "parse should succeed",
        );
        let outcome =
            server.commit_document_symbols_from_ast(&identity, &ast, "sub alpha_barrier {};");

        assert_eq!(
            outcome,
            ParseEffectCommitOutcomeV1::RejectedLifecycleState,
            "a callback racing a completed didClose must not report a current commit"
        );
        assert!(
            !has_symbol(&server, "alpha_barrier"),
            "the closed URI's index row must stay empty once close wins the race"
        );
        assert_eq!(
            server.test_last_committed_document_symbols(&server.normalize_uri_key(uri)),
            Some(ledger_before_install),
            "the evicted URI must keep exactly its pre-race record: the late \
             callback must not advance or reinstate a committed entry"
        );
    }
}
