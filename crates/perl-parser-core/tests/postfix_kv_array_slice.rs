mod cpan_test_helpers;

use cpan_test_helpers::{assert_clean_parse, assert_no_blocking_diagnostics, parse};
use perl_parser_core::{Node, NodeKind};

fn find_slice<'a>(node: &'a Node, op: &str) -> Option<&'a Node> {
    if matches!(&node.kind, NodeKind::Binary { op: actual, .. } if actual == op) {
        return Some(node);
    }
    node.children().into_iter().find_map(|child| find_slice(child, op))
}

#[test]
fn postfix_key_value_array_slice_preserves_operands_and_span() {
    for (source, op) in
        [("my %pairs = $array->%[0, 1];", "->%[]"), ("my @values = $array->@[0, 1];", "->@[]")]
    {
        assert_clean_parse(source);
        assert_no_blocking_diagnostics(source);
        let ast = parse(source);
        let slice =
            find_slice(&ast, op).unwrap_or_else(|| panic!("missing {op}: {}", ast.to_sexp()));
        let NodeKind::Binary { left, right, .. } = &slice.kind else { unreachable!() };
        assert_eq!(
            &source[slice.location.start..slice.location.end],
            &source[source.find('$').unwrap()..source.find(';').unwrap()]
        );
        assert_eq!(&source[left.location.start..left.location.end], "$array");
        assert_eq!(&source[right.location.start..right.location.end], "0, 1");
    }
}

#[test]
fn postfix_key_value_slice_subscript_requires_arrow_or_parentheses() {
    for source in ["$a->%[0][1]; my $after = 1;", "$a->%[0]{x}; my $after = 1;"] {
        let mut parser = perl_parser_core::Parser::new(source);
        let output = parser.parse_with_recovery();
        assert!(output.diagnostics.iter().any(|e| e.blocks_clean_parse()), "{source}");
        assert!(find_slice(&output.ast, "->%[]").is_some(), "{source}");
        assert!(
            output.ast.to_sexp().contains("(name after)"),
            "following declaration lost: {}",
            output.ast.to_sexp()
        );
    }
    for source in ["$a->%[0]->[1];", "($a->%[0])[1];"] {
        assert_clean_parse(source);
        assert_no_blocking_diagnostics(source);
    }
}
