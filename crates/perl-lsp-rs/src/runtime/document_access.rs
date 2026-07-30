//! Document access, position conversion, and URI helpers.
//!
//! Methods that look up documents, normalize URIs, convert between byte
//! offsets and LSP positions, and provide text-based fallback extractors.

use super::*;

#[allow(dead_code)]
impl LspServer {
    // === BEGIN_TEST_ONLY_POSITION_HELPERS ===
    /// Convert offset to line/column position (UTF-16 aware, CRLF safe)
    #[allow(deprecated)]
    pub fn offset_to_position(&self, content: &str, offset: usize) -> (u32, u32) {
        // Implementation moved to lsp/utils
        let p = offset_to_position(content, offset);
        (p.line, p.character)
    }

    /// Convert line/column position to offset (UTF-16 aware, CRLF safe)
    #[allow(deprecated)]
    pub fn position_to_offset(&self, content: &str, line: u32, character: u32) -> usize {
        // Implementation moved to lsp/utils
        position_to_offset(content, line, character).unwrap_or(content.len())
    }
    // === END_TEST_ONLY_POSITION_HELPERS ===

    /// Position conversion using cached line starts for O(log n) performance
    #[inline]
    pub(crate) fn pos16_to_offset(&self, doc: &DocumentState, line: u32, ch: u32) -> usize {
        // Uses the cached, CRLF/UTF-16 aware converter
        doc.line_starts.position_to_offset_rope(&doc.rope, line, ch)
    }

    /// Normalize URI key for consistent document lookup
    pub(crate) fn normalize_uri_key(&self, raw: &str) -> String {
        perl_uri::uri_key(raw)
    }

    /// Get document by URI with normalization fallback
    pub(crate) fn get_document<'a>(
        &self,
        documents: &'a parking_lot::MutexGuard<'_, HashMap<String, DocumentState>>,
        uri: &str,
    ) -> Option<&'a DocumentState> {
        let normalized = self.normalize_uri_key(uri);
        documents.get(&normalized).or_else(|| documents.get(uri))
    }

    /// Get mutable document by URI with normalization fallback
    pub(crate) fn get_document_mut<'a>(
        &self,
        documents: &'a mut parking_lot::MutexGuard<'_, HashMap<String, DocumentState>>,
        uri: &str,
    ) -> Option<&'a mut DocumentState> {
        let normalized = self.normalize_uri_key(uri);
        if documents.contains_key(&normalized) {
            documents.get_mut(&normalized)
        } else {
            documents.get_mut(uri)
        }
    }

    /// Helper to create a ContentModified error response
    pub(crate) fn content_modified() -> JsonRpcError {
        JsonRpcError {
            code: CONTENT_MODIFIED,
            message: "Document changed before request executed".to_string(),
            data: None,
        }
    }

    /// Ensure the request version matches the current document version
    pub(crate) fn ensure_latest(
        &self,
        uri: &str,
        req_version: Option<i32>,
    ) -> Result<(), JsonRpcError> {
        if let Some(v) = req_version {
            let documents = self.documents.lock();
            if let Some(doc) = self.get_document(&documents, uri) {
                if v < doc.version {
                    return Err(Self::content_modified());
                }
            }
        }
        Ok(())
    }

    /// Whether the workspace index snapshot for `uri` is older than the open
    /// document generation.
    pub(crate) fn workspace_index_stale_for_document(&self, uri: &str) -> bool {
        #[cfg(feature = "workspace")]
        {
            let document_generation = {
                let documents = self.documents.lock();
                self.get_document(&documents, uri).map(DocumentState::current_generation)
            };
            let Some(document_generation) = document_generation else {
                return false;
            };
            if document_generation == 0 {
                return false;
            }
            let Some(coordinator) = self.coordinator() else {
                return false;
            };
            coordinator.index().is_index_generation_stale(uri, document_generation)
        }

        #[cfg(not(feature = "workspace"))]
        {
            let _ = uri;
            false
        }
    }

    /// Whether any edited open document lacks a current workspace-index snapshot.
    ///
    /// A cross-file provider can query an unchanged caller while the definition
    /// file is still waiting for its asynchronous index update. Checking only
    /// the caller's URI therefore does not establish that the workspace snapshot
    /// is safe for cross-file navigation. Generation zero is the intentional
    /// `didOpen` baseline and is not considered stale here. Documents that were
    /// intentionally kept out of the parser/index (for example templates or
    /// binary/oversized buffers) are also excluded; a pending parse remains
    /// eligible because its last published AST is retained as the latest snapshot.
    #[cfg(feature = "workspace")]
    pub(crate) fn workspace_index_stale_for_any_open_document(&self) -> bool {
        let Some(coordinator) = self.coordinator() else {
            return false;
        };

        for _attempt in 0..=1 {
            let document_generations = {
                let documents = self.documents.lock();
                documents
                    .iter()
                    .filter_map(|(uri, document)| {
                        let generation = document.current_generation();
                        let expected_to_index = document
                            .latest_parsed()
                            .is_some_and(|snapshot| snapshot.ast().is_some());
                        (generation > 0 && expected_to_index).then(|| (uri.clone(), generation))
                    })
                    .collect::<Vec<_>>()
            };

            let stale = document_generations.iter().any(|(uri, generation)| {
                match coordinator.index().indexed_generation(uri) {
                    Some(indexed_generation) => indexed_generation < *generation,
                    None => true,
                }
            });

            let documents = self.documents.lock();
            let snapshot_is_current = document_generations.iter().all(|(uri, generation)| {
                documents.get(uri).is_some_and(|document| {
                    document.current_generation() == *generation
                        && document.latest_parsed().is_some_and(|snapshot| snapshot.ast().is_some())
                })
            });
            if snapshot_is_current {
                return stale;
            }
        }

        // A document changed while both validation passes were running. Fail
        // closed so navigation cannot consume an index snapshot from a
        // generation that was never proven stable.
        true
    }

    /// Offset to position conversion using cached line starts for O(log n) performance
    #[inline]
    pub(crate) fn offset_to_pos16(&self, doc: &DocumentState, offset: usize) -> (u32, u32) {
        doc.line_starts.offset_to_position_rope(&doc.rope, offset)
    }

    /// Extract code lenses from text when AST parsing fails
    pub(crate) fn extract_text_based_code_lenses(
        &self,
        text: &str,
        uri: &str,
    ) -> Vec<crate::code_lens_provider::CodeLens> {
        extract_text_based_code_lenses(text, uri)
    }

    /// Extract symbols from text when AST parsing fails
    #[cfg(feature = "workspace")]
    pub(crate) fn extract_text_based_symbols(
        &self,
        text: &str,
        uri: &str,
        query: &str,
    ) -> Vec<LspWorkspaceSymbol> {
        extract_text_based_symbols(text, uri, query)
    }

    /// Extract symbols stub when workspace feature is disabled
    #[cfg(not(feature = "workspace"))]
    pub(crate) fn extract_text_based_symbols(
        &self,
        _text: &str,
        _uri: &str,
        _query: &str,
    ) -> Vec<serde_json::Value> {
        Vec::new()
    }

    /// Get text around an offset position
    pub(crate) fn get_text_around_offset(
        &self,
        content: &str,
        offset: usize,
        radius: usize,
    ) -> String {
        get_text_around_offset(content, offset, radius)
    }

    /// Get text around an offset and return the adjusted byte start.
    pub(crate) fn get_text_window_around_offset(
        &self,
        content: &str,
        offset: usize,
        radius: usize,
    ) -> (usize, String) {
        get_text_window_around_offset(content, offset, radius)
    }

    /// Extract module reference from text (e.g., from "use Module::Name" or "require Module::Name")
    pub(crate) fn extract_module_reference(&self, text: &str, cursor_pos: usize) -> Option<String> {
        extract_module_reference(text, cursor_pos)
    }

    /// Extract module reference including `use parent`/`use base` argument modules.
    pub(crate) fn extract_module_reference_extended(
        &self,
        text: &str,
        cursor_pos: usize,
    ) -> Option<String> {
        extract_module_reference_extended(text, cursor_pos)
    }

    /// Get buffer text for a URI
    ///
    /// Normalizes the URI via `normalize_uri_key` before lookup so that
    /// client-supplied URIs with e.g. uppercase Windows drive letters resolve
    /// correctly against the normalized keys used when documents are stored.
    pub(crate) fn buffer_text(&self, uri: &str) -> Option<String> {
        let docs = self.documents.lock();
        let normalized = self.normalize_uri_key(uri);
        docs.get(&normalized).map(|d| d.text.clone())
    }

    /// Current document generation counter for `uri`, if the document is open.
    ///
    /// The generation atomic is bumped on every `update_content` call (see
    /// [`DocumentState::update_content`]). It is the canonical "the buffer
    /// has changed" signal used to detect stale read requests in the
    /// scheduler — distinct from the LSP-supplied `version`, which is
    /// client-controlled.
    ///
    /// Normalizes the URI so that stale-read cancellation works even when the
    /// client supplies a non-canonical URI (e.g. uppercase drive letter on Windows).
    pub(crate) fn document_generation(&self, uri: &str) -> Option<u32> {
        let docs = self.documents.lock();
        let normalized = self.normalize_uri_key(uri);
        docs.get(&normalized).map(|d| d.generation.load(std::sync::atomic::Ordering::SeqCst))
    }

    /// Capture the document generation, version, and instance identity under
    /// one document-store lock acquisition.
    pub(crate) fn document_freshness(
        &self,
        uri: &str,
    ) -> Option<(u32, i32, Arc<std::sync::atomic::AtomicU32>)> {
        let docs = self.documents.lock();
        let normalized = self.normalize_uri_key(uri);
        docs.get(&normalized).map(|doc| {
            (
                doc.generation.load(std::sync::atomic::Ordering::SeqCst),
                doc.version,
                Arc::clone(&doc.generation),
            )
        })
    }

    /// Current LSP document version for `uri`, if the document is open.
    ///
    /// Normalizes the URI so the lookup aligns with the normalized keys used
    /// when documents are stored in `text_sync.rs`.
    pub(crate) fn document_version(&self, uri: &str) -> Option<i32> {
        let docs = self.documents.lock();
        let normalized = self.normalize_uri_key(uri);
        docs.get(&normalized).map(|d| d.version)
    }

    /// Iterate over all open buffers (for reference search)
    pub(crate) fn iter_open_buffers(&self) -> Vec<(String, String)> {
        let docs = self.documents.lock();
        docs.iter().map(|(uri, doc)| (uri.clone(), doc.text.clone())).collect()
    }
}

#[cfg(all(test, feature = "workspace"))]
mod tests {
    use crate::runtime::LspServer;
    use serde_json::json;

    #[test]
    fn workspace_index_stale_for_document_false_when_document_is_not_open()
    -> Result<(), Box<dyn std::error::Error>> {
        let server = LspServer::new();

        if server.workspace_index_stale_for_document("file:///workspace/missing.pl") {
            return Err("missing open document must not be treated as stale".into());
        }
        Ok(())
    }

    #[test]
    fn workspace_index_stale_for_document_false_when_coordinator_is_absent()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut server = LspServer::new();
        let uri = "file:///workspace/no-coordinator.pl";
        let text = "my $value = 1;\n";

        server.test_apply_did_open(uri, text, 1)?;
        server.test_replace_document_without_index(uri, text, 2).map_err(std::io::Error::other)?;
        server.index_coordinator = None;

        if server.workspace_index_stale_for_document(uri) {
            return Err(
                "missing coordinator must fail closed to non-stale rather than blocking local providers"
                    .into(),
            );
        }

        Ok(())
    }

    #[test]
    fn workspace_index_stale_for_document_boundary_discriminator_document_generation_zero()
    -> Result<(), Box<dyn std::error::Error>> {
        let server = LspServer::new();
        let uri = "file:///workspace/fresh-open.pl";
        let text = "my $value = 1;\n";

        server.test_apply_did_open(uri, text, 1)?;

        if server.document_generation(uri) != Some(0) {
            return Err(
                "didOpen must start at document_generation == 0 before any didChange".into()
            );
        }
        if server.workspace_index_stale_for_document(uri) {
            return Err("document_generation == 0 must never be reported as stale".into());
        }

        Ok(())
    }

    #[test]
    fn workspace_index_stale_for_any_open_document_detects_stale_target()
    -> Result<(), Box<dyn std::error::Error>> {
        let server = LspServer::new();
        let caller_uri = "file:///workspace/caller.pl";
        let target_uri = "file:///workspace/target.pl";
        let target_v1 = "package Target;\nsub old_target {}\n";

        server.test_apply_did_open(caller_uri, "Target::old_target();\n", 1)?;
        server.test_apply_did_open(target_uri, target_v1, 1)?;

        let coordinator = server
            .index_coordinator
            .as_ref()
            .ok_or("test server must have an index coordinator")?;
        coordinator
            .index()
            .index_file_with_generation(url::Url::parse(target_uri)?, target_v1.to_string(), 0)
            .map_err(std::io::Error::other)?;

        server
            .test_replace_document_without_index(
                target_uri,
                "package Target;\nsub new_target {}\n",
                2,
            )
            .map_err(std::io::Error::other)?;

        if server.workspace_index_stale_for_document(caller_uri) {
            return Err(
                "the unchanged caller must not be reported stale by the per-document helper".into(),
            );
        }
        if !server.workspace_index_stale_for_any_open_document() {
            return Err("an edited definition target must block cross-file index navigation".into());
        }

        Ok(())
    }

    #[test]
    fn workspace_index_stale_for_any_open_document_is_false_when_all_snapshots_are_current()
    -> Result<(), Box<dyn std::error::Error>> {
        let server = LspServer::new();
        let target_uri = "file:///workspace/current-target.pl";
        let target_text = "package Target;\nsub current_target {}\n";

        server.test_apply_did_open(target_uri, target_text, 1)?;
        server
            .test_replace_document_without_index(target_uri, target_text, 2)
            .map_err(std::io::Error::other)?;

        let coordinator = server
            .index_coordinator
            .as_ref()
            .ok_or("test server must have an index coordinator")?;
        coordinator
            .index()
            .index_file_with_generation(url::Url::parse(target_uri)?, target_text.to_string(), 1)
            .map_err(std::io::Error::other)?;

        if server.workspace_index_stale_for_any_open_document() {
            return Err("a current indexed snapshot must not block navigation".into());
        }

        Ok(())
    }

    #[test]
    fn workspace_index_stale_for_any_open_document_is_false_without_coordinator()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut server = LspServer::new();
        let uri = "file:///workspace/no-cross-file-index.pl";
        server.test_apply_did_open(uri, "my $value = 1;\n", 1)?;
        server
            .test_replace_document_without_index(uri, "my $value = 2;\n", 2)
            .map_err(std::io::Error::other)?;
        server.index_coordinator = None;

        if server.workspace_index_stale_for_any_open_document() {
            return Err("missing coordinator must not block navigation".into());
        }
        Ok(())
    }

    #[test]
    fn workspace_index_stale_for_any_open_document_detects_missing_snapshot()
    -> Result<(), Box<dyn std::error::Error>> {
        let server = LspServer::new();
        let uri = "file:///workspace/missing-snapshot.pl";
        let text = "package MissingSnapshot;\nsub target {}\n";

        server.test_apply_did_open(uri, text, 1)?;
        let coordinator = server
            .index_coordinator
            .as_ref()
            .ok_or("test server must have an index coordinator")?;
        coordinator.index().remove_file(uri);
        server.test_replace_document_without_index(uri, text, 2).map_err(std::io::Error::other)?;

        if !server.workspace_index_stale_for_any_open_document() {
            return Err("an eligible document without an indexed snapshot must be stale".into());
        }
        Ok(())
    }

    #[test]
    fn workspace_index_stale_for_any_open_document_ignores_generation_zero_without_snapshot()
    -> Result<(), Box<dyn std::error::Error>> {
        let server = LspServer::new();
        let uri = "file:///workspace/unindexed-open.pl";
        let text = "package UnindexedOpen;\nsub target {}\n";

        server.test_apply_did_open(uri, text, 1)?;
        let coordinator = server
            .index_coordinator
            .as_ref()
            .ok_or("test server must have an index coordinator")?;
        coordinator.index().remove_file(uri);

        if server.workspace_index_stale_for_any_open_document() {
            return Err("generation zero without an indexed snapshot must not be stale".into());
        }
        Ok(())
    }

    #[test]
    fn workspace_index_stale_for_any_open_document_ignores_intentionally_unindexed_document()
    -> Result<(), Box<dyn std::error::Error>> {
        let server = LspServer::new();
        let uri = "file:///workspace/templates/welcome.html.ep";

        server.test_handle_did_open(Some(json!({
            "textDocument": {
                "uri": uri,
                "languageId": "html",
                "version": 1,
                "text": "<div><%= $name %></div>"
            }
        })))?;
        server.test_handle_did_change(Some(json!({
            "textDocument": { "uri": uri, "version": 2 },
            "contentChanges": [{ "text": "<div><%= $title %></div>" }]
        })))?;

        if server.workspace_index_stale_for_any_open_document() {
            return Err("intentionally unindexed documents must not block navigation".into());
        }
        Ok(())
    }
}
