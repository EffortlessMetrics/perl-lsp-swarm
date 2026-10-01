//! Server and document state management
//!
//! This module manages the stateful aspects of the LSP server:
//! - Document content and AST caching
//! - Server configuration
//! - Cancellation tracking
//! - Resource limits and bounded behavior

mod config;
mod document;
// #10247 stages private primitives; #8286 removes this expectation at cutover.
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "policy:allow-10247-staged-document-revision: remove with #8286 lifecycle cutover"
    )
)]
pub(crate) mod document_revision;

pub use config::*;
pub use document::*;
pub use perl_lsp_rs_core::runtime::limits::*;
