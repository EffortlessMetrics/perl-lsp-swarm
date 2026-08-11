//! Concept-level parser proofs for packages, imports, and dynamic loading (#6695).
//!
//! These tests pin parser-visible source identity. Feature activation, import
//! effects, module resolution, and compile-time execution remain downstream.

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
fn package_statement_and_block_forms_keep_name_geometry() -> Result<(), String> {
    let source = concat!(
        "package Alpha::One 1.23;\n",
        "package Beta::Two { sub inside { 1 } }\n",
    );
    let ast = parse_clean(source)?;
    let mut statement_package = Vec::new();
    let mut block_package = Vec::new();

    walk(&ast, &mut |node| {
        if let NodeKind::Package { name, name_span, block } = &node.kind {
            let observed_name = source.get(name_span.start..name_span.end);
            match (name.as_str(), block.is_some()) {
                ("Alpha::One", false) => {
                    assert_eq!(observed_name, Some("Alpha::One"));
                    if let Some(text) = source_text(source, node) {
                        statement_package.push(text);
                    }
                }
                ("Beta::Two", true) => {
                    assert_eq!(observed_name, Some("Beta::Two"));
                    if let Some(text) = source_text(source, node) {
                        block_package.push(text);
                    }
                }
                _ => {}
            }
        }
    });

    assert_eq!(
        statement_package,
        vec!["package Alpha::One 1.23".to_string()],
        "semicolon package form had an incorrect source range"
    );
    assert_eq!(block_package, vec!["package Beta::Two { sub inside { 1 } }"]);
    Ok(())
}

#[test]
fn use_and_no_keep_module_and_argument_boundaries() -> Result<(), String> {
    let source = concat!(
        "use Feature::Bundle qw(alpha beta);\n",
        "no warnings 'experimental::signatures';\n",
    );
    let ast = parse_clean(source)?;
    let mut use_payloads = Vec::new();
    let mut no_payloads = Vec::new();

    walk(&ast, &mut |node| match &node.kind {
        NodeKind::Use { module, args, .. } if module == "Feature::Bundle" => {
            use_payloads.push((args.clone(), source_text(source, node)));
        }
        NodeKind::No { module, args, .. } if module == "warnings" => {
            no_payloads.push((args.clone(), source_text(source, node)));
        }
        _ => {}
    });

    assert_eq!(
        use_payloads,
        vec![(
            vec!["qw(alpha beta)".to_string()],
            Some("use Feature::Bundle qw(alpha beta)".to_string()),
        )]
    );
    assert_eq!(
        no_payloads,
        vec![(
            vec!["'experimental::signatures'".to_string()],
            Some("no warnings 'experimental::signatures'".to_string()),
        )]
    );
    Ok(())
}

#[test]
fn require_keeps_dynamic_bareword_and_string_targets_distinct() -> Result<(), String> {
    let source = concat!(
        "my $module = 'Dynamic::Module';\n",
        "require $module;\n",
        "require Static::Module;\n",
        "require 'relative/file.pl';\n",
    );
    let ast = parse_clean(source)?;
    let mut dynamic_targets = Vec::new();
    let mut bareword_targets = Vec::new();
    let mut string_targets = Vec::new();

    walk(&ast, &mut |node| {
        if let NodeKind::FunctionCall { name, args } = &node.kind
            && name == "require"
        {
            assert_eq!(args.len(), 1, "require must retain exactly one target expression");
            if let Some(target) = args.first()
                && let (Some(target_span), Some(call_span)) =
                    (source_text(source, target), source_text(source, node))
            {
                match &target.kind {
                    NodeKind::Variable { sigil, name } if sigil == "$" && name == "module" => {
                        dynamic_targets.push((target_span, call_span));
                    }
                    NodeKind::Identifier { name } if name == "Static::Module" => {
                        bareword_targets.push((target_span, call_span));
                    }
                    NodeKind::String { .. } => {
                        string_targets.push((target_span, call_span));
                    }
                    _ => {}
                }
            }
        }
    });

    assert_eq!(
        dynamic_targets,
        vec![("$module".to_string(), "require $module".to_string())]
    );
    assert_eq!(
        bareword_targets,
        vec![("Static::Module".to_string(), "require Static::Module".to_string())]
    );
    assert_eq!(
        string_targets,
        vec![(
            "'relative/file.pl'".to_string(),
            "require 'relative/file.pl'".to_string(),
        )]
    );
    Ok(())
}
