use super::{LspServer, Value, json};
#[cfg(all(feature = "workspace", not(target_arch = "wasm32")))]
use crate::runtime::readiness::IndexReadinessPolicy;
use perl_lsp_rs_core::providers::navigation::hover_shadow::{
    HoverCutoverOutcome, HoverCutoverResult, hover_cutover,
};
use perl_semantic_facts::ProviderFactSourceKind;

#[derive(Debug, Clone)]
pub(super) struct LiveHoverCompilerContext {
    uri: String,
    symbol: String,
    byte_offset: u32,
    /// Trace-only metadata from [`perl_parser_core::SourceRegionIndex`]; routing in #4967.
    #[expect(
        dead_code,
        reason = "policy:5003-pr1: trace substrate field for upcoming hover routing"
    )]
    source_region_kind: Option<String>,
}

impl LspServer {
    pub(super) fn live_hover_compiler_context(
        uri: &str,
        text: &str,
        offset: usize,
        source_region_kind: Option<String>,
    ) -> Option<LiveHoverCompilerContext> {
        let symbol = Self::get_token_at_position_static(text, offset);
        if symbol.is_empty() {
            return None;
        }

        let byte_offset = u32::try_from(offset).ok()?;
        Some(LiveHoverCompilerContext {
            uri: uri.to_string(),
            symbol,
            byte_offset,
            source_region_kind,
        })
    }

    pub(super) fn try_live_compiler_hover(
        &self,
        legacy_value: Option<&Value>,
        context: Option<&LiveHoverCompilerContext>,
    ) -> Option<Value> {
        let context = context?;
        let legacy_text = legacy_value.and_then(Self::hover_value_markdown);

        #[cfg(not(all(feature = "workspace", not(target_arch = "wasm32"))))]
        {
            let _ = (legacy_text, context);
            None
        }

        #[cfg(all(feature = "workspace", not(target_arch = "wasm32")))]
        {
            let _ = self.check_index_readiness(IndexReadinessPolicy::WaitBriefly);
            if self.workspace_index_stale_for_document(&context.uri) {
                return None;
            }
            let workspace_index = self.workspace_index()?;
            let outcome = workspace_index.with_semantic_queries_for_uri(
                &context.uri,
                |file_id, queries| {
                    hover_cutover(
                        legacy_text.clone(),
                        &queries,
                        &context.symbol,
                        file_id,
                        context.byte_offset,
                        None,
                    )
                },
            )?;

            if !Self::hover_outcome_uses_live_compiler_facts(&outcome) {
                return None;
            }

            match outcome.result {
                HoverCutoverResult::Exact(explanation)
                | HoverCutoverResult::Ambiguous(explanation)
                | HoverCutoverResult::DynamicBoundary(explanation) => {
                    if Self::should_preserve_legacy_hover(
                        legacy_text.as_deref(),
                        &explanation.markdown,
                    ) {
                        return None;
                    }
                    Some(Self::hover_markdown_value(explanation.markdown))
                }
                HoverCutoverResult::LegacyFallback(_) => None,
            }
        }
    }

    fn should_preserve_legacy_hover(legacy_text: Option<&str>, compiler_markdown: &str) -> bool {
        let Some(legacy_text) = legacy_text else {
            return false;
        };

        legacy_text.contains("**Moo/Moose Attribute Accessor**")
            && !compiler_markdown.contains("Moo/Moose Attribute Accessor")
    }

    fn hover_outcome_uses_live_compiler_facts(outcome: &HoverCutoverOutcome) -> bool {
        outcome.receipt.fact_source_traces.iter().any(|trace| {
            matches!(
                trace.source,
                ProviderFactSourceKind::CompilerFact
                    | ProviderFactSourceKind::FrameworkAdapter
                    | ProviderFactSourceKind::DynamicBoundary
            )
        })
    }

    fn hover_value_markdown(value: &Value) -> Option<String> {
        let contents = value.get("contents")?;
        if let Some(markdown) = contents.as_str() {
            return Some(markdown.to_string());
        }
        contents.get("value").and_then(Value::as_str).map(str::to_string)
    }

    fn hover_markdown_value(markdown: String) -> Value {
        json!({
            "contents": {
                "kind": "markdown",
                "value": markdown,
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_richer_moo_accessor_hover_over_generated_compiler_card()
    -> Result<(), Box<dyn std::error::Error>> {
        let legacy = "**Moo/Moose Attribute Accessor**\n\n**Attribute**: `email`\n**Type**: `Str`";
        let compiler =
            "**Symbol** `email` (generated)\n\nSource: framework adapter / framework synthesis";

        if !LspServer::should_preserve_legacy_hover(Some(legacy), compiler) {
            return Err("expected rich Moo/Moose accessor hover to be preserved".into());
        }

        Ok(())
    }

    #[test]
    fn uses_compiler_hover_when_it_keeps_accessor_attribution()
    -> Result<(), Box<dyn std::error::Error>> {
        let legacy = "**Moo/Moose Attribute Accessor**\n\n**Attribute**: `email`";
        let compiler = "**Moo/Moose Attribute Accessor**\n\n**Attribute**: `email`";

        if LspServer::should_preserve_legacy_hover(Some(legacy), compiler) {
            return Err("expected compiler hover with accessor attribution to be usable".into());
        }

        Ok(())
    }

    #[cfg(feature = "workspace")]
    fn hover_position_of(
        text: &str,
        needle: &str,
    ) -> Result<(u32, u32), Box<dyn std::error::Error>> {
        for (line_idx, line) in text.lines().enumerate() {
            if let Some(byte_offset) = line.find(needle) {
                let line_number = u32::try_from(line_idx)?;
                let character = line[..byte_offset].chars().map(char::len_utf16).sum::<usize>();
                let character = u32::try_from(character)?;
                return Ok((line_number, character));
            }
        }
        Err(format!("needle `{needle}` not found").into())
    }

    #[cfg(feature = "workspace")]
    fn hover_markdown(value: &Value) -> Option<String> {
        value.get("contents").and_then(|contents| {
            contents
                .as_str()
                .map(str::to_string)
                .or_else(|| contents.get("value").and_then(Value::as_str).map(str::to_string))
        })
    }

    /// Regression (#5016 item 2): generation N+1 open document must not drive
    /// live compiler hover from an indexed generation N workspace snapshot.
    #[cfg(feature = "workspace")]
    #[test]
    fn live_compiler_hover_skips_generation_stale_workspace_index()
    -> Result<(), Box<dyn std::error::Error>> {
        use std::fs;
        use tempfile::tempdir;

        let temp = tempdir()?;
        let workspace = temp.path().join("workspace");
        let module_dir = workspace.join("lib").join("StaleHover");
        fs::create_dir_all(&module_dir)?;
        let module_source = concat!(
            "package StaleHover::Exports;\n",
            "use Exporter 'import';\n",
            "our @EXPORT_OK = qw(exported);\n",
            "sub exported { 1 }\n",
            "1;\n",
        );
        fs::write(module_dir.join("Exports.pm"), module_source)?;

        let script = workspace.join("script.pl");
        let caller_v1 = "use lib 'lib';\nuse StaleHover::Exports qw(exported);\nexported();\n";
        let caller_v2 =
            "use lib 'lib';\nuse StaleHover::Exports qw(exported);\nexported(); # stale\n";
        fs::write(&script, caller_v1)?;

        let folder_uri = url::Url::from_directory_path(&workspace)
            .map_err(|()| "invalid workspace directory path")?
            .to_string();
        let module_uri = url::Url::from_file_path(module_dir.join("Exports.pm"))
            .map_err(|()| "invalid module file path")?
            .to_string();
        let caller_uri =
            url::Url::from_file_path(&script).map_err(|()| "invalid caller file path")?.to_string();

        let server = LspServer::default();
        server.test_set_root_path(workspace.clone());
        server.test_set_workspace_folder_uris(&[folder_uri.as_str()]);

        server.test_apply_did_open(&module_uri, module_source, 1)?;
        server.test_apply_did_open(&caller_uri, caller_v1, 1)?;
        server
            .test_index_file_in_building_state(&module_uri, module_source)
            .map_err(std::io::Error::other)?;
        server
            .test_index_file_in_building_state(&caller_uri, caller_v1)
            .map_err(std::io::Error::other)?;
        server.test_simulate_indexing_complete();

        let (line, character) = hover_position_of(caller_v1, "exported()")?;
        let fresh_hover = server
            .handle_hover(Some(json!({
                "textDocument": { "uri": &caller_uri },
                "position": { "line": line, "character": character },
            })))?
            .ok_or("missing fresh live-compiler hover")?;
        let fresh_markdown =
            hover_markdown(&fresh_hover).ok_or("fresh hover must include markdown contents")?;
        assert!(
            fresh_markdown.contains("Source: compiler fact"),
            "fresh workspace index should drive live compiler hover provenance: {fresh_markdown}"
        );

        server
            .test_replace_document_without_index(&caller_uri, caller_v2, 2)
            .map_err(std::io::Error::other)?;
        assert!(
            server.workspace_index_stale_for_document(&caller_uri),
            "test setup must leave the caller document newer than the workspace index"
        );

        let stale_hover = server
            .handle_hover(Some(json!({
                "textDocument": { "uri": &caller_uri },
                "position": { "line": line, "character": character },
            })))?
            .ok_or("missing stale live-compiler hover")?;
        let stale_markdown =
            hover_markdown(&stale_hover).ok_or("stale hover must include markdown contents")?;
        assert!(
            !stale_markdown.contains("Source: compiler fact"),
            "stale workspace index must not drive live compiler hover provenance: {stale_markdown}"
        );

        Ok(())
    }
}
