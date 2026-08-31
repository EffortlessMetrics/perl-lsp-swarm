//! Text document request handlers
//!
//! Wraps textDocument/* LSP requests.

#[cfg(test)]
use super::super::*;
use super::super::{JsonRpcError, LspServer, Value};
// `json!` is only used by the test-only fallback fast paths and the unit
// tests; both are compiled out of production builds (#4628, #5108).
#[cfg(any(test, feature = "test-fallbacks"))]
use super::super::json;

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

        // Production path: the canonical handler's outcome is terminal. The
        // outer references compatibility fallback is removed (#5108): dispatch
        // is an adapter, not a fallback planner, so cancelled, stale, invalid,
        // and provider failures reach the client at their typed JSON-RPC codes
        // instead of being relabeled as apparently-successful empty searches.
        self.handle_references_with_request_id(params, request_id)
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
        // must equal the number of env var reads plus the number of gated
        // test-only imports (the `json` import used only by the gated fast
        // paths and the unit tests).
        let cfg_count =
            production_source.matches("#[cfg(any(test, feature = \"test-fallbacks\"))]").count();
        let env_var_count =
            production_source.matches("std::env::var(\"LSP_TEST_FALLBACKS\")").count();
        let gated_import_count = production_source
            .matches("#[cfg(any(test, feature = \"test-fallbacks\"))]\nuse ")
            .count();

        assert_eq!(
            cfg_count,
            env_var_count + gated_import_count,
            "production code has {env_var_count} LSP_TEST_FALLBACKS env var reads and \
             {gated_import_count} gated test-only imports but {cfg_count} \
             #[cfg(any(test, feature = \"test-fallbacks\"))] gates — every read and import \
             must be gated (#4628)"
        );

        // Additionally verify there are no bare `let use_fallback =` patterns
        // (the old shape that read the env var unconditionally).
        assert!(
            !production_source.contains("let use_fallback ="),
            "production code must not contain `let use_fallback =` — \
             the old unconditional env var read pattern (#4628)"
        );
    }

    /// Static-analysis test: production references dispatch must be a
    /// transparent adapter over the canonical handler (#5108). The outer
    /// compatibility fallback is removed: dispatch is an adapter, not a
    /// fallback planner, so cancelled, stale, invalid, and provider failures
    /// reach the client at their typed JSON-RPC codes instead of being
    /// relabeled as apparently-successful empty searches.
    #[test]
    fn production_references_dispatch_does_not_retry_errors_as_empty()
    -> Result<(), Box<dyn std::error::Error>> {
        let source = include_str!("text_document.rs");
        let production_source = source.split("#[cfg(test)]\nmod tests").next().unwrap_or(source);
        let method_body =
            dispatch_method_source(production_source, "handle_references_cancellable_dispatch")?;

        assert!(
            method_body.contains("self.handle_references_with_request_id(params, request_id)"),
            "production references dispatch must tail-call the canonical handler (#5108)\n{method_body}"
        );
        assert!(
            !method_body.contains(".or_else"),
            "production references dispatch must not regain an error-to-empty `.or_else` retry \
             (#5108)\n{method_body}"
        );
        assert!(
            !method_body.contains("on_references(json!({})"),
            "production references dispatch must not replace errors with an empty-params \
             fallback (#5108)\n{method_body}"
        );
        assert!(
            method_body.contains("#[cfg(any(test, feature = \"test-fallbacks\"))]"),
            "retained LSP_TEST_FALLBACKS path must stay cfg-gated (#4628)"
        );
        assert!(
            method_body.contains("std::env::var(\"LSP_TEST_FALLBACKS\")"),
            "test-only references fallback must remain behind LSP_TEST_FALLBACKS"
        );
        assert!(
            method_body.contains("advertised_features.lock().references"),
            "test-only references fallback must not run when the feature is unadvertised (#4628)"
        );

        // The eligibility predicate and every reference to it are removed with
        // the fallback: no deny-list "fallback-eligible" planning may return
        // (#5108). Any future fallback must be an explicit allow-list, and
        // dispatch must remain an adapter. The needle is split so this
        // assertion does not reintroduce the literal it forbids.
        assert!(
            !source.contains(concat!("references_fallback_", "eligible")),
            "the removed references fallback eligibility predicate must stay removed: \
             dispatch is an adapter, not a fallback planner (#5108)"
        );

        // The test-only provider stays compiled out of production builds,
        // mirroring on_definition (#5108) and on_folding_range (#13981).
        let handler_source = include_str!("../language/references.rs");
        let handler_production =
            handler_source.split("#[cfg(test)]\nmod tests").next().unwrap_or(handler_source);
        assert!(
            handler_production.contains(
                "#[cfg(any(test, feature = \"test-fallbacks\"))]\n    pub(crate) fn on_references("
            ),
            "on_references must stay cfg-gated out of production builds (#5108)"
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

    /// Recurrence guard (#5108), strengthened: no production dispatch source
    /// may call a provider fallback with empty parameters — or synthesize
    /// empty parameters for one. An empty-parameter fallback receives no URI
    /// or position, can only answer `[]`, and therefore turns any failure it
    /// catches into an apparently-successful empty search.
    ///
    /// Unlike the original single-file literal scan, this guard:
    /// - walks every `.rs` file under `src/runtime/dispatch` (the original
    ///   defect could have landed in any dispatch file);
    /// - removes `#[cfg(any(test, feature = "test-fallbacks"))]`-gated blocks
    ///   first, so the sanctioned test-only fast paths stay exempt while the
    ///   remaining production text is scanned;
    /// - forbids any reference to the test-only fallback providers in
    ///   production dispatch text, so the empty-params object cannot hide in
    ///   a variable, a reformatting, or a defaulting expression;
    /// - matches whitespace-normalized needles, so the synthesis shapes the
    ///   removed fallback used (`params.clone().unwrap_or_else(|| json!({}))`)
    ///   cannot evade the scan by formatting.
    #[test]
    fn production_dispatch_has_no_empty_param_fallbacks() -> Result<(), Box<dyn std::error::Error>>
    {
        use std::{fs, path::Path};

        /// Remove `#[cfg(any(test, feature = "test-fallbacks"))]`-gated
        /// blocks. The attribute gates one brace-bearing item (the `if`
        /// fast path or a fallback `fn`); its extent is the first balanced
        /// brace group after the attribute. Braceless gated items are left
        /// in place rather than over-stripped.
        fn strip_cfg_gated_blocks(source: &str) -> String {
            const ATTR: &str = "#[cfg(any(test, feature = \"test-fallbacks\"))]";
            let mut kept = String::with_capacity(source.len());
            let mut rest = source;
            while let Some(at) = rest.find(ATTR) {
                kept.push_str(&rest[..at]);
                let after_attr = &rest[at + ATTR.len()..];
                let Some(open_rel) = after_attr.find('{') else {
                    kept.push_str(after_attr);
                    break;
                };
                if after_attr[..open_rel].contains(';') {
                    // The attribute gates a braceless item (e.g. `use ...;`);
                    // keep the text and keep scanning after this attribute.
                    kept.push_str(ATTR);
                    rest = after_attr;
                    continue;
                }
                let mut depth = 0usize;
                let mut block_end = None;
                for (idx, ch) in after_attr[open_rel..].char_indices() {
                    match ch {
                        '{' => depth += 1,
                        '}' => {
                            depth -= 1;
                            if depth == 0 {
                                block_end = Some(open_rel + idx + 1);
                                break;
                            }
                        }
                        _ => {}
                    }
                }
                rest = match block_end {
                    Some(end) => &after_attr[end..],
                    // Unbalanced remainder: treat all of it as gated.
                    None => "",
                };
            }
            kept.push_str(rest);
            kept
        }

        // Any reference to a test-only fallback provider in production
        // dispatch text is a violation, whatever the parameters are built
        // from (#5108).
        const FALLBACK_PROVIDERS: [&str; 3] =
            ["on_definition(", "on_references(", "on_folding_range("];
        // Empty-params synthesis in production dispatch is the defect shape
        // even without a provider call beside it. Needles are matched against
        // whitespace-stripped text so reformatting cannot evade them.
        const EMPTY_PARAM_SYNTHESIS: [&str; 3] =
            ["unwrap_or(json!({}))", "unwrap_or_else(||json!({}))", "unwrap_or_else(|_|json!({}))"];

        let dispatch_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/runtime/dispatch");
        let mut scanned = 0usize;
        let mut stack = vec![dispatch_dir];
        while let Some(dir) = stack.pop() {
            for entry in fs::read_dir(&dir)? {
                let path = entry?.path();
                if path.is_dir() {
                    // Directory-named test modules are test-only by
                    // construction; the scan is about production dispatch.
                    if path.file_name().and_then(|name| name.to_str()) == Some("tests") {
                        continue;
                    }
                    stack.push(path);
                    continue;
                }
                if path.extension().and_then(|ext| ext.to_str()) != Some("rs") {
                    continue;
                }
                let source = fs::read_to_string(&path)?;
                let production = source.split("#[cfg(test)]\nmod tests").next().unwrap_or(&source);
                let stripped = strip_cfg_gated_blocks(production);
                let compact: String = stripped.chars().filter(|c| !c.is_whitespace()).collect();
                for needle in FALLBACK_PROVIDERS.iter().chain(EMPTY_PARAM_SYNTHESIS.iter()) {
                    assert!(
                        !compact.contains(needle),
                        "production dispatch `{}` must not contain `{needle}` — dispatch is \
                         an adapter, not a fallback planner (#5108)",
                        path.display()
                    );
                }
                scanned += 1;
            }
        }
        assert!(
            scanned >= 14,
            "expected to scan every dispatch source file, only scanned {scanned}"
        );
        Ok(())
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
    /// URI/position remains a request error. The canonical INVALID_PARAMS
    /// error is constructed and must reach the client untouched — the
    /// dispatch layer has no compatibility fallback left to relabel it as an
    /// apparently-successful empty search.
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
    /// CONTENT_MODIFIED from the references dispatch. The canonical stale
    /// error propagates untouched — dispatch has no fallback left that could
    /// swallow it into an empty success.
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
}
