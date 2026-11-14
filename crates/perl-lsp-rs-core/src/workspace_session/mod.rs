//! Transport-neutral workspace session facade (#7218).
//!
//! This module **composes** existing lifecycle, configuration, source-identity,
//! and #6816 provider-query authorities. It does not own a document store,
//! index, readiness vocabulary, or provider query port.
//!
//! Construction is a pure projection: neither constructor mutates canonical
//! lifecycle state. Stale, ambiguous, missing, unreadable, escaped, or
//! unsupported-multi-root inputs fail as typed construction errors rather than
//! becoming an apparently valid session.

mod construct;
mod model;

pub use construct::{
    DiskObservationError, DiskObservationFacts, DocumentSourceOffer, LocalHostFacts,
    LspSessionFacts, OpenDocumentFacts, SourceSelection, SuppliedDocumentFacts, UnsupportedRoots,
};
pub use model::{
    ConfigurationIdentity, DocumentSource, DocumentSourceKind, OperationIdentity, SessionIdentity,
    WorkspaceRootBinding, WorkspaceSession, WorkspaceSessionError,
};

#[cfg(test)]
mod tests;
