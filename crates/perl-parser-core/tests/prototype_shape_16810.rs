//! Production-path proof for canonical prototype-shape facts (#16810).
//!
//! These tests lower real subroutine declarations so a projector that exists
//! only as an unused helper cannot pass. They also pin the negative controls
//! that keep signatures and ampersand-call work out of this claim.

use perl_parser_core::Parser;
use perl_parser_core::hir::{CompileConfidence, CompileProvenance, HirFile, lower_ast};
use perl_parser_core::prototype_shape::{
    PrototypeDefault, PrototypeRecovery, PrototypeSlotKind, PrototypeSyntaxClass,
};
use perl_tdd_support::must_some_with;

fn lower_source(source: &str) -> HirFile {
    let mut parser = Parser::new(source);
    let output = parser.parse_with_recovery();
    lower_ast(&output.ast)
}

fn fact<'a>(file: &'a HirFile, name: &str) -> &'a perl_parser_core::hir::PrototypeFact {
    must_some_with(
        file.prototype_table.facts.iter().find(|fact| fact.sub_name == name),
        format!("missing prototype fact for {name}"),
    )
}

#[test]
fn optional_scalar_shape_reaches_the_hir_fact() {
    let source = "sub optional ($;$) { }";
    let file = lower_source(source);
    let proto = fact(&file, "optional");
    assert_eq!(proto.content, "$;$");
    assert_eq!(&source[proto.range.start..proto.range.end], "($;$)");
    assert!(proto.shape.is_exact(), "valid optional prototype must be exact");
    assert_eq!(proto.shape.slots().len(), 2);
    assert!(!proto.shape.slots()[0].is_optional());
    assert!(proto.shape.slots()[1].is_optional());
    assert!(matches!(proto.shape.slots()[0].kind(), PrototypeSlotKind::Scalar));
    assert_eq!(proto.provenance, CompileProvenance::ExactAst);
    assert_eq!(proto.confidence, CompileConfidence::High);
}

#[test]
fn topic_default_shape_reaches_the_hir_fact() {
    let source = "sub topic_default (;_) { }";
    let file = lower_source(source);
    let proto = fact(&file, "topic_default");
    assert_eq!(proto.content, ";_");
    let slot = must_some_with(proto.shape.slots().first(), "topic-default slot");
    assert!(matches!(slot.kind(), PrototypeSlotKind::TopicDefaultScalar));
    assert!(slot.is_optional());
    assert_eq!(slot.default(), PrototypeDefault::TopicVariable);
}

#[test]
fn required_forms_from_the_issue_matrix_stay_ordered() {
    let source = r#"
        sub scalar ($) { }
        sub block_and_rest (&@) { }
        sub glob (*) { }
        sub scalar_or_ref (+) { }
        sub refs (\$\@\%) { }
    "#;
    let file = lower_source(source);
    assert!(matches!(fact(&file, "scalar").shape.slots()[0].kind(), PrototypeSlotKind::Scalar));
    assert_eq!(
        fact(&file, "block_and_rest").shape.syntax_class(),
        PrototypeSyntaxClass::BlockTaking
    );
    assert!(matches!(fact(&file, "glob").shape.slots()[0].kind(), PrototypeSlotKind::Glob));
    assert!(matches!(
        fact(&file, "scalar_or_ref").shape.slots()[0].kind(),
        PrototypeSlotKind::ScalarOrReference
    ));
    assert_eq!(fact(&file, "refs").shape.slots().len(), 3);
}

#[test]
fn whitespace_equivalent_prototypes_share_a_digest_not_raw_text() {
    let file = lower_source("sub compact ($$) { }\nsub spaced ($ $) { }");
    let compact = fact(&file, "compact");
    let spaced = fact(&file, "spaced");
    assert_eq!(compact.content, "$$");
    assert_eq!(spaced.content, "$ $");
    assert_eq!(compact.shape.semantic_digest(), spaced.shape.semantic_digest());
    assert_ne!(compact.content, spaced.content);
}

#[test]
fn empty_prototype_is_nullary_on_the_fact() {
    let file = lower_source("sub empty () { }");
    let proto = fact(&file, "empty");
    assert_eq!(proto.content, "");
    assert!(proto.shape.slots().is_empty());
    assert_eq!(proto.shape.syntax_class(), PrototypeSyntaxClass::Nullary);
    assert!(proto.shape.is_exact());
}

#[test]
fn forward_declaration_still_publishes_a_shape() {
    let file = lower_source("sub forward($);");
    let proto = fact(&file, "forward");
    assert_eq!(proto.content, "$");
    assert!(proto.shape.is_exact());
}

#[test]
fn prototype_attribute_publishes_a_shape_without_a_second_parser() {
    let source = "sub via_attr :prototype($$) { }";
    let file = lower_source(source);
    let proto = fact(&file, "via_attr");
    assert_eq!(proto.content, "$$");
    assert_eq!(proto.shape.slots().len(), 2);
    assert!(proto.shape.is_exact());
}

#[test]
fn prototype_plus_signature_keeps_the_attribute_shape() {
    let source = "use v5.36; sub both :prototype($$) ($left, $right) { }";
    let file = lower_source(source);
    let proto = fact(&file, "both");
    assert_eq!(proto.content, "$$");
    assert!(proto.shape.is_exact());
}

#[test]
fn same_name_in_another_package_is_a_distinct_fact() {
    let file = lower_source("package A; sub shared ($) { }\npackage B; sub shared ($) { }\n");
    let facts: Vec<_> =
        file.prototype_table.facts.iter().filter(|fact| fact.sub_name == "shared").collect();
    assert_eq!(facts.len(), 2, "each package keeps its own prototype fact");
    assert_ne!(facts[0].package_context, facts[1].package_context);
    assert_eq!(facts[0].shape.semantic_digest(), facts[1].shape.semantic_digest());
}

#[test]
fn duplicate_semicolon_cannot_appear_exact_on_the_fact() {
    let file = lower_source("sub bad ($;;) { }");
    let proto = fact(&file, "bad");
    assert!(!proto.shape.is_exact());
    assert!(matches!(
        proto.shape.completeness(),
        perl_parser_core::prototype_shape::PrototypeCompleteness::Recovered {
            reason: PrototypeRecovery::DuplicateOptionalBoundary
        }
    ));
}

#[test]
fn invalid_prototype_text_cannot_appear_exact_on_the_fact() {
    let file = lower_source("sub bad (XYZ) { }");
    let proto = fact(&file, "bad");
    assert_eq!(proto.content, "XYZ");
    assert!(!proto.shape.is_exact());
}

#[test]
fn semicolon_signature_form_does_not_become_a_prototype_shape() {
    let source = "sub f ($x; $y) { }";
    let mut parser = Parser::new(source);
    let output = parser.parse_with_recovery();
    let file = lower_ast(&output.ast);
    assert!(
        file.prototype_table.facts.iter().all(|fact| fact.sub_name != "f"),
        "`;` in a signature-like header must not mint a prototype shape: {:?}",
        file.prototype_table.facts
    );
}

#[test]
fn recovered_shape_does_not_use_raw_string_equality_as_semantics() {
    let file = lower_source("sub a ($$) { }\nsub b ($;$) { }");
    let a = fact(&file, "a");
    let b = fact(&file, "b");
    assert_ne!(a.content, b.content);
    assert_ne!(
        a.shape.semantic_digest(),
        b.shape.semantic_digest(),
        "semantic digest must see the optional boundary"
    );
}
