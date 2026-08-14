use super::{
    CodeFormatter, FormatContext, FormatterMode, JsonRpcError, JsonRpcId, LspServer,
    RequestCleanupGuard, Surface, Value, actual_engine_for_mode, cancellation_token,
    invalid_params, json, parse_range, req_position,
};

#[path = "multi_range.rs"]
mod multi_range;

impl LspServer {
    /// Run document formatting through the shared runtime policy.
    pub(crate) fn handle_formatting_policy(
        &self,
        params: Option<Value>,
        request_id: Option<&Value>,
    ) -> Result<Option<Value>, JsonRpcError> {
        let typed_id = request_id.and_then(JsonRpcId::try_from_value);
        let _cleanup = RequestCleanupGuard::from_ref(typed_id.as_ref());
        let token = cancellation_token(typed_id.as_ref(), Surface::Document);
        self.ensure_not_cancelled(Surface::Document, token.as_ref(), None, None)?;
        self.ensure_surface_advertised(Surface::Document)?;
        let params = params.ok_or_else(|| invalid_params("Missing formatting parameters"))?;
        let snapshot = self.admit(Surface::Document, &params)?;
        self.ensure_not_cancelled(Surface::Document, token.as_ref(), Some(&snapshot), None)?;

        let formatter = CodeFormatter::with_config_and_mode(
            snapshot.config.perltidy.clone(),
            snapshot.config.mode,
        );
        let context = FormatContext::new(Some(snapshot.uri.clone()), Some(snapshot.generation));
        let decision =
            formatter.format_document_decision(&snapshot.text, &snapshot.options, &context);
        self.ensure_not_cancelled(
            Surface::Document,
            token.as_ref(),
            Some(&snapshot),
            Some(actual_engine_for_mode(snapshot.config.mode)),
        )?;
        let decision = decision
            .map_err(|error| self.formatting_failure(&snapshot, "Formatting failed", error))?;
        self.ensure_current(&snapshot)?;
        self.project(&snapshot, decision)
    }

    /// Run range formatting through the shared runtime policy.
    pub(crate) fn handle_range_formatting_policy(
        &self,
        params: Option<Value>,
        request_id: Option<&Value>,
    ) -> Result<Option<Value>, JsonRpcError> {
        let typed_id = request_id.and_then(JsonRpcId::try_from_value);
        let _cleanup = RequestCleanupGuard::from_ref(typed_id.as_ref());
        let token = cancellation_token(typed_id.as_ref(), Surface::Range);
        self.ensure_not_cancelled(Surface::Range, token.as_ref(), None, None)?;
        let params = params.ok_or_else(|| invalid_params("Missing range-formatting parameters"))?;
        let snapshot = self.admit(Surface::Range, &params)?;
        let range = parse_range(
            params
                .get("range")
                .ok_or_else(|| invalid_params("Missing required parameter: range"))?,
            "range",
        )?;
        self.ensure_not_cancelled(Surface::Range, token.as_ref(), Some(&snapshot), None)?;

        let formatter = CodeFormatter::with_config_and_mode(
            snapshot.config.perltidy.clone(),
            snapshot.config.mode,
        );
        let context = FormatContext::new(Some(snapshot.uri.clone()), Some(snapshot.generation));
        let decision =
            formatter.format_range_decision(&snapshot.text, &range, &snapshot.options, &context);
        self.ensure_not_cancelled(
            Surface::Range,
            token.as_ref(),
            Some(&snapshot),
            Some(actual_engine_for_mode(snapshot.config.mode)),
        )?;
        let decision = decision.map_err(|error| {
            self.formatting_failure(&snapshot, "Range formatting failed", error)
        })?;
        self.ensure_current(&snapshot)?;
        self.project(&snapshot, decision)
    }

    /// Run LSP 3.18 multi-range formatting through one atomic plan.
    pub(crate) fn handle_ranges_formatting_policy(
        &self,
        params: Option<Value>,
        request_id: Option<&Value>,
    ) -> Result<Option<Value>, JsonRpcError> {
        multi_range::handle(self, params, request_id)
    }

    /// Run bounded on-type indentation through the shared runtime policy.
    pub(crate) fn handle_on_type_formatting_policy(
        &self,
        params: Option<Value>,
        request_id: Option<&Value>,
    ) -> Result<Option<Value>, JsonRpcError> {
        let typed_id = request_id.and_then(JsonRpcId::try_from_value);
        let _cleanup = RequestCleanupGuard::from_ref(typed_id.as_ref());
        let token = cancellation_token(typed_id.as_ref(), Surface::OnType);
        self.ensure_not_cancelled(Surface::OnType, token.as_ref(), None, None)?;
        let params =
            params.ok_or_else(|| invalid_params("Missing on-type formatting parameters"))?;
        let snapshot = self.admit(Surface::OnType, &params)?;
        self.ensure_not_cancelled(Surface::OnType, token.as_ref(), Some(&snapshot), None)?;

        let character = params
            .get("ch")
            .and_then(Value::as_str)
            .and_then(|text| text.chars().next())
            .ok_or_else(|| invalid_params("Missing or invalid on-type trigger character"))?;
        let (line, column) = req_position(&params)?;

        if snapshot.config.mode == FormatterMode::Off {
            self.ensure_current(&snapshot)?;
            self.record_formatting_receipt(
                &snapshot,
                "blocked",
                json!("formatter_disabled"),
                "disabled",
                "no_edit",
                0,
                None,
            );
            return Ok(Some(json!([])));
        }

        let tabs = snapshot.config.perltidy.tabs.unwrap_or(!snapshot.options.insert_spaces);
        if tabs {
            self.ensure_current(&snapshot)?;
            self.record_formatting_receipt(
                &snapshot,
                "blocked",
                json!("unsupported_syntax"),
                "on_type_indentation",
                "no_edit",
                0,
                None,
            );
            return Ok(Some(json!([])));
        }

        let indent =
            snapshot.config.perltidy.indent_columns.unwrap_or(snapshot.options.tab_size).max(1)
                as usize;
        let decision = perl_lsp_rs_core::providers::on_type_formatting::compute_on_type_decision(
            &snapshot.text,
            line,
            column,
            character,
            indent,
        );
        let (receipt_action, receipt_reason, edits) = match decision {
            perl_lsp_rs_core::providers::on_type_formatting::OnTypeEditDecision::NoChange => {
                ("acted", json!("already_formatted"), Vec::new())
            }
            perl_lsp_rs_core::providers::on_type_formatting::OnTypeEditDecision::Suppressed(
                suppression,
            ) => ("blocked", json!(suppression.reason()), Vec::new()),
            perl_lsp_rs_core::providers::on_type_formatting::OnTypeEditDecision::Edits(edits) => {
                let reason = if edits.is_empty() { "already_formatted" } else { "applied" };
                ("acted", json!(reason), edits)
            }
        };

        self.ensure_not_cancelled(
            Surface::OnType,
            token.as_ref(),
            Some(&snapshot),
            Some("on_type_indentation"),
        )?;
        self.ensure_current(&snapshot)?;
        self.record_formatting_receipt(
            &snapshot,
            receipt_action,
            receipt_reason,
            "on_type_indentation",
            "none",
            edits.len(),
            None,
        );
        Ok(Some(Value::Array(edits)))
    }
}

#[cfg(test)]
mod call_presence_tests {
    use super::super::{JsonRpcId, LspServer, PROVIDER, Surface, Value, json};

    fn advertise(server: &LspServer) {
        server.advertised_feature_ids.lock().push(Surface::Document.feature_id());
    }

    fn receipt(server: &LspServer) -> Result<Value, Box<dyn std::error::Error>> {
        server
            .provider_decision_traces
            .lock()
            .get(PROVIDER)
            .cloned()
            .ok_or_else(|| "missing formatting receipt".into())
    }

    #[test]
    fn handle_formatting_policy_call_presence_observer() -> Result<(), Box<dyn std::error::Error>> {
        let server = LspServer::new();
        advertise(&server);
        server.config.lock().perltidy_enabled = false;
        let uri = "file:///call-presence-formatting.pl";
        server.test_apply_did_open(uri, "my$x=1;\n", 1)?;

        let result = server.handle_formatting_policy(
            Some(json!({
                "textDocument": { "uri": uri, "version": 1 },
                "options": { "tabSize": 4, "insertSpaces": true },
            })),
            None,
        )?;

        assert_eq!(
            result,
            Some(json!([])),
            "input that reaches call self.admit(Surface::Document, &params)"
        );
        assert_eq!(
            receipt(&server)?["decision"],
            "blocked",
            "input that reaches call Some(&snapshot)"
        );
        assert_eq!(
            receipt(&server)?["actual_engine"],
            "disabled",
            "input that reaches call CodeFormatter::with_config_and_mode(\n            snapshot.config.perltidy.clone(),\n            snapshot.config.mode,\n        )"
        );
        assert_eq!(
            receipt(&server)?["reason"],
            "formatter_disabled",
            "input that reaches call snapshot.config.perltidy.clone()"
        );
        assert!(
            receipt(&server)?["source_generation"].is_u64(),
            "input that reaches call Some(snapshot.generation)"
        );
        assert!(
            receipt(&server)?["source_id_hash"].is_string(),
            "input that reaches call Some(snapshot.uri.clone())"
        );
        assert!(
            receipt(&server)?["source_id_hash"].is_string(),
            "input that reaches call snapshot.uri.clone()"
        );
        Ok(())
    }

    #[test]
    fn handle_formatting_policy_call_presence_observer_ensure_surface_advertised()
    -> Result<(), Box<dyn std::error::Error>> {
        let server = LspServer::new();
        server.advertised_features.lock().formatting = false;
        server.advertised_feature_ids.lock().clear();
        let error = server
            .handle_formatting_policy(None, Some(&JsonRpcId::Integer(301).to_value()))
            .err()
            .ok_or("expected method-not-advertised")?;
        assert_eq!(
            error.code, -32601,
            "input that reaches call self.ensure_surface_advertised(Surface::Document)"
        );
        Ok(())
    }

    #[test]
    fn handle_formatting_policy_call_presence_observer_missing_params()
    -> Result<(), Box<dyn std::error::Error>> {
        let server = LspServer::new();
        advertise(&server);
        let error = server
            .handle_formatting_policy(None, Some(&JsonRpcId::Integer(302).to_value()))
            .err()
            .ok_or("expected invalid params")?;
        assert_eq!(error.code, crate::protocol::INVALID_PARAMS);
        assert!(
            error.message.contains("Missing formatting parameters"),
            "input that reaches call params.ok_or_else(|| invalid_params(\"Missing formatting parameters\"))"
        );
        assert!(
            error.message.contains("Missing formatting parameters"),
            "input that reaches call invalid_params(\"Missing formatting parameters\")"
        );
        Ok(())
    }
}
