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
    source.get(node.location.start..node.location.end).map(str::to_owned)
}

fn subtree_contains(node: &Node, predicate: &impl Fn(&NodeKind) -> bool) -> bool {
    predicate(&node.kind)
        || node.children().into_iter().any(|child| subtree_contains(child, predicate))
}

fn is_recovery_node(node: &Node) -> bool {
    matches!(
        &node.kind,
        NodeKind::Error { .. }
            | NodeKind::MissingExpression
            | NodeKind::MissingStatement
            | NodeKind::MissingIdentifier
            | NodeKind::MissingBlock
            | NodeKind::UnknownRest
    )
}

#[test]
fn nested_lexical_declaration_retains_owned_group_shape_and_initializer() -> Result<(), String> {
    let source = "my ($head, ($middle, $tail)) = @values;";
    let ast = parse_clean(source)?;
    let mut declarations = Vec::new();
    let mut nested_count = 0usize;

    walk(&ast, &mut |node| match &node.kind {
        NodeKind::VariableListDeclaration { .. } => declarations.push(node.clone()),
        NodeKind::NestedVariableList { .. } => nested_count += 1,
        _ => {}
    });

    assert_eq!(
        declarations.len(),
        1,
        "the outer declaration must remain exactly one list declaration"
    );
    assert_eq!(
        nested_count, 1,
        "the inner declaration group must remain exactly one NestedVariableList node"
    );

    let declaration = &declarations[0];
    let NodeKind::VariableListDeclaration { declarator, variables, initializer, .. } =
        &declaration.kind
    else {
        return Err("collected declaration changed NodeKind".to_string());
    };
    assert_eq!(declarator, "my");
    assert_eq!(variables.len(), 2, "outer declaration must own two direct entries");
    assert!(matches!(
        &variables[0].kind,
        NodeKind::Variable { sigil, name } if sigil == "$" && name == "head"
    ));
    assert_eq!(source_text(source, &variables[0]).as_deref(), Some("$head"));

    let nested = &variables[1];
    assert_eq!(
        source_text(source, nested).as_deref(),
        Some("($middle, $tail)"),
        "nested group span must include its own delimiters"
    );
    let NodeKind::NestedVariableList { items } = &nested.kind else {
        return Err("the second outer entry was not the owned nested list".to_string());
    };
    assert_eq!(items.len(), 2, "nested list must own both inner variables");
    assert!(matches!(
        &items[0].kind,
        NodeKind::Variable { sigil, name } if sigil == "$" && name == "middle"
    ));
    assert!(matches!(
        &items[1].kind,
        NodeKind::Variable { sigil, name } if sigil == "$" && name == "tail"
    ));
    assert_eq!(source_text(source, &items[0]).as_deref(), Some("$middle"));
    assert_eq!(source_text(source, &items[1]).as_deref(), Some("$tail"));

    let initializer = initializer
        .as_deref()
        .ok_or_else(|| "list declaration lost its initializer".to_string())?;
    assert!(matches!(
        &initializer.kind,
        NodeKind::Variable { sigil, name } if sigil == "@" && name == "values"
    ));
    assert_eq!(source_text(source, initializer).as_deref(), Some("@values"));
    Ok(())
}

#[test]
fn array_hash_and_key_value_slices_keep_target_and_payload_ownership() -> Result<(), String> {
    let source = "@items[0, 2]; @lookup{qw(alpha beta)}; %lookup{qw(alpha beta)};";
    let ast = parse_clean(source)?;
    let mut array_slices = Vec::new();
    let mut hash_slices = Vec::new();
    let mut key_value_slices = Vec::new();

    walk(&ast, &mut |node| match &node.kind {
        NodeKind::ArraySlice { target, indices } => array_slices.push((
            source_text(source, node),
            source_text(source, target),
            source_text(source, indices),
            matches!(
                &target.kind,
                NodeKind::Variable { sigil, name } if sigil == "@" && name == "items"
            ),
        )),
        NodeKind::HashSlice { target, keys } => hash_slices.push((
            source_text(source, node),
            source_text(source, target),
            source_text(source, keys),
            matches!(
                &target.kind,
                NodeKind::Variable { sigil, name } if sigil == "@" && name == "lookup"
            ),
        )),
        NodeKind::KeyValueSlice { target, keys } => key_value_slices.push((
            source_text(source, node),
            source_text(source, target),
            source_text(source, keys),
            matches!(
                &target.kind,
                NodeKind::Variable { sigil, name } if sigil == "%" && name == "lookup"
            ),
        )),
        _ => {}
    });

    assert_eq!(
        array_slices,
        vec![(
            Some("@items[0, 2]".to_string()),
            Some("@items".to_string()),
            Some("0, 2".to_string()),
            true,
        )]
    );
    assert_eq!(
        hash_slices,
        vec![(
            Some("@lookup{qw(alpha beta)}".to_string()),
            Some("@lookup".to_string()),
            Some("qw(alpha beta)".to_string()),
            true,
        )]
    );
    assert_eq!(
        key_value_slices,
        vec![(
            Some("%lookup{qw(alpha beta)}".to_string()),
            Some("%lookup".to_string()),
            Some("qw(alpha beta)".to_string()),
            true,
        )]
    );
    Ok(())
}

#[test]
fn dereferenced_hash_slices_keep_deref_target_identity() -> Result<(), String> {
    let source = "@$href{qw(alpha beta)}; %$href{qw(alpha beta)};";
    let ast = parse_clean(source)?;
    let mut hash_slice = Vec::new();
    let mut key_value_slice = Vec::new();

    walk(&ast, &mut |node| match &node.kind {
        NodeKind::HashSlice { target, keys } => hash_slice.push((
            source_text(source, node),
            source_text(source, target),
            source_text(source, keys),
            matches!(
                &target.kind,
                NodeKind::Unary { op, operand }
                    if op == "@{}"
                        && matches!(
                            &operand.kind,
                            NodeKind::Variable { sigil, name }
                                if sigil == "$" && name == "href"
                        )
            ),
        )),
        NodeKind::KeyValueSlice { target, keys } => key_value_slice.push((
            source_text(source, node),
            source_text(source, target),
            source_text(source, keys),
            matches!(
                &target.kind,
                NodeKind::Unary { op, operand }
                    if op == "%{}"
                        && matches!(
                            &operand.kind,
                            NodeKind::Variable { sigil, name }
                                if sigil == "$" && name == "href"
                        )
            ),
        )),
        _ => {}
    });

    assert_eq!(
        hash_slice,
        vec![(
            Some("@$href{qw(alpha beta)}".to_string()),
            Some("@$href".to_string()),
            Some("qw(alpha beta)".to_string()),
            true,
        )]
    );
    assert_eq!(
        key_value_slice,
        vec![(
            Some("%$href{qw(alpha beta)}".to_string()),
            Some("%$href".to_string()),
            Some("qw(alpha beta)".to_string()),
            true,
        )]
    );
    Ok(())
}

#[test]
fn typeglob_alias_keeps_both_operands_attached_to_one_assignment() -> Result<(), String> {
    let source = "sub original { 1 }\n*alias = \\&original;\n";
    let ast = parse_clean(source)?;
    let mut observed = Vec::new();

    walk(&ast, &mut |node| {
        if let NodeKind::Assignment { lhs, rhs, op } = &node.kind
            && op == "="
            && source_text(source, node).as_deref() == Some("*alias = \\&original")
        {
            observed.push((
                source_text(source, lhs),
                source_text(source, rhs),
                subtree_contains(
                    lhs,
                    &|kind| matches!(kind, NodeKind::Typeglob { name } if name == "alias"),
                ),
                subtree_contains(
                    rhs,
                    &|kind| matches!(kind, NodeKind::AmperCall { name, .. } if name == "original"),
                ),
            ));
        }
    });

    assert_eq!(
        observed,
        vec![(Some("*alias".to_string()), Some("\\&original".to_string()), true, true,)],
        "the alias must be represented by one exact assignment with both operands attached"
    );
    Ok(())
}

#[test]
fn lexical_declaration_recovery_preserves_following_code_without_normalizing_element_syntax() {
    let source = "my $items[0] = 1; my $after = 2;";
    let mut parser = Parser::new(source);
    let output = parser.parse_with_recovery();
    let mut items_declaration_spans = Vec::new();
    let mut after_declaration_spans = Vec::new();
    let mut recovery_nodes = Vec::new();

    walk(&output.ast, &mut |node| {
        if is_recovery_node(node) {
            recovery_nodes.push((node.kind.kind_name(), node.location.start, node.location.end));
        }
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
        !recovery_nodes.is_empty(),
        "the malformed declaration must leave typed recovery evidence in the AST"
    );
    assert!(
        items_declaration_spans.iter().all(|span| !span.contains("[0]")),
        "recovery normalized direct-element syntax into an ordinary declaration: {items_declaration_spans:?}"
    );
    assert_eq!(after_declaration_spans.len(), 1);
    assert!(after_declaration_spans[0].starts_with("my $after = 2"));
    assert!(!output.terminated_early, "local declaration recovery must preserve following code");
}
