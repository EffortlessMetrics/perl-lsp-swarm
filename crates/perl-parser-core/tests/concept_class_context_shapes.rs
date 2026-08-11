//! Concept-level parser proofs for class and OO source forms (#6700).
//!
//! These tests pin contextual syntax only. Inheritance, generated accessors,
//! dispatch, MRO, and framework semantics remain downstream concerns.

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
fn class_owns_exact_field_method_and_adjust_members() -> Result<(), String> {
    let source = concat!(
        "use feature 'class';\n",
        "no warnings 'experimental::class';\n",
        "class Example 1.23 :isa(Base) {\n",
        "    field $value :param :reader = 1;\n",
        "    method get($fallback = 0) :lvalue { return $value // $fallback; }\n",
        "    ADJUST { $value = 2; }\n",
        "}\n",
    );
    let ast = parse_clean(source)?;
    let mut classes = Vec::new();

    walk(&ast, &mut |node| {
        let NodeKind::Class {
            name,
            name_span,
            parents,
            body,
        } = &node.kind
        else {
            return;
        };

        let name_text = name_span
            .and_then(|span| source.get(span.start..span.end))
            .map(str::to_owned);
        let header_tail = name_span
            .and_then(|span| source.get(span.end..body.location.start))
            .map(str::trim)
            .map(str::to_owned);
        let NodeKind::Block { statements } = &body.kind else {
            classes.push((
                name.clone(),
                name_text,
                parents.clone(),
                source_text(source, node),
                header_tail,
                source_text(source, body),
                Vec::new(),
            ));
            return;
        };

        let mut members = Vec::new();
        for member in statements {
            match &member.kind {
                NodeKind::VariableDeclaration {
                    declarator,
                    variable,
                    attributes,
                    initializer,
                } if declarator == "field" => {
                    members.push((
                        "field".to_string(),
                        source_text(source, member),
                        source_text(source, variable),
                        attributes.clone(),
                        initializer
                            .as_deref()
                            .and_then(|value| source_text(source, value)),
                        matches!(
                            &variable.kind,
                            NodeKind::Variable { sigil, name }
                                if sigil == "$" && name == "value"
                        ) && initializer.as_deref().is_some_and(|value| {
                            matches!(&value.kind, NodeKind::Number { value } if value == "1")
                        }),
                    ));
                }
                NodeKind::Method {
                    name,
                    name_span,
                    signature,
                    attributes,
                    body,
                } if name == "get" => {
                    let signature_shape = signature.as_deref().map(|signature| {
                        let NodeKind::Signature { parameters } = &signature.kind else {
                            return (source_text(source, signature), 0, false, false);
                        };
                        (
                            source_text(source, signature),
                            parameters.len(),
                            parameters.len() == 1
                                && matches!(
                                    &parameters[0].kind,
                                    NodeKind::OptionalParameter { .. }
                                )
                                && source_text(source, &parameters[0]).as_deref()
                                    == Some("$fallback = 0"),
                            parameters.len() == 1
                                && subtree_contains(&parameters[0], &|kind| {
                                    matches!(kind, NodeKind::Number { value } if value == "0")
                                }),
                        )
                    });
                    members.push((
                        "method".to_string(),
                        source_text(source, member),
                        name_span
                            .and_then(|span| source.get(span.start..span.end))
                            .map(str::to_owned),
                        attributes.clone(),
                        Some(format!("{signature_shape:?}")),
                        signature_shape
                            == Some((
                                Some("($fallback = 0)".to_string()),
                                1,
                                true,
                                true,
                            ))
                            && source_text(source, body).as_deref()
                                == Some("{ return $value // $fallback; }"),
                    ));
                }
                NodeKind::Method {
                    name,
                    name_span,
                    signature,
                    attributes,
                    body,
                } if name == "ADJUST" => {
                    members.push((
                        "adjust".to_string(),
                        source_text(source, member),
                        name_span
                            .and_then(|span| source.get(span.start..span.end))
                            .map(str::to_owned),
                        attributes.clone(),
                        signature
                            .as_deref()
                            .and_then(|value| source_text(source, value)),
                        name_span.is_none()
                            && signature.is_none()
                            && attributes.is_empty()
                            && source_text(source, body).as_deref()
                                == Some("{ $value = 2; }"),
                    ));
                }
                _ => members.push((
                    member.kind.kind_name().to_string(),
                    source_text(source, member),
                    None,
                    Vec::new(),
                    None,
                    false,
                )),
            }
        }

        classes.push((
            name.clone(),
            name_text,
            parents.clone(),
            source_text(source, node),
            header_tail,
            source_text(source, body),
            members,
        ));
    });

    assert_eq!(classes.len(), 1, "expected exactly one class declaration");
    let (name, name_text, parents, class_text, header_tail, body_text, members) = &classes[0];
    assert_eq!(name, "Example");
    assert_eq!(name_text.as_deref(), Some("Example"));
    assert_eq!(parents, &["Base".to_string()]);
    assert_eq!(header_tail.as_deref(), Some("1.23 :isa(Base)"));
    assert_eq!(
        class_text.as_deref(),
        Some(concat!(
            "class Example 1.23 :isa(Base) {\n",
            "    field $value :param :reader = 1;\n",
            "    method get($fallback = 0) :lvalue { return $value // $fallback; }\n",
            "    ADJUST { $value = 2; }\n",
            "}",
        ))
    );
    assert_eq!(
        body_text.as_deref(),
        Some(concat!(
            "{\n",
            "    field $value :param :reader = 1;\n",
            "    method get($fallback = 0) :lvalue { return $value // $fallback; }\n",
            "    ADJUST { $value = 2; }\n",
            "}",
        ))
    );
    assert_eq!(members.len(), 3, "class body must own exactly three direct members");

    assert_eq!(members[0].0, "field");
    assert_eq!(
        members[0].1.as_deref().map(|text| text.trim_end_matches(';')),
        Some("field $value :param :reader = 1")
    );
    assert_eq!(members[0].2.as_deref(), Some("$value"));
    assert_eq!(members[0].3, vec!["param".to_string(), "reader".to_string()]);
    assert_eq!(members[0].4.as_deref(), Some("1"));
    assert!(members[0].5, "field must own its variable and initializer");

    assert_eq!(members[1].0, "method");
    assert_eq!(
        members[1].1.as_deref(),
        Some("method get($fallback = 0) :lvalue { return $value // $fallback; }")
    );
    assert_eq!(members[1].2.as_deref(), Some("get"));
    assert_eq!(members[1].3, vec!["lvalue".to_string()]);
    assert!(members[1].5, "method must own its optional parameter, default, and body");

    assert_eq!(members[2].0, "adjust");
    assert_eq!(members[2].1.as_deref(), Some("ADJUST { $value = 2; }"));
    assert!(members[2].2.is_none());
    assert!(members[2].3.is_empty());
    assert!(members[2].4.is_none());
    assert!(members[2].5, "ADJUST must remain a nameless method-shaped direct member");
    Ok(())
}

#[test]
fn class_keywords_remain_exact_ordinary_calls_outside_class_context() -> Result<(), String> {
    let source = "field($outside); method($outside); ADJUST($outside);";
    let ast = parse_clean(source)?;
    let mut calls = Vec::new();

    walk(&ast, &mut |node| {
        if let NodeKind::FunctionCall { name, args } = &node.kind
            && matches!(name.as_str(), "field" | "method" | "ADJUST")
        {
            calls.push((
                name.clone(),
                source_text(source, node),
                args.iter().filter_map(|arg| source_text(source, arg)).collect::<Vec<_>>(),
                args.len() == 1
                    && matches!(
                        &args[0].kind,
                        NodeKind::Variable { sigil, name }
                            if sigil == "$" && name == "outside"
                    ),
            ));
        }
    });

    assert_eq!(
        calls,
        vec![
            (
                "field".to_string(),
                Some("field($outside)".to_string()),
                vec!["$outside".to_string()],
                true,
            ),
            (
                "method".to_string(),
                Some("method($outside)".to_string()),
                vec!["$outside".to_string()],
                true,
            ),
            (
                "ADJUST".to_string(),
                Some("ADJUST($outside)".to_string()),
                vec!["$outside".to_string()],
                true,
            ),
        ],
        "class keywords outside class context must remain ordinary one-argument calls"
    );
    Ok(())
}
