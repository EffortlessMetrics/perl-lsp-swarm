//! JSON-RPC protocol types, error handling, capabilities, and identity contracts.
//!
//! This module re-exports the protocol layer from `perl-lsp-rs-core::protocol`
//! (Wave G3: `perl-lsp-protocol` absorbed into rs-core, #4535). The runtime
//! identity packet is also exposed here so the public `perllsp` facade can
//! consume the canonical contract without adding a second direct core dependency.

pub use perl_lsp_rs_core::product_identity;
pub use perl_lsp_rs_core::protocol::*;
