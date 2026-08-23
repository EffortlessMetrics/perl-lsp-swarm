//! Accepted-ticket document-symbol store sink (#11674).
//!
//! Every parser-triggered local symbol replacement/clear commits through
//! [`LspServer::commit_document_symbols`]: under one sink-local critical
//! section the boundary (1) re-validates the candidate's accepted parse
//! ticket against the live document instance + generation, (2) compares and
//! records the committed symbol ticket and a monotonic per-URI sequence, and
//! (3) atomically replaces or clears the complete local result while still
//! inside the section.
//!
//! The committed record is also the accepted-result identity anchor for
//! #6729's document-symbol result-ID row: a future cache must key off this
//! exact ticket identity, never URI/content/pointer/generation alone. No
//! result-ID consumer exists on current main (the `textDocument/documentSymbol`
//! request path computes live), so this claim only records the identity.
//!
//! Lock order: sink lock -> documents lock (brief validation read) and sink
//! lock -> `symbol_index` lock (the mutation itself). No path acquires either
//! pair in reverse order.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::AtomicU32;

use parking_lot::Mutex;

use super::LspServer;

/// Accepted parse state a document-symbol candidate was derived from. Same
/// identity shape as the push-diagnostics sink (#11673) and
/// `parse_worker::PublishedParseTicket`.
#[derive(Clone)]
pub(crate) struct DocumentSymbolIdentity {
    pub(crate) normalized_uri: String,
    pub(crate) document_instance: Arc<AtomicU32>,
    pub(crate) generation: u32,
}

impl DocumentSymbolIdentity {
    pub(crate) fn for_document(
        normalized_uri: &str,
        document_instance: &Arc<AtomicU32>,
        generation: u32,
    ) -> Self {
        Self {
            normalized_uri: normalized_uri.to_string(),
            document_instance: Arc::clone(document_instance),
            generation,
        }
    }
}

/// Whether the candidate replaces the complete local symbol set with an exact
/// extraction or clears it (parse failure / no-parser-state policy).
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum DocumentSymbolsDisposition {
    Replace(Vec<String>),
    Clear,
}

/// Exact outcome of one sink-boundary symbol commit attempt. Claim-local
/// vocabulary (#11674), shaped for retargeting onto #11672's
/// `ParseEffectCommitOutcome` family when that contract lands. The local
/// store mutation cannot fail on its own, so there is no transport outcome.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DocumentSymbolCommitOutcome {
    /// Current exact replacement installed atomically.
    CommittedCurrent,
    /// Current clear installed atomically.
    SafeClearCommitted,
    /// Document absent from the map: closed entirely or never opened.
    RejectedDocumentClosed,
    /// Live document exists but is a different instance than the candidate's
    /// (close/reopen ABA).
    RejectedWrongDocumentInstance,
    /// Same instance but a newer generation was accepted before this
    /// candidate reached the boundary, or the ledger already recorded a
    /// newer commit.
    RejectedSupersededGeneration,
}

struct CommittedDocumentSymbols {
    document_instance: Arc<AtomicU32>,
    generation: u32,
    sequence: u64,
}

/// Sink state: one committed-symbol record per open document URI.
#[derive(Default)]
pub(crate) struct DocumentSymbolsSink {
    committed: Mutex<HashMap<String, CommittedDocumentSymbols>>,
}

impl DocumentSymbolsSink {
    /// Test/receipt observation of the last committed record for `uri`:
    /// `(accepted generation, monotonic sequence)`.
    #[cfg(any(test, feature = "expose_lsp_test_api"))]
    pub(crate) fn last_committed(&self, normalized_uri: &str) -> Option<(u32, u64)> {
        self.committed.lock().get(normalized_uri).map(|entry| (entry.generation, entry.sequence))
    }
}

impl LspServer {
    /// Commit one document-local symbol replacement/clear at the sink
    /// boundary. See the module docs for the contract.
    ///
    /// The `symbol_index` mutation happens only after currency passes and
    /// while the boundary holds, so a stale callback that lost the race can
    /// never overwrite or clear a newer row.
    pub(crate) fn commit_document_symbols(
        &self,
        identity: &DocumentSymbolIdentity,
        disposition: DocumentSymbolsDisposition,
    ) -> DocumentSymbolCommitOutcome {
        let mut committed = self.document_symbols_sink.committed.lock();

        let currency = {
            let docs = self.documents.lock();
            docs.get(&identity.normalized_uri).map(|doc| {
                (
                    Arc::ptr_eq(&doc.generation, &identity.document_instance),
                    doc.current_generation(),
                )
            })
        };
        let rejection = match currency {
            None => DocumentSymbolCommitOutcome::RejectedDocumentClosed,
            Some((false, _)) => DocumentSymbolCommitOutcome::RejectedWrongDocumentInstance,
            Some((true, live_generation)) if live_generation != identity.generation => {
                DocumentSymbolCommitOutcome::RejectedSupersededGeneration
            }
            Some((true, _)) => {
                return self.install_committed_document_symbols(
                    &mut committed,
                    identity,
                    disposition,
                );
            }
        };

        tracing::debug!(
            uri = %identity.normalized_uri,
            generation = identity.generation,
            ?rejection,
            "Rejected document-symbol candidate at sink boundary"
        );
        rejection
    }

    /// Ledger compare/record + atomic store mutation. Caller has proven
    /// ticket currency and holds the sink lock.
    fn install_committed_document_symbols(
        &self,
        committed: &mut HashMap<String, CommittedDocumentSymbols>,
        identity: &DocumentSymbolIdentity,
        disposition: DocumentSymbolsDisposition,
    ) -> DocumentSymbolCommitOutcome {
        if let Some(entry) = committed.get(&identity.normalized_uri)
            && Arc::ptr_eq(&entry.document_instance, &identity.document_instance)
            && identity.generation < entry.generation
        {
            return DocumentSymbolCommitOutcome::RejectedSupersededGeneration;
        }

        let sequence =
            committed.get(&identity.normalized_uri).map(|entry| entry.sequence + 1).unwrap_or(1);

        committed.insert(
            identity.normalized_uri.clone(),
            CommittedDocumentSymbols {
                document_instance: Arc::clone(&identity.document_instance),
                generation: identity.generation,
                sequence,
            },
        );

        // Irreversible local-store mutation inside the boundary.
        match disposition {
            DocumentSymbolsDisposition::Replace(symbols) => {
                self.symbol_index
                    .lock()
                    .replace_document_symbols(&identity.normalized_uri, symbols);
                tracing::debug!(
                    uri = %identity.normalized_uri,
                    generation = identity.generation,
                    sequence,
                    "Committed document symbols at sink boundary"
                );
                self.attach_active_document_effect(
                    &identity.normalized_uri,
                    &identity.document_instance,
                    identity.generation,
                    crate::runtime::readiness::CoreEffectKind::DocumentSymbols,
                );
                DocumentSymbolCommitOutcome::CommittedCurrent
            }
            DocumentSymbolsDisposition::Clear => {
                self.symbol_index.lock().remove_document(&identity.normalized_uri);
                tracing::debug!(
                    uri = %identity.normalized_uri,
                    generation = identity.generation,
                    sequence,
                    "Committed document-symbol clear at sink boundary"
                );
                self.attach_active_document_effect(
                    &identity.normalized_uri,
                    &identity.document_instance,
                    identity.generation,
                    crate::runtime::readiness::CoreEffectKind::DocumentSymbols,
                );
                DocumentSymbolCommitOutcome::SafeClearCommitted
            }
        }
    }

    /// Receipt observation for focused tests.
    #[cfg(any(test, feature = "expose_lsp_test_api"))]
    pub(crate) fn test_last_committed_document_symbols(
        &self,
        normalized_uri: &str,
    ) -> Option<(u32, u64)> {
        self.document_symbols_sink.last_committed(normalized_uri)
    }
}
