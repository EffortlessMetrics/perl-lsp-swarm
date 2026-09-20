use super::{LspServer, Node};
use crate::runtime::document_symbols_sink::{DocumentSymbolIdentity, DocumentSymbolsDisposition};
use crate::runtime::parse_effect_contract::ParseEffectCommitOutcomeV1;

impl LspServer {
    /// Extract the exact symbol set for one accepted parse ticket and commit
    /// it through the document-symbol sink (#11674). The store mutation is
    /// sink-local: currency is validated at the boundary, not before this
    /// helper was called.
    pub(crate) fn commit_document_symbols_from_ast(
        &self,
        identity: &DocumentSymbolIdentity,
        ast: &Node,
        source: &str,
    ) -> ParseEffectCommitOutcomeV1 {
        // Test seam mirroring #11673: lets a falsifier mutate document state
        // between extraction and the sink-boundary mutation.
        #[cfg(test)]
        if let Some(hook) = self.document_symbols_before_commit_hook.lock().as_ref() {
            hook();
        }
        let extractor = crate::symbol::SymbolExtractor::new_with_source(source);
        let table = extractor.extract(ast);
        let symbols = table.symbols.keys().cloned().collect::<Vec<_>>();
        self.commit_document_symbols(identity, DocumentSymbolsDisposition::Replace(symbols))
    }

    /// Commit a parse-derived clear (parse failure / no-AST policy for the
    /// ticket's generation) through the document-symbol sink.
    pub(crate) fn clear_document_symbols_for_identity(
        &self,
        identity: &DocumentSymbolIdentity,
    ) -> ParseEffectCommitOutcomeV1 {
        // Test seam: same interposition point as replacement commits.
        #[cfg(test)]
        if let Some(hook) = self.document_symbols_before_commit_hook.lock().as_ref() {
            hook();
        }
        self.commit_document_symbols(identity, DocumentSymbolsDisposition::Clear)
    }

    /// Lifecycle-owned removal for didClose eviction sweeps. This is NOT a
    /// parse-derived candidate: the document has already left the map, so no
    /// accepted ticket can validate against it. Every parser-triggered path
    /// must use [`Self::commit_document_symbols`] /
    /// [`Self::clear_document_symbols_for_identity`] instead.
    pub(crate) fn clear_document_symbols(&self, uri: &str) {
        self.symbol_index.lock().remove_document(uri);
    }
}
