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
}
