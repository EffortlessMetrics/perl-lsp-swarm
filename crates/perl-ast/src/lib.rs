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
//! - [`invariant_policy`] -- Exhaustive range, child, payload, and recovery policy.
//! - [`invariants`] -- Bounded structural validation shared by parser paths.
//! - [`kind_schema`] -- Structural `NodeKind` registry, field-aware traversal, schema identity, and NodeKind inventory.
//! - [`source_syntax`] -- Source-backed string and heredoc payload contract: raw
//!   source, cooked value, and ordered segments as separate typed propositions.
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
//! [`Node`] exposes `to_sexp()` for a native debug S-expression projection and
//! `count_nodes()` for an exact iterative size metric. [`validate_ast`] uses the
//! canonical exhaustive child iterator to check source and tree invariants
//! without a recursive call stack. The policy registry is reconciled directly
//! with [`NodeKind::ALL_KIND_NAMES`], so a new variant cannot inherit an
//! undocumented permissive policy.
//!
//! # Depth safety
//!
//! [`Node`] remains recursively owned. Destruction is iterative: a 50,000-node
//! chain on a 256 KiB worker does not overflow the thread stack.
//! Construct/destroy equality is proven at 10,000-node cycle depth, not on
//! the overflow fixture. [`Clone`] is likewise iterative: a 50,000-node chain
//! on a 256 KiB worker does not overflow the thread stack. [`PartialEq`] is
//! iterative exact structural equality: a 50,000-node chain on a 256 KiB
//! worker does not overflow the thread stack. [`Debug`] is an iterative
//! bounded human projection: a 50,000-node chain on a 256 KiB worker does
//! not overflow the thread stack, output stays under the documented byte
//! bound, and truncation is visible. Rust [`Debug`] is not machine identity.
//! Exact whole-tree reads (`count_nodes`, `find_deepest_containing_offset`) are
//! iterative over the #8424 visit table and do not silently truncate; bounded
//! variants expose [`AstReadResult`]. [`Node::render_debug_sexp`] is the iterative
//! bounded native debug renderer (`Complete` / `Truncated` / `InstrumentFailure`).
//! [`Node::to_sexp`] is a `String` convenience over that engine and cannot prove
//! completeness. See [`Node`] for the operation-by-operation contract.

pub mod ast;
/// Static classification metadata for [`NodeKind`] variants: categories and flags.
pub mod classification;
/// Owner-neutral source syntax for declaration attributes.
pub mod declaration;
/// Exhaustive invariant policy metadata for every [`NodeKind`] variant.
pub mod invariant_policy;
/// Bounded structural validation for parser-produced ASTs.
pub mod invariants;
/// Structural `NodeKind` registry, field-aware traversal, schema identity, and
/// freshness-gated NodeKind inventory.
///
/// Production FieldId membership and field-aware child traversal are derived
/// from this module. Native debug S-expression rendering consumes the visit
/// table for child order but keeps payload disposition renderer-local. Schema
/// identity and generated NodeKind status are derived from the same registry
/// and do not change parser or AST structure.
pub mod kind_schema;
/// Source-backed string and heredoc payload types: raw source, cooked value,
/// and ordered segments as separate typed propositions.
///
/// This is a data contract only. It adds no [`NodeKind`] variant, changes no
/// existing variant's child fields, and does not affect parser output,
/// traversal, rendering, or the [`kind_schema`] registry.
pub mod source_syntax;

/// Incremental parsing AST types extracted into a dedicated microcrate.
pub use perl_ast_v2 as v2;

/// Discriminant for the three semantically distinct forms of Perl's `goto` statement.
pub use ast::GotoTargetForm;
/// Primary AST node -- the building block of every syntax tree.
pub use ast::{
    AstReadExact, AstReadInstrumentCause, AstReadLimits, AstReadPath, AstReadPathStep,
    AstReadResult, AstReadTruncation, AstReadWork, DeepestContainingMatch, FieldId,
    NATIVE_DEBUG_SEXP_DEPTH_LIMIT_MARKER, NATIVE_DEBUG_SEXP_GRAMMAR,
    NativeDebugSexpInstrumentCause, NativeDebugSexpLimits, NativeDebugSexpOmitted,
    NativeDebugSexpResult, NativeDebugSexpTruncation, NativeDebugSexpWork, Node, NodeKind,
};
/// Owner-neutral declaration-attribute source contracts.
pub use declaration::{
    DeclarationAttributeArgumentDisposition, DeclarationAttributeArgumentSyntax,
    DeclarationAttributeCompleteness, DeclarationAttributeDelimiter, DeclarationAttributeSeparator,
    DeclarationAttributeSyntax, DeclarationAttributeSyntaxError,
};
/// Exhaustive AST invariant policy types and registry.
pub use invariant_policy::{
    AST_NODE_POLICIES, AST_NODE_POLICY_SCHEMA_VERSION, AstChildContainmentPolicy,
    AstChildOrderPolicy, AstChildOverlapPolicy, AstEmptyRangePolicy, AstNodeClassification,
    AstNodePolicy, AstPayloadPolicy, AstSourceBacking, NodeKindFixture, all_ast_node_policies,
    ast_node_policy, ast_node_policy_of, node_kind_fixtures, policy_accepts_observed_children,
};
/// AST structural validation types and entry point.
pub use invariants::{
    AstInvariantCode, AstInvariantFinding, AstInvariantOptions, AstInvariantReport, validate_ast,
};
/// Byte-offset span indicating where a node appears in source text.
pub use perl_position_tracking::SourceLocation;
/// Source-backed string and heredoc payload types.
pub use source_syntax::{
    CookedValue, HeredocDeclarationIdentity, HeredocForm, HeredocSyntax, NormalizationRule,
    PayloadContradiction, PayloadTerminal, SegmentRecoveryCause, SourceSegment,
    SourceSegmentPayload, SourceSegmentation, StringDelimiter, StringForm, StringSyntax,
};


