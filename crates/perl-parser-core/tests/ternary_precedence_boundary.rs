use perl_parser_core::{Node, NodeKind, Parser};

fn find<'a>(node: &'a Node, predicate: &impl Fn(&NodeKind) -> bool) -> Option<&'a Node> {
    if predicate(&node.kind) {
        Some(node)
    } else {
        node.children().into_iter().find_map(|child| find(child, predicate))
    }
}

fn text<'a>(source: &'a str, node: &Node) -> &'a str {
    &source[node.location.start..node.location.end]
}

#[test]
fn statement_ternary_leaves_following_list_to_statement() -> Result<(), Box<dyn std::error::Error>>
{
    for (source, expected_else) in [
        ("defined $x ? 1 : 2, key => 3;", "2"),
        ("defined $x ? 1 : $y ? 2 : 3, key => 4;", "$y ? 2 : 3"),
    ] {
        let result = Parser::new(source).parse_with_recovery();
        assert!(result.diagnostics.is_empty(), "{source}: {:?}", result.diagnostics);
        let list = find(&result.ast, &|kind| matches!(kind, NodeKind::ArrayLiteral { .. }))
            .ok_or("missing surrounding list")?;
        let NodeKind::ArrayLiteral { elements } = &list.kind else {
            return Err("wrong list".into());
        };
        assert_eq!(elements.len(), 3, "{source}: {list:?}");
        let NodeKind::Ternary { else_expr, .. } = &elements[0].kind else {
            return Err("missing leading ternary".into());
        };
        assert_eq!(text(source, else_expr), expected_else);
        assert_eq!(text(source, &elements[1]), "key");
        assert_eq!(text(source, &elements[2]), if source.ends_with("4;") { "4" } else { "3" });
    }
    Ok(())
}

#[test]
fn grouped_false_arm_retains_its_list() -> Result<(), Box<dyn std::error::Error>> {
    let source = "defined $x ? 1 : (2, 3);";
    let result = Parser::new(source).parse_with_recovery();
    assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
    let ternary = find(&result.ast, &|kind| matches!(kind, NodeKind::Ternary { .. }))
        .ok_or("missing ternary")?;
    let NodeKind::Ternary { else_expr, .. } = &ternary.kind else {
        return Err("wrong node".into());
    };
    assert_eq!(text(source, else_expr), "(2, 3)");
    assert!(matches!(&else_expr.kind, NodeKind::ArrayLiteral { elements } if elements.len() == 2));
    Ok(())
}
