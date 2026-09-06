//! Behavioral proof for the scope-aware class grammar context (#10740).
//!
//! The parser admits `ADJUST { ... }` as a class member only while it is
//! inside class grammar; everywhere else `ADJUST` stays an ordinary
//! identifier. That admission used to ride on a scalar `in_class_body: usize`
//! counter and is now owned by a scope-aware context with structured
//! restoration.
//!
//! These tests pin the *observable* boundary rather than the mechanism, so
//! they hold across the replacement and would fail for a context that leaks
//! past a class body, is cleared by a nested block, or activates statement
//! form early.

mod cpan_test_helpers;

use cpan_test_helpers::*;
use perl_parser_core::{
    Node, NodeKind, Parser,
    incremental::{IncrementalEdit, IncrementalState},
};
use perl_tdd_support::{must, must_some};

/// Count the `ADJUST` blocks the parser admitted as class members.
///
/// An admitted ADJUST block is emitted as a `Method` node named `ADJUST`
/// (`parse_adjust_block`). Outside class grammar the same source stays an
/// ordinary identifier expression, so this count is the exact admission
/// signal — and it cannot be satisfied by an unrelated method that merely
/// appears nearby.
fn admitted_adjust_blocks(node: &Node) -> usize {
    let here = usize::from(matches!(&node.kind, NodeKind::Method { name, .. } if name == "ADJUST"));
    here + node.children().into_iter().map(admitted_adjust_blocks).sum::<usize>()
}

fn adjust_blocks_in(source: &str) -> usize {
    admitted_adjust_blocks(&parse(source))
}

// ---------------------------------------------------------------------------
// Positive control: the context does admit class members where it should.
// ---------------------------------------------------------------------------

#[test]
fn adjust_inside_a_block_class_body_is_admitted_as_a_class_member() {
    assert_eq!(
        adjust_blocks_in("class Foo { ADJUST { my $x = 1; } }"),
        1,
        "ADJUST directly inside a class body must be admitted as a class member"
    );
}

#[test]
fn each_of_two_sibling_classes_admits_its_own_members() {
    assert_eq!(
        adjust_blocks_in("class A { ADJUST { } }\nclass B { ADJUST { } }"),
        2,
        "leaving the first class body must not disable admission for the second"
    );
}

// ---------------------------------------------------------------------------
// Negative controls: class-only syntax outside class grammar stays ordinary.
//
// These are the falsifiers the previous scalar convention never had. A
// context that is entered but never restored passes every positive test above
// and fails each of these.
// ---------------------------------------------------------------------------

#[test]
fn adjust_outside_any_class_stays_ordinary_syntax() {
    assert_eq!(
        adjust_blocks_in("ADJUST { my $x = 1; }"),
        0,
        "ADJUST with no enclosing class must not be promoted to a class member"
    );
}

#[test]
fn adjust_after_a_class_body_closes_stays_ordinary_syntax() {
    assert_eq!(
        adjust_blocks_in("class Foo { }\nADJUST { my $x = 1; }"),
        0,
        "class grammar must not leak past the closing brace of the class body"
    );
}

#[test]
fn adjust_between_two_classes_stays_ordinary_syntax() {
    assert_eq!(
        adjust_blocks_in("class A { }\nADJUST { }\nclass B { }"),
        0,
        "the gap between two classes is outside class grammar"
    );
}

#[test]
fn adjust_after_nested_class_bodies_close_stays_ordinary_syntax() {
    assert_eq!(
        adjust_blocks_in("class Foo { class Bar { } }\nADJUST { }"),
        0,
        "unwinding nested class frames must return fully to outside class grammar"
    );
}

#[test]
fn adjust_after_a_class_nested_in_a_sub_stays_ordinary_syntax() {
    assert_eq!(
        adjust_blocks_in("sub outer { class Foo { } }\nADJUST { }"),
        0,
        "a class inside a sub must restore the surrounding non-class context"
    );
}

#[test]
fn a_recovered_class_body_does_not_leak_class_grammar() {
    // The class body contains a syntax error and is closed by recovery. The
    // frame must still be restored, so the following ADJUST stays ordinary.
    assert_eq!(
        adjust_blocks_in("class Foo { ] }\nADJUST { }"),
        0,
        "error recovery inside a class body must not leave class grammar active"
    );
}

// ---------------------------------------------------------------------------
// Restoration must be exact, not merely "cleared".
// ---------------------------------------------------------------------------

#[test]
fn leaving_an_inner_class_body_restores_the_enclosing_class_context() {
    // The ADJUST here follows the inner class but is still inside the outer
    // class body. A context that clears on exit instead of restoring the
    // enclosing frame would drop this admission.
    assert_eq!(
        adjust_blocks_in("class Foo { class Bar { } ADJUST { } }"),
        1,
        "closing an inner class must restore the enclosing class frame, not clear it"
    );
}

#[test]
fn a_nested_ordinary_block_does_not_clear_the_enclosing_class_context() {
    // Inherited admission behavior: while inside a class body the parser
    // admits ADJUST within nested blocks too. #10740 preserves this exactly;
    // narrowing admission to the class body's own statement list is a
    // semantic-scope decision owned downstream (#10346 / #6672), not a
    // parser-state change.
    assert_eq!(
        adjust_blocks_in("class Foo { if (1) { ADJUST { } } }"),
        1,
        "a nested ordinary block must not erase the enclosing class context"
    );
    assert_eq!(
        adjust_blocks_in("class Foo { method m { ADJUST { } } }"),
        1,
        "a nested method body must not erase the enclosing class context"
    );
}

// ---------------------------------------------------------------------------
// Statement form is represented but must not be admitted by this change.
// ---------------------------------------------------------------------------

#[test]
fn statement_form_classes_are_still_not_admitted() {
    // #10864 owns activating `class Foo;`. Until then the parser must keep
    // rejecting it, and it must not open a class grammar frame for the
    // statements that follow.
    let mut parser = Parser::new("class Foo;\nADJUST { }");
    let ast = must(parser.parse());

    assert!(
        !parser.errors().is_empty(),
        "statement-form class must still be a parse error until #10864 activates it"
    );
    assert_eq!(
        admitted_adjust_blocks(&ast),
        0,
        "a rejected statement-form class must not activate class grammar for following statements"
    );
}

#[test]
fn no_class_declaration_node_is_emitted_for_statement_form() {
    fn has_class(node: &Node) -> bool {
        matches!(node.kind, NodeKind::Class { .. }) || node.children().into_iter().any(has_class)
    }

    assert!(
        !has_class(&parse("class Foo;")),
        "statement-form class must not yet emit a class declaration node"
    );
}

// ---------------------------------------------------------------------------
// Constructs that are NOT gated on class grammar must stay ungated.
// ---------------------------------------------------------------------------

#[test]
fn field_and_method_keep_their_identity_outside_class_grammar() {
    // `field` and `method` are admitted by token lookahead, not by the class
    // grammar context. Pinning this keeps a later change from quietly routing
    // them through the context and altering behavior outside classes.
    fn has_field(node: &Node) -> bool {
        matches!(&node.kind, NodeKind::VariableDeclaration { declarator, .. } if declarator == "field")
            || node.children().into_iter().any(has_field)
    }
    fn has_named_method(node: &Node, wanted: &str) -> bool {
        matches!(&node.kind, NodeKind::Method { name, .. } if name == wanted)
            || node.children().into_iter().any(|child| has_named_method(child, wanted))
    }

    assert!(has_field(&parse("field $x;")), "`field` must parse outside a class as it does today");
    assert!(
        has_named_method(&parse("method m { 1 }"), "m"),
        "`method` must parse outside a class as it does today"
    );
}

// ---------------------------------------------------------------------------
// Fresh and incremental parses must agree, including on admission.
// ---------------------------------------------------------------------------

#[test]
fn an_edit_inside_a_class_body_reparses_to_the_fresh_result() {
    let source = "class Foo {\n    ADJUST { my $x = 1; }\n}\nADJUST { }\n";
    let new_source = "class Foo {\n    ADJUST { my $x = 22; }\n}\nADJUST { }\n";
    let start = must_some(source.find('1'));
    let edit = IncrementalEdit::new(start, start + 1, "22");

    let mut state = IncrementalState::new(source);
    let incremental = must(state.reparse(new_source, &edit));
    let mut fresh_parser = Parser::new(new_source);
    let fresh = must(fresh_parser.parse());

    assert_eq!(
        incremental.to_sexp(),
        fresh.to_sexp(),
        "incremental reparse must match the fresh parse of the same final source"
    );
    assert_eq!(
        state.diagnostics(),
        fresh_parser.errors(),
        "incremental diagnostics must match the fresh parse"
    );
    assert_eq!(
        admitted_adjust_blocks(&fresh),
        1,
        "only the in-class ADJUST is admitted; the trailing one stays ordinary"
    );
}
