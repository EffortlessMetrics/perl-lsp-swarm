//! Concept-level parser proofs for subroutine declarations and call forms (#6676).
//!
//! These tests pin parser-visible structure only. Runtime prototype enforcement,
//! dispatch, signature binding, and provider behavior remain downstream concerns.

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

fn assert_span_contains(parent: &Node, child: &Node, relationship: &str) -> Result<(), String> {
    if parent.location.start <= child.location.start && child.location.end <= parent.location.end {
        Ok(())
    } else {
        Err(format!(
            "{relationship}: parent span {} must contain child span {}",
            parent.location, child.location
        ))
    }
}

fn collect_named_subroutines(ast: &Node, expected_name: &str) -> Vec<Node> {
    let mut subroutines = Vec::new();
    walk(ast, &mut |node| {
        if matches!(
            &node.kind,
            NodeKind::Subroutine { name: Some(name), .. } if name == expected_name
        ) {
            subroutines.push(node.clone());
        }
    });
    subroutines
}

#[test]
fn prototype_is_owned_by_the_exact_forward_declaration() -> Result<(), String> {
    let source = "sub shaped ($;@);";
    let ast = parse_clean(source)?;
    let subroutines = collect_named_subroutines(&ast, "shaped");
    assert_eq!(subroutines.len(), 1, "expected one shaped declaration");

    let mut prototype_payloads = Vec::new();
    walk(&ast, &mut |node| {
        if let NodeKind::Prototype { content } = &node.kind {
            prototype_payloads.push((source_text(source, node), content.clone()));
        }
    });
    assert_eq!(
        prototype_payloads,
        vec![(Some("($;@)".to_string()), "$;@".to_string())],
        "the declaration must expose exactly one prototype node"
    );

    let NodeKind::Subroutine { name_span, prototype, signature, body, .. } = &subroutines[0].kind
    else {
        return Err("collected declaration changed NodeKind".to_string());
    };
    assert_eq!(name_span.and_then(|span| source.get(span.start..span.end)), Some("shaped"));
    assert!(signature.is_none(), "prototype declaration must not fabricate a signature");

    let prototype = prototype
        .as_deref()
        .ok_or_else(|| "forward declaration lost its prototype child".to_string())?;
    let NodeKind::Prototype { content } = &prototype.kind else {
        return Err("subroutine prototype child changed NodeKind".to_string());
    };
    assert_eq!(content, "$;@");
    assert_eq!(source_text(source, prototype).as_deref(), Some("($;@)"));

    let NodeKind::Block { statements } = &body.kind else {
        return Err("forward declaration must retain its empty body sentinel".to_string());
    };
    assert!(statements.is_empty(), "forward declaration body sentinel must stay empty");
    Ok(())
}

#[test]
fn signature_owns_parameter_order_kinds_and_optional_default() -> Result<(), String> {
    let source = "sub shaped ($required, $optional = 1, @rest) { return $required; }";
    let ast = parse_clean(source)?;
    let subroutines = collect_named_subroutines(&ast, "shaped");
    assert_eq!(subroutines.len(), 1, "expected one shaped declaration");

    let NodeKind::Subroutine { prototype, signature, .. } = &subroutines[0].kind else {
        return Err("collected declaration changed NodeKind".to_string());
    };
    assert!(prototype.is_none(), "signature declaration must not fabricate a prototype");
    let signature =
        signature.as_deref().ok_or_else(|| "subroutine lost its signature child".to_string())?;
    assert_eq!(
        source_text(source, signature).as_deref(),
        Some("($required, $optional = 1, @rest)")
    );

    let NodeKind::Signature { parameters } = &signature.kind else {
        return Err("subroutine signature child changed NodeKind".to_string());
    };
    assert_eq!(parameters.len(), 3, "signature must retain exactly three parameters");
    for (index, parameter) in parameters.iter().enumerate() {
        assert_span_contains(signature, parameter, &format!("signature parameter {index}"))?;
    }

    let NodeKind::MandatoryParameter { variable: required } = &parameters[0].kind else {
        return Err("required parameter changed NodeKind".to_string());
    };
    assert_span_contains(&parameters[0], required, "required parameter variable")?;
    assert!(matches!(
        &required.kind,
        NodeKind::Variable { sigil, name } if sigil == "$" && name == "required"
    ));
    assert_eq!(source_text(source, required).as_deref(), Some("$required"));

    let NodeKind::OptionalParameter { variable: optional, default_value } = &parameters[1].kind
    else {
        return Err("optional parameter changed NodeKind".to_string());
    };
    assert_span_contains(&parameters[1], optional, "optional parameter variable")?;
    assert_span_contains(&parameters[1], default_value, "optional parameter default")?;
    assert!(matches!(
        &optional.kind,
        NodeKind::Variable { sigil, name } if sigil == "$" && name == "optional"
    ));
    assert_eq!(source_text(source, optional).as_deref(), Some("$optional"));
    assert!(matches!(&default_value.kind, NodeKind::Number { value } if value == "1"));
    assert_eq!(source_text(source, default_value).as_deref(), Some("1"));

    let NodeKind::SlurpyParameter { variable: rest } = &parameters[2].kind else {
        return Err("array-slurpy parameter changed NodeKind".to_string());
    };
    assert_span_contains(&parameters[2], rest, "array-slurpy parameter variable")?;
    assert!(matches!(
        &rest.kind,
        NodeKind::Variable { sigil, name } if sigil == "@" && name == "rest"
    ));
    assert_eq!(source_text(source, rest).as_deref(), Some("@rest"));
    assert_eq!(source_text(source, &parameters[0]).as_deref(), Some("$required"));
    assert_eq!(
        source_text(source, &parameters[1]).as_deref().map(str::trim),
        Some("$optional = 1")
    );
    assert_eq!(source_text(source, &parameters[2]).as_deref(), Some("@rest"));

    let hash_source = "sub hash_shaped (%rest) { return %rest; }";
    let hash_ast = parse_clean(hash_source)?;
    let hash_subroutines = collect_named_subroutines(&hash_ast, "hash_shaped");
    assert_eq!(hash_subroutines.len(), 1, "expected one hash_shaped declaration");
    let NodeKind::Subroutine { signature: Some(hash_signature), .. } = &hash_subroutines[0].kind
    else {
        return Err("hash slurpy declaration lost its signature".to_string());
    };
    let NodeKind::Signature { parameters } = &hash_signature.kind else {
        return Err("hash slurpy signature child changed NodeKind".to_string());
    };
    assert_eq!(parameters.len(), 1);
    assert_span_contains(hash_signature, &parameters[0], "hash signature parameter")?;
    let NodeKind::SlurpyParameter { variable: hash_rest } = &parameters[0].kind else {
        return Err("hash-slurpy parameter changed NodeKind".to_string());
    };
    assert_span_contains(&parameters[0], hash_rest, "hash-slurpy parameter variable")?;
    assert!(matches!(
        &hash_rest.kind,
        NodeKind::Variable { sigil, name } if sigil == "%" && name == "rest"
    ));
    assert_eq!(source_text(hash_source, hash_rest).as_deref(), Some("%rest"));
    assert_eq!(source_text(hash_source, &parameters[0]).as_deref(), Some("%rest"));
    Ok(())
}

#[test]
fn anonymous_subroutine_keeps_signature_attribute_and_body_ownership() -> Result<(), String> {
    let source = "my $callback = sub ($value) :lvalue { return $value; };";
    let ast = parse_clean(source)?;
    let mut anonymous = Vec::new();

    walk(&ast, &mut |node| {
        if matches!(&node.kind, NodeKind::Subroutine { name: None, .. }) {
            anonymous.push(node.clone());
        }
    });
    assert_eq!(anonymous.len(), 1, "expected one anonymous closure");

    let NodeKind::Subroutine { signature, attributes, body, .. } = &anonymous[0].kind else {
        return Err("anonymous closure changed NodeKind".to_string());
    };
    assert_eq!(
        source_text(source, &anonymous[0]).as_deref(),
        Some("sub ($value) :lvalue { return $value; }")
    );
    assert_eq!(attributes, &["lvalue".to_string()]);
    let signature =
        signature.as_deref().ok_or_else(|| "anonymous closure lost its signature".to_string())?;
    let NodeKind::Signature { parameters } = &signature.kind else {
        return Err("anonymous closure signature changed NodeKind".to_string());
    };
    assert_eq!(parameters.len(), 1);
    assert_span_contains(signature, &parameters[0], "anonymous signature parameter")?;
    let NodeKind::MandatoryParameter { variable } = &parameters[0].kind else {
        return Err("anonymous closure parameter changed NodeKind".to_string());
    };
    assert_span_contains(&parameters[0], variable, "anonymous parameter variable")?;
    assert!(matches!(
        &variable.kind,
        NodeKind::Variable { sigil, name } if sigil == "$" && name == "value"
    ));
    assert_eq!(source_text(source, variable).as_deref(), Some("$value"));
    assert_eq!(source_text(source, &parameters[0]).as_deref(), Some("$value"));
    assert!(matches!(&body.kind, NodeKind::Block { .. }));
    assert_eq!(source_text(source, body).as_deref(), Some("{ return $value; }"));
    Ok(())
}

#[test]
fn source_call_forms_keep_exact_targets_receivers_and_arguments() -> Result<(), String> {
    let source = concat!(
        "named(1);\n",
        "&named(2);\n",
        "$object->method(3);\n",
        "new Widget 4;\n",
        "$callback->(5);\n",
        "new(6);\n",
    );
    let ast = parse_clean(source)?;
    let mut named_calls = Vec::new();
    let mut amper_calls = Vec::new();
    let mut method_calls = Vec::new();
    let mut indirect_calls = Vec::new();
    let mut coderef_calls = Vec::new();
    let mut ordinary_new_calls = Vec::new();

    walk(&ast, &mut |node| match &node.kind {
        NodeKind::FunctionCall { name, args } if name == "named" => named_calls.push((
            source_text(source, node),
            args.iter().filter_map(|arg| source_text(source, arg)).collect::<Vec<_>>(),
            args.len() == 1 && matches!(&args[0].kind, NodeKind::Number { value } if value == "1"),
        )),
        NodeKind::AmperCall { name, args } if name == "named" => amper_calls.push((
            source_text(source, node),
            args.iter().filter_map(|arg| source_text(source, arg)).collect::<Vec<_>>(),
            args.len() == 1 && matches!(&args[0].kind, NodeKind::Number { value } if value == "2"),
        )),
        NodeKind::MethodCall { object, method, args } if method == "method" => method_calls.push((
            source_text(source, node),
            source_text(source, object),
            args.iter().filter_map(|arg| source_text(source, arg)).collect::<Vec<_>>(),
            matches!(
                &object.kind,
                NodeKind::Variable { sigil, name } if sigil == "$" && name == "object"
            ),
            args.len() == 1 && matches!(&args[0].kind, NodeKind::Number { value } if value == "3"),
        )),
        NodeKind::IndirectCall { method, object, args } if method == "new" => {
            indirect_calls.push((
                source_text(source, node),
                source_text(source, object),
                args.iter().filter_map(|arg| source_text(source, arg)).collect::<Vec<_>>(),
                matches!(&object.kind, NodeKind::Identifier { name } if name == "Widget"),
                args.len() == 1
                    && matches!(&args[0].kind, NodeKind::Number { value } if value == "4"),
            ))
        }
        NodeKind::FunctionCall { name, args } if name == "->()" => coderef_calls.push((
            source_text(source, node),
            args.iter().filter_map(|arg| source_text(source, arg)).collect::<Vec<_>>(),
            args.len() == 2
                && matches!(
                    &args[0].kind,
                    NodeKind::Variable { sigil, name } if sigil == "$" && name == "callback"
                )
                && matches!(&args[1].kind, NodeKind::Number { value } if value == "5"),
        )),
        NodeKind::FunctionCall { name, args } if name == "new" => ordinary_new_calls.push((
            source_text(source, node),
            args.iter().filter_map(|arg| source_text(source, arg)).collect::<Vec<_>>(),
            args.len() == 1 && matches!(&args[0].kind, NodeKind::Number { value } if value == "6"),
        )),
        _ => {}
    });

    assert_eq!(named_calls, vec![(Some("named(1)".to_string()), vec!["1".to_string()], true)]);
    assert_eq!(amper_calls, vec![(Some("&named(2)".to_string()), vec!["2".to_string()], true)]);
    assert_eq!(
        method_calls,
        vec![(
            Some("$object->method(3)".to_string()),
            Some("$object".to_string()),
            vec!["3".to_string()],
            true,
            true,
        )]
    );
    assert_eq!(
        indirect_calls,
        vec![(
            // Current main ends the IndirectCall span before the trailing bare
            // argument; the argument remains a child. Extending the span is a
            // separate production follow-up, not this concept proof.
            Some("new Widget".to_string()),
            Some("Widget".to_string()),
            vec!["4".to_string()],
            true,
            true,
        )],
        "indirect object construction must keep object and argument ownership"
    );
    assert_eq!(
        coderef_calls,
        vec![(
            Some("$callback->(5)".to_string()),
            vec!["$callback".to_string(), "5".to_string()],
            true,
        )]
    );
    assert_eq!(ordinary_new_calls, vec![(Some("new(6)".to_string()), vec!["6".to_string()], true)]);
    Ok(())
}
