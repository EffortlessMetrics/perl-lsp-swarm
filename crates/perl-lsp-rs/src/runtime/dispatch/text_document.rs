//! Text document request handlers
//!
//! Wraps textDocument/* LSP requests.

use super::super::*;

impl LspServer {
    // Text synchronization handlers
    pub(super) fn handle_did_open_dispatch(
        &self,
        params: Option<Value>,
    ) -> Result<Option<Value>, JsonRpcError> {
        let uri = params
            .as_ref()
            .and_then(|p| p.pointer("/textDocument/uri"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let token = self.new_parse_token(uri);
        match self.handle_did_open_with_cancellation(params, Some(token)) {
            Ok(_) => Ok(None),
            Err(e) => Err(e),
        }
    }

    pub(super) fn handle_did_change_dispatch(
        &self,
        params: Option<Value>,
    ) -> Result<Option<Value>, JsonRpcError> {
        let uri = params
            .as_ref()
            .and_then(|p| p.pointer("/textDocument/uri"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let token = self.new_parse_token(uri);
        match self.handle_did_change_with_cancellation(params, Some(token)) {
            Ok(_) => Ok(None),
            Err(e) => Err(e),
        }
    }

    pub(super) fn handle_did_close_dispatch(
        &self,
        params: Option<Value>,
    ) -> Result<Option<Value>, JsonRpcError> {
        match self.handle_did_close(params) {
            Ok(_) => Ok(None),
            Err(e) => Err(e),
        }
    }

    pub(super) fn handle_did_save_dispatch(
        &self,
        params: Option<Value>,
    ) -> Result<Option<Value>, JsonRpcError> {
        match self.handle_did_save(params) {
            Ok(_) => Ok(None),
            Err(e) => Err(e),
        }
    }

    pub(super) fn handle_will_save_dispatch(
        &self,
        params: Option<Value>,
    ) -> Result<Option<Value>, JsonRpcError> {
        match self.handle_will_save(params) {
            Ok(_) => Ok(None),
            Err(e) => Err(e),
        }
    }

    pub(super) fn handle_will_save_wait_until_dispatch(
        &self,
        params: Option<Value>,
    ) -> Result<Option<Value>, JsonRpcError> {
        self.handle_will_save_wait_until(params)
    }

    // Notebook document handlers
    pub(super) fn handle_notebook_did_open_dispatch(
        &self,
        params: Option<Value>,
    ) -> Result<Option<Value>, JsonRpcError> {
        match self.handle_notebook_did_open(params) {
            Ok(_) => Ok(None),
            Err(e) => Err(e),
        }
    }

    pub(super) fn handle_notebook_did_change_dispatch(
        &self,
        params: Option<Value>,
    ) -> Result<Option<Value>, JsonRpcError> {
        match self.handle_notebook_did_change(params) {
            Ok(_) => Ok(None),
            Err(e) => Err(e),
        }
    }

    pub(super) fn handle_notebook_did_save_dispatch(
        &self,
        params: Option<Value>,
    ) -> Result<Option<Value>, JsonRpcError> {
        match self.handle_notebook_did_save(params) {
            Ok(_) => Ok(None),
            Err(e) => Err(e),
        }
    }

    pub(super) fn handle_notebook_did_close_dispatch(
        &self,
        params: Option<Value>,
    ) -> Result<Option<Value>, JsonRpcError> {
        match self.handle_notebook_did_close(params) {
            Ok(_) => Ok(None),
            Err(e) => Err(e),
        }
    }

    // Completion handlers
    pub(super) fn handle_completion_cancellable_dispatch(
        &self,
        params: Option<Value>,
        id: Option<&Value>,
    ) -> Result<Option<Value>, JsonRpcError> {
        self.handle_completion_cancellable(params, id)
    }

    pub(super) fn handle_completion_resolve_dispatch(
        &self,
        params: Option<Value>,
    ) -> Result<Option<Value>, JsonRpcError> {
        self.handle_completion_resolve(params)
    }

    // Hover and signature help
    pub(super) fn handle_hover_cancellable_dispatch(
        &self,
        params: Option<Value>,
        id: Option<&Value>,
    ) -> Result<Option<Value>, JsonRpcError> {
        self.handle_hover_cancellable(params, id)
    }

    pub(super) fn handle_signature_help_dispatch(
        &self,
        params: Option<Value>,
    ) -> Result<Option<Value>, JsonRpcError> {
        self.handle_signature_help(params)
    }

    // Definition and navigation
    pub(super) fn handle_definition_cancellable_dispatch(
        &self,
        params: Option<Value>,
        id: Option<&Value>,
    ) -> Result<Option<Value>, JsonRpcError> {
        // Gate unadvertised feature
        if !self.advertised_features.lock().definition {
            return Err(crate::protocol::method_not_advertised());
        }
        // Test-only fast path: skip the real handler when the test-fallbacks
        // feature is enabled and LSP_TEST_FALLBACKS is set.  Compiled out of
        // production builds so the env var is never read on the hot path (#4628).
        #[cfg(any(test, feature = "test-fallbacks"))]
        if std::env::var("LSP_TEST_FALLBACKS").is_ok() {
            return match self.on_definition(params.clone().unwrap_or(json!({}))) {
                Ok(res) => Ok(Some(res)),
                Err(_) => self.handle_definition_cancellable(params, id),
            };
        }

        // Production path: try the real handler first, fall back on
        // non-cancellation errors.  REQUEST_CANCELLED is preserved so the
        // client receives the cancellation instead of an empty result (#4628).
        self.handle_definition_cancellable(params, id).or_else(|error| {
            if error.code == crate::protocol::REQUEST_CANCELLED {
                Err(error)
            } else {
                tracing::warn!(error = %error, "definition handler error, using empty-params fallback");
                self.on_definition(json!({})).map(Some)
            }
        })
    }

    pub(super) fn handle_declaration_dispatch(
        &self,
        params: Option<Value>,
    ) -> Result<Option<Value>, JsonRpcError> {
        self.handle_declaration(params)
    }

    pub(super) fn handle_references_cancellable_dispatch(
        &self,
        params: Option<Value>,
        request_id: Option<&Value>,
    ) -> Result<Option<Value>, JsonRpcError> {
        // Gate unadvertised feature
        if !self.advertised_features.lock().references {
            return Err(crate::protocol::method_not_advertised());
        }
        // Test-only fast path (#4628): compiled out of production builds.
        #[cfg(any(test, feature = "test-fallbacks"))]
        if std::env::var("LSP_TEST_FALLBACKS").is_ok() {
            return match self.on_references(params.clone().unwrap_or(json!({})), request_id) {
                Ok(res) => Ok(Some(res)),
                Err(error) if error.code == crate::protocol::REQUEST_CANCELLED => Err(error),
                Err(_) => self.handle_references_with_request_id(params, request_id),
            };
        }

        // Production path: try real handler first, fall back on
        // non-cancellation errors.  REQUEST_CANCELLED is preserved.
        let fallback_params = params.clone().unwrap_or_else(|| json!({}));
        self.handle_references_with_request_id(params, request_id).or_else(|error| {
            if error.code == crate::protocol::REQUEST_CANCELLED {
                Err(error)
            } else {
                tracing::warn!(error = %error, "references handler error, using empty-params fallback");
                self.on_references(fallback_params, request_id).map(Some)
            }
        })
    }

    pub(super) fn handle_document_highlight_dispatch(
        &self,
        params: Option<Value>,
    ) -> Result<Option<Value>, JsonRpcError> {
        self.handle_document_highlight(params)
    }

    // Type hierarchy
    pub(super) fn handle_prepare_type_hierarchy_dispatch(
        &self,
        params: Option<Value>,
    ) -> Result<Option<Value>, JsonRpcError> {
        self.handle_prepare_type_hierarchy(params)
    }

    pub(super) fn handle_type_hierarchy_supertypes_dispatch(
        &self,
        params: Option<Value>,
    ) -> Result<Option<Value>, JsonRpcError> {
        self.handle_type_hierarchy_supertypes(params)
    }

    pub(super) fn handle_type_hierarchy_subtypes_dispatch(
        &self,
        params: Option<Value>,
    ) -> Result<Option<Value>, JsonRpcError> {
        self.handle_type_hierarchy_subtypes(params)
    }

    // Diagnostics
    pub(super) fn handle_document_diagnostic_dispatch(
        &self,
        params: Option<Value>,
    ) -> Result<Option<Value>, JsonRpcError> {
        self.handle_document_diagnostic(params)
    }

    pub(super) fn handle_workspace_diagnostic_dispatch(
        &self,
        params: Option<Value>,
    ) -> Result<Option<Value>, JsonRpcError> {
        self.handle_workspace_diagnostic(params)
    }

    // Rename
    pub(super) fn handle_prepare_rename_dispatch(
        &self,
        params: Option<Value>,
    ) -> Result<Option<Value>, JsonRpcError> {
        self.handle_prepare_rename(params)
    }

    pub(super) fn handle_rename_workspace_cancellable_dispatch(
        &self,
        params: Option<Value>,
        request_id: Option<&Value>,
    ) -> Result<Option<Value>, JsonRpcError> {
        self.handle_rename_workspace_cancellable(params, request_id)
    }

    // Code actions
    pub(super) fn handle_code_action_cancellable_dispatch(
        &self,
        params: Option<Value>,
        request_id: Option<&Value>,
    ) -> Result<Option<Value>, JsonRpcError> {
        self.handle_code_action_cancellable(params, request_id)
    }

    pub(super) fn handle_code_action_resolve_dispatch(
        &self,
        params: Option<Value>,
    ) -> Result<Option<Value>, JsonRpcError> {
        self.handle_code_action_resolve(params)
    }

    // Semantic tokens
    pub(super) fn handle_semantic_tokens_cancellable_dispatch(
        &self,
        params: Option<Value>,
        request_id: Option<&Value>,
    ) -> Result<Option<Value>, JsonRpcError> {
        self.handle_semantic_tokens_cancellable(params, request_id)
    }

    pub(super) fn handle_semantic_tokens_range_dispatch(
        &self,
        params: Option<Value>,
    ) -> Result<Option<Value>, JsonRpcError> {
        self.handle_semantic_tokens_range(params)
    }

    pub(super) fn handle_semantic_tokens_delta_dispatch(
        &self,
        params: Option<Value>,
    ) -> Result<Option<Value>, JsonRpcError> {
        self.handle_semantic_tokens_delta(params)
    }

    // Inlay hints
    pub(super) fn handle_inlay_hints_dispatch(
        &self,
        params: Option<Value>,
    ) -> Result<Option<Value>, JsonRpcError> {
        self.handle_inlay_hints(params)
    }

    pub(super) fn handle_inlay_hint_resolve_dispatch(
        &self,
        params: Option<Value>,
    ) -> Result<Option<Value>, JsonRpcError> {
        self.handle_inlay_hint_resolve(params)
    }

    // Document links
    pub(super) fn handle_document_links_dispatch(
        &self,
        params: Option<Value>,
    ) -> Result<Option<Value>, JsonRpcError> {
        self.handle_document_links(params)
    }

    pub(super) fn handle_document_link_resolve_dispatch(
        &self,
        params: Option<Value>,
    ) -> Result<Option<Value>, JsonRpcError> {
        self.handle_document_link_resolve(params)
    }

    // Selection ranges
    pub(super) fn handle_selection_range_dispatch(
        &self,
        params: Option<Value>,
    ) -> Result<Option<Value>, JsonRpcError> {
        self.handle_selection_range(params)
    }

    // On-type formatting
    pub(super) fn handle_on_type_formatting_dispatch(
        &self,
        params: Option<Value>,
    ) -> Result<Option<Value>, JsonRpcError> {
        self.handle_on_type_formatting(params)
    }

    // Code lens
    pub(super) fn handle_code_lens_dispatch(
        &self,
        params: Option<Value>,
    ) -> Result<Option<Value>, JsonRpcError> {
        self.handle_code_lens(params)
    }

    pub(super) fn handle_code_lens_resolve_dispatch(
        &self,
        params: Option<Value>,
    ) -> Result<Option<Value>, JsonRpcError> {
        self.handle_code_lens_resolve(params)
    }

    // Linked editing
    pub(super) fn handle_linked_editing_range_dispatch(
        &self,
        params: Option<Value>,
    ) -> Result<Option<Value>, JsonRpcError> {
        self.handle_linked_editing_range(params)
    }

    // Inline completion
    pub(super) fn handle_inline_completion_dispatch(
        &self,
        params: Option<Value>,
    ) -> Result<Option<Value>, JsonRpcError> {
        self.handle_inline_completion(params)
    }

    // Streaming inline completion (custom request)
    pub(super) fn handle_streaming_inline_completion_dispatch(
        &self,
        params: Option<Value>,
    ) -> Result<Option<Value>, JsonRpcError> {
        self.handle_streaming_inline_completion(params)
    }

    // Inline value
    pub(super) fn handle_inline_value_dispatch(
        &self,
        params: Option<Value>,
    ) -> Result<Option<Value>, JsonRpcError> {
        self.handle_inline_value(params)
    }

    // Moniker
    pub(super) fn handle_moniker_dispatch(
        &self,
        params: Option<Value>,
    ) -> Result<Option<Value>, JsonRpcError> {
        self.handle_moniker(params)
    }

    // Document colors
    pub(super) fn handle_document_color_dispatch(
        &self,
        params: Option<Value>,
    ) -> Result<Option<Value>, JsonRpcError> {
        self.handle_document_color(params)
    }

    pub(super) fn handle_color_presentation_dispatch(
        &self,
        params: Option<Value>,
    ) -> Result<Option<Value>, JsonRpcError> {
        self.handle_color_presentation(params)
    }

    // Type definition
    pub(super) fn handle_type_definition_dispatch(
        &self,
        params: Option<Value>,
    ) -> Result<Option<Value>, JsonRpcError> {
        self.handle_type_definition(params)
    }

    // Implementation
    pub(super) fn handle_implementation_dispatch(
        &self,
        params: Option<Value>,
    ) -> Result<Option<Value>, JsonRpcError> {
        self.handle_implementation(params)
    }

    // Folding range
    pub(super) fn handle_folding_range_dispatch(
        &self,
        params: Option<Value>,
    ) -> Result<Option<Value>, JsonRpcError> {
        // Gate unadvertised feature
        if !self.advertised_features.lock().folding_range {
            return Err(crate::protocol::method_not_advertised());
        }
        // Test-only fast path (#4628): compiled out of production builds.
        #[cfg(any(test, feature = "test-fallbacks"))]
        if std::env::var("LSP_TEST_FALLBACKS").is_ok() {
            return match self.on_folding_range(params.clone().unwrap_or(json!({}))) {
                Ok(res) => Ok(Some(res)),
                Err(_) => self.handle_folding_range(params),
            };
        }

        // Production path: try real handler first, fall back on
        // non-cancellation errors.  REQUEST_CANCELLED is preserved so the
        // client receives the cancellation instead of an empty result (#4628).
        self.handle_folding_range(params).or_else(|error| {
            if error.code == crate::protocol::REQUEST_CANCELLED {
                Err(error)
            } else {
                tracing::debug!(error = %error, "foldingRange handler error, using empty-params fallback");
                self.on_folding_range(json!({})).map(Some)
            }
        })
    }

    // Formatting
    pub(super) fn handle_formatting_cancellable_dispatch(
        &self,
        params: Option<Value>,
        request_id: Option<&Value>,
    ) -> Result<Option<Value>, JsonRpcError> {
        self.handle_formatting_cancellable(params, request_id)
    }

    pub(super) fn handle_range_formatting_dispatch(
        &self,
        params: Option<Value>,
    ) -> Result<Option<Value>, JsonRpcError> {
        self.handle_range_formatting(params)
    }

    pub(super) fn handle_ranges_formatting_dispatch(
        &self,
        params: Option<Value>,
    ) -> Result<Option<Value>, JsonRpcError> {
        self.handle_ranges_formatting(params)
    }

    // Call hierarchy
    pub(super) fn handle_prepare_call_hierarchy_dispatch(
        &self,
        params: Option<Value>,
    ) -> Result<Option<Value>, JsonRpcError> {
        self.handle_prepare_call_hierarchy(params)
    }

    pub(super) fn handle_incoming_calls_dispatch(
        &self,
        params: Option<Value>,
    ) -> Result<Option<Value>, JsonRpcError> {
        self.handle_incoming_calls(params)
    }

    pub(super) fn handle_outgoing_calls_dispatch(
        &self,
        params: Option<Value>,
    ) -> Result<Option<Value>, JsonRpcError> {
        self.handle_outgoing_calls(params)
    }

    // Document symbol
    pub(super) fn handle_document_symbol_cancellable_dispatch(
        &self,
        params: Option<Value>,
        request_id: Option<&Value>,
    ) -> Result<Option<Value>, JsonRpcError> {
        self.handle_document_symbol_cancellable(params, request_id)
    }

    // Execute command
    pub(super) fn handle_execute_command_dispatch(
        &self,
        params: Option<Value>,
    ) -> Result<Option<Value>, JsonRpcError> {
        self.handle_execute_command(params)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cancellation::{GLOBAL_CANCELLATION_REGISTRY, PerlLspCancellationToken};

    /// Static-analysis test: verify that `LSP_TEST_FALLBACKS` is only read
    /// inside `#[cfg(any(test, feature = "test-fallbacks"))]` blocks, never
    /// in production code paths (#4628).
    #[test]
    fn lsp_test_fallbacks_env_var_is_cfg_gated() {
        let source = include_str!("text_document.rs");

        // Only inspect the production portion — everything before the
        // `#[cfg(test)] mod tests` block at the end of the file.
        let production_source = source.split("#[cfg(test)]\nmod tests").next().unwrap_or(source);

        // In production code, every `std::env::var("LSP_TEST_FALLBACKS")` call
        // must be preceded by a `#[cfg(any(test, feature = "test-fallbacks"))]`
        // attribute.  We verify this by counting: the number of cfg attributes
        // must equal the number of env var reads.
        let cfg_count =
            production_source.matches("#[cfg(any(test, feature = \"test-fallbacks\"))]").count();
        let env_var_count =
            production_source.matches("std::env::var(\"LSP_TEST_FALLBACKS\")").count();

        assert_eq!(
            cfg_count, env_var_count,
            "production code has {env_var_count} LSP_TEST_FALLBACKS env var reads but only \
             {cfg_count} #[cfg(any(test, feature = \"test-fallbacks\"))] gates — \
             every read must be gated (#4628)"
        );

        // Additionally verify there are no bare `let use_fallback =` patterns
        // (the old shape that read the env var unconditionally).
        assert!(
            !production_source.contains("let use_fallback ="),
            "production code must not contain `let use_fallback =` — \
             the old unconditional env var read pattern (#4628)"
        );
    }

    /// Static-analysis test: verify that all three production fallback paths
    /// (definition, references, folding) preserve REQUEST_CANCELLED instead
    /// of silently swallowing it (#4628).
    #[test]
    fn production_fallback_paths_preserve_request_cancelled() {
        let source = include_str!("text_document.rs");

        // Find each dispatch method and verify its production path checks
        // for REQUEST_CANCELLED.
        let methods = [
            "handle_definition_cancellable_dispatch",
            "handle_references_cancellable_dispatch",
            "handle_folding_range_dispatch",
        ];

        for method_name in &methods {
            // Find the method body
            let method_start = source.find(&format!("fn {method_name}(")).unwrap_or(0);

            // Find the next method after this one (or end of file)
            let method_end = source[method_start + 20..]
                .find("\n    }")
                .map(|pos| method_start + 20 + pos + 6)
                .unwrap_or(source.len());

            let method_body = &source[method_start..method_end];

            // The production path must reference REQUEST_CANCELLED
            assert!(
                method_body.contains("REQUEST_CANCELLED"),
                "{method_name} production path must preserve REQUEST_CANCELLED (#4628)"
            );

            // The production path must not use a bare `or_else(|_|` that
            // swallows all errors — it must inspect the error.
            assert!(
                !method_body.contains(".or_else(|_|"),
                "{method_name} production path must not swallow all errors with or_else(|_| ...) \
                 — it must check for REQUEST_CANCELLED (#4628)"
            );
        }
    }

    /// Behavioral test: verify that a cancelled definition request returns
    /// REQUEST_CANCELLED from the dispatch method, not a fallback empty
    /// result (#4628).
    #[test]
    #[serial_test::serial]
    fn definition_dispatch_preserves_cancellation() -> Result<(), Box<dyn std::error::Error>> {
        // If LSP_TEST_FALLBACKS is set (by a parallel test), the test-fallback
        // branch intercepts and bypasses the production path we want to
        // exercise.  Skip in that case — the static-analysis test above
        // covers the production path unconditionally.
        if std::env::var("LSP_TEST_FALLBACKS").is_ok() {
            eprintln!("Skipping cancellation test: LSP_TEST_FALLBACKS is set");
            return Ok(());
        }

        let server = LspServer::new();
        let request_id = JsonRpcId::Integer(46280);
        let typed_id = request_id.clone();

        // Pre-register and cancel the token so handle_definition_cancellable
        // finds a cancelled token and returns REQUEST_CANCELLED immediately.
        let token =
            PerlLspCancellationToken::new(typed_id.clone(), "textDocument/definition".to_string());
        let _ = GLOBAL_CANCELLATION_REGISTRY.register_token(token);
        let _ = GLOBAL_CANCELLATION_REGISTRY.cancel_request(&typed_id);

        let params = json!({
            "textDocument": {"uri": "file:///nonexistent.pl"},
            "position": {"line": 0, "character": 0}
        });

        let result = server
            .handle_definition_cancellable_dispatch(Some(params), Some(&request_id.to_value()));

        // Clean up the registry entry
        GLOBAL_CANCELLATION_REGISTRY.remove_request(&typed_id);

        match result {
            Err(error) => {
                assert_eq!(
                    error.code, REQUEST_CANCELLED,
                    "cancelled definition request must return REQUEST_CANCELLED, not a fallback"
                );
            }
            Ok(Some(_)) => {
                return Err(
                    "cancelled definition request returned a result instead of REQUEST_CANCELLED"
                        .into(),
                );
            }
            Ok(None) => {
                return Err(
                    "cancelled definition request returned None instead of REQUEST_CANCELLED"
                        .into(),
                );
            }
        }

        Ok(())
    }
}
