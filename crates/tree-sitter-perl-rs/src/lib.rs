//! Rust-native Perl parser with tree-sitter-style ergonomics and tree-sitter-compatible
//! output. This is a facade over the v3 native parser (`perl-parser-core`); it is NOT
//! bindings to the C tree-sitter grammar. For the conventional tree-sitter binding, see
//! `tree-sitter-perl-c`.
//!
//! # Quick start
//!
//! ```rust
//! use tree_sitter_perl_rs::Parser;
//!
//! let mut parser = Parser::new();
//! if let Some(tree) = parser.parse("my $x = 42;") {
//!     let root = tree.root_node();
//!     println!("{}", root.to_sexp());
//! }
//! ```
//!
//! # Design
//!
//! This crate wraps the v3 recursive-descent Perl parser (`perl-parser-core`) with an API
//! surface that matches the conventions of the `tree-sitter` crate. Users familiar with
//! tree-sitter can work with Perl ASTs immediately, while the underlying engine is the
//! full-featured native v3 stack (not the C tree-sitter grammar).
//!
//! Key properties:
//! - `Parser::parse()` returns `Option<Tree>` — `None` only on complete parse failure.
//!   The v3 parser is highly error-tolerant and almost always produces a partial tree.
//! - `Node::to_sexp()` delegates to `perl_ast::Node::to_sexp()` for a native debug
//!   S-expression. Tree-sitter compatibility CST serialization is issue 8047.
//! - `Node::kind()` returns the tree-sitter grammar-canonical kind string.
//! - `Node::start_byte()` / `Node::end_byte()` expose the `SourceLocation` byte offsets.
//! - `Node::children()` and `Node::child()` mirror tree-sitter traversal conventions.
//!
//! # Relationship to `tree-sitter-perl-c`
//!
//! | Crate | Backing engine | Use when |
//! |-------|---------------|----------|
//! | `tree-sitter-perl-rs` | v3 native Rust parser (this crate) | You want the full-featured Rust toolchain |
//! | `tree-sitter-perl-c` | C tree-sitter grammar | You need compatibility with the tree-sitter C ecosystem |

#![deny(unsafe_code)]
#![deny(unreachable_pub)]
#![warn(rust_2018_idioms)]
#![warn(missing_docs)]
#![allow(
    clippy::module_name_repetitions,
    clippy::must_use_candidate,
    clippy::missing_errors_doc,
    clippy::missing_panics_doc
)]

mod cursor;
mod language;
mod node;
mod parser;
mod point;
#[cfg(feature = "queries")]
mod query;
#[cfg(feature = "semantic-overlay")]
mod semantic_overlay;
mod support;
mod tree;

pub use cursor::TreeCursor;
pub use language::{LANGUAGE, PerlLanguage, language};
pub use node::Node;
pub use parser::{
    FallbackReason, IncrementalMetrics, ParseFailure, ParseOutcome, Parser, ReparseMode,
};
pub use point::Point;
#[cfg(feature = "semantic-overlay")]
pub use semantic_overlay::{OverlayDefinition, SemanticOverlay, VisibleImport};
pub use tree::Tree;

/// Parser diagnostics surfaced by [`Parser::parse_detailed`].
pub use perl_parser_core::ParseError as ParseDiagnostic;

/// Re-export of Edit type for tree-sitter-compatible incremental parsing.
///
/// Mirrors `tree_sitter::InputEdit` field layout for drop-in compatibility.
pub use perl_parser_core::edit::Edit as InputEdit;

/// Re-export of [`perl_ast::NodeKind`] so callers can pattern-match node variants
/// without a direct dependency on `perl-ast`.
pub use perl_ast::{FieldId, NodeKind as PerlNodeKind};

#[cfg(feature = "queries")]
pub use query::{
    Query, QueryCapture, QueryCursor, QueryError, QueryMatch, QueryMatches, QuerySetting,
};

#[cfg(test)]
mod tests;
