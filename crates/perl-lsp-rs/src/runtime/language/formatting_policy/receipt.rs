use super::{
    CONTENT_MODIFIED, FormatDisposition, FormattingDecision, FormattingError, JsonRpcError,
    LspServer, PROVIDER, PerlLspCancellationToken, REQUEST_CANCELLED, Snapshot, Surface, Value,
    actual_engine_for_mode, formatting_error_reason, json, sanitized_outcome, value,
};

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

    pub(super) fn project(
        &self,
        snapshot: &Snapshot,
        decision: FormattingDecision,
    ) -> Result<Option<Value>, JsonRpcError> {
        let disposition = decision.outcome.disposition;
        let edit_count = decision.document.edits.len();
        let outcome = sanitized_outcome(&decision);
        let actual_engine = outcome
            .pointer("/identity/actual_engine")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_string();
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
            &actual_engine,
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
