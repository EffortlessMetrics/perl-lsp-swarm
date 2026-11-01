//! Perl 5.40.1 `-MO=Deparse,-p` quotes only the ungrouped, unqualified
//! bareword in `f($cond ? 'C' : k => 'v')`. With a declared `sub k`, a
//! comma or parentheses keep `k` executable. Argument count alone cannot
//! distinguish these ASTs.
mod cpan_test_helpers;

use perl_parser_core::{Node, NodeKind, Parser};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn clean(source: &str) -> Result<Node, Box<dyn std::error::Error>> {
    let output = Parser::new(source).parse_with_recovery();
    assert!(output.diagnostics.is_empty(), "{source}: {:?}", output.diagnostics);
    cpan_test_helpers::assert_clean_parse(source);
    Ok(output.ast)
}

fn call_args(ast: &Node) -> Result<&[Node], Box<dyn std::error::Error>> {
    let NodeKind::Program { statements } = &ast.kind else {
        return Err("expected Program".into());
    };
    assert_eq!(statements.len(), 1, "unexpected recovered or trailing statement");
    let node = statements.first().ok_or("missing call")?;
    let node = match &node.kind {
        NodeKind::ExpressionStatement { expression } => expression.as_ref(),
        _ => node,
    };
    let NodeKind::FunctionCall { name, args } = &node.kind else {
        return Err(format!("expected call, got {:?}", node.kind).into());
    };
    assert_eq!(name, "f");
    assert_eq!(args.len(), 2, "fat comma must remain an argument separator");
    Ok(args)
}

fn else_operand(node: &Node) -> Result<&Node, Box<dyn std::error::Error>> {
    let NodeKind::Ternary { else_expr, .. } = &node.kind else {
        return Err(format!("expected ternary, got {:?}", node.kind).into());
    };
    Ok(else_expr)
}

fn assert_key(source: &str, node: &Node) {
    assert!(
        matches!(&node.kind, NodeKind::String { value, interpolated: false } if value == "k"),
        "expected autoquoted k, got {:?}",
        node.kind
    );
    assert_eq!(source.get(node.location.start..node.location.end), Some("k"));
}

#[test]
fn direct_fat_arrow_quotes_the_else_operand_without_absorbing_its_sibling() -> TestResult {
    for source in [
        "f($cond ? 'C' : k => 'v');",
        "f($cond ? 'C' : k # key\n => 'v');",
        "f($cond ? 'C' : k\r\n => 'v');",
    ] {
        let ast = clean(source)?;
        let args = call_args(&ast)?;
        let ternary = args.first().ok_or("missing ternary")?;
        assert_key(source, else_operand(ternary)?);
        assert_eq!(
            source.get(ternary.location.start..ternary.location.end),
            Some("$cond ? 'C' : k")
        );
        let value = args.get(1).ok_or("missing sibling")?;
        assert_eq!(source.get(value.location.start..value.location.end), Some("'v'"));
        assert!(matches!(&value.kind, NodeKind::String { interpolated: false, .. }));
    }
    Ok(())
}

#[test]
fn nested_else_and_assignment_keep_the_direct_fat_arrow_boundary() -> TestResult {
    let source = "f($outer ? 'A' : $inner ? 'B' : k => 'v');";
    let ast = clean(source)?;
    let args = call_args(&ast)?;
    let inner = else_operand(args.first().ok_or("missing outer ternary")?)?;
    assert_key(source, else_operand(inner)?);

    let source = "f($result = $cond ? 'C' : k => 'v');";
    let ast = clean(source)?;
    let args = call_args(&ast)?;
    let NodeKind::Assignment { rhs, .. } = &args.first().ok_or("missing assignment")?.kind else {
        return Err("expected assignment".into());
    };
    assert_key(source, else_operand(rhs)?);
    Ok(())
}

#[test]
fn comma_and_grouped_operands_are_not_autoquoted() -> TestResult {
    for source in
        ["f($cond ? 'C' : k, 'v');", "f($cond ? 'C' : (k) => 'v');", "f(($cond ? 'C' : k) => 'v');"]
    {
        let ast = clean(source)?;
        let args = call_args(&ast)?;
        let operand = else_operand(args.first().ok_or("missing ternary")?)?;
        assert!(
            matches!(&operand.kind, NodeKind::Identifier { name } if name == "k"),
            "group/comma must retain the bareword, got {:?}",
            operand.kind
        );
    }
    Ok(())
}

#[test]
fn qualified_names_are_not_autoquoted() -> TestResult {
    let source = "f($cond ? 'C' : Foo::k => 'v');";
    let ast = clean(source)?;
    let args = call_args(&ast)?;
    let operand = else_operand(args.first().ok_or("missing ternary")?)?;
    assert!(
        matches!(&operand.kind, NodeKind::Identifier { name } if name == "Foo::k"),
        "qualified name must not become a string, got {:?}",
        operand.kind
    );
    Ok(())
}
