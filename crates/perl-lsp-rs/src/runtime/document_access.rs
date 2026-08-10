//! Document access, position conversion, and URI helpers.
//!
//! Methods that look up documents, normalize URIs, convert between byte
//! offsets and LSP positions, and provide text-based fallback extractors.

use super::{
    Arc, CONTENT_MODIFIED, DocumentState, HashMap, JsonRpcError, LspServer, LspWorkspaceSymbol,
    extract_module_reference, extract_module_reference_extended, extract_text_based_code_lenses,
    extract_text_based_symbols, get_text_around_offset, get_text_window_around_offset,
    offset_to_position, position_to_offset,
};

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
            if let Some(doc) = self.get_document(&documents, uri)
                && v < doc.version
            {
                return Err(Self::content_modified());
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
    /// intentionally kept out of the parser/index (for example templates opened
    /// under a non-Perl language id) are excluded only while they have never
    /// reached the index; a pending parse remains eligible because its last
    /// published AST is retained as the latest snapshot.
    ///
    /// Eligibility is deliberately asymmetric, because the two `indexed_generation`
    /// answers mean different things:
    ///
    /// - `Some(indexed)` — this URI already contributes cross-file symbols, so it
    ///   is stale until the index catches up with the edit, *regardless* of
    ///   whether the document still parses. A previously indexed document that is
    ///   edited into a hard parse failure, or past the oversized/binary guards in
    ///   `handle_did_change_with_cancellation`, keeps its pre-edit entry:
    ///   `run_post_parse_side_effects` only schedules re-indexing when the
    ///   snapshot has an AST, and it never removes the superseded entry. Dropping
    ///   such a document from the gate would report the workspace fresh while
    ///   go-to-definition still resolved through pre-edit symbols — exactly the
    ///   stale location this predicate exists to block.
    /// - `None` — this URI contributes nothing cross-file. It is stale only if it
    ///   was ever expected to reach the index (its latest snapshot has an AST);
    ///   an intentionally unindexed buffer cannot make the workspace stale, and
    ///   must not disable cross-file navigation for every other document.
    #[cfg(feature = "workspace")]
    pub(crate) fn workspace_index_stale_for_any_open_document(&self) -> bool {
        let Some(coordinator) = self.coordinator() else {
            return false;
        };

        for _attempt in 0..=1 {
            let sampled = self.edited_open_document_snapshot();

            let stale = sampled.iter().any(|(uri, (generation, expected_to_index))| {
                match coordinator.index().indexed_generation(uri) {
                    Some(indexed_generation) => indexed_generation < *generation,
                    None => *expected_to_index,
                }
            });

            // Re-validating the *whole* snapshot, not each sampled entry, is what
            // makes this sound. Checking only the sampled URIs would miss a
            // document that was opened and edited between the two passes: it is
            // absent from the sample, so no per-entry check covers it, and its
            // stale index entry would be reported as a fresh workspace.
            if self.edited_open_document_snapshot() == sampled {
                return stale;
            }
        }

        // The open-document set or one of its snapshots changed while both
        // validation passes were running. Fail closed so navigation cannot
        // consume an index snapshot from a state that was never proven stable.
        true
    }

    /// Every edited open document with the two facts the freshness comparison
    /// needs, keyed by URI.
    ///
    /// Keyed and ordered so that two snapshots compare by *membership* as well
    /// as by value: an opened, closed, or newly edited document changes the map
    /// even when every URI common to both passes is unchanged.
    #[cfg(feature = "workspace")]
    fn edited_open_document_snapshot(&self) -> std::collections::BTreeMap<String, (u32, bool)> {
        let documents = self.documents.lock();
        documents
            .iter()
            .filter_map(|(uri, document)| {
                let generation = document.current_generation();
                let expected_to_index =
                    document.latest_parsed().is_some_and(|snapshot| snapshot.ast().is_some());
                (generation > 0).then(|| (uri.clone(), (generation, expected_to_index)))
            })
            .collect()
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
        docs.get(&normalized).map(|d| d.text_arc.to_string())
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
        docs.iter().map(|(uri, doc)| (uri.clone(), doc.text_arc.to_string())).collect()
    }
}

#[cfg(all(test, feature = "workspace"))]
mod tests {
    use crate::runtime::LspServer;
    use serde_json::json;

    #[test]
    fn workspace_index_stale_for_document_false_when_document_is_not_open() {
        let server = LspServer::new();

        assert!(
            !server.workspace_index_stale_for_document("file:///workspace/missing.pl"),
            "missing open document must not be treated as stale"
        );
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

        assert!(
            !server.workspace_index_stale_for_document(uri),
            "missing coordinator must fail closed to non-stale rather than blocking local providers"
        );

        Ok(())
    }

    #[test]
    fn workspace_index_stale_for_document_boundary_discriminator_document_generation_zero()
    -> Result<(), Box<dyn std::error::Error>> {
        let server = LspServer::new();
        let uri = "file:///workspace/fresh-open.pl";
        let text = "my $value = 1;\n";

        server.test_apply_did_open(uri, text, 1)?;

        assert_eq!(
            server.document_generation(uri),
            Some(0),
            "didOpen must start at document_generation == 0 before any didChange"
        );
        assert!(
            !server.workspace_index_stale_for_document(uri),
            "document_generation == 0 must never be reported as stale"
        );

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

        let target_v2 = "package Target;\nsub new_target {}\n";
        server
            .test_replace_document_without_index(target_uri, target_v2, 2)
            .map_err(std::io::Error::other)?;

        assert!(
            !server.workspace_index_stale_for_document(caller_uri),
            "the unchanged caller must not be reported stale by the per-document helper"
        );
        assert!(
            server.workspace_index_stale_for_any_open_document(),
            "an edited definition target must block cross-file index navigation"
        );

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

        assert!(
            !server.workspace_index_stale_for_any_open_document(),
            "a current indexed snapshot must not block navigation"
        );

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

        assert!(
            !server.workspace_index_stale_for_any_open_document(),
            "missing coordinator must not block navigation"
        );

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

        assert!(
            server.workspace_index_stale_for_any_open_document(),
            "an eligible document without an indexed snapshot must be stale"
        );

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

        assert!(
            !server.workspace_index_stale_for_any_open_document(),
            "generation zero without an indexed snapshot must not be stale"
        );

        Ok(())
    }

    #[test]
    fn workspace_index_stale_for_any_open_document_ignores_intentionally_unindexed_document()
    -> Result<(), Box<dyn std::error::Error>> {
        let server = LspServer::new();
        let uri = "file:///workspace/templates/welcome.html.ep";
        let opened = json!({
            "textDocument": {
                "uri": uri,
                "languageId": "html",
                "version": 1,
                "text": "<div><%= $name %></div>"
            }
        });
        let changed = json!({
            "textDocument": { "uri": uri, "version": 2 },
            "contentChanges": [{ "text": "<div><%= $title %></div>" }]
        });

        server.test_handle_did_open(Some(opened))?;
        server.test_handle_did_change(Some(changed))?;

        assert!(
            !server.workspace_index_stale_for_any_open_document(),
            "intentionally unindexed documents must not block navigation"
        );

        Ok(())
    }

    /// Boundary discriminator for `indexed_generation < generation`: an index
    /// entry that is *ahead* of the open document (a workspace scan committed a
    /// newer on-disk revision than the buffer has reached) is not stale. A `!=`
    /// comparison here would block cross-file navigation for the whole server.
    #[test]
    fn workspace_index_stale_for_any_open_document_allows_index_ahead_of_document()
    -> Result<(), Box<dyn std::error::Error>> {
        let server = LspServer::new();
        let uri = "file:///workspace/index-ahead.pl";
        let text = "package IndexAhead;\nsub target {}\n";

        server.test_apply_did_open(uri, text, 1)?;
        server.test_replace_document_without_index(uri, text, 2).map_err(std::io::Error::other)?;

        let coordinator = server
            .index_coordinator
            .as_ref()
            .ok_or("test server must have an index coordinator")?;
        coordinator
            .index()
            .index_file_with_generation(url::Url::parse(uri)?, text.to_string(), 7)
            .map_err(std::io::Error::other)?;

        assert_eq!(
            server.document_generation(uri),
            Some(1),
            "the edited document must be at generation 1"
        );
        assert_eq!(
            coordinator.index().indexed_generation(uri),
            Some(7),
            "the index entry must be ahead of the open document"
        );
        assert!(
            !server.workspace_index_stale_for_any_open_document(),
            "an index entry ahead of the open document must not be stale"
        );

        Ok(())
    }

    /// A document that was already contributing cross-file symbols and is then
    /// edited past the binary guard in `handle_did_change_with_cancellation`
    /// keeps its pre-edit workspace-index entry: that path bumps the generation,
    /// republishes an AST-less `DocumentState`, and returns before scheduling
    /// any re-index, and nothing removes the superseded entry. If the freshness
    /// gate dropped such a document because its latest snapshot has no AST, it
    /// would report the workspace fresh while go-to-definition still resolved
    /// through the pre-edit symbols.
    #[test]
    fn workspace_index_stale_for_any_open_document_retains_indexed_document_that_stops_parsing()
    -> Result<(), Box<dyn std::error::Error>> {
        let server = LspServer::new();
        let uri = "file:///workspace/becomes-unparseable.pl";
        let text = "package BecomesUnparseable;\nsub target {}\n";

        server.test_apply_did_open(uri, text, 1)?;
        let coordinator = server
            .index_coordinator
            .as_ref()
            .ok_or("test server must have an index coordinator")?;
        coordinator
            .index()
            .index_file_with_generation(url::Url::parse(uri)?, text.to_string(), 0)
            .map_err(std::io::Error::other)?;

        // Real production edit that drives the binary guard: generation is
        // bumped, `parsed` is reset to `None`, and no re-index is scheduled.
        // Uses dense NUL content (>5% ratio) to trigger the binary guard
        // under the ratio heuristic (#5209).
        let changed = json!({
            "textDocument": { "uri": uri, "version": 2 },
            "contentChanges": [{ "text": "package BecomesUnparseable;\u{0000}\u{0000}\u{0000}\u{0000}\u{0000}\u{0000}\u{0000}\u{0000}\n" }]
        });
        server.test_handle_did_change(Some(changed))?;

        assert_eq!(
            server.document_generation(uri),
            Some(1),
            "the edit must advance the open document generation"
        );
        assert_eq!(
            coordinator.index().indexed_generation(uri),
            Some(0),
            "the pre-edit index entry must still be present at its older generation"
        );
        assert!(
            server.workspace_index_stale_for_any_open_document(),
            "an indexed document edited into an unindexable state must remain stale"
        );

        Ok(())
    }

    /// Regression: the freshness re-validation must compare the whole
    /// open-document snapshot, not just the entries it sampled first.
    ///
    /// A document opened and edited between the two passes is absent from the
    /// sample, so a per-entry re-check reports "all sampled documents still
    /// current" and the predicate returns a staleness verdict computed without
    /// it. This pins both halves of that claim on the same fixture: every
    /// originally sampled entry is still current (the weaker per-entry check
    /// passes), while the full snapshot differs (the membership check catches
    /// it), so the second document's stale index entry cannot slip through.
    #[test]
    fn edited_open_document_snapshot_detects_membership_added_between_passes()
    -> Result<(), Box<dyn std::error::Error>> {
        let server = LspServer::new();
        let first_uri = "file:///workspace/membership-first.pl";
        let first_text = "package MembershipFirst;\nsub target {}\n";
        let late_uri = "file:///workspace/membership-late.pl";
        let late_text = "package MembershipLate;\nsub target {}\n";

        server.test_apply_did_open(first_uri, first_text, 1)?;
        server
            .test_replace_document_without_index(first_uri, first_text, 2)
            .map_err(std::io::Error::other)?;

        // Pass one samples only `first_uri`.
        let sampled = server.edited_open_document_snapshot();
        assert!(
            sampled.contains_key(first_uri),
            "the edited first document must be in the sampled snapshot"
        );
        assert!(
            !sampled.contains_key(late_uri),
            "the late document must not exist yet when the first pass samples"
        );

        // A document is opened and edited while the predicate is between passes.
        server.test_apply_did_open(late_uri, late_text, 1)?;
        server
            .test_replace_document_without_index(late_uri, late_text, 2)
            .map_err(std::io::Error::other)?;

        let revalidated = server.edited_open_document_snapshot();

        // The weaker per-entry re-check every sampled URI is unchanged would
        // pass here, which is exactly why it was not sufficient.
        assert!(
            sampled.iter().all(|(uri, facts)| revalidated.get(uri) == Some(facts)),
            "every originally sampled entry is still current, so a per-entry \
             re-check cannot see the late document"
        );

        // Whole-snapshot comparison is what actually catches it.
        assert_ne!(
            revalidated, sampled,
            "a document opened and edited between passes must change the snapshot \
             so the predicate retries instead of answering without it"
        );
        assert!(
            revalidated.contains_key(late_uri),
            "the late document must be present in the re-validation snapshot"
        );

        Ok(())
    }
}
