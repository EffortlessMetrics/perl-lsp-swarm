//! JSON-RPC protocol types, error handling, and capabilities for perl-lsp.
//!
//! This module isolates protocol types from the LSP runtime so they can be
//! shared across binaries and provider layers. Key submodules:
//!
//! - [`binary_identity`] — Canonical server, DAP, and VSIX identity transport
//! - [`command`] — Canonical LSP 3.18 `Command` construction, including `tooltip`
//! - `jsonrpc` — Core JSON-RPC 2.0 request, response, and error message types
//! - `errors` — Standard and LSP-specific JSON-RPC error codes and builders
//! - [`methods`] — LSP 3.18 method name constants for request/notification routing
//! - [`capabilities`] — Server capability configuration advertised during `initialize`
//! - [`document_version`] — Typed decoder for `textDocument/version` fields (#10240)
//! - [`schema`] — Versioned, bounded method/direction payload validation
//!
//! Previously the standalone `perl-lsp-protocol` crate; absorbed into
//! `perl-lsp-rs-core::protocol` in Wave G3 (#4535).

/// Canonical server, DAP, and VSIX identity protocol family.
pub mod binary_identity;
/// Perl-owned projection of the neutral cancellation error onto the Perl
/// error taxonomy (#13997).
pub mod cancellation_error_class;
pub mod capabilities;
/// Canonical LSP 3.18 `Command` type and presented-command construction.
pub mod command;
pub mod document_version;
/// Typed deterministic final-surface authority: `EffectiveLspSurface`,
/// registration plan and effective identities (#9665, #8032 train S02).
pub mod effective_surface;
pub mod error_disposition;
pub mod error_inventory;
mod errors;
/// Deterministic final-surface capability/registration/mutation ledger
/// (#9662, #8032 train stage S01).
#[cfg(test)]
pub(crate) mod final_surface_inventory;
mod jsonrpc;
pub mod methods;
pub mod resolve_envelope;
pub mod schema;

pub use cancellation_error_class::{cancellation_error_category, cancellation_error_disposition};
pub use document_version::{
    ClientDocumentVersion, DocumentVersionDecodeError, DocumentVersionField, IntegerRangeClass,
    JsonValueKind, Signedness, decode_document_version, decode_version_value,
};
pub use error_disposition::{Disposition, disposition_for};
pub use error_inventory::{
    ClassificationRoute, ErrorInventoryEntry, classified_count, error_type_inventory,
    unclassified_count, unclassified_types,
};
pub use errors::*;
pub use jsonrpc::*;

/// Convenience function: create an LSP error (ServerErrorStart code).
pub fn lsp_error(message: &str) -> JsonRpcError {
    JsonRpcError::new(crate::protocol::SERVER_ERROR_START, message)
}
