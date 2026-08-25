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
//! Outcomes use the shared #11672 contract vocabulary
//! [`ParseEffectCommitOutcomeV1`] with the sink-local mapping:
//!
//! - document absent from the map (closed entirely or never opened) ->
//!   [`ParseEffectCommitOutcomeV1::RejectedLifecycleState`]: the close/open
//!   lifecycle left no live sink subject the ticket could validate against;
//! - live document on a different instance than the candidate (close/reopen
//!   ABA) -> [`ParseEffectCommitOutcomeV1::RejectedWrongDocumentInstance`];
//! - same instance but a newer generation was accepted before this candidate
//!   reached the boundary -> [`ParseEffectCommitOutcomeV1::RejectedStaleTicket`];
//! - currency passed but the ledger already recorded a newer committed
//!   generation for this instance ->
//!   [`ParseEffectCommitOutcomeV1::RejectedSinkGenerationAdvanced`].
//!
//! The local store mutation cannot fail on its own, so transport/failure
//! variants are unreachable here and never returned.
//!
//! Lock order: sink lock -> documents lock -> `symbol_index` lock. The
//! documents guard is held across the serialized install so lifecycle
//! eviction cannot interleave with the store mutation; no path acquires any
//! pair of these locks in reverse order.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::AtomicU32;

use parking_lot::Mutex;

use super::LspServer;
use super::parse_effect_contract::ParseEffectCommitOutcomeV1;

/// `(instance matches, live generation)` observed for one candidate URI
/// while holding the documents lock.
type DocumentCurrency = Option<(bool, u32)>;

/// Boundary rejection for a candidate that is no longer current against the
/// live document map. `None` means the ticket may proceed to install.
fn classify_currency_rejection(
    currency: DocumentCurrency,
    identity: &DocumentSymbolIdentity,
) -> Option<ParseEffectCommitOutcomeV1> {
    match currency {
        None => Some(ParseEffectCommitOutcomeV1::RejectedLifecycleState),
        Some((false, _)) => Some(ParseEffectCommitOutcomeV1::RejectedWrongDocumentInstance),
        Some((true, live_generation)) if live_generation != identity.generation => {
            Some(ParseEffectCommitOutcomeV1::RejectedStaleTicket)
        }
        Some((true, _)) => None,
    }
}

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
    ) -> ParseEffectCommitOutcomeV1 {
        let mut committed = self.document_symbols_sink.committed.lock();

        let currency = self.documents.lock().get(&identity.normalized_uri).map(|doc| {
            (Arc::ptr_eq(&doc.generation, &identity.document_instance), doc.current_generation())
        });
        if let Some(rejection) = classify_currency_rejection(currency, identity) {
            tracing::debug!(
                uri = %identity.normalized_uri,
                generation = identity.generation,
                ?rejection,
                "Rejected document-symbol candidate at sink boundary"
            );
            return rejection;
        }

        // Test seam mirroring #11673: fires after the currency precheck
        // passes and before the serialized install, letting a falsifier
        // mutate lifecycle state in the exact validation -> mutation window.
        #[cfg(test)]
        if let Some(hook) = self.document_symbols_before_install_hook.lock().as_ref() {
            hook();
        }

        self.install_committed_document_symbols(&mut committed, identity, disposition)
    }

    /// Ledger compare/record + atomic store mutation. Caller has proven
    /// ticket currency against the precheck and holds the sink lock.
    ///
    /// The store mutation is additionally serialized against lifecycle
    /// eviction (#11674): the precheck above releases `documents`, and the
    /// didClose sweep clears this store through `clear_document_symbols`
    /// without taking the sink lock, so a candidate that passed the precheck
    /// could otherwise reinstall symbols for a URI the lifecycle already
    /// closed. Re-validating under a documents guard held across both the
    /// ledger record and the irreversible store mutation totally orders
    /// eviction against commit: whichever runs second observes the other's
    /// effect. Lock order stays sink -> documents -> `symbol_index`; the
    /// sweep holds no lock while taking `symbol_index`, so close cannot
    /// deadlock against this section.
    fn install_committed_document_symbols(
        &self,
        committed: &mut HashMap<String, CommittedDocumentSymbols>,
        identity: &DocumentSymbolIdentity,
        disposition: DocumentSymbolsDisposition,
    ) -> ParseEffectCommitOutcomeV1 {
        let documents = self.documents.lock();
        let currency = documents.get(&identity.normalized_uri).map(|doc| {
            (Arc::ptr_eq(&doc.generation, &identity.document_instance), doc.current_generation())
        });
        if let Some(rejection) = classify_currency_rejection(currency, identity) {
            tracing::debug!(
                uri = %identity.normalized_uri,
                generation = identity.generation,
                ?rejection,
                "Rejected document-symbol candidate at serialized install boundary"
            );
            return rejection;
        }

        if let Some(entry) = committed.get(&identity.normalized_uri)
            && Arc::ptr_eq(&entry.document_instance, &identity.document_instance)
            && identity.generation < entry.generation
        {
            return ParseEffectCommitOutcomeV1::RejectedSinkGenerationAdvanced;
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

        // Irreversible local-store mutation inside the boundary. The
        // documents guard stays held across it; the readiness projection is
        // attached only after the guard drops so the notification write
        // never extends the documents hold.
        let outcome = {
            let mut index = self.symbol_index.lock();
            match disposition {
                DocumentSymbolsDisposition::Replace(symbols) => {
                    index.replace_document_symbols(&identity.normalized_uri, symbols);
                    tracing::debug!(
                        uri = %identity.normalized_uri,
                        generation = identity.generation,
                        sequence,
                        "Committed document symbols at sink boundary"
                    );
                    ParseEffectCommitOutcomeV1::CommittedCurrent
                }
                DocumentSymbolsDisposition::Clear => {
                    index.remove_document(&identity.normalized_uri);
                    tracing::debug!(
                        uri = %identity.normalized_uri,
                        generation = identity.generation,
                        sequence,
                        "Committed document-symbol clear at sink boundary"
                    );
                    ParseEffectCommitOutcomeV1::SafeClearCommitted
                }
            }
        };
        drop(documents);

        self.attach_active_document_effect(
            &identity.normalized_uri,
            &identity.document_instance,
            identity.generation,
            crate::runtime::readiness::CoreEffectKind::DocumentSymbols,
        );
        outcome
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
