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

    assert_eq!(statement_package.len(), 1, "semicolon package form was not preserved");
    assert!(statement_package[0].starts_with("package Alpha::One 1.23"));
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
    let mut use_payload = None;
    let mut no_payload = None;

    walk(&ast, &mut |node| match &node.kind {
        NodeKind::Use { module, args, .. } if module == "Feature::Bundle" => {
            use_payload = Some((args.clone(), source_text(source, node)));
        }
        NodeKind::No { module, args, .. } if module == "warnings" => {
            no_payload = Some((args.clone(), source_text(source, node)));
        }
        _ => {}
    });

    let (use_args, use_span) = use_payload.ok_or_else(|| "use Module was not preserved".to_string())?;
    let joined_use = use_args.join(" ");
    assert!(joined_use.contains("alpha") && joined_use.contains("beta"));
    assert_eq!(use_span.as_deref(), Some("use Feature::Bundle qw(alpha beta)"));

    let (no_args, no_span) = no_payload.ok_or_else(|| "no Module was not preserved".to_string())?;
    assert!(no_args.join(" ").contains("experimental::signatures"));
    assert_eq!(no_span.as_deref(), Some("no warnings 'experimental::signatures'"));
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
            && let Some(target) = args.first()
        {
            let span = source_text(source, target);
            match &target.kind {
                NodeKind::Variable { sigil, name } if sigil == "$" && name == "module" => {
                    if let Some(span) = span {
                        dynamic_targets.push(span);
                    }
                }
                NodeKind::Identifier { name } if name == "Static::Module" => {
                    if let Some(span) = span {
                        bareword_targets.push(span);
                    }
                }
                NodeKind::String { .. } => {
                    if let Some(span) = span {
                        string_targets.push(span);
                    }
                }
                _ => {}
            }
        }
    });

    assert_eq!(dynamic_targets, vec!["$module"]);
    assert_eq!(bareword_targets, vec!["Static::Module"]);
    assert_eq!(string_targets, vec!["'relative/file.pl'"]);
    Ok(())
}
