//! Typed `inlayHint/resolve` subject carried by the #8342 resolve envelope.
//!
//! `inlayHint/resolve` receives an item that has been round-tripped through an
//! untrusted client. Before #14672 the resolver recovered its subject from that
//! item's `data` object — a client-supplied `uri` selected the document and a
//! client-supplied `functionName` selected the declaration by first name match —
//! so an item this server never emitted still resolved to a real source range.
//!
//! This module owns the provider half of the fix: the exact facts the inlay-hint
//! producer knew when it emitted the hint, authenticated as one
//! [`ResolveEnvelopeSubject`] in the [`ResolveFamily::InlayHint`] family. The
//! generic envelope owns authenticity, bounds and canonical encoding; this type
//! owns what "the same hint, still current" means for inlay hints.

use crate::protocol::resolve_envelope::{ResolveEnvelopeSubject, ResolveFamily};
use serde::{Deserialize, Serialize};

/// Exact producer facts for one resolvable inlay hint.
///
/// Every field is server-owned: it is recorded by `textDocument/inlayHint` from
/// the document and parsed snapshot that produced the hint, never read back from
/// the client's copy of the item.
///
/// The field set is deliberately minimal — each one is consulted when resolving:
///
/// - [`uri`](Self::uri) selects the document, so the client cannot redirect the
///   lookup at another open file;
/// - [`incarnation`](Self::incarnation) pins the exact open instance, so a
///   `didClose` + `didOpen` cycle on unchanged text cannot revive the item;
/// - [`generation`](Self::generation) and [`content_hash`](Self::content_hash)
///   pin the exact parsed snapshot, so a hint issued before an edit is refused
///   rather than reprojected against different source;
/// - [`line`](Self::line) and [`character`](Self::character) pin which hint the
///   envelope was issued for, so a valid envelope cannot be moved onto a
///   different hint in the same document;
/// - [`function_name`](Self::function_name) is the callable the producer
///   recorded, used for the declaration lookup in place of the client's string.
///
/// `incarnation` is required because `generation` alone is not an instance
/// identity: it resets when a URI is reopened, and unchanged text reproduces
/// the same content hash, so the pair repeats across a close/reopen cycle.
///
/// The schema version stays at 1 across changes to this struct while the
/// envelope remains session-local: the issuing session's keys are destroyed at
/// teardown, so a token minted by any other build can only ever be rejected as
/// `ForeignSession` or `IntegrityFailure`, never mis-decoded against a newer
/// field set.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InlayHintResolveSubjectV1 {
    /// Document URI that produced this hint.
    pub uri: String,
    /// Process-unique identity of the open document instance.
    pub incarnation: u64,
    /// Parsed-snapshot generation that produced this hint.
    pub generation: u32,
    /// Content hash of the source that produced this hint.
    pub content_hash: u64,
    /// Zero-based wire line of the hint position.
    pub line: u32,
    /// Zero-based wire character of the hint position.
    pub character: u32,
    /// Callable name recorded by the producer for this hint.
    pub function_name: String,
}

impl ResolveEnvelopeSubject for InlayHintResolveSubjectV1 {
    const FAMILY: ResolveFamily = ResolveFamily::InlayHint;
    const VERSION: u16 = 1;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::resolve_envelope::ResolveMethod;

    fn subject() -> InlayHintResolveSubjectV1 {
        InlayHintResolveSubjectV1 {
            uri: "file:///workspace/lib/Greeter.pm".to_string(),
            incarnation: 42,
            generation: 7,
            content_hash: 0x0123_4567_89ab_cdef,
            line: 12,
            character: 30,
            function_name: "greet".to_string(),
        }
    }

    #[test]
    fn subject_registers_in_the_inlay_hint_family_and_method() {
        assert_eq!(InlayHintResolveSubjectV1::FAMILY, ResolveFamily::InlayHint);
        assert_eq!(InlayHintResolveSubjectV1::FAMILY.method(), ResolveMethod::InlayHint);
        assert_eq!(InlayHintResolveSubjectV1::VERSION, 1);
    }

    #[test]
    fn subject_round_trips_through_json() -> Result<(), serde_json::Error> {
        let original = subject();
        let encoded = serde_json::to_vec(&original)?;
        let decoded: InlayHintResolveSubjectV1 = serde_json::from_slice(&encoded)?;
        assert_eq!(decoded, original);
        Ok(())
    }

    #[test]
    fn every_recorded_fact_changes_the_subject() {
        let base = subject();

        let mut other_document = base.clone();
        other_document.uri = "file:///workspace/lib/Other.pm".to_string();
        assert_ne!(other_document, base, "the document must be part of the subject");

        let mut reopened_instance = base.clone();
        reopened_instance.incarnation += 1;
        assert_ne!(
            reopened_instance, base,
            "the open-document instance must be part of the subject"
        );

        let mut later_generation = base.clone();
        later_generation.generation += 1;
        assert_ne!(later_generation, base, "the snapshot generation must be part of the subject");

        let mut edited_source = base.clone();
        edited_source.content_hash ^= 1;
        assert_ne!(edited_source, base, "the source content hash must be part of the subject");

        let mut moved_hint = base.clone();
        moved_hint.character += 1;
        assert_ne!(moved_hint, base, "the hint position must be part of the subject");

        let mut other_callable = base.clone();
        other_callable.function_name = "farewell".to_string();
        assert_ne!(other_callable, base, "the callable name must be part of the subject");
    }
}
