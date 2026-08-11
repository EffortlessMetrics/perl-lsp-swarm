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

fn count_in_subtree(node: &Node, predicate: &impl Fn(&NodeKind) -> bool) -> usize {
    usize::from(predicate(&node.kind))
        + node
            .children()
            .into_iter()
            .map(|child| count_in_subtree(child, predicate))
            .sum::<usize>()
}

#[test]
fn package_statement_and_block_forms_keep_exact_name_and_body_geometry() -> Result<(), String> {
    let source = concat!(
        "package Alpha::One 1.23;\n",
        "package Beta::Two { sub inside { 1 } }\n",
    );
    let ast = parse_clean(source)?;
    let mut packages = Vec::new();

    walk(&ast, &mut |node| {
        if let NodeKind::Package {
            name,
            name_span,
            block,
        } = &node.kind
        {
            let name_text = source
                .get(name_span.start..name_span.end)
                .map(str::to_owned);
            let statement_suffix = block.is_none().then(|| {
                source
                    .get(name_span.end..node.location.end)
                    .map(str::trim)
                    .map(str::to_owned)
            });
            let block_text = block.as_deref().and_then(|owned| source_text(source, owned));
            let block_is_block = block
                .as_deref()
                .is_some_and(|owned| matches!(&owned.kind, NodeKind::Block { .. }));
            let owned_inside_subroutines = block.as_deref().map_or(0, |owned| {
                count_in_subtree(owned, &|kind| {
                    matches!(
                        kind,
                        NodeKind::Subroutine { name: Some(name), .. } if name == "inside"
                    )
                })
            });

            packages.push((
                name.clone(),
                name_text,
                source_text(source, node),
                statement_suffix.flatten(),
                block_text,
                block_is_block,
                owned_inside_subroutines,
            ));
        }
    });

    assert_eq!(
        packages,
        vec![
            (
                "Alpha::One".to_string(),
                Some("Alpha::One".to_string()),
                Some("package Alpha::One 1.23".to_string()),
                Some("1.23".to_string()),
                None,
                false,
                0,
            ),
            (
                "Beta::Two".to_string(),
                Some("Beta::Two".to_string()),
                Some("package Beta::Two { sub inside { 1 } }".to_string()),
                None,
                Some("{ sub inside { 1 } }".to_string()),
                true,
                1,
            ),
        ],
        "package versions must remain outside the name while block packages own their body"
    );
    Ok(())
}

#[test]
fn use_and_no_keep_exact_module_and_flattened_argument_boundaries() -> Result<(), String> {
    let source = concat!(
        "use Feature::Bundle qw(alpha beta);\n",
        "use Feature::Bundle ();\n",
        "use v5.38;\n",
        "no warnings 'experimental::signatures';\n",
    );
    let ast = parse_clean(source)?;
    let mut uses = Vec::new();
    let mut no_directives = Vec::new();

    walk(&ast, &mut |node| match &node.kind {
        NodeKind::Use { module, args, .. } => uses.push((
            module.clone(),
            args.clone(),
            source_text(source, node),
        )),
        NodeKind::No { module, args, .. } => no_directives.push((
            module.clone(),
            args.clone(),
            source_text(source, node),
        )),
        _ => {}
    });

    assert_eq!(
        uses,
        vec![
            (
                "Feature::Bundle".to_string(),
                vec!["qw(alpha beta)".to_string()],
                Some("use Feature::Bundle qw(alpha beta)".to_string()),
            ),
            (
                "Feature::Bundle".to_string(),
                Vec::<String>::new(),
                Some("use Feature::Bundle ()".to_string()),
            ),
            (
                "v5.38".to_string(),
                Vec::<String>::new(),
                Some("use v5.38".to_string()),
            ),
        ],
        "version directives and explicit empty imports must not collapse into another module"
    );
    assert_eq!(
        no_directives,
        vec![(
            "warnings".to_string(),
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
    let mut targets = Vec::new();

    walk(&ast, &mut |node| {
        if let NodeKind::FunctionCall { name, args } = &node.kind
            && name == "require"
        {
            let Some(target) = args.first() else {
                targets.push((
                    "missing".to_string(),
                    None,
                    source_text(source, node),
                    args.len(),
                ));
                return;
            };
            let target_kind = match &target.kind {
                NodeKind::Variable { sigil, name } if sigil == "$" && name == "module" => {
                    "dynamic"
                }
                NodeKind::Identifier { name } if name == "Static::Module" => "bareword",
                NodeKind::String { .. } => "string",
                _ => "unexpected",
            };
            targets.push((
                target_kind.to_string(),
                source_text(source, target),
                source_text(source, node),
                args.len(),
            ));
        }
    });

    assert_eq!(
        targets,
        vec![
            (
                "dynamic".to_string(),
                Some("$module".to_string()),
                Some("require $module".to_string()),
                1,
            ),
            (
                "bareword".to_string(),
                Some("Static::Module".to_string()),
                Some("require Static::Module".to_string()),
                1,
            ),
            (
                "string".to_string(),
                Some("'relative/file.pl'".to_string()),
                Some("require 'relative/file.pl'".to_string()),
                1,
            ),
        ],
        "each require form must own exactly one target with a distinct node identity"
    );
    Ok(())
}
