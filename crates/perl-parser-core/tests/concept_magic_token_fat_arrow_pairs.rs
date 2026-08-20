//! Exact fat-arrow attachment contracts for Perl magic tokens (#6670).
//!
//! Per perlop: the fat-arrow operator (`=>`) auto-quotes the word on its left if it
//! begins with a letter or underscore and is composed only of letters, digits, and
//! underscores.  Magic tokens (`__FILE__`, `__LINE__`, `__PACKAGE__`, `__SUB__`) fit
//! that pattern, so they ARE autoquoted in key position and become String nodes rather
//! than FunctionCall nodes.  Only magic tokens in *value* position (to the right of
//! `=>`) remain nullary FunctionCall nodes.

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

        // Keys before `=>` are autoquoted by the fat-arrow operator: they are stored as
        // non-interpolated String nodes with the bare word as the value.
        let (first_key, first_value) = &pairs[0];
        assert!(
            matches!(&first_key.kind, NodeKind::String { value, interpolated } if value == "__FILE__" && !*interpolated),
            "first key (__FILE__ before =>) must be an autoquoted String node; got {:?}",
            first_key.kind.kind_name()
        );
        assert!(matches!(&first_value.kind, NodeKind::Identifier { name } if name == "line"));
        assert_eq!(source_text(source, first_key), Some("__FILE__"));
        assert_eq!(source_text(source, first_value), Some("line"));

        // "file" before `=>` is also autoquoted.
        let (second_key, second_value) = &pairs[1];
        assert!(
            matches!(&second_key.kind, NodeKind::String { value, .. } if value == "file"),
            "second key (file before =>) must be an autoquoted String node; got {:?}",
            second_key.kind.kind_name()
        );
        // Magic token in value position remains a nullary FunctionCall.
        assert!(matches!(
            &second_value.kind,
            NodeKind::FunctionCall { name, args } if name == "__FILE__" && args.is_empty()
        ));
        assert_eq!(source_text(source, second_key), Some("file"));
        assert_eq!(source_text(source, second_value), Some("__FILE__"));

        // "line" before `=>` is also autoquoted; __LINE__ in value position stays FunctionCall.
        let (third_key, third_value) = &pairs[2];
        assert!(
            matches!(&third_key.kind, NodeKind::String { value, .. } if value == "line"),
            "third key (line before =>) must be an autoquoted String node; got {:?}",
            third_key.kind.kind_name()
        );
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
