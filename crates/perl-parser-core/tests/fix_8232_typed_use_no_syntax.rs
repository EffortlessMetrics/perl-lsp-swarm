//! Typed `use` / `no` directive syntax and exact spans (#8232).

use perl_ast::{
    CompileDirectiveSubject, ImportArgumentItem, ImportListPresence, NodeKind,
    VersionRequirementSyntax,
};
use perl_parser_core::{Node, Parser};

fn parse_clean(source: &str) -> Result<Node, String> {
    let mut parser = Parser::new(source);
    let ast = parser.parse().map_err(|error| format!("parse failed: {error:?}"))?;
    if parser.errors().is_empty() {
        Ok(ast)
    } else {
        Err(format!("expected clean parse, got: {:?}", parser.errors()))
    }
}

fn walk(node: &Node, visit: &mut impl FnMut(&Node)) {
    visit(node);
    for child in node.children() {
        walk(child, visit);
    }
}

fn slice(source: &str, start: usize, end: usize) -> &str {
    source.get(start..end).unwrap_or("")
}

#[test]
fn use_foo_version_qw_preserves_module_version_and_word_spans() -> Result<(), String> {
    let source = "use Foo 1.23 qw(a b);";
    let ast = parse_clean(source)?;
    let mut found = false;
    walk(&ast, &mut |node| {
        if let NodeKind::Use { module, args, syntax, module_span, .. } = &node.kind {
            assert_eq!(module, "Foo 1.23");
            assert_eq!(args, &vec!["qw(a b)".to_string()]);
            assert_eq!(slice(source, module_span.start, module_span.end), "Foo");
            match &syntax.subject {
                CompileDirectiveSubject::ModuleWithVersion { module, version } => {
                    assert_eq!(module.name, "Foo");
                    assert_eq!(slice(source, module.span.start, module.span.end), "Foo");
                    match version {
                        VersionRequirementSyntax::Numeric { raw, span, .. } => {
                            assert_eq!(raw, "1.23");
                            assert_eq!(slice(source, span.start, span.end), "1.23");
                        }
                        other => panic!("expected numeric version, got {other:?}"),
                    }
                }
                other => panic!("expected module+version subject, got {other:?}"),
            }
            let qw_words: Vec<_> = syntax
                .arguments
                .items
                .iter()
                .filter_map(|item| match item {
                    ImportArgumentItem::QwWord { value, span } => {
                        Some((value.clone(), slice(source, span.start, span.end).to_string()))
                    }
                    _ => None,
                })
                .collect();
            assert_eq!(
                qw_words,
                vec![("a".to_string(), "a".to_string()), ("b".to_string(), "b".to_string())]
            );
            found = true;
        }
    });
    assert!(found, "missing Use node");
    Ok(())
}

#[test]
fn use_vstring_is_version_pragma_not_module_import() -> Result<(), String> {
    let source = "use v5.36;";
    let ast = parse_clean(source)?;
    walk(&ast, &mut |node| {
        if let NodeKind::Use { module, args, syntax, .. } = &node.kind {
            assert_eq!(module, "v5.36");
            assert!(args.is_empty());
            assert!(matches!(
                syntax.subject,
                CompileDirectiveSubject::VersionPragma(VersionRequirementSyntax::VString { .. })
            ));
            assert!(matches!(syntax.arguments.presence, ImportListPresence::Absent));
        }
    });
    Ok(())
}

#[test]
fn no_strict_string_literal_span() -> Result<(), String> {
    let source = "no strict 'refs';";
    let ast = parse_clean(source)?;
    walk(&ast, &mut |node| {
        if let NodeKind::No { module, args, syntax, module_span, .. } = &node.kind {
            assert_eq!(module, "strict");
            assert_eq!(slice(source, module_span.start, module_span.end), "strict");
            assert_eq!(args, &vec!["'refs'".to_string()]);
            let lit = syntax.arguments.items.iter().find_map(|item| match item {
                ImportArgumentItem::StringLiteral { raw, span } => {
                    Some((raw.clone(), slice(source, span.start, span.end).to_string()))
                }
                _ => None,
            });
            assert_eq!(lit, Some(("'refs'".to_string(), "'refs'".to_string())));
        }
    });
    Ok(())
}

#[test]
fn explicit_empty_import_is_distinct_from_absent() -> Result<(), String> {
    let source = "use Foo ();";
    let ast = parse_clean(source)?;
    walk(&ast, &mut |node| {
        if let NodeKind::Use { module, args, syntax, .. } = &node.kind {
            assert_eq!(module, "Foo");
            assert!(args.is_empty());
            assert!(matches!(syntax.arguments.presence, ImportListPresence::ExplicitEmpty { .. }));
        }
    });
    Ok(())
}

#[test]
fn absent_import_list_for_bare_use() -> Result<(), String> {
    let source = "use Foo;";
    let ast = parse_clean(source)?;
    walk(&ast, &mut |node| {
        if let NodeKind::Use { args, syntax, .. } = &node.kind {
            assert!(args.is_empty());
            assert!(matches!(syntax.arguments.presence, ImportListPresence::Absent));
        }
    });
    Ok(())
}
