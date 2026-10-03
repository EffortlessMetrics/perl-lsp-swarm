//! Ingress-side desynchronization marking for size-rejected text syncs (#16659).
//!
//! `textDocument/didOpen` and `textDocument/didChange` are notifications: when
//! the pre-dispatch admission gate
//! (`perl_lsp_rs_core::runtime::input_validation::validate_request_admission`)
//! rejects an over-ceiling frame, the refusal used to be a *silent* drop: the
//! client was never told, and a document that was already stored kept its predecessor
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
//! A rejected first `didOpen` has no stored document and therefore no episode
//! latch. It produces a bounded warning for that distinct open attempt (#16653).
//! Later unassociated sync refusals go to `window/logMessage`, avoiding one
//! popup per keystroke while still leaving a client-visible refusal.
//!
//! "Once per episode" for stored documents is derived from the document's own
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

/// Keep an unvalidated URI from turning a refusal into an oversized UI message.
fn display_uri(uri: &str) -> String {
    let mut chars = uri.chars();
    let prefix: String = chars
        .by_ref()
        .take(120)
        .map(|character| if character.is_control() { '?' } else { character })
        .collect();
    if chars.next().is_some() { format!("{prefix}…") } else { prefix }
}

enum RejectionDisposition {
    NoDocument,
    Unchanged,
    EnteredDesync,
}

impl LspServer {
    /// Record a text-sync notification that admission rejected before dispatch.
    ///
    /// Called from the request preflight when
    /// [`validate_request_admission`](perl_lsp_rs_core::runtime::input_validation::validate_request_admission)
    /// refuses a `textDocument/didOpen`/`didChange`/`didSave` frame. For
    /// notifications that refusal is silent. A rejected frame carrying text
    /// must be associated with its document here: the stored snapshot is
    /// marked not-current (fail-closing user answers, including formatting)
    /// and the client is told once per episode why the file went quiet.
    /// A textless `didSave` carries no replacement buffer and leaves the
    /// snapshot current even when its envelope is rejected.
    ///
    /// `notify_client` is false for request-shaped text syncs, where the
    /// caller already receives the typed `InvalidRequest` response and a
    /// window message would only duplicate it.
    ///
    /// A rejected first `didOpen` has no stored document to invalidate, but
    /// must still tell the client that the buffer was never opened (#16653).
    pub(crate) fn mark_text_sync_admission_rejected(
        &self,
        method: &str,
        params: &Value,
        notify_client: bool,
    ) {
        if !is_text_sync_method(method) {
            return;
        }
        // didSave.text is optional in LSP 3.17. Without a string replacement,
        // rejecting the envelope does not lose document content, so the
        // stored snapshot remains current even when the envelope is oversized.
        if method == "textDocument/didSave"
            && params.pointer("/text").and_then(Value::as_str).is_none()
        {
            return;
        }
        let Some(uri) = params.pointer("/textDocument/uri").and_then(Value::as_str) else {
            tracing::debug!(
                method,
                "Text-sync admission rejected without a textDocument.uri; nothing to invalidate"
            );
            if notify_client {
                self.report_unstored_sync_refusal(
                    method,
                    &format!("{method} was rejected before its document could be identified; no text was synchronized"),
                );
            }
            return;
        };

        let disposition = {
            let mut documents = self.documents.lock();
            match self.get_document_mut(&mut documents, uri) {
                None => RejectionDisposition::NoDocument,
                Some(doc) => {
                    let incoming_version_i64 =
                        params.pointer("/textDocument/version").and_then(Value::as_i64);
                    let incoming_version = incoming_version_i64.and_then(|v| i32::try_from(v).ok());
                    // Mirror the admitted handlers' stale gate: a didChange
                    // notification at or below the stored version is ignored
                    // there because it cannot have changed the buffer, so
                    // rejecting that frame at admission must not mark the
                    // document either — both sides already hold the same text,
                    // and marking would disable current answers for nothing.
                    // didSave accepts a same-version save, didOpen has no
                    // admitted stale gate (it replaces the buffer), and a
                    // frame with no usable version carries no ordering
                    // evidence at all: those keep the conservative marking.
                    let allow_same_version = method != "textDocument/didChange";
                    let admitted_handler_would_ignore = match incoming_version {
                        Some(version) => {
                            version < doc.version || (!allow_same_version && version == doc.version)
                        }
                        None => false,
                    };
                    if admitted_handler_would_ignore {
                        RejectionDisposition::Unchanged
                    } else {
                        // Record the dropped frame's version watermark exactly
                        // as the in-dispatcher violation path does — also while
                        // the document is already desynchronized, where a newer
                        // dropped version must still raise it so a delayed
                        // intermediate replacement cannot land afterwards.
                        if let Some(version) = incoming_version {
                            doc.observe_change_version(version);
                        }
                        if doc.full_sync_required() {
                            // Already desynchronized: the generation bump and
                            // the episode's client message already happened;
                            // only the watermark above still matters.
                            RejectionDisposition::Unchanged
                        } else {
                            doc.mark_full_sync_required();
                            RejectionDisposition::EnteredDesync
                        }
                    }
                }
            }
        };
        // Drop the document lock before any outbound notification work.
        match disposition {
            RejectionDisposition::NoDocument => {
                tracing::debug!(
                    uri,
                    method,
                    "Text-sync admission rejected for an unopened document"
                );
                if notify_client {
                    let ceiling = text_sync_params_ceiling();
                    self.report_unstored_sync_refusal(
                        method,
                        &format!(
                            "{method} for {} was rejected at the {ceiling}-byte text-sync limit; \
                             the document was not opened. Reopen or resend it below the limit",
                            display_uri(uri)
                        ),
                    );
                }
                return;
            }
            RejectionDisposition::Unchanged => return,
            RejectionDisposition::EnteredDesync => {}
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
                "{}: document exceeds the {ceiling}-byte ({} MiB) text-sync limit; the last change was \
                 not applied, and formatting and diagnostics are paused for this file until it is \
                 reopened or fully resent below the limit",
                display_uri(uri),
                mib(ceiling)
            ),
        );
    }

    /// First-open refusal needs user attention. Later notifications for a
    /// document we never opened can repeat on every edit, so log those to the
    /// client without repeating a popup.
    fn report_unstored_sync_refusal(&self, method: &str, message: &str) {
        if method == "textDocument/didOpen" {
            self.show_message_or_log(MessageType::Warning, message);
        } else if let Err(error) = self.log_message(MessageType::Warning, message) {
            tracing::warn!(%error, %message, "Failed to report refused text sync to client log");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::JsonRpcRequest;
    use serde_json::json;

    #[test]
    fn refusal_message_uri_is_bounded_and_single_line() {
        let uri = format!("file:///\n{}", "x".repeat(3_000_000));
        let shown = display_uri(&uri);
        assert!(shown.starts_with("file:///?"));
        assert!(shown.ends_with('…'));
        assert!(shown.chars().count() <= 121);
        assert!(!shown.contains('\n'));
    }

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

    /// A rejected didChange the admitted handler would have ignored (explicit
    /// version at or below the stored one) must not mark the document: both
    /// sides already hold the same text, and marking would disable current
    /// answers for nothing (#16709 review).
    #[test]
    fn stale_rejected_change_does_not_mark_the_document() -> Result<(), Box<dyn std::error::Error>>
    {
        let server = LspServer::new();
        let uri = "file:///admission_desync_stale.pl";
        server.test_apply_did_open(uri, "sub hello{my $x=1;return $x;}\n", 1)?;
        // Advance the stored version so the rejected frames below are stale.
        server.handle_did_change(Some(json!({
            "textDocument": { "uri": uri, "version": 5 },
            "contentChanges": [{ "text": "sub newer{my $y=2;return $y;}\n" }],
        })))?;

        let big_text = "x".repeat(3_000_000);
        for stale_version in [4, 5] {
            let params = json!({
                "textDocument": { "uri": uri, "version": stale_version },
                "contentChanges": [{ "text": big_text }],
            });
            server.mark_text_sync_admission_rejected("textDocument/didChange", &params, false);
        }

        let documents = server.documents.lock();
        let doc = server.get_document(&documents, uri).ok_or("document must remain stored")?;
        assert!(
            !doc.full_sync_required(),
            "an oversized frame the handler would ignore must not desynchronize the document"
        );
        assert_eq!(doc.version, 5, "stale rejections must not move the watermark");
        drop(documents);

        // Current answers must still work: nothing about this run went stale.
        let formatted = server.handle_formatting(Some(json!({
            "textDocument": { "uri": uri, "version": 5 },
            "options": { "tabSize": 4, "insertSpaces": true },
        })))?;
        assert!(formatted.is_some(), "formatting must stay available after only stale rejections");
        Ok(())
    }

    /// A newer rejection while already desynchronized must still raise the
    /// version watermark: otherwise a delayed admitted replacement at an
    /// intermediate version passes the stale gate, clears the desync flag,
    /// and formats text older than the client's buffer (#16709 review).
    #[test]
    fn newer_rejection_while_desynchronized_raises_the_watermark()
    -> Result<(), Box<dyn std::error::Error>> {
        let server = LspServer::new();
        let uri = "file:///admission_desync_watermark.pl";
        server.test_apply_did_open(uri, "sub hello{my $x=1;return $x;}\n", 1)?;

        let big_text = "x".repeat(3_000_000);
        for dropped_version in [7, 9] {
            let params = json!({
                "textDocument": { "uri": uri, "version": dropped_version },
                "contentChanges": [{ "text": big_text }],
            });
            server.mark_text_sync_admission_rejected("textDocument/didChange", &params, false);
        }

        {
            let documents = server.documents.lock();
            let doc = server.get_document(&documents, uri).ok_or("document must remain stored")?;
            assert_eq!(
                doc.version, 9,
                "the newest rejected version must be the watermark even while desynchronized"
            );
            assert!(doc.full_sync_required(), "the desync latch must hold");
        }

        // An admitted replacement at the intermediate version must NOT land
        // after the version-9 rejection: the stale gate refuses it.
        server.handle_did_change(Some(json!({
            "textDocument": { "uri": uri, "version": 8 },
            "contentChanges": [{ "text": "sub intermediate{my $y=2;return $y;}\n" }],
        })))?;
        let still_desynced = {
            let documents = server.documents.lock();
            server
                .get_document(&documents, uri)
                .map(|doc| doc.full_sync_required())
                .unwrap_or(false)
        };
        assert!(
            still_desynced,
            "an intermediate replacement below the dropped watermark must not clear the desync"
        );
        let error = server
            .handle_formatting(Some(json!({
                "textDocument": { "uri": uri, "version": 8 },
                "options": { "tabSize": 4, "insertSpaces": true },
            })))
            .err()
            .ok_or("formatting against the intermediate replacement must fail closed")?;
        assert_eq!(error.code, crate::protocol::CONTENT_MODIFIED);

        // The declared recovery path still works at a genuinely newer version.
        server.handle_did_change(Some(json!({
            "textDocument": { "uri": uri, "version": 10 },
            "contentChanges": [{ "text": "sub recovered{my $y=2;return $y;}\n" }],
        })))?;
        let recovered = server.handle_formatting(Some(json!({
            "textDocument": { "uri": uri, "version": 10 },
            "options": { "tabSize": 4, "insertSpaces": true },
        })))?;
        assert!(
            recovered.is_some(),
            "an admitted replacement above the watermark must restore current answers"
        );
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

    /// A textless save has no replacement buffer to lose. Force admission to
    /// reject its envelope with an inert extension field, then contrast that
    /// with a rejected save that actually carries replacement text.
    #[test]
    fn over_ceiling_did_save_only_desynchronizes_when_text_is_present()
    -> Result<(), Box<dyn std::error::Error>> {
        use perl_lsp_rs_core::runtime::input_validation::validate_request_admission;
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

        let server = LspServer::with_io(
            Box::new(Cursor::new(Vec::<u8>::new())),
            Box::new(CaptureWriter(Arc::clone(&output))),
        );
        let uri = "file:///oversize_save_without_text.pl";
        let original = "sub hello{my $x=1;return $x;}\n";
        server.test_apply_did_open(uri, original, 1)?;
        let before = {
            let documents = server.documents.lock();
            let doc = server.get_document(&documents, uri).ok_or("open document missing")?;
            (doc.text.clone(), doc.version, doc.current_generation())
        };
        let ceiling = text_sync_params_ceiling();
        let textless = json!({
            "textDocument": { "uri": uri },
            "extensionPadding": "x".repeat(ceiling + 1),
        });
        if validate_request_admission("textDocument/didSave", &textless).is_ok() {
            return Err("textless save fixture must exceed the admission ceiling".into());
        }
        let notification = |params| JsonRpcRequest {
            _jsonrpc: "2.0".to_string(),
            id: None,
            method: "textDocument/didSave".to_string(),
            params: Some(params),
        };
        if server.handle_request(notification(textless)).is_some() {
            return Err("didSave notification must not receive a response".into());
        }
        {
            let documents = server.documents.lock();
            let doc = server.get_document(&documents, uri).ok_or("document disappeared")?;
            if doc.full_sync_required()
                || doc.text != before.0
                || doc.version != before.1
                || doc.current_generation() != before.2
            {
                return Err("rejected textless save changed stored document currentness".into());
            }
        }
        let formatting = server.handle_formatting(Some(json!({
            "textDocument": { "uri": uri },
            "options": { "tabSize": 4, "insertSpaces": true },
        })))?;
        if formatting.as_ref().and_then(Value::as_array).is_none_or(Vec::is_empty) {
            return Err("formatting must remain available after a rejected textless save".into());
        }

        // A save with replacement text is different: rejecting it loses the
        // client's buffer, so the existing fail-close must still apply.
        let with_text = json!({
            "textDocument": { "uri": uri },
            "text": "x".repeat(ceiling + 1),
        });
        if validate_request_admission("textDocument/didSave", &with_text).is_ok() {
            return Err("text-bearing save fixture must exceed the admission ceiling".into());
        }
        if server.handle_request(notification(with_text)).is_some() {
            return Err("didSave notification must not receive a response".into());
        }
        {
            let documents = server.documents.lock();
            let doc = server.get_document(&documents, uri).ok_or("document disappeared")?;
            if !doc.full_sync_required() || doc.current_generation() <= before.2 {
                return Err("rejected text-bearing save must desynchronize the document".into());
            }
        }
        let error = server
            .handle_formatting(Some(json!({
                "textDocument": { "uri": uri },
                "options": { "tabSize": 4, "insertSpaces": true },
            })))
            .err()
            .ok_or("formatting must refuse after rejected replacement text")?;
        if error.code != crate::protocol::CONTENT_MODIFIED {
            return Err(format!("unexpected formatting error: {}", error.code).into());
        }

        server.handle_did_change(Some(json!({
            "textDocument": { "uri": uri, "version": 2 },
            "contentChanges": [{ "text": "sub recovered{my $y=2;return $y;}\n" }],
        })))?;
        let recovered = server.handle_formatting(Some(json!({
            "textDocument": { "uri": uri },
            "options": { "tabSize": 4, "insertSpaces": true },
        })))?;
        if recovered.as_ref().and_then(Value::as_array).is_none_or(Vec::is_empty) {
            return Err("admitted full replacement must restore formatting edits".into());
        }
        drop(server);
        let outbound = String::from_utf8(output.lock().clone())?;
        if outbound.matches("window/showMessage").count() != 1 {
            return Err(format!("only the text-bearing save should warn: {outbound}").into());
        }
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
