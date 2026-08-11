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
    let mut array_slices = 0usize;
    let mut hash_slices = 0usize;
    let mut key_value_slices = 0usize;

    walk(&ast, &mut |node| match node.kind {
        NodeKind::ArraySlice { .. } => array_slices += 1,
        NodeKind::HashSlice { .. } => hash_slices += 1,
        NodeKind::KeyValueSlice { .. } => key_value_slices += 1,
        _ => {}
    });

    assert_eq!(array_slices, 1, "@array[...] must retain ArraySlice identity");
    assert_eq!(hash_slices, 1, "@hash{...} must retain HashSlice identity");
    assert_eq!(
        key_value_slices, 1,
        "%hash{...} must retain KeyValueSlice identity rather than collapsing into HashSlice"
    );
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
