//! LSP completion provider for Perl
//!
//! This crate provides code completion functionality for Perl.
//!
//! ## Features
//!
//! - Context-aware completion
//! - Multiple completion sources (builtins, functions, variables, etc.)
//! - Workspace integration
//!
//! ## Usage
//!
//! ```rust,ignore
//! use perl_lsp_completion::CompletionProvider;
//!
//! let provider = CompletionProvider::new(&ast, Some(&workspace_index))?;
//! let completions = provider.get_completions(source, position)?;
//! ```

#[allow(clippy::module_inception)]
mod completion;
/// Completion visibility shadow compare path (semantic migration).
pub mod completion_shadow;
/// Short-TTL cache for module prefix directory scans (issue #8514).
pub mod module_scan_cache;

pub use crate::providers::completion_item::{render_snippet_plaintext, snippet_body_defects};
pub use completion::FUNDAMENTAL_CONSTRUCT_LABELS;
pub use completion::{
    CompletionContext, CompletionItem, CompletionItemKind, CompletionProvider, InsertTextFormat,
    add_xs_api_completions_for_prefix, collect_module_names_from_roots_with_cache,
    get_dbi_method_documentation, get_test_more_documentation, get_xs_api_documentation,
    is_xs_source,
};
