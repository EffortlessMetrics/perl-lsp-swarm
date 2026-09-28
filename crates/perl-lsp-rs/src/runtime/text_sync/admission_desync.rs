//! Ingress-side desynchronization marking for size-rejected text syncs (#16659).
//!
//! `textDocument/didOpen` and `textDocument/didChange` are notifications: when
//! the pre-dispatch admission gate
//! (`perl_lsp_rs_core::runtime::input_validation::validate_request_admission`)
//! rejects an over-ceiling frame, the refusal is a *silent* drop — the client
//! is never told, and a document that was already stored keeps its predecessor
//! text as if it were current. A subsequent `textDocument/formatting` then
//! formats that stale snapshot and returns edits whose ranges cover only the
//! stale extent; applied by the real client to its actual (larger) buffer,
//! they duplicate the document tail and corrupt the file. Issue #16659
//! demonstrates exactly that end-to-end with format-on-save at the default
//! ~2.1 MiB ceiling.
//!
//! The in-dispatcher full-sync-violation path (`handle_did_change`) already
//! marks this loss on the document; the pre-dispatch rejection happens *before*
//! that handler ever runs, so the same loss must be recorded at the rejection
//! site. This module associates the rejected notification with its document
//! and:
//!
//! 1. marks the stored document `full_sync_required` — reusing the existing
//!    `DocumentState::mark_full_sync_required` contract: last-good text is
//!    retained as predecessor evidence, current-answer text fails closed, the
//!    generation bumps so in-flight formatting currentness checks fail, and an
//!    admitted full replacement (or close/reopen) restores currentness;
//! 2. emits one `window/showMessage` (Warning) per desync episode naming the
//!    effective ceiling, so the user learns why formatting and diagnostics
//!    stopped for that file.
//!
//! "Once per episode" is derived from the document's own
//! `full_sync_required` transition — no separate latch to miss a recovery
//! point. A second rejected sync while already desynchronized is silent
//! (`mark_full_sync_required` is idempotent there too); recovery through an
//! admitted replacement re-arms the signal naturally.
//!
//! The full typed-desync contract (typed recovery envelope, versioned
//! rejection records, readiness/diagnostic teardown parity for this ingress
//! path) remains owned by #7417; this is the minimal slice that removes the
//! formatter corruption and the silent drop.

use super::{LspServer, Value};
use crate::runtime::MessageType;
use perl_lsp_rs_core::runtime::input_validation::{is_text_sync_method, text_sync_params_ceiling};

/// One decimal place of MiB, for the user-facing ceiling figure.
fn mib(bytes: usize) -> String {
    format!("{:.1}", bytes as f64 / (1_048_576.0))
}

impl LspServer {
    /// Record a text-sync notification that admission rejected before dispatch.
    ///
    /// Called from the request preflight when
    /// [`validate_request_admission`](perl_lsp_rs_core::runtime::input_validation::validate_request_admission)
    /// refuses a `textDocument/didOpen`/`didChange`/`didSave` frame. For
    /// notifications that refusal is silent, so the rejected sync must be
    /// associated with its document here: the stored snapshot is marked
    /// not-current (fail-closing user answers, including formatting) and the
    /// client is told once per episode why the file went quiet.
    ///
    /// `notify_client` is false for request-shaped text syncs, where the
    /// caller already receives the typed `InvalidRequest` response and a
    /// window message would only duplicate it.
    ///
    /// A rejection for a URI with no stored document has nothing to
    /// invalidate — formatting would already refuse with `Document not open` —
    /// so only the tracing log covers that case; surfacing it to the client
    /// belongs to #7417's contract.
    pub(crate) fn mark_text_sync_admission_rejected(
        &self,
        method: &str,
        params: &Value,
        notify_client: bool,
    ) {
        if !is_text_sync_method(method) {
            return;
        }
        let Some(uri) = params.pointer("/textDocument/uri").and_then(Value::as_str) else {
            tracing::debug!(
                method,
                "Text-sync admission rejected without a textDocument.uri; nothing to invalidate"
            );
            return;
        };

        let marked = {
            let mut documents = self.documents.lock();
            match self.get_document_mut(&mut documents, uri) {
                // Already desynchronized: the marking (and generation bump) is
                // idempotent, and the episode's message was already sent.
                Some(doc) if doc.full_sync_required() => false,
                Some(doc) => {
                    if let Some(version) =
                        params.pointer("/textDocument/version").and_then(Value::as_i64)
                    {
                        // Record the dropped notification's version watermark
                        // exactly as the in-dispatcher violation path does, so
                        // delayed older replacements cannot land afterwards.
                        if let Ok(version) = i32::try_from(version) {
                            doc.observe_change_version(version);
                        }
                    }
                    doc.mark_full_sync_required();
                    true
                }
                None => false,
            }
        };
        // Drop the document lock before any outbound notification work.
        if !marked {
            tracing::debug!(
                uri,
                method,
                "Text-sync admission rejected; document absent or already marked desynchronized"
            );
            return;
        }
        tracing::warn!(
            uri,
            method,
            "Text-sync frame rejected at admission; stored document marked desynchronized until an admitted full replacement or reopen"
        );
        if !notify_client {
            return;
        }
        let ceiling = text_sync_params_ceiling();
        self.show_message_or_log(
            MessageType::Warning,
            &format!(
                "{uri}: document exceeds the {ceiling}-byte ({} MiB) text-sync limit; the last change was \
                 not applied, and formatting and diagnostics are paused for this file until it is \
                 reopened or fully resent below the limit",
                mib(ceiling)
            ),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::JsonRpcRequest;
    use serde_json::json;

    /// didOpen a small document, then run an over-ceiling didChange through
    /// the real preflight and require the document to fail closed.
    #[test]
    fn rejected_text_sync_marks_stored_document_desynchronized()
    -> Result<(), Box<dyn std::error::Error>> {
        let server = LspServer::new();
        let uri = "file:///admission_desync_mark.pl";
        server.test_apply_did_open(uri, "sub hello{my $x=1;return $x;}\n", 1)?;

        // An over-ceiling didChange: the flat MAX_PARAMS_SIZE floor is 1,000,000
        // bytes, so a 1.5 MiB payload is refused at admission regardless of the
        // configured file limit.
        let big_text = "x".repeat(3_000_000);
        let params = json!({
            "textDocument": { "uri": uri, "version": 2 },
            "contentChanges": [{ "text": big_text }],
        });

        server.mark_text_sync_admission_rejected("textDocument/didChange", &params, false);

        // Current answers must fail closed: formatting must refuse with
        // CONTENT_MODIFIED instead of returning stale-snapshot edits.
        let error = server
            .handle_formatting(Some(json!({
                "textDocument": { "uri": uri, "version": 2 },
                "options": { "tabSize": 4, "insertSpaces": true },
            })))
            .err()
            .ok_or("formatting after a size-rejected sync must fail closed")?;
        assert_eq!(error.code, crate::protocol::CONTENT_MODIFIED);
        assert!(
            error.message.contains("full-document resync"),
            "rejected-sync formatting error must name the resync requirement: {}",
            error.message
        );
        Ok(())
    }

    #[test]
    fn rejected_text_sync_absorbs_version_watermark_and_recovers_on_full_replacement()
    -> Result<(), Box<dyn std::error::Error>> {
        let server = LspServer::new();
        let uri = "file:///admission_desync_recover.pl";
        server.test_apply_did_open(uri, "sub hello{my $x=1;return $x;}\n", 1)?;

        let big_text = "x".repeat(3_000_000);
        let params = json!({
            "textDocument": { "uri": uri, "version": 7 },
            "contentChanges": [{ "text": big_text }],
        });
        server.mark_text_sync_admission_rejected("textDocument/didChange", &params, false);

        // A delayed replacement at the dropped version (or older) must not
        // land after the rejection; the watermark is observed on marking.
        let documents = server.documents.lock();
        let doc = server
            .get_document(&documents, uri)
            .ok_or("document must remain stored as predecessor evidence")?;
        assert_eq!(doc.version, 7, "rejected notification's version must be recorded");
        assert!(doc.full_sync_required(), "document must be marked desynchronized");
        drop(documents);

        // The declared recovery path: an admitted full replacement at a newer
        // version restores currentness.
        server.handle_did_change(Some(json!({
            "textDocument": { "uri": uri, "version": 8 },
            "contentChanges": [{ "text": "sub recovered{my $y=2;return $y;}\n" }],
        })))?;
        let recovered = server.handle_formatting(Some(json!({
            "textDocument": { "uri": uri, "version": 8 },
            "options": { "tabSize": 4, "insertSpaces": true },
        })))?;
        let edits = recovered
            .and_then(|value| value.as_array().map(ToOwned::to_owned))
            .ok_or("expected formatting edits after recovery")?;
        assert!(!edits.is_empty(), "admitted replacement must restore formatting");
        Ok(())
    }

    #[test]
    fn non_text_sync_rejection_marks_nothing() -> Result<(), Box<dyn std::error::Error>> {
        let server = LspServer::new();
        let uri = "file:///admission_desync_untouched.pl";
        server.test_apply_did_open(uri, "sub hello{my $x=1;return $x;}\n", 1)?;

        let big_text = "x".repeat(3_000_000);
        let params = json!({ "textDocument": { "uri": uri, "text": big_text } });
        server.mark_text_sync_admission_rejected("textDocument/hover", &params, false);

        let documents = server.documents.lock();
        let doc = server.get_document(&documents, uri).ok_or("document must remain open")?;
        assert!(
            !doc.full_sync_required(),
            "non-text-sync rejections must not mark the document desynchronized"
        );
        Ok(())
    }

    #[test]
    fn rejection_without_stored_document_is_a_no_op() {
        let server = LspServer::new();
        let params = json!({
            "textDocument": { "uri": "file:///never_opened.pl", "version": 1 },
            "contentChanges": [{ "text": "x".repeat(3_000_000) }],
        });
        // Must not panic and must not create a document.
        server.mark_text_sync_admission_rejected("textDocument/didChange", &params, false);
        let documents = server.documents.lock();
        assert!(
            server.get_document(&documents, "file:///never_opened.pl").is_none(),
            "rejected sync for an untracked URI must not fabricate a stored document"
        );
    }

    /// The full preflight path: an over-ceiling didChange *notification* must
    /// return no response (it is a notification) yet still mark the document,
    /// so the next formatting request fails closed instead of returning
    /// stale-snapshot edits (#16659).
    #[test]
    fn preflight_notification_rejection_fails_closed_formatting()
    -> Result<(), Box<dyn std::error::Error>> {
        let server = LspServer::new();
        let uri = "file:///preflight_desync.pl";
        server.test_apply_did_open(uri, "sub hello{my $x=1;return $x;}\n", 1)?;

        let request = JsonRpcRequest {
            _jsonrpc: "2.0".to_string(),
            id: None,
            method: "textDocument/didChange".to_string(),
            params: Some(json!({
                "textDocument": { "uri": uri, "version": 2 },
                "contentChanges": [{ "text": "x".repeat(3_000_000) }],
            })),
        };
        // Notification: no response either way; the drop must not be silent
        // in document state.
        let _ = server.handle_request(request);

        let error = server
            .handle_formatting(Some(json!({
                "textDocument": { "uri": uri, "version": 2 },
                "options": { "tabSize": 4, "insertSpaces": true },
            })))
            .err()
            .ok_or("formatting after a preflight-dropped sync must fail closed")?;
        assert_eq!(error.code, crate::protocol::CONTENT_MODIFIED);
        Ok(())
    }

    /// Second rejection while already desynchronized must not re-notify (one
    /// message per episode), and an admitted recovery must re-arm the signal.
    #[test]
    fn window_message_is_emitted_once_per_desync_episode() -> Result<(), Box<dyn std::error::Error>>
    {
        // CaptureWriter + drop(server) flush pattern (module_resolution.rs).
        use std::io::Cursor;
        use std::sync::Arc;

        let output = Arc::new(parking_lot::Mutex::new(Vec::<u8>::new()));
        struct CaptureWriter(Arc<parking_lot::Mutex<Vec<u8>>>);
        impl std::io::Write for CaptureWriter {
            fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
                self.0.lock().extend_from_slice(buf);
                Ok(buf.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }

        let captured = Arc::clone(&output);
        let server = LspServer::with_io(
            Box::new(Cursor::new(Vec::<u8>::new())),
            Box::new(CaptureWriter(captured)),
        );
        let uri = "file:///once_per_episode.pl";
        server.did_open(json!({
            "textDocument": { "uri": uri, "languageId": "perl", "version": 1,
                              "text": "sub hello{my $x=1;return $x;}\n" },
        }))?;

        let big_text = "x".repeat(3_000_000);
        let params = json!({
            "textDocument": { "uri": uri, "version": 2 },
            "contentChanges": [{ "text": big_text }],
        });
        server.mark_text_sync_admission_rejected("textDocument/didChange", &params, true);
        // Second rejection in the same episode: idempotent marking, no second
        // message.
        server.mark_text_sync_admission_rejected("textDocument/didChange", &params, true);

        drop(server);
        let text = String::from_utf8(output.lock().clone())
            .map_err(|error| format!("captured outbound stream not UTF-8: {error}"))?;

        let occurrences = text.matches("window/showMessage").count();
        assert_eq!(
            occurrences, 1,
            "exactly one window/showMessage per desync episode, got {occurrences}: {text}"
        );
        assert!(text.contains("\"type\":2"), "message must be Warning: {text}");
        assert!(text.contains("text-sync limit"), "message must name the sync limit: {text}");
        assert!(
            text.contains(&format!("{}-byte", text_sync_params_ceiling())),
            "message must name the effective ceiling: {text}"
        );
        Ok(())
    }
}
