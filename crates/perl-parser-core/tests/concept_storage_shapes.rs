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

fn subtree_contains(node: &Node, predicate: &impl Fn(&NodeKind) -> bool) -> bool {
    predicate(&node.kind)
        || node
            .children()
            .into_iter()
            .any(|child| subtree_contains(child, predicate))
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
fn typeglob_alias_keeps_both_operands_attached_to_the_assignment() -> Result<(), String> {
    let source = "sub original { 1 }\n*alias = \\&original;\n";
    let ast = parse_clean(source)?;
    let mut observed = None;

    walk(&ast, &mut |node| {
        if let NodeKind::Assignment { lhs, rhs, op } = &node.kind
            && op == "="
            && source_text(source, node).as_deref() == Some("*alias = \\&original")
        {
            observed = Some((
                source_text(source, lhs),
                source_text(source, rhs),
                subtree_contains(lhs, &|kind| {
                    matches!(kind, NodeKind::Typeglob { name } if name == "alias")
                }),
                subtree_contains(rhs, &|kind| {
                    matches!(kind, NodeKind::AmperCall { name, .. } if name == "original")
                }),
            ));
        }
    });

    let (lhs_span, rhs_span, lhs_is_typeglob, rhs_contains_coderef) = observed
        .ok_or_else(|| "typeglob alias was not preserved as one assignment node".to_string())?;
    assert_eq!(lhs_span.as_deref(), Some("*alias"));
    assert_eq!(rhs_span.as_deref(), Some("\\&original"));
    assert!(lhs_is_typeglob, "assignment lhs lost Typeglob identity");
    assert!(rhs_contains_coderef, "assignment rhs lost the &original coderef identity");
    Ok(())
}

#[test]
fn lexical_declaration_recovery_preserves_following_code_without_normalizing_element_syntax() {
    let source = "my $items[0] = 1; my $after = 2;";
    let mut parser = Parser::new(source);
    let output = parser.parse_with_recovery();
    let mut items_declaration_spans = Vec::new();
    let mut after_declaration_spans = Vec::new();

    walk(&output.ast, &mut |node| {
        if let NodeKind::VariableDeclaration { variable, .. } = &node.kind
            && let NodeKind::Variable { sigil, name } = &variable.kind
            && sigil == "$"
            && let Some(text) = source_text(source, node)
        {
            match name.as_str() {
                "items" => items_declaration_spans.push(text),
                "after" => after_declaration_spans.push(text),
                _ => {}
            }
        }
    });

    assert!(
        !output.diagnostics.is_empty(),
        "direct element declaration must not be represented as a clean parse"
    );
    assert!(
        items_declaration_spans.iter().all(|span| !span.contains("[0]")),
        "recovery normalized direct-element syntax into an ordinary declaration: {items_declaration_spans:?}"
    );
    assert_eq!(after_declaration_spans.len(), 1);
    assert!(after_declaration_spans[0].starts_with("my $after = 2"));
    assert!(!output.terminated_early, "local declaration recovery must preserve following code");
}
