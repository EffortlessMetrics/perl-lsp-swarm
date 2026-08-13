use super::{
    actual_engine_for_mode, cancellation_token, CodeFormatter, FormatContext, JsonRpcError,
    JsonRpcId, LspServer, RequestCleanupGuard, Surface, Value, invalid_params,
};

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

#[cfg(test)]
mod call_presence_tests {
    use super::{LspServer, Surface, Value, JsonRpcId};
    use super::super::{json, PROVIDER};

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
