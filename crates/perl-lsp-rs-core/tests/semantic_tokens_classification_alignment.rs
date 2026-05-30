//! Drift-guard tests: semantic-token declaration-modifier carriers vs. NodeKind classification.
//!
//! The live semantic-tokens provider (semantic_tokens.rs) hardcodes the declaration modifier
//! (bit 0) on tokens emitted for `Package`, `Subroutine`, `Method`, and `Class` NodeKind
//! variants. These tests assert that all four variants satisfy `declares_symbol() == true`
//! in the shared `perl-ast` classification, ensuring the provider's hand-coded decision
//! stays aligned with the canonical classification API.
//!
//! # Known divergence: LabeledStatement
//!
//! The provider also emits the declaration modifier for `LabeledStatement` (jump-target
//! label definitions, e.g. `OUTER:`), but `LabeledStatement::declares_symbol() == false`.
//! This divergence is intentional and documented in the provider source:
//! labels are jump targets, not symbol declarations in any scope — the declaration modifier
//! is applied purely for editor UX (visually distinguishing label definitions from uses).
//!
//! See the comment at `NodeKind::LabeledStatement` in
//! `crates/perl-lsp-rs-core/src/providers/semantic_tokens/semantic_tokens.rs` for the
//! full rationale and the TODO for a follow-up cleanup.

use perl_ast::{NodeKind, SourceLocation};

/// Construct a minimal dummy `SourceLocation` (zero-length at offset 0).
fn zero_span() -> SourceLocation {
    SourceLocation::default()
}

/// Construct a minimal dummy `perl_ast::Node` for use as a box-field placeholder.
fn dummy_node() -> perl_ast::Node {
    perl_ast::Node::new(NodeKind::Block { statements: vec![] }, zero_span())
}

/// The live semantic-tokens provider hardcodes the declaration modifier on `Package` tokens.
/// This test pins that `Package::declares_symbol() == true`, so a future classification
/// change that flips this is caught immediately.
#[test]
fn package_variant_satisfies_declares_symbol() {
    let kind = NodeKind::Package { name: "Foo".to_string(), name_span: zero_span(), block: None };
    assert!(
        kind.declares_symbol(),
        "NodeKind::Package must satisfy declares_symbol() — \
         the live semantic-tokens provider emits the declaration modifier (bit 0) on \
         Package tokens; classification drift would silently mis-describe the provider"
    );
}

/// The live semantic-tokens provider hardcodes the declaration modifier on `Subroutine` tokens.
/// This test pins that `Subroutine::declares_symbol() == true`.
#[test]
fn subroutine_variant_satisfies_declares_symbol() {
    let kind = NodeKind::Subroutine {
        name: Some("foo".to_string()),
        name_span: Some(zero_span()),
        prototype: None,
        signature: None,
        attributes: vec![],
        body: Box::new(dummy_node()),
    };
    assert!(
        kind.declares_symbol(),
        "NodeKind::Subroutine must satisfy declares_symbol() — \
         the live semantic-tokens provider emits the declaration modifier on Subroutine tokens"
    );
}

/// The live semantic-tokens provider hardcodes the declaration modifier on `Method` tokens.
/// This test pins that `Method::declares_symbol() == true`.
#[test]
fn method_variant_satisfies_declares_symbol() {
    let kind = NodeKind::Method {
        name: "bar".to_string(),
        signature: None,
        attributes: vec![],
        body: Box::new(dummy_node()),
    };
    assert!(
        kind.declares_symbol(),
        "NodeKind::Method must satisfy declares_symbol() — \
         the live semantic-tokens provider emits the declaration modifier on Method tokens"
    );
}

/// The live semantic-tokens provider hardcodes the declaration modifier on `Class` tokens.
/// This test pins that `Class::declares_symbol() == true`.
#[test]
fn class_variant_satisfies_declares_symbol() {
    let kind = NodeKind::Class {
        name: "MyClass".to_string(),
        parents: vec![],
        body: Box::new(dummy_node()),
    };
    assert!(
        kind.declares_symbol(),
        "NodeKind::Class must satisfy declares_symbol() — \
         the live semantic-tokens provider emits the declaration modifier on Class tokens"
    );
}

/// Documents the known intentional divergence: `LabeledStatement` is emitted with the
/// declaration modifier by the live provider, but `declares_symbol() == false`.
///
/// This test does NOT assert that labels should gain `declares_symbol()`; it documents
/// the divergence so it remains visible in the test suite and is not mistaken for drift.
#[test]
fn labeled_statement_known_divergence_from_declares_symbol() {
    let kind = NodeKind::LabeledStatement {
        label: "OUTER".to_string(),
        statement: Box::new(dummy_node()),
    };
    assert!(
        !kind.declares_symbol(),
        "NodeKind::LabeledStatement is expected to return declares_symbol() == false — \
         labels are jump targets, not symbol declarations. \
         The live provider emits the declaration modifier on labels as an editor UX decision; \
         see the TODO comment in semantic_tokens.rs for the planned follow-up."
    );
}
