use super::{
    CONTENT_MODIFIED, FormatDisposition, FormattingDecision, FormattingError, JsonRpcError,
    LspServer, PROVIDER, PerlLspCancellationToken, REQUEST_CANCELLED, Snapshot, Surface, Value,
    actual_engine_for_mode, formatting_error_reason, json, sanitized_outcome, value,
};
use crate::runtime::MessageType;
use perl_lsp_rs_core::tooling::perltidy::native::FormatReasonCode;

const UNSUPPORTED_SYNTAX_REASON: &str = "unsupported_syntax";
const UNSUPPORTED_NATIVE_FORMATTING_MESSAGE: &str = "Native formatting left the source unchanged because its syntax is outside the formatter's current safe subset. Format a smaller supported range or select explicit external Perl::Tidy compatibility.";

impl LspServer {
    pub(super) fn stale_error(
        &self,
        snapshot: &Snapshot,
        reason: &'static str,
        message: &'static str,
    ) -> JsonRpcError {
        self.stale_error_with_engine(snapshot, reason, message, None)
    }

    pub(super) fn stale_error_with_engine(
        &self,
        snapshot: &Snapshot,
        reason: &'static str,
        message: &'static str,
        actual_engine: Option<&str>,
    ) -> JsonRpcError {
        let receipt = self.record_formatting_receipt(
            snapshot,
            "blocked",
            json!(reason),
            actual_engine.unwrap_or("not_started"),
            "no_edit",
            0,
            None,
        );
        JsonRpcError {
            code: CONTENT_MODIFIED,
            message: message.to_string(),
            data: Some(json!({
                "error_kind": "content_modified",
                "reason": reason,
                "formatting_receipt": receipt,
            })),
        }
    }

    pub(super) fn ensure_not_cancelled(
        &self,
        surface: Surface,
        token: Option<&PerlLspCancellationToken>,
        snapshot: Option<&Snapshot>,
        actual_engine: Option<&str>,
    ) -> Result<(), JsonRpcError> {
        if !token.is_some_and(PerlLspCancellationToken::is_cancelled_relaxed) {
            return Ok(());
        }

        let receipt = snapshot.map(|snapshot| {
            self.record_formatting_receipt(
                snapshot,
                "blocked",
                json!("request_cancelled"),
                actual_engine.unwrap_or("not_started"),
                "no_edit",
                0,
                None,
            )
        });
        Err(JsonRpcError {
            code: REQUEST_CANCELLED,
            message: format!("Request cancelled - {}", surface.method()),
            data: Some(json!({
                "reason": "request_cancelled",
                "formatting_receipt": receipt,
            })),
        })
    }

    pub(super) fn record_formatting_receipt(
        &self,
        snapshot: &Snapshot,
        decision: &'static str,
        reason: Value,
        actual_engine: &str,
        fallback: &'static str,
        result_count: usize,
        outcome: Option<Value>,
    ) -> Value {
        let receipt = json!({
            "provider": PROVIDER,
            "provider_action": snapshot.surface.method(),
            "decision": decision,
            "reason": reason,
            "fact_source": if actual_engine == "native" { "parser_syntax" } else { "provider_runtime" },
            "confidence": if decision == "acted" { "high" } else { "low" },
            "freshness": if reason.as_str().is_some_and(|value| value.starts_with("stale_")) {
                "stale"
            } else {
                "fresh"
            },
            "fallback": fallback,
            "dynamic_boundary": false,
            "source_backed": actual_engine == "native",
            "source_id_hash": snapshot.uri_hash,
            "source_generation": snapshot.generation,
            "document_version": snapshot.version,
            "configured_enabled": snapshot.config.configured_enabled,
            "configured_mode": value(&snapshot.config.configured_mode),
            "requested_mode": value(&snapshot.config.configured_mode),
            "effective_mode": value(&snapshot.config.mode),
            "actual_engine": actual_engine,
            "config_fingerprint": snapshot.config.fingerprint,
            "result_count": result_count,
            "format_outcome": outcome,
            "claim_boundary": "the LSP response is projected from this formatting decision after source/configuration freshness checks",
        });
        self.record_provider_decision_trace(PROVIDER, &receipt);
        receipt
    }

    pub(super) fn formatting_failure(
        &self,
        snapshot: &Snapshot,
        context: &str,
        error: FormattingError,
    ) -> JsonRpcError {
        self.formatting_failure_with_evidence(snapshot, context, error, None)
    }

    pub(super) fn formatting_failure_with_evidence(
        &self,
        snapshot: &Snapshot,
        context: &str,
        error: FormattingError,
        evidence: Option<Value>,
    ) -> JsonRpcError {
        let reason = formatting_error_reason(&error);
        let receipt = self.record_formatting_receipt(
            snapshot,
            "blocked",
            reason.clone(),
            actual_engine_for_mode(snapshot.config.mode),
            "no_edit",
            0,
            evidence,
        );
        JsonRpcError {
            code: -32603,
            message: format!("{context}: {error}"),
            data: Some(json!({
                "error_kind": error.error_kind(),
                "reason": reason,
                "formatting_receipt": receipt,
            })),
        }
    }

    /// Decide whether this refusal should produce a user-facing warning.
    ///
    /// The last formatting receipt is presentation-only state, so this provides
    /// best-effort consecutive suppression rather than a synchronization or
    /// semantic-authority boundary. Concurrent identical requests may both warn;
    /// formatting correctness and receipt truth do not depend on suppression.
    /// The identity includes the canonical URI, source generation, formatting
    /// configuration, and surface. Request options and mixed multi-range plans
    /// are outside this narrow consecutive-refusal claim.
    fn should_notify_unsupported_syntax(
        &self,
        snapshot: &Snapshot,
        reason: FormatReasonCode,
        disposition: FormatDisposition,
        edit_count: usize,
    ) -> bool {
        if disposition != FormatDisposition::Refused
            || reason != FormatReasonCode::UnsupportedSyntax
            || edit_count != 0
        {
            return false;
        }

        !self.provider_decision_traces.lock().get(PROVIDER).is_some_and(|receipt| {
            receipt.get("reason").and_then(Value::as_str) == Some(UNSUPPORTED_SYNTAX_REASON)
                && receipt.get("provider_action").and_then(Value::as_str)
                    == Some(snapshot.surface.method())
                && receipt.get("source_id_hash").and_then(Value::as_str)
                    == Some(snapshot.uri_hash.as_str())
                && receipt.get("source_generation").and_then(Value::as_u64)
                    == Some(snapshot.generation)
                && receipt.get("config_fingerprint").and_then(Value::as_str)
                    == Some(snapshot.config.fingerprint.as_str())
        })
    }

    pub(super) fn maybe_notify_unsupported_syntax(
        &self,
        snapshot: &Snapshot,
        decision: &FormattingDecision,
        disposition: FormatDisposition,
        edit_count: usize,
    ) {
        let notify = self.should_notify_unsupported_syntax(
            snapshot,
            decision.outcome.reason,
            disposition,
            edit_count,
        );
        if notify {
            self.show_message_or_log(MessageType::Warning, UNSUPPORTED_NATIVE_FORMATTING_MESSAGE);
        }
    }

    pub(crate) fn clear_formatting_receipt_for_close(&self, uri: &str) {
        let source_id_hash = super::digest(&self.canonical_uri(uri));
        let mut traces = self.provider_decision_traces.lock();
        if traces
            .get(PROVIDER)
            .and_then(|receipt| receipt.get("source_id_hash"))
            .and_then(Value::as_str)
            == Some(source_id_hash.as_str())
        {
            traces.remove(PROVIDER);
        }
    }

    pub(super) fn project(
        &self,
        snapshot: &Snapshot,
        decision: FormattingDecision,
    ) -> Result<Option<Value>, JsonRpcError> {
        let disposition = decision.outcome.disposition;
        let edit_count = decision.document.edits.len();
        self.maybe_notify_unsupported_syntax(snapshot, &decision, disposition, edit_count);
        let outcome = sanitized_outcome(&decision);
        let actual_engine = super::actual_engine_for_decision(&decision);
        let reason = outcome.get("reason").cloned().unwrap_or_else(|| json!("unknown"));
        let (provider_decision, fallback) = match disposition {
            FormatDisposition::Applied | FormatDisposition::NoChange => ("acted", "none"),
            FormatDisposition::Refused | FormatDisposition::FailedOrNotProven => {
                ("blocked", "no_edit")
            }
        };
        let receipt = self.record_formatting_receipt(
            snapshot,
            provider_decision,
            reason,
            actual_engine,
            fallback,
            edit_count,
            Some(outcome),
        );

        match disposition {
            FormatDisposition::Applied if edit_count > 0 => {
                Ok(Some(json!(decision.document.edits)))
            }
            FormatDisposition::NoChange | FormatDisposition::Refused if edit_count == 0 => {
                Ok(Some(json!([])))
            }
            FormatDisposition::FailedOrNotProven => Err(JsonRpcError {
                code: -32603,
                message:
                    "Formatting returned an unproven successful value; no edits were returned."
                        .to_string(),
                data: Some(json!({
                    "error_kind": "formatting_outcome_contract",
                    "reason": "instrument_failure",
                    "formatting_receipt": receipt,
                })),
            }),
            _ => Err(JsonRpcError {
                code: -32603,
                message: "Formatting outcome and edits disagree; no edits were returned."
                    .to_string(),
                data: Some(json!({
                    "error_kind": "formatting_outcome_contract",
                    "reason": "instrument_failure",
                    "formatting_receipt": receipt,
                })),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::formatting::{CodeFormatter, FormatContext};
    use std::error::Error;
    use std::io::{self, Cursor, Write};
    use std::sync::Arc;

    type TestResult = Result<(), Box<dyn Error>>;

    fn verify(condition: bool, message: impl Into<String>) -> TestResult {
        if condition { Ok(()) } else { Err(io::Error::other(message.into()).into()) }
    }

    fn outbound_messages(bytes: &[u8]) -> Result<Vec<Value>, Box<dyn Error>> {
        let mut cursor = 0usize;
        let mut messages = Vec::new();

        while cursor < bytes.len() {
            let header_end = bytes[cursor..]
                .windows(4)
                .position(|window| window == b"\r\n\r\n")
                .ok_or_else(|| io::Error::other("outbound frame has no header terminator"))?;
            let header_end = cursor + header_end;
            let header = std::str::from_utf8(&bytes[cursor..header_end])?;
            let content_length = header
                .split("\r\n")
                .find_map(|line| line.strip_prefix("Content-Length: "))
                .ok_or_else(|| io::Error::other("outbound frame has no Content-Length header"))?
                .parse::<usize>()?;
            let body_start = header_end + 4;
            let body_end = body_start
                .checked_add(content_length)
                .ok_or_else(|| io::Error::other("outbound frame length overflowed"))?;
            if body_end > bytes.len() {
                return Err(io::Error::other("outbound frame body is truncated").into());
            }
            messages.push(serde_json::from_slice(&bytes[body_start..body_end])?);
            cursor = body_end;
        }

        Ok(messages)
    }

    fn show_message_count(messages: &[Value]) -> usize {
        messages
            .iter()
            .filter(|message| {
                message.get("method").and_then(Value::as_str) == Some("window/showMessage")
            })
            .count()
    }

    #[derive(Clone)]
    struct SharedWriter(Arc<parking_lot::Mutex<Vec<u8>>>);

    impl Write for SharedWriter {
        fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
            self.0.lock().extend_from_slice(buffer);
            Ok(buffer.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    fn server_with_output() -> (LspServer, Arc<parking_lot::Mutex<Vec<u8>>>) {
        let output = Arc::new(parking_lot::Mutex::new(Vec::new()));
        let server = LspServer::with_io(
            Box::new(Cursor::new(Vec::<u8>::new())),
            Box::new(SharedWriter(Arc::clone(&output))),
        );
        server
            .advertised_feature_ids
            .lock()
            .extend([Surface::Document.feature_id(), Surface::Range.feature_id()]);
        server.config.lock().perltidy_enabled = true;
        (server, output)
    }

    fn document_formatting_params(uri: &str, version: i32) -> Value {
        json!({
            "textDocument": { "uri": uri, "version": version },
            "options": { "tabSize": 4, "insertSpaces": true },
        })
    }

    fn range_formatting_params(uri: &str, version: i32) -> Value {
        json!({
            "textDocument": { "uri": uri, "version": version },
            "range": {
                "start": { "line": 0, "character": 0 },
                "end": { "line": 0, "character": 7 }
            },
            "options": { "tabSize": 4, "insertSpaces": true },
        })
    }

    #[test]
    fn unsupported_native_syntax_warns_once_per_surface() -> TestResult {
        let (server, output) = server_with_output();
        let uri = "file:///unsupported-native-formatting.pl";
        server.test_apply_did_open(uri, "sub f {\nreturn 1;\n}\n", 1)?;

        let first_document =
            server.handle_formatting_policy(Some(document_formatting_params(uri, 1)), None)?;
        let repeated_document =
            server.handle_formatting_policy(Some(document_formatting_params(uri, 1)), None)?;
        let first_range =
            server.handle_range_formatting_policy(Some(range_formatting_params(uri, 1)), None)?;
        let repeated_range =
            server.handle_range_formatting_policy(Some(range_formatting_params(uri, 1)), None)?;

        verify(first_document == Some(json!([])), "document refusal returned edits")?;
        verify(repeated_document == Some(json!([])), "repeated document refusal returned edits")?;
        verify(first_range == Some(json!([])), "range refusal returned edits")?;
        verify(repeated_range == Some(json!([])), "repeated range refusal returned edits")?;
        {
            let traces = server.provider_decision_traces.lock();
            let receipt = traces
                .get(PROVIDER)
                .ok_or_else(|| io::Error::other("formatting receipt was not recorded"))?;
            verify(
                receipt.get("reason").and_then(Value::as_str) == Some(UNSUPPORTED_SYNTAX_REASON),
                "last formatting receipt lost the unsupported-syntax reason",
            )?;
            verify(
                receipt.get("provider_action").and_then(Value::as_str)
                    == Some("textDocument/rangeFormatting"),
                "last formatting receipt lost the range surface identity",
            )?;
        }
        drop(server);

        let bytes = output.lock().clone();
        let messages = outbound_messages(&bytes)?;
        let warning_count = show_message_count(&messages);
        verify(
            warning_count == 2,
            format!(
                "each formatting surface should warn once without suppressing the other; observed {warning_count}: {messages:?}"
            ),
        )?;
        Ok(())
    }

    #[test]
    fn unsupported_native_multi_range_warns_once() -> TestResult {
        let (server, output) = server_with_output();
        server.advertised_feature_ids.lock().push(Surface::Ranges.feature_id());
        let uri = "file:///unsupported-native-multi-range-formatting.pl";
        server.test_apply_did_open(uri, "sub f {\nreturn 1;\n}\n", 1)?;
        let params = json!({
            "textDocument": { "uri": uri, "version": 1 },
            "ranges": [
                {
                    "start": { "line": 0, "character": 0 },
                    "end": { "line": 0, "character": 7 }
                },
                {
                    "start": { "line": 1, "character": 0 },
                    "end": { "line": 1, "character": 9 }
                }
            ],
            "options": { "tabSize": 4, "insertSpaces": true },
        });

        let first = server.handle_ranges_formatting_policy(Some(params.clone()), None)?;
        let repeated = server.handle_ranges_formatting_policy(Some(params), None)?;
        verify(first == Some(json!([])), "first multi-range refusal returned edits")?;
        verify(repeated == Some(json!([])), "repeated multi-range refusal returned edits")?;
        drop(server);

        let messages = outbound_messages(&output.lock())?;
        verify(
            show_message_count(&messages) == 1,
            format!("multi-range refusal warning count was not suppressed: {messages:?}"),
        )?;
        Ok(())
    }

    #[test]
    fn reopening_document_reenables_unsupported_warning() -> TestResult {
        let (server, output) = server_with_output();
        let uri = "file:///reopened-native-formatting.pl";
        let source = "sub f {\nreturn 1;\n}\n";
        server.test_apply_did_open(uri, source, 1)?;
        let first =
            server.handle_formatting_policy(Some(document_formatting_params(uri, 1)), None)?;
        server.test_apply_did_close(uri)?;
        server.test_apply_did_open(uri, source, 1)?;
        let reopened =
            server.handle_formatting_policy(Some(document_formatting_params(uri, 1)), None)?;

        verify(first == Some(json!([])), "first refusal returned edits")?;
        verify(reopened == Some(json!([])), "reopened refusal returned edits")?;
        drop(server);

        let messages = outbound_messages(&output.lock())?;
        verify(
            show_message_count(&messages) == 2,
            format!("reopened document did not warn again: {messages:?}"),
        )?;
        Ok(())
    }

    #[test]
    fn uri_alias_same_server_reopen_reenables_unsupported_warning() -> TestResult {
        let (server, output) = server_with_output();
        let opened_uri = "file:///C:/workspace/%61lias-formatting.pl";
        let close_uri = "file:///c:/workspace/alias-formatting.pl";
        let source = "sub f {\nreturn 1;\n}\n";
        // One server owns the document instance across the aliased close and
        // reopen; the second open must not inherit the first instance's receipt.
        server.test_apply_did_open(opened_uri, source, 1)?;
        let first = server
            .handle_formatting_policy(Some(document_formatting_params(opened_uri, 1)), None)?;
        server.test_apply_did_close(close_uri)?;
        verify(
            !server.provider_decision_traces.lock().contains_key(PROVIDER),
            "aliased close retained the prior formatting receipt",
        )?;
        server.test_apply_did_open(opened_uri, source, 1)?;
        let reopened = server
            .handle_formatting_policy(Some(document_formatting_params(opened_uri, 1)), None)?;

        verify(first == Some(json!([])), "aliased first refusal returned edits")?;
        verify(reopened == Some(json!([])), "aliased reopen returned edits")?;
        drop(server);

        let messages = outbound_messages(&output.lock())?;
        verify(
            show_message_count(&messages) == 2,
            format!("URI alias reopen did not warn again: {messages:?}"),
        )?;
        Ok(())
    }

    #[test]
    fn formatting_configuration_change_reenables_unsupported_warning() -> TestResult {
        let (server, output) = server_with_output();
        let uri = "file:///changed-formatting-config.pl";
        server.test_apply_did_open(uri, "sub f {\nreturn 1;\n}\n", 1)?;
        let first =
            server.handle_formatting_policy(Some(document_formatting_params(uri, 1)), None)?;
        server.config.lock().perltidy_indent_columns = Some(8);
        let changed =
            server.handle_formatting_policy(Some(document_formatting_params(uri, 1)), None)?;

        verify(first == Some(json!([])), "first refusal returned edits")?;
        verify(changed == Some(json!([])), "config-changed refusal returned edits")?;
        drop(server);

        let messages = outbound_messages(&output.lock())?;
        verify(
            show_message_count(&messages) == 2,
            format!("configuration change did not re-enable warning: {messages:?}"),
        )?;
        Ok(())
    }

    #[test]
    fn on_type_tab_refusal_stays_quiet() -> TestResult {
        let (server, output) = server_with_output();
        server.advertised_feature_ids.lock().push(Surface::OnType.feature_id());
        server.config.lock().perltidy_tabs = Some(true);
        let uri = "file:///unsupported-on-type-formatting.pl";
        server.test_apply_did_open(uri, "if ($ok) {\n\n", 1)?;
        let params = json!({
            "textDocument": { "uri": uri, "version": 1 },
            "position": { "line": 1, "character": 0 },
            "ch": "\n",
            "options": { "tabSize": 4, "insertSpaces": true },
        });

        let first = server.handle_on_type_formatting_policy(Some(params.clone()), None)?;
        let repeated = server.handle_on_type_formatting_policy(Some(params), None)?;
        verify(first == Some(json!([])), "first on-type refusal returned edits")?;
        verify(repeated == Some(json!([])), "repeated on-type refusal returned edits")?;
        drop(server);

        let messages = outbound_messages(&output.lock())?;
        verify(
            show_message_count(&messages) == 0,
            format!("valid tab-formatting refusal emitted a warning: {messages:?}"),
        )?;
        Ok(())
    }

    #[test]
    fn closing_one_document_preserves_another_receipt() -> TestResult {
        let (server, output) = server_with_output();
        let first_uri = "file:///first-native-formatting.pl";
        let second_uri = "file:///second-native-formatting.pl";
        let source = "sub f {\nreturn 1;\n}\n";
        server.test_apply_did_open(first_uri, source, 1)?;
        server.test_apply_did_open(second_uri, source, 1)?;

        let first = server
            .handle_formatting_policy(Some(document_formatting_params(first_uri, 1)), None)?;
        server.test_apply_did_close(second_uri)?;
        let repeated_first = server
            .handle_formatting_policy(Some(document_formatting_params(first_uri, 1)), None)?;

        verify(first == Some(json!([])), "first refusal returned edits")?;
        verify(
            repeated_first == Some(json!([])),
            "first refusal after closing second returned edits",
        )?;
        drop(server);

        let messages = outbound_messages(&output.lock())?;
        verify(
            show_message_count(&messages) == 1,
            format!("closing second document disturbed first receipt: {messages:?}"),
        )?;
        Ok(())
    }

    #[test]
    fn source_generation_change_reenables_warning() -> TestResult {
        let (server, output) = server_with_output();
        let uri = "file:///changed-native-formatting.pl";
        server.test_apply_did_open(uri, "sub f {\nreturn 1;\n}\n", 1)?;
        let first =
            server.handle_formatting_policy(Some(document_formatting_params(uri, 1)), None)?;
        server.test_apply_did_change(uri, "sub g {\nreturn 2;\n}\n", 2)?;
        let changed =
            server.handle_formatting_policy(Some(document_formatting_params(uri, 2)), None)?;

        verify(first == Some(json!([])), "first refusal returned edits")?;
        verify(changed == Some(json!([])), "changed-source refusal returned edits")?;
        drop(server);

        let bytes = output.lock().clone();
        let messages = outbound_messages(&bytes)?;
        verify(
            show_message_count(&messages) == 2,
            format!("a new source generation did not re-enable the warning: {messages:?}"),
        )?;
        Ok(())
    }

    #[test]
    fn intervening_formatting_result_reenables_warning() -> TestResult {
        let (server, output) = server_with_output();
        let refused_uri = "file:///refused-native-formatting.pl";
        let canonical_uri = "file:///canonical-intervening-formatting.pl";
        server.test_apply_did_open(refused_uri, "sub f {\nreturn 1;\n}\n", 1)?;
        server.test_apply_did_open(canonical_uri, "use strict;\nmy $x = 1;\n", 1)?;

        let first_refusal = server
            .handle_formatting_policy(Some(document_formatting_params(refused_uri, 1)), None)?;
        let intervening = server
            .handle_formatting_policy(Some(document_formatting_params(canonical_uri, 1)), None)?;
        let repeated_after_intervening = server
            .handle_formatting_policy(Some(document_formatting_params(refused_uri, 1)), None)?;

        verify(first_refusal == Some(json!([])), "first refusal returned edits")?;
        verify(intervening == Some(json!([])), "canonical result returned edits")?;
        verify(
            repeated_after_intervening == Some(json!([])),
            "refusal after an intervening result returned edits",
        )?;
        drop(server);

        let bytes = output.lock().clone();
        let messages = outbound_messages(&bytes)?;
        verify(
            show_message_count(&messages) == 2,
            format!("an intervening result did not re-enable the warning: {messages:?}"),
        )?;
        Ok(())
    }

    #[test]
    fn canonical_native_source_stays_quiet() -> TestResult {
        let (server, output) = server_with_output();
        let uri = "file:///canonical-native-formatting.pl";
        server.test_apply_did_open(uri, "use strict;\nmy $x = 1;\n", 1)?;

        let response =
            server.handle_formatting_policy(Some(document_formatting_params(uri, 1)), None)?;

        verify(response == Some(json!([])), "canonical source returned edits")?;
        {
            let traces = server.provider_decision_traces.lock();
            let receipt = traces
                .get(PROVIDER)
                .ok_or_else(|| io::Error::other("formatting receipt was not recorded"))?;
            verify(
                receipt.get("reason").and_then(Value::as_str) == Some("already_formatted"),
                "canonical source was not classified as already formatted",
            )?;
        }
        drop(server);

        let bytes = output.lock().clone();
        let messages = outbound_messages(&bytes)?;
        verify(
            show_message_count(&messages) == 0,
            format!("canonical source emitted a warning: {messages:?}"),
        )?;
        Ok(())
    }

    #[test]
    fn malformed_prior_receipts_do_not_suppress_warning() -> TestResult {
        let cases = [
            ("empty", json!({})),
            ("missing", json!({ "provider_action": "blocked" })),
            ("null", Value::Null),
            ("scalar", json!("not-a-receipt")),
            (
                "wrong_type",
                json!({
                    "reason": [UNSUPPORTED_SYNTAX_REASON],
                    "provider_action": "blocked",
                    "source_id_hash": ["wrong-type"],
                    "source_generation": "wrong-type",
                }),
            ),
            ("reason_only", json!({ "reason": UNSUPPORTED_SYNTAX_REASON })),
            (
                "correct_reason_and_surface_without_identity",
                json!({
                    "reason": UNSUPPORTED_SYNTAX_REASON,
                    "provider_action": "textDocument/formatting",
                }),
            ),
        ];

        for (label, prior_receipt) in cases {
            let (server, output) = server_with_output();
            let uri = format!("file:///{label}-prior-receipt.pl");
            server.test_apply_did_open(&uri, "sub f {\nreturn 1;\n}\n", 1)?;
            server.provider_decision_traces.lock().insert(PROVIDER.to_string(), prior_receipt);

            let response =
                server.handle_formatting_policy(Some(document_formatting_params(&uri, 1)), None)?;

            verify(response == Some(json!([])), format!("{label} receipt returned edits"))?;
            drop(server);

            let messages = outbound_messages(&output.lock())?;
            verify(
                show_message_count(&messages) == 1,
                format!("{label} receipt suppressed the warning: {messages:?}"),
            )?;
        }

        for (label, field, replacement) in [
            ("wrong_surface", "provider_action", json!("textDocument/rangeFormatting")),
            ("wrong_source", "source_id_hash", json!("different-source-id")),
            ("wrong_generation", "source_generation", json!(99)),
        ] {
            let uri = format!("file:///{label}-prior-receipt.pl");
            let (seed_server, _) = server_with_output();
            seed_server.test_apply_did_open(&uri, "sub f {\nreturn 1;\n}\n", 1)?;
            let _ = seed_server
                .handle_formatting_policy(Some(document_formatting_params(&uri, 1)), None)?;
            let mut prior_receipt = seed_server
                .provider_decision_traces
                .lock()
                .get(PROVIDER)
                .cloned()
                .ok_or_else(|| io::Error::other("valid formatting receipt was not recorded"))?;
            prior_receipt[field] = replacement;

            let (server, output) = server_with_output();
            server.test_apply_did_open(&uri, "sub f {\nreturn 1;\n}\n", 1)?;
            server.provider_decision_traces.lock().insert(PROVIDER.to_string(), prior_receipt);

            let response =
                server.handle_formatting_policy(Some(document_formatting_params(&uri, 1)), None)?;

            verify(response == Some(json!([])), format!("{label} receipt returned edits"))?;
            drop(server);

            let messages = outbound_messages(&output.lock())?;
            verify(
                show_message_count(&messages) == 1,
                format!("{label} receipt suppressed the warning: {messages:?}"),
            )?;
        }

        Ok(())
    }

    #[test]
    fn excluded_formatting_outcomes_never_emit_unsupported_warning() -> TestResult {
        let excluded_reasons = [
            ("disabled", FormatReasonCode::FormatterDisabled),
            ("literal_preservation", FormatReasonCode::LiteralPreservationUnsupported),
            ("unsafe_range", FormatReasonCode::UnsafeRange),
            ("parse_error", FormatReasonCode::SourceParseError),
            ("stale", FormatReasonCode::StaleSource),
            ("instrument_failure", FormatReasonCode::InstrumentFailure),
        ];

        for (label, reason) in excluded_reasons {
            let (server, output) = server_with_output();
            let uri = format!("file:///{label}-warning-exclusion.pl");
            server.test_apply_did_open(&uri, "my $x = 1;\n", 1)?;
            let params = document_formatting_params(&uri, 1);
            let snapshot = server.admit(Surface::Document, &params)?;
            let formatter = CodeFormatter::with_config_and_mode(
                snapshot.config.perltidy.clone(),
                snapshot.config.mode,
            );
            let context = FormatContext::new(Some(snapshot.uri.clone()), Some(snapshot.generation));
            let mut decision =
                formatter.format_document_decision(&snapshot.text, &snapshot.options, &context)?;
            decision.outcome.disposition = FormatDisposition::Refused;
            decision.outcome.reason = reason;
            decision.document.edits.clear();

            let response = server.project(&snapshot, decision)?;
            verify(response == Some(json!([])), format!("{label} returned edits"))?;
            drop(server);

            let messages = outbound_messages(&output.lock())?;
            verify(
                show_message_count(&messages) == 0,
                format!("{label} exclusion emitted an unsupported-syntax warning: {messages:?}"),
            )?;
        }

        Ok(())
    }
}
