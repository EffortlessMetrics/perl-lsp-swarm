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
    let mut class_span = None;
    let mut field_span = None;
    let mut method_span = None;
    let mut adjust_span = None;

    walk(&ast, &mut |node| match &node.kind {
        NodeKind::Class { name, name_span, parents, .. } if name == "Example" => {
            assert_eq!(
                name_span.and_then(|span| source.get(span.start..span.end)),
                Some("Example"),
                "class name span must identify only the declared name"
            );
            assert_eq!(parents, &["Base".to_string()]);
            class_span = source_text(source, node);
        }
        NodeKind::VariableDeclaration { declarator, variable, attributes, .. }
            if declarator == "field"
                && matches!(&variable.kind, NodeKind::Variable { sigil, name } if sigil == "$" && name == "value") =>
        {
            assert!(attributes.iter().any(|attribute| attribute == "param"));
            assert!(attributes.iter().any(|attribute| attribute == "reader"));
            field_span = source_text(source, node);
        }
        NodeKind::Method { name, name_span: Some(name_span), signature, attributes, .. }
            if name == "get" =>
        {
            assert_eq!(source.get(name_span.start..name_span.end), Some("get"));
            assert!(signature.is_some(), "method signature must remain attached");
            assert!(attributes.iter().any(|attribute| attribute == "lvalue"));
            method_span = source_text(source, node);
        }
        NodeKind::Method { name, name_span: None, .. } if name == "ADJUST" => {
            adjust_span = source_text(source, node);
        }
        _ => {}
    });

    let class_span = class_span.ok_or_else(|| "class declaration was not preserved".to_string())?;
    assert!(class_span.starts_with("class Example 1.23 :isa(Base) {"));
    assert!(class_span.ends_with("}\n") || class_span.ends_with('}'));

    let field_span = field_span.ok_or_else(|| "class field was not preserved".to_string())?;
    assert_eq!(field_span.trim_end_matches(';'), "field $value :param :reader = 1");

    let method_span = method_span.ok_or_else(|| "method declaration was not preserved".to_string())?;
    assert_eq!(
        method_span,
        "method get($fallback = 0) :lvalue { return $value // $fallback; }"
    );

    let adjust_span = adjust_span.ok_or_else(|| "ADJUST block was not preserved".to_string())?;
    assert_eq!(adjust_span, "ADJUST { $value = 2; }");
    Ok(())
}

#[test]
fn class_keywords_remain_ordinary_calls_outside_class_context() -> Result<(), String> {
    let source = "field($outside); method($outside);";
    let ast = parse_clean(source)?;
    let mut field_calls = Vec::new();
    let mut method_calls = Vec::new();

    walk(&ast, &mut |node| {
        if let NodeKind::FunctionCall { name, .. } = &node.kind {
            match name.as_str() {
                "field" => {
                    if let Some(text) = source_text(source, node) {
                        field_calls.push(text);
                    }
                }
                "method" => {
                    if let Some(text) = source_text(source, node) {
                        method_calls.push(text);
                    }
                }
                _ => {}
            }
        }
    });

    assert_eq!(field_calls, vec!["field($outside)"]);
    assert_eq!(method_calls, vec!["method($outside)"]);
    Ok(())
}
