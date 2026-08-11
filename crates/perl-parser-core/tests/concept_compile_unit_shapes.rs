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

#[test]
fn package_statement_and_block_forms_keep_name_geometry() -> Result<(), String> {
    let source = concat!(
        "package Alpha::One 1.23;\n",
        "package Beta::Two { sub inside { 1 } }\n",
    );
    let ast = parse_clean(source)?;
    let mut statement_package = 0usize;
    let mut block_package = 0usize;

    walk(&ast, &mut |node| {
        if let NodeKind::Package { name, name_span, block } = &node.kind {
            let observed = source.get(name_span.start..name_span.end);
            match (name.as_str(), block.is_some()) {
                ("Alpha::One", false) => {
                    assert_eq!(observed, Some("Alpha::One"));
                    statement_package += 1;
                }
                ("Beta::Two", true) => {
                    assert_eq!(observed, Some("Beta::Two"));
                    block_package += 1;
                }
                _ => {}
            }
        }
    });

    assert_eq!(statement_package, 1, "semicolon package form was not preserved");
    assert_eq!(block_package, 1, "block package form was not preserved");
    Ok(())
}

#[test]
fn use_and_no_keep_module_and_argument_boundaries() -> Result<(), String> {
    let source = concat!(
        "use Feature::Bundle qw(alpha beta);\n",
        "no warnings 'experimental::signatures';\n",
    );
    let ast = parse_clean(source)?;
    let mut use_seen = false;
    let mut no_seen = false;

    walk(&ast, &mut |node| match &node.kind {
        NodeKind::Use { module, args, .. } if module == "Feature::Bundle" => {
            let joined = args.join(" ");
            assert!(joined.contains("alpha"), "use arguments lost alpha: {joined}");
            assert!(joined.contains("beta"), "use arguments lost beta: {joined}");
            use_seen = true;
        }
        NodeKind::No { module, args, .. } if module == "warnings" => {
            let joined = args.join(" ");
            assert!(
                joined.contains("experimental::signatures"),
                "no arguments lost warning category: {joined}"
            );
            no_seen = true;
        }
        _ => {}
    });

    assert!(use_seen, "use Module must remain a Use node");
    assert!(no_seen, "no Module must remain a No node");
    Ok(())
}

#[test]
fn dynamic_require_keeps_the_target_expression() -> Result<(), String> {
    let source = "my $module = 'Dynamic::Module'; require $module;";
    let ast = parse_clean(source)?;
    let mut dynamic_require = 0usize;

    walk(&ast, &mut |node| {
        if let NodeKind::FunctionCall { name, args } = &node.kind
            && name == "require"
            && args.iter().any(|arg| {
                matches!(&arg.kind, NodeKind::Variable { sigil, name } if sigil == "$" && name == "module")
            })
        {
            dynamic_require += 1;
        }
    });

    assert_eq!(
        dynamic_require, 1,
        "require $module must preserve the dynamic target expression"
    );
    Ok(())
}
