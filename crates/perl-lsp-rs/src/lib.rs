//! Perl Language Server Protocol runtime.
//!
//! This crate owns the Perl LSP server lifecycle, JSON-RPC transport, document
//! and workspace state, and editor-facing language features. Parsing and
//! semantic analysis are delegated to the workspace's native Perl engine.
//!
//! The public code-intelligence executable is `perllsp`. Debug Adapter Protocol
//! support is provided separately by the native `perl-dap` server. This crate
//! does not re-export or activate an alternate DAP implementation.
//!
//! # Native tooling boundary
//!
//! Formatting and critic diagnostics use the native implementations by default.
//! External Perl tools may be selected explicitly for compatibility work, but
//! their presence does not change product defaults and they are not bundled.
//!
//! # Public JSON-RPC envelope compatibility facade
//!
//! [`dispatch`] re-exports the canonical envelope types only. Request routing,
//! lifecycle checks, cancellation, and response finalization remain internal to
//! `runtime::dispatch` and are reached through [`LspServer::handle_request`].

#![deny(unsafe_code)]
#![cfg_attr(test, allow(clippy::print_stderr, clippy::print_stdout))]
#![warn(missing_docs)]
#![allow(
    // Migrated from perl-parser - these patterns are acceptable in LSP runtime code
    clippy::collapsible_match,
    clippy::only_used_in_recursion,
    clippy::while_let_loop,
    clippy::needless_range_loop,
    clippy::for_kv_map,
    clippy::arc_with_non_send_sync,
    clippy::mutable_key_type,
    clippy::new_without_default,
    clippy::if_same_then_else
)]

// Module declarations - migrated from perl-parser
pub mod cancellation;
pub mod cli;
pub mod convert;
pub mod diagnostics_catalog;
pub mod dispatch;
pub(crate) mod documentation_targets;
pub mod execute_command;
pub mod fallback;
pub mod features;
pub mod handlers;
pub(crate) mod perl_remediation;
pub mod protocol;
pub mod runtime;
pub mod security;
pub mod server;
pub mod state;
pub mod textdoc;
pub mod transport;
pub mod util;

// Re-exports for key types
pub use cli::run_cli;
pub use protocol::{JsonRpcError, JsonRpcId, JsonRpcRequest, JsonRpcResponse};
pub use server::LspServer;

// Wave F re-exports: absorbed capability-map crate
pub use perl_lsp_rs_core::capability_map;

// =============================================================================
// Internal compatibility re-exports (crate-internal, not API surface)
// =============================================================================
// These re-exports allow migrated code to use `crate::...` paths for engine
// pieces while we incrementally update paths to `perl_parser::...`

/// Parser re-export for migrated code
pub(crate) use perl_parser::Parser;

/// Position utilities re-export
pub(crate) mod position {
    pub use perl_parser::position::*;
}

/// Declaration types re-export
pub(crate) mod declaration {
    pub use perl_parser::declaration::*;
}

/// Workspace index re-export
pub(crate) mod workspace_index {
    pub use perl_parser::workspace_index::*;
}

/// Symbol types re-export
pub(crate) mod symbol {
    pub use perl_parser::symbol::*;
}

/// AST types re-export
pub(crate) mod ast {
    pub use perl_parser::ast::*;
}

/// Feature re-exports for old intra-crate paths
pub(crate) mod code_actions_enhanced {
    #[allow(unused_imports)]
    pub use crate::features::code_actions_enhanced::*;
}

pub(crate) mod code_lens_provider {
    pub use crate::features::code_lens_provider::*;
}

pub(crate) mod diagnostics {
    #[allow(unused_imports)]
    pub use crate::features::diagnostics::*;
}

// More feature re-exports for runtime imports
pub(crate) mod inlay_hints {
    pub use crate::features::inlay_hints::*;
}

pub(crate) mod document_links {
    pub use crate::features::document_links::*;
}

pub(crate) mod lsp_document_link {
    pub use crate::features::lsp_document_link::*;
}

pub(crate) mod linked_editing {
    pub use crate::features::linked_editing::*;
}

pub(crate) mod code_actions_pragmas {
    pub use crate::features::code_actions_pragmas::*;
}

// Engine re-exports for runtime
pub(crate) mod perl_critic {
    pub use perl_lsp_rs_core::tooling::perl_critic::*;
}

pub(crate) mod semantic {
    pub use perl_parser::semantic::*;
}

pub(crate) mod error {
    pub use perl_parser::error::*;
}

pub(crate) mod completion {
    pub use crate::features::completion::*;
}

pub(crate) mod on_type_formatting {
    pub use crate::features::on_type_formatting::*;
}

pub(crate) mod inline_completions {
    pub use crate::features::inline_completions::*;
}

pub(crate) mod type_hierarchy {
    pub use crate::features::type_hierarchy::*;
}

// Re-export SourceLocation at crate root for convenience
pub(crate) use perl_parser::ast::SourceLocation;

// Engine modules needed by runtime
pub(crate) mod type_inference {
    pub use perl_parser::type_inference::*;
}

pub(crate) mod builtin_signatures {
    pub use perl_parser::builtin_signatures::*;
}

pub(crate) mod semantic_tokens {
    pub use crate::features::semantic_tokens::*;
}

pub(crate) mod call_hierarchy_provider;

// Parser module re-export for tests using crate::parser::Parser
pub(crate) mod parser {
    #[allow(unused_imports)]
    pub use perl_parser::parser::*;
}

// Folding re-export
pub(crate) mod folding {
    pub use crate::features::folding::*;
}

// References re-export
pub(crate) mod references {
    #[allow(unused_imports)]
    pub use crate::features::references::*;
}

// Rename re-export
pub(crate) mod rename {
    #[allow(unused_imports)]
    pub use crate::features::rename::*;
}

// Signature help re-export
pub(crate) mod signature_help {
    #[allow(unused_imports)]
    pub use crate::features::signature_help::*;
}

/// Run the LSP server in stdio mode.
///
/// This is the main entry point for the LSP server. It reads JSON-RPC messages
/// from stdin and writes responses to stdout, following the Language Server
/// Protocol specification.
///
/// # Errors
///
/// Returns an error if the transport fails to initialize, message framing or
/// parsing fails, or the server encounters an unrecoverable error.
pub fn run_stdio() -> Result<(), Box<dyn std::error::Error>> {
    let server = LspServer::new();
    server.run().map_err(|e| Box::new(e) as Box<dyn std::error::Error>)
}
