//! Structural parser contracts for quote-like operators (#6692 follow-up).
//!
//! The parser-accuracy manifest proves fixture-level coverage. These tests also
//! pin native AST payloads and forbid paired quote delimiters from becoming blocks.

use perl_parser_core::{Node, NodeKind, Parser};

fn parse_clean(source: &str) -> Result<Node, String> {
    let mut parser = Parser::new(source);
    let ast = parser.parse().map_err(|error| format!("parse failed: {error:?}"))?;
    if parser.errors().is_empty() {
        Ok(ast)
    } else {
        Err(format!("expected a clean parse, got diagnostics: {:?}", parser.errors()))
    }
}

fn walk(node: &Node, visit: &mut impl FnMut(&Node)) {
    visit(node);
    for child in node.children() {
        walk(child, visit);
    }
}

#[test]
fn substitution_and_transliteration_keep_payloads_modifiers_and_target() -> Result<(), String> {
    let source = concat!(
        "my $message = q{hello};\n",
        "$message =~ s/hello/hello world/g;\n",
        "$message =~ tr/a-z/A-Z/;\n",
        "return qq{$message};\n",
    );
    let ast = parse_clean(source)?;
    let mut substitution_seen = false;
    let mut transliteration_seen = false;
    let mut quote_spans = Vec::new();

    walk(&ast, &mut |node| match &node.kind {
        NodeKind::Substitution {
            expr,
            pattern,
            replacement,
            modifiers,
            has_embedded_code,
            negated,
        } => {
            assert!(matches!(
                &expr.kind,
                NodeKind::Variable { sigil, name } if sigil == "$" && name == "message"
            ));
            assert_eq!(pattern, "hello");
            assert_eq!(replacement, "hello world");
            assert_eq!(modifiers, "g");
            assert!(!has_embedded_code);
            assert!(!negated);
            assert_eq!(
                source.get(node.location.start..node.location.end),
                Some("$message =~ s/hello/hello world/g")
            );
            substitution_seen = true;
        }
        NodeKind::Transliteration { expr, search, replace, modifiers, negated } => {
            assert!(matches!(
                &expr.kind,
                NodeKind::Variable { sigil, name } if sigil == "$" && name == "message"
            ));
            assert_eq!(search, "a-z");
            assert_eq!(replace, "A-Z");
            assert!(modifiers.is_empty());
            assert!(!negated);
            assert_eq!(
                source.get(node.location.start..node.location.end),
                Some("$message =~ tr/a-z/A-Z/")
            );
            transliteration_seen = true;
        }
        NodeKind::String { .. } => {
            if let Some(text) = source.get(node.location.start..node.location.end)
                && (text.starts_with("q{") || text.starts_with("qq{"))
            {
                quote_spans.push(text.to_owned());
            }
        }
        _ => {}
    });

    quote_spans.sort();
    assert!(substitution_seen, "substitution node was not preserved");
    assert!(transliteration_seen, "transliteration node was not preserved");
    assert_eq!(quote_spans, vec!["q{hello}", "qq{$message}"]);
    Ok(())
}

#[test]
fn paired_quote_delimiters_do_not_fabricate_block_nodes() -> Result<(), String> {
    let source = "my $literal = q{hello}; my $interpolated = qq{$literal};";
    let ast = parse_clean(source)?;
    let mut fabricated_blocks = Vec::new();

    walk(&ast, &mut |node| {
        if matches!(&node.kind, NodeKind::Block { .. })
            && let Some(text) = source.get(node.location.start..node.location.end)
            && (text == "{hello}" || text == "{$literal}")
        {
            fabricated_blocks.push(text.to_owned());
        }
    });

    assert!(
        fabricated_blocks.is_empty(),
        "quote delimiters were misclassified as blocks: {fabricated_blocks:?}"
    );
    Ok(())
}
