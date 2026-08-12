use super::*;

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

    /// Run LSP 3.18 multi-range formatting through the shared runtime policy.
    pub(crate) fn handle_ranges_formatting_policy(
        &self,
        params: Option<Value>,
        request_id: Option<&Value>,
    ) -> Result<Option<Value>, JsonRpcError> {
        let typed_id = request_id.and_then(JsonRpcId::try_from_value);
        let _cleanup = RequestCleanupGuard::from_ref(typed_id.as_ref());
        let token = cancellation_token(typed_id.as_ref(), Surface::Ranges);
        self.ensure_not_cancelled(Surface::Ranges, token.as_ref(), None, None)?;
        let params =
            params.ok_or_else(|| invalid_params("Missing multi-range formatting parameters"))?;
        let snapshot = self.admit(Surface::Ranges, &params)?;
        let ranges = params
            .get("ranges")
            .and_then(Value::as_array)
            .ok_or_else(|| invalid_params("Missing required parameter: ranges"))?;
        if ranges.is_empty() {
            self.ensure_not_cancelled(Surface::Ranges, token.as_ref(), Some(&snapshot), None)?;
            self.ensure_current(&snapshot)?;
            self.record_formatting_receipt(
                &snapshot,
                "acted",
                json!("already_formatted"),
                "not_started",
                "none",
                0,
                None,
            );
            return Ok(Some(json!([])));
        }

        let parsed_ranges = ranges
            .iter()
            .enumerate()
            .map(|(index, range)| parse_range(range, &format!("ranges[{index}]")))
            .collect::<Result<Vec<_>, _>>()?;
        let overlaps = parsed_ranges.iter().enumerate().any(|(index, left)| {
            parsed_ranges.iter().skip(index + 1).any(|right| ranges_overlap(*left, *right))
        });
        if overlaps {
            self.ensure_current(&snapshot)?;
            self.record_formatting_receipt(
                &snapshot,
                "blocked",
                json!("overlapping_ranges"),
                "not_started",
                "no_edit",
                0,
                None,
            );
            return Ok(Some(json!([])));
        }

        let formatter = CodeFormatter::with_config_and_mode(
            snapshot.config.perltidy.clone(),
            snapshot.config.mode,
        );
        let context = FormatContext::new(Some(snapshot.uri.clone()), Some(snapshot.generation));
        let mut decisions = Vec::with_capacity(ranges.len());
        for (index, range) in parsed_ranges.iter().enumerate() {
            self.ensure_not_cancelled(Surface::Ranges, token.as_ref(), Some(&snapshot), None)?;
            let decision =
                formatter.format_range_decision(&snapshot.text, range, &snapshot.options, &context);
            self.ensure_not_cancelled(
                Surface::Ranges,
                token.as_ref(),
                Some(&snapshot),
                Some(actual_engine_for_mode(snapshot.config.mode)),
            )?;
            let decision = decision.map_err(|error| {
                self.formatting_failure(
                    &snapshot,
                    &format!("Range formatting failed for range {index}"),
                    error,
                )
            })?;
            let refused = decision.outcome.disposition == FormatDisposition::Refused;
            decisions.push(decision);
            if refused {
                break;
            }
        }

        self.ensure_not_cancelled(
            Surface::Ranges,
            token.as_ref(),
            Some(&snapshot),
            Some(actual_engine_for_mode(snapshot.config.mode)),
        )?;
        self.ensure_current(&snapshot)?;
        if let Some(refused) = decisions
            .iter()
            .find(|decision| decision.outcome.disposition == FormatDisposition::Refused)
        {
            let outcome = sanitized_outcome(refused);
            let reason = outcome.get("reason").cloned().unwrap_or_else(|| json!("unknown"));
            let actual_engine = outcome
                .pointer("/identity/actual_engine")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
                .to_string();
            let receipt = self.record_formatting_receipt(
                &snapshot,
                "blocked",
                reason,
                &actual_engine,
                "no_edit",
                0,
                Some(outcome),
            );
            if !refused.document.edits.is_empty() {
                return Err(JsonRpcError {
                    code: -32603,
                    message: "A refused range carried edits; no multi-range edits were returned."
                        .to_string(),
                    data: Some(json!({
                        "error_kind": "formatting_outcome_contract",
                        "reason": "instrument_failure",
                        "formatting_receipt": receipt,
                    })),
                });
            }
            return Ok(Some(json!([])));
        }

        let mut edits: Vec<FormatTextEdit> = Vec::new();
        let outcomes: Vec<Value> = decisions.iter().map(sanitized_outcome).collect();
        for decision in decisions {
            match decision.outcome.disposition {
                FormatDisposition::Applied if !decision.document.edits.is_empty() => {
                    edits.extend(decision.document.edits);
                }
                FormatDisposition::NoChange if decision.document.edits.is_empty() => {}
                _ => {
                    let receipt = self.record_formatting_receipt(
                        &snapshot,
                        "blocked",
                        json!("instrument_failure"),
                        actual_engine_for_mode(snapshot.config.mode),
                        "no_edit",
                        0,
                        Some(Value::Array(outcomes)),
                    );
                    return Err(JsonRpcError {
                        code: -32603,
                        message: "Multi-range formatting outcomes disagree with their edits."
                            .to_string(),
                        data: Some(json!({
                            "error_kind": "formatting_outcome_contract",
                            "reason": "instrument_failure",
                            "formatting_receipt": receipt,
                        })),
                    });
                }
            }
        }

        let disposition = if edits.is_empty() { "already_formatted" } else { "applied" };
        self.record_formatting_receipt(
            &snapshot,
            "acted",
            json!(disposition),
            actual_engine_for_mode(snapshot.config.mode),
            "none",
            edits.len(),
            Some(Value::Array(outcomes)),
        );
        Ok(Some(json!(edits)))
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
        let edits = crate::on_type_formatting::compute_on_type_edit(
            &snapshot.text,
            line,
            column,
            character,
            indent,
        )
        .unwrap_or_default();

        self.ensure_not_cancelled(
            Surface::OnType,
            token.as_ref(),
            Some(&snapshot),
            Some("on_type_indentation"),
        )?;
        self.ensure_current(&snapshot)?;
        self.record_formatting_receipt(
            &snapshot,
            "acted",
            if edits.is_empty() { json!("already_formatted") } else { json!("applied") },
            "on_type_indentation",
            "none",
            edits.len(),
            None,
        );
        Ok(Some(Value::Array(edits)))
    }
}

fn ranges_overlap(left: WireRange, right: WireRange) -> bool {
    let left_start = (left.start.line, left.start.character);
    let left_end = (left.end.line, left.end.character);
    let right_start = (right.start.line, right.start.character);
    let right_end = (right.end.line, right.end.character);
    left_start < right_end && right_start < left_end
}
