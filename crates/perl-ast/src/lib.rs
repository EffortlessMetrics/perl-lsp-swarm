#![warn(missing_docs)]
#![cfg_attr(clippy, allow(missing_docs))]

//! Perl AST library -- typed syntax tree for Perl source code.
//!
//! This crate defines the Abstract Syntax Tree used by `perl-parser-core` and
//! downstream analysis tools. Every parsed Perl construct is represented as a
//! [`Node`] carrying a [`NodeKind`] discriminant and a [`SourceLocation`]
//! (byte-offset span).
//!
//! # Modules
//!
//! - [`ast`] -- The primary AST used by the current recursive-descent parser.
//! - [`invariants`] -- Bounded structural validation shared by parser paths.
//! - [`v2`] -- Experimental second-generation AST re-exported from `perl-ast-v2`
//!   for incremental parsing.
//!
//! # Quick start
//!
//! ```rust
//! use perl_ast::{Node, NodeKind, SourceLocation};
//!
//! // Build a small AST by hand
//! let loc = SourceLocation { start: 0, end: 2 };
//! let num = Node::new(NodeKind::Number { value: "42".to_string() }, loc);
//!
//! assert_eq!(num.kind.kind_name(), "Number");
//! assert_eq!(num.location.start, 0);
//! assert_eq!(num.location.end, 2);
//! ```
//!
//! In practice the AST is produced by the parser (requires `perl-parser-core`):
//!
//! ```rust,ignore
//! use perl_parser_core::Parser;
//! use perl_ast::NodeKind;
//!
//! let mut parser = Parser::new("my $x = 42;");
//! let ast = parser.parse().expect("should parse");
//! assert!(matches!(ast.kind, NodeKind::Program { .. }));
//! ```
//!
//! # Traversal
//!
//! [`Node`] exposes `to_sexp()` for a tree-sitter-compatible S-expression and
//! `count_nodes()` for a quick size metric. [`validate_ast`] uses the canonical
//! exhaustive child iterator to check source and tree invariants without a
//! recursive call stack.

pub mod ast;
/// Static classification metadata for [`NodeKind`] variants: categories and flags.
pub mod classification;
/// Bounded structural validation for parser-produced ASTs.
pub mod invariants;

/// Incremental parsing AST types extracted into a dedicated microcrate.
pub use perl_ast_v2 as v2;

/// Discriminant for the three semantically distinct forms of Perl's `goto` statement.
pub use ast::GotoTargetForm;
/// Primary AST node -- the building block of every syntax tree.
pub use ast::{FieldId, Node, NodeKind};
/// AST structural validation types and entry point.
pub use invariants::{
    AstInvariantCode, AstInvariantFinding, AstInvariantOptions, AstInvariantReport, validate_ast,
};
/// Byte-offset span indicating where a node appears in source text.
pub use perl_position_tracking::SourceLocation;
