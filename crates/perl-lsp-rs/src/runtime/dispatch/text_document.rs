//! Text document request handlers
//!
//! Wraps textDocument/* LSP requests.

#[cfg(test)]
use super::super::*;
use super::super::{JsonRpcError, LspServer, Value, json};

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

        // Production path: the canonical handler's outcome is terminal.
        // Cancelled, stale, invalid, and provider failures reach the client at
        // their typed JSON-RPC codes; a failed request is never flattened into
        // an apparently-successful empty search by an empty-params fallback
        // (#5108).
        self.handle_definition_cancellable(params, id)
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

        // Production path: try the real handler first. The compatibility
        // fallback runs only when the failure carries no terminal protocol
        // verdict (`references_fallback_eligible`): cancelled, stale, invalid,
        // and internal-failure outcomes are refused and reach the client at
        // their typed codes (#5108). The fallback receives the exact original
        // request (`fallback_params`), and its provider and reason are logged.
        let fallback_params = params.clone().unwrap_or_else(|| json!({}));
        self.handle_references_with_request_id(params, request_id).or_else(|error| {
            if !references_fallback_eligible(error.code) {
                return Err(error);
            }
            tracing::warn!(
                error = %error,
                provider = "on_references",
                "references handler error is fallback-eligible; running bounded text fallback"
            );
            self.on_references(fallback_params, request_id).map(Some)
        })
    }

    pub(super) fn handle_document_highlight_dispatch(
        &self,
        params: Option<Value>,
        _request_id: Option<&Value>,
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
    //
    // Withdrawn (#11955): no dispatch arm exists; the shared policy route
    // refuses the method before the routing table is consulted.

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
        // Test-only fast path (#4628): compiled out of production builds and
        // incapable of satisfying production acceptance (#13981). Unadvertised
        // folding must still refuse; the fallback must not become a second
        // success path around the handler gate.
        #[cfg(any(test, feature = "test-fallbacks"))]
        if std::env::var("LSP_TEST_FALLBACKS").is_ok()
            && self.advertised_features.lock().folding_range
        {
            return match self.on_folding_range(params.clone().unwrap_or(json!({}))) {
                Ok(res) => Ok(Some(res)),
                Err(_) => self.handle_folding_range(params),
            };
        }

        self.handle_folding_range(params)
    }

    // Formatting
    pub(super) fn handle_formatting_cancellable_dispatch(
        &self,
        params: Option<Value>,
        request_id: Option<&Value>,
    ) -> Result<Option<Value>, JsonRpcError> {
        self.handle_formatting_cancellable(params, request_id)
    }

    // Range formatting
    //
    // Withdrawn (#11955): no dispatch arms exist for
    // `textDocument/rangeFormatting` or `textDocument/rangesFormatting`; the
    // shared policy route refuses both before the routing table is consulted.

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

/// Explicit eligibility predicate for the `textDocument/references`
/// compatibility fallback (#5108).
///
/// The fallback may serve an answer only when the canonical handler failed
/// without a terminal protocol verdict. Cancelled, stale, invalid, and
/// internal-failure outcomes must reach the client at their typed JSON-RPC
/// codes; they can never enter the fallback, because a fallback result would
/// relabel the failure as an apparently-successful (possibly empty) search.
fn references_fallback_eligible(code: i32) -> bool {
    use crate::protocol::{
        CONTENT_MODIFIED, INTERNAL_ERROR, INVALID_PARAMS, INVALID_REQUEST, PARSE_ERROR,
        REQUEST_CANCELLED, SERVER_CANCELLED,
    };

    !matches!(
        code,
        REQUEST_CANCELLED
            | SERVER_CANCELLED
            | CONTENT_MODIFIED
            | PARSE_ERROR
            | INVALID_REQUEST
            | INVALID_PARAMS
            | INTERNAL_ERROR
    )
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

    /// Static-analysis test: the production references fallback must stay
    /// behind the explicit eligibility predicate
    /// (`references_fallback_eligible`), so cancelled, stale, invalid, and
    /// internal-failure outcomes can never enter it (#5108). The production
    /// definition dispatch is a transparent adapter and must not regain any
    /// error-to-empty fallback (see
    /// `production_definition_dispatch_does_not_retry_errors_as_empty`).
    #[test]
    fn production_references_fallback_requires_explicit_eligibility()
    -> Result<(), Box<dyn std::error::Error>> {
        let source = include_str!("text_document.rs");
        let method_body = dispatch_method_source(source, "handle_references_cancellable_dispatch")?;

        // The fallback decision must go through the named eligibility
        // predicate; a bare error-code comparison cannot express the contract.
        assert!(
            method_body.contains("references_fallback_eligible(error.code)"),
            "references production path must gate the fallback behind \
             references_fallback_eligible (#5108)\n{method_body}"
        );

        // The production path must not use a bare `or_else(|_|` that
        // swallows all errors — it must inspect the error (#4628).
        assert!(
            !method_body.contains(".or_else(|_|"),
            "references production path must not swallow all errors with or_else(|_| ...) \
             — it must inspect the error (#4628)"
        );

        // The predicate itself must refuse cancellation; behavioral coverage
        // is in `references_dispatch_preserves_cancellation`.
        assert!(
            !references_fallback_eligible(crate::protocol::REQUEST_CANCELLED),
            "REQUEST_CANCELLED must never enter the references fallback (#5108)"
        );
        Ok(())
    }

    fn dispatch_method_source<'a>(
        source: &'a str,
        method_name: &str,
    ) -> Result<&'a str, &'static str> {
        let start_marker = format!("fn {method_name}(");
        let method_start = source.find(&start_marker).ok_or("dispatch method present")?;
        let after_start = method_start + start_marker.len();
        let next_fn = source[after_start..]
            .find("\n    pub(super) fn ")
            .map(|offset| after_start + offset)
            .unwrap_or(source.len());
        Ok(&source[method_start..next_fn])
    }

    fn folding_range_dispatch_source(source: &str) -> Result<&str, &'static str> {
        dispatch_method_source(source, "handle_folding_range_dispatch")
    }

    /// Recurrence guard (#5108): no production dispatch in this file may call
    /// a provider fallback with empty parameters. An empty-parameter fallback
    /// receives no URI or position, can only answer `[]`, and therefore turns
    /// any failure it catches into an apparently-successful empty search.
    #[test]
    fn production_dispatch_has_no_empty_param_fallbacks() {
        let source = include_str!("text_document.rs");
        let production_source = source.split("#[cfg(test)]\nmod tests").next().unwrap_or(source);

        for forbidden in
            ["on_definition(json!({}))", "on_references(json!({})", "on_folding_range(json!({}))"]
        {
            assert!(
                !production_source.contains(forbidden),
                "empty-parameter production fallback `{forbidden}` must not return (#5108)"
            );
        }
    }

    /// Production foldingRange dispatch must be a transparent adapter over the
    /// canonical handler. An `.or_else` retry through `on_folding_range(json!({}))`
    /// flattens invalid, stale, and provider failures into empty success (#13981).
    #[test]
    fn production_folding_range_dispatch_does_not_retry_errors_as_empty()
    -> Result<(), Box<dyn std::error::Error>> {
        let source = include_str!("text_document.rs");
        let production_source = source.split("#[cfg(test)]\nmod tests").next().unwrap_or(source);
        let method_body = folding_range_dispatch_source(production_source)?;

        assert!(
            method_body.contains("self.handle_folding_range(params)"),
            "production foldingRange dispatch must call the canonical handler"
        );
        assert!(
            !method_body.contains(".or_else"),
            "production foldingRange dispatch must not regain an error-to-empty `.or_else` (#13981)\n{method_body}"
        );
        assert!(
            !method_body.contains("on_folding_range(json!({}))"),
            "production foldingRange dispatch must not replace errors with on_folding_range(json!({{}})) (#13981)\n{method_body}"
        );
        assert!(
            method_body.contains("#[cfg(any(test, feature = \"test-fallbacks\"))]"),
            "retained LSP_TEST_FALLBACKS path must stay cfg-gated (#13981)"
        );
        assert!(
            method_body.contains("std::env::var(\"LSP_TEST_FALLBACKS\")"),
            "test-only foldingRange fallback must remain behind LSP_TEST_FALLBACKS"
        );
        assert!(
            method_body.contains("advertised_features.lock().folding_range"),
            "test-only foldingRange fallback must not run when the feature is unadvertised (#13981)"
        );
        let handler_source = include_str!("../language/symbols.rs");
        let handler_production =
            handler_source.split("#[cfg(test)]\nmod tests").next().unwrap_or(handler_source);
        assert!(
            handler_production.contains(
                "#[cfg(any(test, feature = \"test-fallbacks\"))]\n    pub(crate) fn on_folding_range("
            ),
            "on_folding_range must stay cfg-gated out of production builds (#13981)"
        );
        Ok(())
    }

    /// Production definition dispatch must be a transparent adapter over the
    /// canonical handler. An `.or_else` retry through `on_definition(json!({}))`
    /// receives no URI or position and flattens cancelled, stale, invalid, and
    /// provider failures into an apparently-successful empty search (#5108).
    #[test]
    fn production_definition_dispatch_does_not_retry_errors_as_empty()
    -> Result<(), Box<dyn std::error::Error>> {
        let source = include_str!("text_document.rs");
        let production_source = source.split("#[cfg(test)]\nmod tests").next().unwrap_or(source);
        let method_body =
            dispatch_method_source(production_source, "handle_definition_cancellable_dispatch")?;

        assert!(
            method_body.contains("self.handle_definition_cancellable(params, id)"),
            "production definition dispatch must call the canonical handler (#5108)"
        );
        assert!(
            !method_body.contains(".or_else"),
            "production definition dispatch must not regain an error-to-empty `.or_else` (#5108)\n{method_body}"
        );
        assert!(
            !method_body.contains("on_definition(json!({}))"),
            "production definition dispatch must not replace errors with on_definition(json!({{}})) (#5108)\n{method_body}"
        );
        assert!(
            method_body.contains("#[cfg(any(test, feature = \"test-fallbacks\"))]"),
            "retained LSP_TEST_FALLBACKS path must stay cfg-gated (#4628)"
        );
        assert!(
            method_body.contains("std::env::var(\"LSP_TEST_FALLBACKS\")"),
            "test-only definition fallback must remain behind LSP_TEST_FALLBACKS"
        );
        assert!(
            method_body.contains("advertised_features.lock().definition"),
            "test-only definition fallback must not run when the feature is unadvertised (#4628)"
        );
        let handler_source = include_str!("../language/navigation.rs");
        let handler_production =
            handler_source.split("#[cfg(test)]\nmod tests").next().unwrap_or(handler_source);
        assert!(
            handler_production.contains(
                "#[cfg(any(test, feature = \"test-fallbacks\"))]\n    pub(crate) fn on_definition("
            ),
            "on_definition must stay cfg-gated out of production builds (#5108)"
        );
        Ok(())
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

    /// Behavioral test: a cancelled references request returns
    /// REQUEST_CANCELLED from the dispatch method, never a fallback result
    /// (#4628, #5108).
    #[test]
    #[serial_test::serial]
    fn references_dispatch_preserves_cancellation() -> Result<(), Box<dyn std::error::Error>> {
        // If LSP_TEST_FALLBACKS is set (by a parallel test), the test-fallback
        // branch intercepts and bypasses the production path we want to
        // exercise.
        if std::env::var("LSP_TEST_FALLBACKS").is_ok() {
            eprintln!("Skipping cancellation test: LSP_TEST_FALLBACKS is set");
            return Ok(());
        }

        let server = LspServer::new();
        let request_id = JsonRpcId::Integer(51080);
        let typed_id = request_id.clone();

        // The references handler consults the server-side cancelled set (not
        // the global token registry), so mark the request cancelled there.
        server.cancel_mark(&typed_id);

        let params = json!({
            "textDocument": {"uri": "file:///nonexistent.pl", "version": 1},
            "position": {"line": 0, "character": 0}
        });

        let result = server
            .handle_references_cancellable_dispatch(Some(params), Some(&request_id.to_value()));

        // Clean up the cancelled marker
        server.cancel_clear(&typed_id);

        match result {
            Err(error) => {
                assert_eq!(
                    error.code, REQUEST_CANCELLED,
                    "cancelled references request must return REQUEST_CANCELLED, not a fallback"
                );
            }
            Ok(Some(_)) => {
                return Err(
                    "cancelled references request returned a result instead of REQUEST_CANCELLED"
                        .into(),
                );
            }
            Ok(None) => {
                return Err(
                    "cancelled references request returned None instead of REQUEST_CANCELLED"
                        .into(),
                );
            }
        }

        Ok(())
    }

    /// Behavioral falsifier (#5108): a definition request with missing
    /// URI/position remains a request error; the dispatch layer must not
    /// flatten it into an apparently-successful empty search through the
    /// empty-params fallback.
    #[test]
    #[serial_test::serial]
    fn definition_dispatch_refuses_invalid_params() -> Result<(), Box<dyn std::error::Error>> {
        if std::env::var("LSP_TEST_FALLBACKS").is_ok() {
            eprintln!("Skipping invalid-params test: LSP_TEST_FALLBACKS is set");
            return Ok(());
        }

        let server = LspServer::new();
        let request_id = JsonRpcId::Integer(51081);

        let result = server
            .handle_definition_cancellable_dispatch(Some(json!({})), Some(&request_id.to_value()));

        match result {
            Err(error) => {
                assert_eq!(
                    error.code,
                    crate::protocol::INVALID_PARAMS,
                    "missing uri/position must remain a request error (#5108)"
                );
            }
            Ok(result) => {
                return Err(format!(
                    "invalid definition params must not become a successful empty result; got {result:?}"
                )
                .into());
            }
        }

        Ok(())
    }

    /// Behavioral falsifier (#5108): a references request with missing
    /// URI/position remains a request error; the eligibility predicate refuses
    /// it, so the compatibility fallback cannot relabel it as empty success.
    #[test]
    #[serial_test::serial]
    fn references_dispatch_refuses_invalid_params() -> Result<(), Box<dyn std::error::Error>> {
        if std::env::var("LSP_TEST_FALLBACKS").is_ok() {
            eprintln!("Skipping invalid-params test: LSP_TEST_FALLBACKS is set");
            return Ok(());
        }

        let server = LspServer::new();
        let request_id = JsonRpcId::Integer(51082);

        let result = server
            .handle_references_cancellable_dispatch(Some(json!({})), Some(&request_id.to_value()));

        match result {
            Err(error) => {
                assert_eq!(
                    error.code,
                    crate::protocol::INVALID_PARAMS,
                    "missing uri/position must remain a request error (#5108)"
                );
            }
            Ok(result) => {
                return Err(format!(
                    "invalid references params must not become a successful empty result; got {result:?}"
                )
                .into());
            }
        }

        Ok(())
    }

    /// Behavioral falsifier (#5108): an older request version returns
    /// CONTENT_MODIFIED from the definition dispatch, never an empty success.
    #[test]
    #[serial_test::serial]
    fn definition_dispatch_preserves_content_modified() -> Result<(), Box<dyn std::error::Error>> {
        if std::env::var("LSP_TEST_FALLBACKS").is_ok() {
            eprintln!("Skipping stale-request test: LSP_TEST_FALLBACKS is set");
            return Ok(());
        }

        let server = LspServer::new();
        let uri = "file:///5108-stale-definition.pl";
        server.test_apply_did_open(uri, "my $x = 1;\n", 2)?;

        let request_id = JsonRpcId::Integer(51083);
        let params = json!({
            "textDocument": {"uri": uri, "version": 1},
            "position": {"line": 0, "character": 4}
        });

        let result = server
            .handle_definition_cancellable_dispatch(Some(params), Some(&request_id.to_value()));

        match result {
            Err(error) => {
                assert_eq!(
                    error.code,
                    crate::protocol::CONTENT_MODIFIED,
                    "a stale definition request must return CONTENT_MODIFIED (#5108)"
                );
            }
            Ok(result) => {
                return Err(format!(
                    "stale definition request must not become a successful empty result; got {result:?}"
                )
                .into());
            }
        }

        Ok(())
    }

    /// Behavioral falsifier (#5108): an older request version returns
    /// CONTENT_MODIFIED from the references dispatch; the compatibility
    /// fallback must not swallow it.
    #[test]
    #[serial_test::serial]
    fn references_dispatch_preserves_content_modified() -> Result<(), Box<dyn std::error::Error>> {
        if std::env::var("LSP_TEST_FALLBACKS").is_ok() {
            eprintln!("Skipping stale-request test: LSP_TEST_FALLBACKS is set");
            return Ok(());
        }

        let server = LspServer::new();
        let uri = "file:///5108-stale-references.pl";
        server.test_apply_did_open(uri, "my $x = 1;\n$x;\n", 2)?;

        let request_id = JsonRpcId::Integer(51084);
        let params = json!({
            "textDocument": {"uri": uri, "version": 1},
            "position": {"line": 1, "character": 1},
            "context": {"includeDeclaration": true}
        });

        let result = server
            .handle_references_cancellable_dispatch(Some(params), Some(&request_id.to_value()));

        match result {
            Err(error) => {
                assert_eq!(
                    error.code,
                    crate::protocol::CONTENT_MODIFIED,
                    "a stale references request must return CONTENT_MODIFIED (#5108)"
                );
            }
            Ok(result) => {
                return Err(format!(
                    "stale references request must not become a successful empty result; got {result:?}"
                )
                .into());
            }
        }

        Ok(())
    }

    /// Unit test (#5108): the eligibility predicate must refuse every terminal
    /// protocol outcome; only non-terminal failures may take the retained
    /// compatibility fallback.
    #[test]
    fn references_fallback_eligibility_predicate_excludes_terminal_outcomes() {
        use crate::protocol::{
            CONTENT_MODIFIED, INTERNAL_ERROR, INVALID_PARAMS, INVALID_REQUEST, PARSE_ERROR,
            REQUEST_CANCELLED, REQUEST_FAILED, SERVER_CANCELLED,
        };

        for code in [
            REQUEST_CANCELLED,
            SERVER_CANCELLED, // cancelled
            CONTENT_MODIFIED, // stale
            PARSE_ERROR,
            INVALID_REQUEST,
            INVALID_PARAMS, // invalid
            INTERNAL_ERROR, // internal provider failure
        ] {
            assert!(
                !references_fallback_eligible(code),
                "terminal outcome {code} must never enter the references fallback (#5108)"
            );
        }

        // A non-terminal failure may take the retained fallback, which
        // answers from the exact original request.
        assert!(
            references_fallback_eligible(REQUEST_FAILED),
            "a non-terminal failure must stay fallback-eligible (#5108)"
        );
    }
}
