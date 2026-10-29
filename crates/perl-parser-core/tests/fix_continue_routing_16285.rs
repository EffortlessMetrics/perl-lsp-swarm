//! Continue-routing coverage for the when-block fall-through op (#16285).
//!
//! Real-Perl ground truth (`perl -c`): `continue` is a loop-control op in
//! statement, short-circuit, ternary, and empty-parens positions;
//! `continue OUTER` and `continue(1)` are syntax errors.

use perl_ast::ast::{Node, NodeKind};
use perl_parser_core::Parser;

fn parse_clean(source: &str) -> Result<Node, String> {
    let mut parser = Parser::new(source);
    let ast = parser.parse().map_err(|e| format!("expected `{source}` to parse, got {e:?}"))?;
    if !parser.errors().is_empty() {
        return Err(format!(
            "expected no recorded errors for `{source}`, got {:?}",
            parser.errors()
        ));
    }
    Ok(ast)
}

fn parse_rejected(source: &str) -> Result<Node, String> {
    let mut parser = Parser::new(source);
    let ast = parser
        .parse()
        .map_err(|e| format!("recovery keeps `{source}` total; unexpected hard error {e:?}"))?;
    if parser.errors().is_empty() {
        return Err(format!(
            "expected a recorded rejection for `{source}`, got none in {}",
            ast.to_sexp()
        ));
    }
    Ok(ast)
}

fn loop_control_ops(node: &Node, out: &mut Vec<(String, Option<String>)>) {
    if let NodeKind::LoopControl { op, label } = &node.kind {
        out.push((op.clone(), label.clone()));
    }
    for child in node.children() {
        loop_control_ops(child, out);
    }
}

fn ops_in(source: &str) -> Result<Vec<(String, Option<String>)>, String> {
    let ast = parse_clean(source)?;
    let mut ops = Vec::new();
    loop_control_ops(&ast, &mut ops);
    Ok(ops)
}

#[test]
fn expression_position_continue_is_loop_control() -> Result<(), String> {
    // `$ready and continue;` is valid Perl; the right operand must be a
    // LoopControl node, not a bareword Identifier.
    let ops = ops_in("while (1) { $ready and continue; }")?;
    if !ops.iter().any(|(op, label)| op == "continue" && label.is_none()) {
        return Err(format!("expected a bare `continue` LoopControl, got {ops:?}"));
    }
    Ok(())
}

#[test]
fn ternary_continue_is_loop_control() -> Result<(), String> {
    let ops = ops_in("while (1) { $ready ? continue : 0; }")?;
    if !ops.iter().any(|(op, label)| op == "continue" && label.is_none()) {
        return Err(format!("expected a bare `continue` LoopControl, got {ops:?}"));
    }
    Ok(())
}

#[test]
fn empty_parens_continue_is_single_node() -> Result<(), String> {
    let ast = parse_clean("while (1) { continue(); }")?;
    let mut ops = Vec::new();
    loop_control_ops(&ast, &mut ops);
    if ops != vec![("continue".to_string(), None)] {
        return Err(format!("expected exactly one bare `continue` node, got {ops:?}"));
    }
    Ok(())
}

#[test]
fn non_empty_parens_continue_is_rejected() -> Result<(), String> {
    // `continue(1)` is a syntax error in real Perl.
    parse_rejected("while (1) { continue(1); }")?;
    Ok(())
}

#[test]
fn labeled_continue_is_rejected() -> Result<(), String> {
    // `continue OUTER` is a syntax error in real Perl: continue takes no label.
    let ast = parse_rejected("while (1) { continue OUTER; }")?;
    let mut ops = Vec::new();
    loop_control_ops(&ast, &mut ops);
    if ops.iter().any(|(_, label)| label.is_some()) {
        return Err(format!("no LoopControl node may carry a label for `continue`, got {ops:?}"));
    }
    Ok(())
}

#[test]
fn fat_arrow_continue_stays_bareword() -> Result<(), String> {
    // `continue => 1` autoquotes: the keyword becomes a string hash key, not
    // a LoopControl node.
    let ast = parse_clean("my %h = (continue => 1);")?;
    let sexp = ast.to_sexp();
    if !sexp.contains("(key (string (value continue)))") {
        return Err(format!("expected autoquoted hash key, got: {sexp}"));
    }
    let mut ops = Vec::new();
    loop_control_ops(&ast, &mut ops);
    if !ops.is_empty() {
        return Err(format!("expected no LoopControl nodes, got {ops:?}"));
    }
    Ok(())
}

#[test]
fn when_block_continue_has_loop_control_shape() -> Result<(), String> {
    // The when-block fall-through `continue` must be a LoopControl node, not
    // merely a substring in the rendered tree.
    let ops = ops_in("given ($x) { when (1) { do_thing(); continue } when (2) { do_other(); } }")?;
    if !ops.iter().any(|(op, label)| op == "continue" && label.is_none()) {
        return Err(format!("expected a bare `continue` LoopControl, got {ops:?}"));
    }
    Ok(())
}
