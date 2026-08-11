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

fn walk(node: &Node, visit: &mut impl FnMut(&Node) -> Result<(), String>) -> Result<(), String> {
    visit(node)?;
    for child in node.children() {
        walk(child, visit)?;
    }
    Ok(())
}

#[test]
fn substitution_and_transliteration_keep_payloads_modifiers_and_target() -> Result<(), String> {
    let source = concat!(
        "my $message = q{hello};\n",
        "$message =~ s/hello/hello world/g;\n",
        "$message =~ tr/a-z/A-Z/;\n",
        "$message =~ y{a-z}{A-Z}r;\n",
        "return qq{$message};\n",
    );
    let ast = parse_clean(source)?;
    let mut substitution_count = 0usize;
    let mut transliterations = Vec::new();
    let mut quote_spans = Vec::new();

    walk(&ast, &mut |node| {
        match &node.kind {
            NodeKind::Substitution {
                expr,
                pattern,
                replacement,
                modifiers,
                has_embedded_code,
                negated,
            } => {
                if !matches!(
                    &expr.kind,
                    NodeKind::Variable { sigil, name } if sigil == "$" && name == "message"
                ) {
                    return Err("substitution target was not $message".into());
                }
                if pattern != "hello" || replacement != "hello world" || modifiers != "g" {
                    return Err(format!(
                        "unexpected substitution payload: pattern={pattern:?}, replacement={replacement:?}, modifiers={modifiers:?}"
                    ));
                }
                if *has_embedded_code || *negated {
                    return Err("substitution flags were not preserved".into());
                }
                if source.get(node.location.start..node.location.end)
                    != Some("$message =~ s/hello/hello world/g")
                {
                    return Err("substitution source span was not preserved".into());
                }
                substitution_count += 1;
            }
            NodeKind::Transliteration { expr, search, replace, modifiers, negated } => {
                if !matches!(
                    &expr.kind,
                    NodeKind::Variable { sigil, name } if sigil == "$" && name == "message"
                ) {
                    return Err("transliteration target was not $message".into());
                }
                if let Some(text) = source.get(node.location.start..node.location.end) {
                    transliterations.push((
                        text.to_owned(),
                        search.clone(),
                        replace.clone(),
                        modifiers.clone(),
                        *negated,
                    ));
                }
            }
            NodeKind::String { .. } => {
                if let Some(text) = source.get(node.location.start..node.location.end)
                    && (text.starts_with("q{") || text.starts_with("qq{"))
                {
                    quote_spans.push(text.to_owned());
                }
            }
            _ => {}
        }
        Ok(())
    })?;

    transliterations.sort();
    quote_spans.sort();
    if substitution_count != 1 {
        return Err(format!("expected exactly one substitution node, got {substitution_count}"));
    }
    let expected_transliterations = vec![
        (
            "$message =~ tr/a-z/A-Z/".to_string(),
            "a-z".to_string(),
            "A-Z".to_string(),
            String::new(),
            false,
        ),
        (
            "$message =~ y{a-z}{A-Z}r".to_string(),
            "a-z".to_string(),
            "A-Z".to_string(),
            "r".to_string(),
            false,
        ),
    ];
    if transliterations != expected_transliterations {
        return Err(format!("unexpected transliteration payloads: {transliterations:?}"));
    }
    if quote_spans != vec!["qq{$message}", "q{hello}"] {
        return Err(format!("unexpected quote spans: {quote_spans:?}"));
    }
    Ok(())
}

#[test]
fn paired_quote_delimiters_do_not_fabricate_block_nodes() -> Result<(), String> {
    let source = "my $literal = q{hello}; my $interpolated = qq{$literal};";
    let ast = parse_clean(source)?;
    let mut fabricated_blocks = Vec::new();

    walk(&ast, &mut |node| {
        if matches!(&node.kind, NodeKind::Block { .. }) {
            let text = source.get(node.location.start..node.location.end).map_or_else(
                || format!("<unmapped {}..{}>", node.location.start, node.location.end),
                ToOwned::to_owned,
            );
            fabricated_blocks.push(text);
        }
        Ok(())
    })?;

    if !fabricated_blocks.is_empty() {
        return Err(format!(
            "quote delimiters were misclassified as blocks: {fabricated_blocks:?}"
        ));
    }
    Ok(())
}
