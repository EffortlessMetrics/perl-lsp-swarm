//! Structural regression for issue #14789; HIR representation remains unchanged.
use perl_parser_core::hir::{DynamicBoundaryKind, HirKind, lower_ast};
use perl_parser_core::{Node, NodeKind, Parser};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn require(condition: bool, message: impl Into<String>) -> TestResult {
    if condition { Ok(()) } else { Err(message.into().into()) }
}

fn clean(source: &str) -> Result<Node, Box<dyn std::error::Error>> {
    let output = Parser::new(source).parse_with_recovery();
    require(output.diagnostics.is_empty(), format!("{source}: {:?}", output.diagnostics))?;
    Ok(output.ast)
}

fn find<'a>(node: &'a Node, kind: &str) -> Option<&'a Node> {
    if node.kind.kind_name() == kind {
        return Some(node);
    }
    node.children().into_iter().find_map(|child| find(child, kind))
}

fn text<'a>(source: &'a str, node: &Node) -> Option<&'a str> {
    source.get(node.location.start..node.location.end)
}

fn tie<'a>(
    node: &'a Node,
    source: &str,
    target: &str,
    class: &str,
    span: &str,
    count: usize,
) -> Result<&'a [Node], Box<dyn std::error::Error>> {
    let NodeKind::Tie { variable, package, args } = &node.kind else {
        return Err(format!("expected Tie, got {:?}", node.kind).into());
    };
    require(text(source, node) == Some(span), format!("wrong tie span: {:?}", node.location))?;
    require(text(source, variable) == Some(target), "wrong target span")?;
    require(
        matches!(&variable.kind, NodeKind::Variable { .. } | NodeKind::VariableDeclaration { .. }),
        "wrong target kind",
    )?;
    require(
        matches!(&package.kind, NodeKind::String { value, .. } if value == &format!("'{class}'")),
        format!("wrong class: {:?}", package.kind),
    )?;
    require(text(source, package) == Some(format!("'{class}'").as_str()), "wrong class span")?;
    let variable = if let NodeKind::VariableDeclaration { variable, .. } = &variable.kind {
        variable.as_ref()
    } else {
        variable.as_ref()
    };
    require(
        matches!(&variable.kind, NodeKind::Variable { sigil, name } if format!("{sigil}{name}") == target.trim_start_matches("my ")),
        "wrong variable identity",
    )?;
    require(args.len() == count, format!("wrong constructor argument count: {}", args.len()))?;
    Ok(args)
}

#[test]
fn nested_tie_owns_operands_and_spans() -> TestResult {
    for (source, inner_span, seed_count) in [
        ("tie %h, 'C', tie(%h2, 'D');", "tie(%h2, 'D')", 0),
        ("tie %h, 'C', tie(%h2, 'D', $seed);", "tie(%h2, 'D', $seed)", 1),
    ] {
        let ast = clean(source)?;
        let outer = find(&ast, "Tie").ok_or("missing outer tie")?;
        let args = tie(outer, source, "%h", "C", source.trim_end_matches(';'), 1)?;
        let inner = args.first().ok_or("missing nested tie")?;
        let inner_args = tie(inner, source, "%h2", "D", inner_span, seed_count)?;
        if seed_count == 1 {
            require(
                text(source, inner_args.first().ok_or("missing seed")?) == Some("$seed"),
                "wrong seed",
            )?;
            require(
                matches!(&inner_args.first().ok_or("missing seed")?.kind, NodeKind::Variable { sigil, name } if sigil == "$" && name == "seed"),
                "wrong seed identity",
            )?;
        }
        let hir = lower_ast(&ast);
        let ranges: Vec<_> = hir.items.iter().filter_map(|item| {
        matches!(&item.kind, HirKind::DynamicBoundary(b) if b.kind == DynamicBoundaryKind::TiedPlaceBinding)
            .then_some(item.anchor.range)
    }).collect();
        require(
            ranges == vec![inner.location, outer.location],
            format!("wrong HIR tie anchors/order: {ranges:?}"),
        )?;
    }
    Ok(())
}

#[test]
fn parenthesized_tie_does_not_swallow_call_sibling() -> TestResult {
    let source = "f(tie(%h, 'C'), $after);";
    let ast = clean(source)?;
    let call = find(&ast, "FunctionCall").ok_or("missing call")?;
    let NodeKind::FunctionCall { name, args } = &call.kind else {
        return Err("wrong call".into());
    };
    require(name == "f" && args.len() == 2, "outer call must retain two arguments")?;
    tie(args.first().ok_or("missing tie")?, source, "%h", "C", "tie(%h, 'C')", 0)?;
    require(
        matches!(&args.get(1).ok_or("missing sibling")?.kind, NodeKind::Variable { sigil, name } if sigil == "$" && name == "after"),
        "wrong sibling identity",
    )?;
    require(args.get(1).and_then(|arg| text(source, arg)) == Some("$after"), "lost sibling")
}

#[test]
fn assignment_and_separator_controls() -> TestResult {
    for expression in ["tie %h, 'C'", "tie(%h, 'C')", "tie(%h => 'C',)", "tie(my %h, 'C')"] {
        let source = format!("my $obj = {expression};");
        let ast = clean(&source)?;
        let decl = find(&ast, "VariableDeclaration").ok_or("missing declaration")?;
        let NodeKind::VariableDeclaration { initializer: Some(init), .. } = &decl.kind else {
            return Err("lost initializer".into());
        };
        let target = if expression.contains("my %h") { "my %h" } else { "%h" };
        tie(init, &source, target, "C", expression, 0)?;
    }
    Ok(())
}

#[test]
fn malformed_tie_diagnoses_and_preserves_following_statement() -> TestResult {
    for bad in ["my $obj = tie(%h);", "my $obj = tie(%h,);", "f(tie(%h, 'C'];", "f(tie(%h, 'C';"] {
        let source = format!("{bad} my $after = 42;");
        let output = Parser::new(&source).parse_with_recovery();
        require(!output.diagnostics.is_empty(), format!("silently accepted {bad}"))?;
        fn has_after(node: &Node, source: &str) -> bool {
            matches!(&node.kind, NodeKind::VariableDeclaration { variable, initializer: Some(init), .. }
                if matches!(&variable.kind, NodeKind::Variable { sigil, name } if sigil == "$" && name == "after")
                && matches!(&init.kind, NodeKind::Number { value } if value == "42")
                && text(source, node) == Some("my $after = 42"))
                || node.children().into_iter().any(|child| has_after(child, source))
        }
        require(
            has_after(&output.ast, &source),
            format!("lost following declaration for {bad}: {:?}", output.ast),
        )?;
    }
    Ok(())
}

#[test]
fn bare_tie_retains_list_operator_ownership() -> TestResult {
    let source = "f(tie %h, 'C', $after);";
    let ast = clean(source)?;
    let call = find(&ast, "FunctionCall").ok_or("missing call")?;
    let NodeKind::FunctionCall { args, .. } = &call.kind else {
        return Err("wrong call".into());
    };
    require(args.len() == 1, "bare tie must own the remaining list")?;
    let operands =
        tie(args.first().ok_or("missing tie")?, source, "%h", "C", "tie %h, 'C', $after", 1)?;
    require(
        matches!(&operands.first().ok_or("missing after")?.kind, NodeKind::Variable { sigil, name } if sigil == "$" && name == "after"),
        "wrong bare argument",
    )?;
    let source = "tie %h, 'C', tie %h2, 'D', $arg;";
    let ast = clean(source)?;
    let outer = find(&ast, "Tie").ok_or("missing outer")?;
    // Statement spans have their own existing bookkeeping; this control concerns
    // list ownership, while the parenthesized nested test pins both exact spans.
    let NodeKind::Tie { args, .. } = &outer.kind else {
        return Err("wrong outer".into());
    };
    require(args.len() == 1, "outer must have one nested constructor argument")?;
    let operands =
        tie(args.first().ok_or("missing inner")?, source, "%h2", "D", "tie %h2, 'D', $arg", 1)?;
    require(
        matches!(&operands.first().ok_or("missing arg")?.kind, NodeKind::Variable { sigil, name } if sigil == "$" && name == "arg"),
        "wrong nested bare argument",
    )
}

#[test]
fn ternary_class_keeps_constructor_and_caller_arguments() -> TestResult {
    for separator in [",", "=>"] {
        let source = format!("f(tie(%h, $cond ? 'C' : 'D' {separator} $seed), $after);");
        let ast = clean(&source)?;
        let call = find(&ast, "FunctionCall").ok_or("missing outer call")?;
        let NodeKind::FunctionCall { name, args } = &call.kind else {
            return Err("expected function call".into());
        };
        require(name == "f" && args.len() == 2, "caller must own two arguments")?;
        let tied = args.first().ok_or("missing tie argument")?;
        let NodeKind::Tie { variable, package, args: constructor_args } = &tied.kind else {
            return Err("expected tie argument".into());
        };
        require(
            matches!(&variable.kind, NodeKind::Variable { sigil, name } if sigil == "%" && name == "h"),
            "wrong tied variable",
        )?;
        require(constructor_args.len() == 1, "ternary class swallowed constructor argument")?;
        let NodeKind::Ternary { condition, then_expr, else_expr } = &package.kind else {
            return Err("class must be a ternary expression".into());
        };
        require(
            matches!(&condition.kind, NodeKind::Variable { sigil, name } if sigil == "$" && name == "cond"),
            "wrong ternary condition",
        )?;
        for (branch, expected) in [(then_expr, "'C'"), (else_expr, "'D'")] {
            require(
                matches!(&branch.kind, NodeKind::String { value, .. } if value == expected)
                    && text(&source, branch) == Some(expected),
                "ternary branch must retain only its class operand",
            )?;
        }
        require(text(&source, package) == Some("$cond ? 'C' : 'D'"), "wrong class span")?;
        let seed = constructor_args.first().ok_or("missing constructor seed")?;
        require(
            matches!(&seed.kind, NodeKind::Variable { sigil, name } if sigil == "$" && name == "seed")
                && text(&source, seed) == Some("$seed"),
            "wrong constructor argument",
        )?;
        require(
            text(&source, tied)
                == Some(format!("tie(%h, $cond ? 'C' : 'D' {separator} $seed)").as_str()),
            "wrong tie span",
        )?;
        let after = args.get(1).ok_or("missing caller sibling")?;
        require(
            matches!(&after.kind, NodeKind::Variable { sigil, name } if sigil == "$" && name == "after")
                && text(&source, after) == Some("$after"),
            "wrong caller sibling",
        )?;
    }
    Ok(())
}

#[test]
fn tie_operand_boundary_preserves_nested_expression_ownership() -> TestResult {
    for expression in [
        "$cond ? ('C', 'D') : 'E'",
        "$cond ? 'C' : ('D', 'E')",
        "choose('C', 'D')",
        "$class = $cond ? 'C' : 'D'",
        "$cond ? 'C' : $other ? 'D' : 'E'",
    ] {
        let source = format!("f(tie(%h, {expression}, $seed), $after);");
        let ast = clean(&source)?;
        let tied = find(&ast, "Tie").ok_or("missing tie")?;
        let NodeKind::Tie { package, args, .. } = &tied.kind else {
            return Err("expected tie".into());
        };
        require(
            text(&source, package) == Some(expression),
            "class lost nested expression ownership",
        )?;
        require(args.len() == 1, "nested class must leave one constructor argument")?;
        require(
            matches!(&args.first().ok_or("missing seed")?.kind, NodeKind::Variable { sigil, name } if sigil == "$" && name == "seed"),
            "wrong constructor seed",
        )?;
        if expression.starts_with("$cond ?")
            && (expression.contains("('C', 'D')") || expression.contains("('D', 'E')"))
        {
            let NodeKind::Ternary { then_expr, else_expr, .. } = &package.kind else {
                return Err("grouped class must retain ternary".into());
            };
            let grouped = if expression.starts_with("$cond ? (") { then_expr } else { else_expr };
            require(
                matches!(&grouped.kind, NodeKind::ArrayLiteral { elements } if elements.len() == 2),
                "explicit group must retain both list elements",
            )?;
        } else if expression.starts_with("choose") {
            require(
                matches!(&package.kind, NodeKind::FunctionCall { name, args } if name == "choose" && args.len() == 2),
                "nested call must retain both operands",
            )?;
        } else if expression.starts_with("$class =") {
            require(
                matches!(&package.kind, NodeKind::Assignment { rhs, .. } if matches!(&rhs.kind, NodeKind::Ternary { .. })),
                "assignment RHS must retain the ternary",
            )?;
        } else {
            require(
                matches!(&package.kind, NodeKind::Ternary { else_expr, .. } if matches!(&else_expr.kind, NodeKind::Ternary { .. })),
                "else chain must remain nested",
            )?;
        }
    }
    let source = "f(tie(%h, 'C', $cond ? $left : $right, $seed), $after);";
    let ast = clean(source)?;
    let tied = find(&ast, "Tie").ok_or("missing constructor ternary tie")?;
    let args = tie(tied, source, "%h", "C", "tie(%h, 'C', $cond ? $left : $right, $seed)", 2)?;
    require(
        matches!(&args.first().ok_or("missing ternary argument")?.kind, NodeKind::Ternary { .. })
            && args.get(1).and_then(|arg| text(source, arg)) == Some("$seed"),
        "constructor ternary must leave its following operand outside",
    )
}
