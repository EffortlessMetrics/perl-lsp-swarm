use super::*;

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

        let character = params
            .get("ch")
            .and_then(Value::as_str)
            .and_then(|text| text.chars().next())
            .ok_or_else(|| invalid_params("Missing or invalid on-type trigger character"))?;
        let (line, column) = req_position(&params)?;
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
