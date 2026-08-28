//! Regression coverage for Perl's postfix hash-slice dereference form.
//!
//! `EXPR->@{KEYS}` is the postfix equivalent of `@{EXPR}{KEYS}`. It is
//! distinct from both hash-element access (`EXPR->{KEY}`) and postfix array
//! slicing (`EXPR->@[INDICES]`), so the parser must retain a `HashSlice` node.

mod cpan_test_helpers;
use cpan_test_helpers::*;

use perl_parser_core::{Node, NodeKind, Parser};

fn hash_slices<'a>(node: &'a Node, found: &mut Vec<&'a Node>) {
    if matches!(&node.kind, NodeKind::HashSlice { .. }) {
        found.push(node);
    }
    for child in node.children() {
        hash_slices(child, found);
    }
}

fn assert_one_hash_slice(source: &str) {
    assert_clean_parse(source);
    let ast = parse(source);
    let mut slices = Vec::new();
    hash_slices(&ast, &mut slices);
    assert_eq!(
        slices.len(),
        1,
        "expected exactly one HashSlice for source:\n{source}\n\nAST:\n{}",
        ast.to_sexp()
    );
}

#[test]
fn postfix_hash_slice_preserves_target_keys_and_full_span() {
    let source = "$href->@{'alpha', $key};";
    let ast = parse(source);
    let mut slices = Vec::new();
    hash_slices(&ast, &mut slices);
    let [slice] = slices.as_slice() else {
        panic!("expected one HashSlice, got {}\n{}", slices.len(), ast.to_sexp());
    };

    assert_eq!(&source[slice.location.start..slice.location.end], "$href->@{'alpha', $key}");
    let NodeKind::HashSlice { target, keys } = &slice.kind else {
        unreachable!("hash_slices returned a non-HashSlice node");
    };
    assert_eq!(&source[target.location.start..target.location.end], "$href");
    assert_eq!(&source[keys.location.start..keys.location.end], "'alpha', $key");
    assert!(
        matches!(&target.kind, NodeKind::Variable { sigil, name } if sigil == "$" && name == "href")
    );
    let NodeKind::ArrayLiteral { elements } = &keys.kind else {
        panic!("expected an ArrayLiteral key list, got {}", keys.kind.kind_name());
    };
    assert_eq!(elements.len(), 2);
    assert!(
        matches!(&elements[1].kind, NodeKind::Variable { sigil, name } if sigil == "$" && name == "key")
    );
}

#[test]
fn postfix_hash_slice_with_qw_keys() {
    assert_one_hash_slice("my @values = $href->@{qw(alpha beta)};");
}

#[test]
fn postfix_hash_slice_with_variable_keys() {
    assert_one_hash_slice("my @values = $href->@{@keys};");
}

#[test]
fn postfix_hash_slice_remains_an_lvalue() {
    assert_one_hash_slice("$href->@{qw(alpha beta)} = (1, 2);");
}

#[test]
fn postfix_hash_slice_after_chained_receiver() {
    assert_one_hash_slice("my @values = $object->{payload}->@{qw(alpha beta)};");
}

#[test]
fn neighboring_postfix_forms_keep_their_existing_nodes() {
    let source = "my @values = $aref->@[0, 2]; my %pairs = $href->%{qw(alpha beta)};";
    assert_clean_parse(source);
    let ast = parse(source);
    let mut slices = Vec::new();
    hash_slices(&ast, &mut slices);
    assert!(
        slices.is_empty(),
        "array and key/value postfix slices must not be reclassified as HashSlice: {}",
        ast.to_sexp()
    );
}

#[test]
fn incomplete_postfix_hash_slice_recovers_without_panicking() {
    let source = "$href->@{'alpha', $key;";
    let mut parser = Parser::new(source);
    let output = parser.parse_with_recovery();
    assert!(!output.diagnostics.is_empty(), "truncated hash slice must retain diagnostics");
    assert!(matches!(output.ast.kind, NodeKind::Program { .. }));
}

#[test]
fn postfix_hash_slice_reaches_canonical_hir_lowering() {
    use perl_parser_core::hir::{HirExpr, lower_ast};

    let source = "my @values = $href->@{'alpha', $key};";
    let mut parser = Parser::new(source);
    let output = parser.parse_with_recovery();
    let file = lower_ast(&output.ast);
    let body = file.root_body().expect("lower_ast must expose a root body");
    let calls: Vec<_> = (0..body.exprs.len())
        .filter_map(|index| match body.expr(perl_parser_core::hir::HirExprId(index as u32)) {
            Some(HirExpr::Call { ast_kind, args, .. }) if ast_kind == "HashSlice" => {
                Some((args.len(), body.source_map.expr_ranges[index]))
            }
            _ => None,
        })
        .collect();
    assert_eq!(calls.len(), 1, "expected one lowered HashSlice call");
    assert_eq!(calls[0].0, 3, "lowering must retain target plus both key operands");
    assert_eq!(&source[calls[0].1.start..calls[0].1.end], "$href->@{'alpha', $key}");
}
