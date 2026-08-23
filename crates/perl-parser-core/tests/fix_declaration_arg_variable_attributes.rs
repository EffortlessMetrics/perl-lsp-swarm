//! Declaration-as-argument list attributes (#7633).
//!
//! Statement-form `my ($x :shared, $y)` already attaches per-item attributes.
//! Readonly/Const::Fast declaration-arg lists must observe the same Colon /
//! empty-attribute boundaries so `$tagged_ro` remains visible to semantic
//! frozen-modifier oracles.

mod cpan_test_helpers;
use cpan_test_helpers::*;
use perl_parser_core::{Node, NodeKind};
use perl_tdd_support::must_some;

fn find_list_decl(node: &Node) -> Option<&Node> {
    if matches!(node.kind, NodeKind::VariableListDeclaration { .. }) {
        return Some(node);
    }
    node.children().into_iter().find_map(find_list_decl)
}

fn list_variables(node: &Node) -> &[Node] {
    match &node.kind {
        NodeKind::VariableListDeclaration { variables, .. } => variables,
        _ => &[],
    }
}

#[test]
fn readonly_list_declaration_accepts_per_variable_attributes() {
    let source = r#"
use Readonly;
Readonly my ($tagged_ro :shared, $plain_ro) => (1, 2);
"#;
    assert_clean_parse(source);
}

#[test]
fn const_fast_list_declaration_accepts_per_variable_attributes() {
    let source = r#"
use Const::Fast;
const my ($tagged :shared, $plain) => (1, 2);
"#;
    assert_clean_parse(source);
}

#[test]
fn declaration_arg_without_colon_keeps_bare_variables() {
    // Discriminator: peek is not Colon → attributes stay empty → no wrap.
    let source = "Readonly my ($plain_a, $plain_b) => (1, 2);\n";
    assert_clean_parse(source);
    let ast = parse(source);
    let list = must_some(find_list_decl(&ast));
    let vars = list_variables(list);
    assert_eq!(vars.len(), 2);
    for var in vars {
        assert!(
            !matches!(var.kind, NodeKind::VariableWithAttributes { .. }),
            "no-colon path must not wrap VariableWithAttributes, got: {}",
            var.to_sexp()
        );
    }
}

#[test]
fn declaration_arg_colon_wraps_only_tagged_variable() {
    // Discriminator: Colon path builds VariableWithAttributes; sibling stays bare.
    let source = "Readonly my ($tagged_ro :shared, $plain_ro) => (1, 2);\n";
    assert_clean_parse(source);
    let ast = parse(source);
    let list = must_some(find_list_decl(&ast));
    let vars = list_variables(list);
    assert_eq!(vars.len(), 2);

    match &vars[0].kind {
        NodeKind::VariableWithAttributes { variable, attributes } => {
            assert!(
                matches!(
                    variable.kind,
                    NodeKind::Variable { ref name, .. } if name == "tagged_ro"
                ),
                "tagged slot variable, got: {}",
                variable.to_sexp()
            );
            assert_eq!(attributes, &["shared".to_string()]);
        }
        other => panic!("expected VariableWithAttributes for :shared, got: {other:?}"),
    }

    assert!(
        matches!(
            &vars[1].kind,
            NodeKind::Variable { name, .. } if name == "plain_ro"
        ),
        "plain sibling must stay bare Variable, got: {}",
        vars[1].to_sexp()
    );
}

#[test]
fn readonly_list_declaration_preserves_attribute_node() {
    let source = "Readonly my ($tagged_ro :shared, $plain_ro) => (1, 2);\n";
    let ast = parse(source);
    let sexp = ast.to_sexp();
    assert!(
        sexp.contains("attributes shared"),
        "expected VariableWithAttributes for :shared, got: {sexp}"
    );
    assert!(
        !sexp.contains("ERROR"),
        "declaration-arg attribute list must parse cleanly, got: {sexp}"
    );
}
