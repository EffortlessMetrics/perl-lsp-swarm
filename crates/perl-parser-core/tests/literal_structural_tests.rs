mod cpan_test_helpers;

use cpan_test_helpers::parse;
use perl_parser_core::{Node, NodeKind};

fn visit(node: &Node, f: &mut impl FnMut(&Node)) {
    f(node);
    for child in node.children() {
        visit(child, f);
    }
}

fn assert_number(source: &str, expected: &str, start: usize, end: usize) {
    let ast = parse(source);
    let mut found_value = false;
    let mut found_span = false;

    visit(&ast, &mut |node| {
        if let NodeKind::Number { value } = &node.kind {
            if value == expected {
                found_value = true;
                found_span = node.location.start == start && node.location.end == end;
            }
        }
    });

    assert!(found_value, "expected numeric literal {expected:?} in AST:\\n{}", ast.to_sexp());
    assert!(
        found_span,
        "numeric literal {expected:?} did not retain source span {start}..{end}:\\n{}",
        ast.to_sexp()
    );
}

#[test]
fn numeric_literals_preserve_kind_value_and_span() {
    assert_number("my $hex = 0xff;", "0xff", 10, 14);
    assert_number("my $oct = 0o755;", "0o755", 10, 15);
    assert_number("my $bin = 0b1010;", "0b1010", 10, 16);
    assert_number("my $float = 3.14;", "3.14", 12, 16);
}

#[test]
fn strings_preserve_kind_value_and_span() {
    let source = r#"my $single = 'text'; my $double = "hello $name";"#;
    let ast = parse(source);
    let mut single = false;
    let mut double = false;
    let mut single_span = false;
    let mut double_span = false;

    visit(&ast, &mut |node| {
        if let NodeKind::String { value, interpolated } = &node.kind {
            if value == "'text'" && !*interpolated {
                single = true;
                single_span = node.location.start == 13 && node.location.end == 19;
            }
            if value == "\"hello $name\"" && *interpolated {
                double = true;
                double_span = node.location.start == 34 && node.location.end == 47;
            }
        }
    });

    assert!(
        single,
        "single-quoted literal was not represented as a String node:\\n{}",
        ast.to_sexp()
    );
    assert!(
        double,
        "double-quoted literal was not represented as an interpolated String node:\\n{}",
        ast.to_sexp()
    );
    assert!(single_span, "single-quoted literal lost its source span:\\n{}", ast.to_sexp());
    assert!(double_span, "double-quoted literal lost its source span:\\n{}", ast.to_sexp());
}

#[test]
fn array_and_hash_literals_retain_structural_children() {
    let ast = parse(r#"my @values = (1, 2); my %map = (alpha => "beta");"#);
    let mut array_elements = None;
    let mut hash_pairs = None;
    let mut hash_value = false;

    visit(&ast, &mut |node| match &node.kind {
        NodeKind::ArrayLiteral { elements } => {
            array_elements = Some(elements.len());
        }
        NodeKind::HashLiteral { pairs } => {
            hash_pairs = Some(pairs.len());
        }
        NodeKind::String { value, interpolated } if value == "\"beta\"" && !*interpolated => {
            hash_value = true;
        }
        _ => {}
    });

    assert_eq!(array_elements, Some(2), "array literal lost an element: {}", ast.to_sexp());
    assert_eq!(hash_pairs, Some(1), "hash literal lost its pair: {}", ast.to_sexp());

    let sexp = ast.to_sexp();
    assert!(sexp.contains("(number 1)"), "array element 1 was not retained:\\n{sexp}");
    assert!(sexp.contains("(number 2)"), "array element 2 was not retained:\\n{sexp}");
    assert!(hash_value, "hash value was not retained as an interpolated String node:\\n{sexp}");
}
