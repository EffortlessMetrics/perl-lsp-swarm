mod cpan_test_helpers;
use cpan_test_helpers::parse;
use perl_parser_core::{Node, NodeKind};

fn statement(source: &str) -> Node {
    let ast = parse(source);
    match &ast.kind {
        NodeKind::Program { statements } => {
            assert_eq!(statements.len(), 1, "{source}");
            statements[0].clone()
        }
        other => panic!("expected program: {other:?}"),
    }
}

#[test]
fn word_or_follows_list_declaration() {
    let outer = statement("my ($a, $b) = (1, 2) or die;");
    match &outer.kind {
        NodeKind::Binary { op, left, .. } => {
            assert_eq!(op, "or");
            assert!(matches!(left.kind, NodeKind::VariableListDeclaration { .. }), "{outer:?}");
        }
        _ => panic!("expected outer or: {outer:?}"),
    }
}

#[test]
fn grouped_word_or_remains_in_initializer() {
    let grouped = statement("my ($a, $b) = ((1, 2) or die);");
    match &grouped.kind {
        NodeKind::VariableListDeclaration { initializer: Some(rhs), .. } => {
            assert!(matches!(&rhs.kind, NodeKind::Binary { op, .. } if op == "or"), "{grouped:?}");
        }
        _ => panic!("expected declaration: {grouped:?}"),
    }
}

#[test]
fn comma_follows_assignment_before_word_or() {
    let ast = statement("my ($a, $b) = 1, 2 or die;");
    let NodeKind::ExpressionStatement { expression } = &ast.kind else { panic!("{ast:?}") };
    let NodeKind::Binary { op, left, .. } = &expression.kind else { panic!("{ast:?}") };
    assert_eq!(op, "or");
    let NodeKind::ArrayLiteral { elements } = &left.kind else { panic!("{ast:?}") };
    assert_eq!(elements.len(), 2);
    assert!(
        matches!(&elements[0].kind, NodeKind::VariableListDeclaration { initializer: Some(rhs), .. } if matches!(&rhs.kind, NodeKind::Number { value } if value == "1")),
        "{ast:?}"
    );
    assert!(matches!(&elements[1].kind, NodeKind::Number { value } if value == "2"), "{ast:?}");
}

#[test]
fn list_operator_keeps_its_arguments() {
    let ast = statement("my ($a, $b) = split /,/, $x or die;");
    let NodeKind::Binary { op, left, .. } = &ast.kind else { panic!("{ast:?}") };
    assert_eq!(op, "or");
    let NodeKind::VariableListDeclaration { initializer: Some(rhs), .. } = &left.kind else {
        panic!("{ast:?}")
    };
    assert!(
        matches!(&rhs.kind, NodeKind::FunctionCall { name, args } if name == "split" && args.len() == 2 && matches!(args[1].kind, NodeKind::Variable { .. })),
        "{ast:?}"
    );
}

#[test]
fn word_or_in_condition_wraps_declaration() {
    let ast = statement("if (my ($a, $b) = (1, 2) or $flag) { 1; }");
    let NodeKind::If { condition, .. } = &ast.kind else { panic!("{ast:?}") };
    assert!(
        matches!(&condition.kind, NodeKind::Binary { op, left, .. } if op == "or" && matches!(left.kind, NodeKind::VariableListDeclaration { .. })),
        "{ast:?}"
    );
}
