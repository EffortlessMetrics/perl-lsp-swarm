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

#[test]
fn class_field_method_and_adjust_keep_current_ast_identity() -> Result<(), String> {
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
    let mut class_seen = false;
    let mut field_seen = false;
    let mut method_seen = false;
    let mut adjust_seen = false;

    walk(&ast, &mut |node| match &node.kind {
        NodeKind::Class { name, name_span, parents, .. } if name == "Example" => {
            let span = name_span.ok_or_else(|| "class name span is missing".to_string());
            let (start, end) = match span {
                Ok(span) => (span.start, span.end),
                Err(error) => panic!("{error}"),
            };
            assert_eq!(source.get(start..end), Some("Example"));
            assert!(parents.iter().any(|parent| parent == "Base"));
            class_seen = true;
        }
        NodeKind::VariableDeclaration { declarator, variable, attributes, .. }
            if declarator == "field"
                && matches!(&variable.kind, NodeKind::Variable { sigil, name } if sigil == "$" && name == "value") =>
        {
            assert!(attributes.iter().any(|attribute| attribute == "param"));
            assert!(attributes.iter().any(|attribute| attribute == "reader"));
            field_seen = true;
        }
        NodeKind::Method { name, name_span: Some(name_span), signature, attributes, .. }
            if name == "get" =>
        {
            assert_eq!(source.get(name_span.start..name_span.end), Some("get"));
            assert!(signature.is_some(), "method signature must remain attached");
            assert!(attributes.iter().any(|attribute| attribute == "lvalue"));
            method_seen = true;
        }
        NodeKind::Method { name, name_span: None, .. } if name == "ADJUST" => {
            adjust_seen = true;
        }
        _ => {}
    });

    assert!(class_seen, "class declaration was not preserved");
    assert!(field_seen, "class field was not preserved as a field declaration");
    assert!(method_seen, "method declaration was not preserved");
    assert!(adjust_seen, "ADJUST block was not preserved in its current method-like form");
    Ok(())
}

#[test]
fn class_keywords_remain_ordinary_calls_outside_class_context() -> Result<(), String> {
    let ast = parse_clean("field($outside); method($outside);")?;
    let mut field_call = 0usize;
    let mut method_call = 0usize;

    walk(&ast, &mut |node| {
        if let NodeKind::FunctionCall { name, .. } = &node.kind {
            match name.as_str() {
                "field" => field_call += 1,
                "method" => method_call += 1,
                _ => {}
            }
        }
    });

    assert_eq!(field_call, 1, "field(...) outside a class must remain an ordinary call");
    assert_eq!(method_call, 1, "method(...) outside a class must remain an ordinary call");
    Ok(())
}
