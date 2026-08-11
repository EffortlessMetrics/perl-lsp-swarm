//! Concept-level parser proofs for storage syntax (#6673).
//!
//! These tests pin source distinctions that downstream place and alias analysis
//! depends on without claiming runtime lvalue, autovivification, or alias semantics.

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

fn source_text(source: &str, node: &Node) -> Option<String> {
    source
        .get(node.location.start..node.location.end)
        .map(str::to_owned)
}

#[test]
fn nested_lexical_declaration_retains_group_identity_and_span() -> Result<(), String> {
    let source = "my ($head, ($middle, $tail)) = @values;";
    let ast = parse_clean(source)?;
    let mut list_count = 0usize;
    let mut nested_span = None;

    walk(&ast, &mut |node| match &node.kind {
        NodeKind::VariableListDeclaration { .. } => list_count += 1,
        NodeKind::NestedVariableList { .. } => {
            nested_span = Some((node.location.start, node.location.end));
        }
        _ => {}
    });

    assert_eq!(list_count, 1, "the outer declaration must remain one list declaration");
    let (start, end) = nested_span.ok_or_else(|| {
        "the inner parenthesized declaration group lost NestedVariableList identity".to_string()
    })?;
    let observed = source
        .get(start..end)
        .ok_or_else(|| format!("nested declaration span {start}..{end} is outside the source"))?;
    assert_eq!(observed, "($middle, $tail)");
    Ok(())
}

#[test]
fn array_hash_and_key_value_slices_remain_distinct() -> Result<(), String> {
    let source = "@items[0, 2]; @lookup{qw(alpha beta)}; %lookup{qw(alpha beta)};";
    let ast = parse_clean(source)?;
    let mut array_slices = Vec::new();
    let mut hash_slices = Vec::new();
    let mut key_value_slices = Vec::new();

    walk(&ast, &mut |node| match &node.kind {
        NodeKind::ArraySlice { .. } => {
            if let Some(text) = source_text(source, node) {
                array_slices.push(text);
            }
        }
        NodeKind::HashSlice { .. } => {
            if let Some(text) = source_text(source, node) {
                hash_slices.push(text);
            }
        }
        NodeKind::KeyValueSlice { .. } => {
            if let Some(text) = source_text(source, node) {
                key_value_slices.push(text);
            }
        }
        _ => {}
    });

    assert_eq!(array_slices, vec!["@items[0, 2]"]);
    assert_eq!(hash_slices, vec!["@lookup{qw(alpha beta)}"]);
    assert_eq!(key_value_slices, vec!["%lookup{qw(alpha beta)}"]);
    Ok(())
}

#[test]
fn typeglob_alias_keeps_both_storage_and_coderef_identity() -> Result<(), String> {
    let source = "sub original { 1 }\n*alias = \\&original;\n";
    let ast = parse_clean(source)?;
    let mut typeglobs = Vec::new();
    let mut coderefs = Vec::new();

    walk(&ast, &mut |node| match &node.kind {
        NodeKind::Typeglob { .. } => {
            if let Some(text) = source_text(source, node) {
                typeglobs.push(text);
            }
        }
        NodeKind::AmperCall { name, .. } if name == "original" => {
            if let Some(text) = source_text(source, node) {
                coderefs.push(text);
            }
        }
        _ => {}
    });

    assert_eq!(typeglobs, vec!["*alias"]);
    assert_eq!(coderefs, vec!["&original"]);
    Ok(())
}

#[test]
fn lexical_declaration_cannot_silently_accept_direct_element_syntax() -> Result<(), String> {
    let mut parser = Parser::new("my $items[0] = 1;");
    match parser.parse() {
        Err(_) => Ok(()),
        Ok(_) => {
            assert!(
                !parser.errors().is_empty(),
                "direct element declaration must be rejected or recovered with a diagnostic"
            );
            Ok(())
        }
    }
}
