//! Exact fat-arrow attachment contracts for Perl magic tokens (#6670).

use perl_parser_core::{Node, NodeKind, Parser};

fn walk(node: &Node, visit: &mut impl FnMut(&Node)) {
    visit(node);
    for child in node.children() {
        walk(child, visit);
    }
}

fn source_text<'a>(source: &'a str, node: &Node) -> Option<&'a str> {
    source.get(node.location.start..node.location.end)
}

#[test]
fn magic_tokens_keep_exact_hash_key_and_value_positions() -> Result<(), String> {
    let source = "my %metadata = (__FILE__ => line, file => __FILE__, line => __LINE__);";
    let mut parser = Parser::new(source);
    let ast = parser.parse().map_err(|error| format!("parse failed: {error:?}"))?;
    if !parser.errors().is_empty() {
        return Err(format!("expected a clean parse, got diagnostics: {:?}", parser.errors()));
    }

    let mut proven = false;
    walk(&ast, &mut |node| {
        let NodeKind::HashLiteral { pairs } = &node.kind else {
            return;
        };
        if pairs.len() != 3 || source_text(source, &pairs[0].0) != Some("__FILE__") {
            return;
        }

        let (first_key, first_value) = &pairs[0];
        assert!(matches!(
            &first_key.kind,
            NodeKind::FunctionCall { name, args } if name == "__FILE__" && args.is_empty()
        ));
        assert!(matches!(&first_value.kind, NodeKind::Identifier { name } if name == "line"));
        assert_eq!(source_text(source, first_key), Some("__FILE__"));
        assert_eq!(source_text(source, first_value), Some("line"));

        let (second_key, second_value) = &pairs[1];
        assert!(matches!(&second_key.kind, NodeKind::Identifier { name } if name == "file"));
        assert!(matches!(
            &second_value.kind,
            NodeKind::FunctionCall { name, args } if name == "__FILE__" && args.is_empty()
        ));
        assert_eq!(source_text(source, second_key), Some("file"));
        assert_eq!(source_text(source, second_value), Some("__FILE__"));

        let (third_key, third_value) = &pairs[2];
        assert!(matches!(&third_key.kind, NodeKind::Identifier { name } if name == "line"));
        assert!(matches!(
            &third_value.kind,
            NodeKind::FunctionCall { name, args } if name == "__LINE__" && args.is_empty()
        ));
        assert_eq!(source_text(source, third_key), Some("line"));
        assert_eq!(source_text(source, third_value), Some("__LINE__"));
        proven = true;
    });

    assert!(proven, "magic-token hash pair geometry was not preserved");
    Ok(())
}
