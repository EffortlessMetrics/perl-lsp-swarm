//! LSP code actions provider for Perl
//!
//! This crate provides code action functionality for Perl.
//!
//! ## Features
//!
//! - Quick fixes for common mistakes
//! - Refactoring operations
//! - Enhanced actions (extract variable/subroutine)
//!
//! Import management actions are intentionally absent (#10690): hard-coded
//! function→module affinity is not candidate identity and not edit
//! authorization. Restoration requires #790/#8948.
//!
//! ## Usage
//!
//! ```rust,ignore
//! use perl_lsp_code_actions::CodeActionsProvider;
//!
//! let source = String::from("my $x = 1;");
//! let provider = CodeActionsProvider::new(source);
//! let actions = provider.get_code_actions(&ast, (0, 10), &diagnostics);
//! ```

#[allow(clippy::module_inception)]
mod code_actions;
mod diagnostic_routes;
mod enhanced;
mod modernize;
mod quick_fixes;
mod refactors;
mod source_actions;
mod types;

pub use code_actions::{CodeAction, CodeActionKind, CodeActionsProvider};
pub use enhanced::EnhancedCodeActionsProvider;
pub use types::CodeActionEdit;
