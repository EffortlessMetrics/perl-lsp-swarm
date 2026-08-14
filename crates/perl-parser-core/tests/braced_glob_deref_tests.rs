mod cpan_test_helpers;
use cpan_test_helpers::*;

use perl_ast::ast::{Node, NodeKind};
use perl_parser_core::Parser;
use perl_tdd_support::must;

fn find_unary_operand<'a>(node: &'a Node, operator: &str) -> Option<&'a Node> {
    if let NodeKind::Unary { op, operand } = &node.kind {
        if op == operator {
            return Some(operand);
        }
    }

    node.children().into_iter().find_map(|child| find_unary_operand(child, operator))
}

fn find_assignment<'a>(node: &'a Node) -> Option<&'a Node> {
    if matches!(node.kind, NodeKind::Assignment { .. }) {
        return Some(node);
    }

    node.children().into_iter().find_map(find_assignment)
}

fn statement_expression(node: &Node) -> &Node {
    match &node.kind {
        NodeKind::ExpressionStatement { expression } => expression,
        _ => node,
    }
}

fn typeglob_name(node: &Node) -> Option<&str> {
    match &node.kind {
        NodeKind::Typeglob { name } => Some(name),
        _ => None,
    }
}

#[test]
fn braced_glob_deref_forms_parse_cleanly() {
    for source in ["*{$ref};", "*{$self->{key}};"] {
        assert_clean_parse(source);
    }
}

#[test]
fn braced_glob_deref_uses_last_expression_as_operand() -> Result<(), Box<dyn std::error::Error>> {
    let source = "*{$tmp; 'STDOUT'};";
    let mut parser = Parser::new(source);
    let ast = must(parser.parse());
    let operand = find_unary_operand(&ast, "*{}").ok_or("expected braced glob dereference")?;
    let NodeKind::Block { statements } = &operand.kind else {
        return Err(format!("expected the braced glob body to remain a block: {operand:?}").into());
    };
    if statements.len() != 2 {
        return Err("expected both inline expressions to remain".into());
    }
    if !matches!(
        statement_expression(&statements[0]).kind,
        NodeKind::Variable { ref sigil, ref name } if sigil == "$" && name == "tmp"
    ) {
        return Err("expected the first inline expression to be $tmp".into());
    }
    if !matches!(
        statement_expression(&statements[1]).kind,
        NodeKind::String { ref value, .. } if value == "'STDOUT'"
    ) {
        return Err("expected the final inline expression to be 'STDOUT'".into());
    }
    Ok(())
}

#[test]
fn split_token_glob_body_preserves_multiple_expressions() -> Result<(), Box<dyn std::error::Error>>
{
    let source = "* { $tmp; 'STDOUT' };";
    let mut parser = Parser::new(source);
    let ast = must(parser.parse());
    let operand = find_unary_operand(&ast, "*{}").ok_or("expected split-token glob dereference")?;
    let NodeKind::Block { statements } = &operand.kind else {
        return Err(
            format!("expected the split-token glob body to remain a block: {operand:?}").into()
        );
    };
    if statements.len() != 2 {
        return Err("expected both split-token expressions to remain".into());
    }
    if !matches!(
        statement_expression(&statements[1]).kind,
        NodeKind::String { ref value, .. } if value == "'STDOUT'"
    ) {
        return Err("expected the final split-token expression to be 'STDOUT'".into());
    }
    Ok(())
}

#[test]
fn split_token_glob_assignment_preserves_typeglob_lhs() -> Result<(), Box<dyn std::error::Error>> {
    let source = "* { $name; } = \\&target;";
    let mut parser = Parser::new(source);
    let ast = must(parser.parse());
    let assignment = find_assignment(&ast).ok_or("expected dynamic typeglob assignment")?;
    let NodeKind::Assignment { lhs, .. } = &assignment.kind else {
        return Err("find_assignment returned a non-assignment node".into());
    };
    if typeglob_name(lhs) != Some("$name") {
        return Err("expected Typeglob name $name on the dynamic assignment LHS".into());
    }
    Ok(())
}

#[test]
fn fused_token_glob_assignment_preserves_typeglob_name() -> Result<(), Box<dyn std::error::Error>> {
    let source = "*{$name;} = \\&target;";
    let mut parser = Parser::new(source);
    let ast = must(parser.parse());
    let assignment = find_assignment(&ast).ok_or("expected fused dynamic typeglob assignment")?;
    let NodeKind::Assignment { lhs, .. } = &assignment.kind else {
        return Err("find_assignment returned a non-assignment node".into());
    };
    if typeglob_name(lhs) != Some("$name") {
        return Err(format!("expected Typeglob name $name on fused LHS, got {:?}", lhs.kind).into());
    }
    Ok(())
}

#[test]
fn postfix_dynamic_glob_assignment_is_not_typeglob() -> Result<(), Box<dyn std::error::Error>> {
    let source = "* { $glob }{CODE} = \\&target;";
    let mut parser = Parser::new(source);
    let ast = parser.parse()?;
    let assignment = find_assignment(&ast).ok_or("expected postfix dynamic glob assignment")?;
    let NodeKind::Assignment { lhs, .. } = &assignment.kind else {
        return Err("find_assignment returned a non-assignment node".into());
    };
    if matches!(lhs.kind, NodeKind::Typeglob { .. }) {
        return Err("postfix dynamic glob assignment must not use a Typeglob LHS".into());
    }
    if find_unary_operand(lhs, "*{}").is_none() {
        return Err("expected postfix dynamic glob assignment to retain its deref LHS".into());
    }
    Ok(())
}

#[test]
fn fused_postfix_dynamic_glob_assignment_is_not_typeglob() -> Result<(), Box<dyn std::error::Error>>
{
    let source = "*{$glob}{CODE} = \\&target;";
    let mut parser = Parser::new(source);
    let ast = parser.parse()?;
    if !parser.errors().is_empty() {
        return Err(format!("expected clean parse, got {:?}", parser.errors()).into());
    }
    let assignment =
        find_assignment(&ast).ok_or("expected fused postfix dynamic glob assignment")?;
    let NodeKind::Assignment { lhs, .. } = &assignment.kind else {
        return Err("find_assignment returned a non-assignment node".into());
    };
    if matches!(lhs.kind, NodeKind::Typeglob { .. }) {
        return Err("fused postfix dynamic glob assignment must not use a Typeglob LHS".into());
    }
    if find_unary_operand(lhs, "*{}").is_none() {
        return Err("expected fused postfix dynamic glob assignment to retain its deref LHS".into());
    }
    Ok(())
}

#[test]
fn braced_glob_postfix_form_remains_a_deref() {
    let source = "*{$glob}{CODE};";
    assert_clean_parse(source);
    assert_clean_parse("* { $glob }{CODE};");
}

#[test]
fn inline_glob_forwards_recoverable_diagnostics() -> Result<(), Box<dyn std::error::Error>> {
    let source = r#"*{"abab" =~ /(?:[^b]*(?=(b)|(a))ab)*/};"#;
    let mut parser = Parser::new(source);
    parser.parse()?;
    if !parser.errors().iter().any(|diagnostic| {
        matches!(diagnostic, perl_parser_core::ParseError::Advisory { message, .. }
            if message.contains("Nested quantifiers detected"))
    }) {
        return Err(format!("expected forwarded inline advisory, got {:?}", parser.errors()).into());
    }
    Ok(())
}
