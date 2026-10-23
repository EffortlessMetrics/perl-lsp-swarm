//! Structural regression for issue #16210: call owners that own their argument
//! separators must not let the unparenthesized ternary else tail absorb the
//! following comma / fat-arrow sibling. HIR representation remains unchanged.
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

/// Assert the node is the ternary `$COND ? 'C' : <ELSE>` whose span stops
/// before the argument separator, and return its else operand.
fn bounded_ternary<'a>(
    node: &'a Node,
    source: &str,
    condition: &str,
    then_text: &str,
    span: &str,
    else_text: &str,
) -> Result<&'a Node, Box<dyn std::error::Error>> {
    let NodeKind::Ternary { condition: cond, then_expr, else_expr } = &node.kind else {
        return Err(format!("expected Ternary, got {:?}", node.kind).into());
    };
    require(text(source, node) == Some(span), format!("wrong ternary span: {:?}", node.location))?;
    require(text(source, cond) == Some(condition), "wrong ternary condition")?;
    require(text(source, then_expr) == Some(then_text), "wrong ternary then branch")?;
    require(text(source, else_expr) == Some(else_text), "wrong ternary else branch")?;
    Ok(else_expr)
}

fn expect_variable(node: &Node, sigil: &str, name: &str) -> TestResult {
    require(
        matches!(&node.kind, NodeKind::Variable { sigil: s, name: n } if s == sigil && n == name),
        format!("expected variable {sigil}{name}, got {:?}", node.kind),
    )
}

#[test]
fn paren_call_keeps_comma_sibling_out_of_ternary_else() -> TestResult {
    let source = "f($cond ? 'C' : 'D', $seed);";
    let ast = clean(source)?;
    let call = find(&ast, "FunctionCall").ok_or("missing call")?;
    let NodeKind::FunctionCall { name, args } = &call.kind else {
        return Err("wrong call".into());
    };
    require(name == "f", "wrong call name")?;
    require(args.len() == 2, format!("call must retain two arguments, got {}", args.len()))?;
    bounded_ternary(
        args.first().ok_or("missing ternary")?,
        source,
        "$cond",
        "'C'",
        "$cond ? 'C' : 'D'",
        "'D'",
    )?;
    let sibling = args.get(1).ok_or("missing sibling")?;
    expect_variable(sibling, "$", "seed")?;
    require(text(source, sibling) == Some("$seed"), "lost sibling")
}

#[test]
fn paren_call_keeps_fat_arrow_sibling_out_of_ternary_else() -> TestResult {
    let source = "f($cond ? 'C' : 'D', k => 'v');";
    let ast = clean(source)?;
    let call = find(&ast, "FunctionCall").ok_or("missing call")?;
    let NodeKind::FunctionCall { name, args } = &call.kind else {
        return Err("wrong call".into());
    };
    require(name == "f", "wrong call name")?;
    require(args.len() == 3, format!("call must retain three arguments, got {}", args.len()))?;
    bounded_ternary(
        args.first().ok_or("missing ternary")?,
        source,
        "$cond",
        "'C'",
        "$cond ? 'C' : 'D'",
        "'D'",
    )?;
    let key = args.get(1).ok_or("missing fat-arrow key")?;
    require(
        matches!(&key.kind, NodeKind::String { value, interpolated: false } if value == "k"),
        format!("expected autoquoted key, got {:?}", key.kind),
    )?;
    let value = args.get(2).ok_or("missing fat-arrow value")?;
    require(text(source, value) == Some("'v'"), "lost fat-arrow value")
}

#[test]
fn paren_call_keeps_nested_ternary_else_bounded() -> TestResult {
    let source = "f($c1 ? 'A' : $c2 ? 'B' : 'X', $seed);";
    let ast = clean(source)?;
    let call = find(&ast, "FunctionCall").ok_or("missing call")?;
    let NodeKind::FunctionCall { args, .. } = &call.kind else {
        return Err("wrong call".into());
    };
    require(args.len() == 2, format!("call must retain two arguments, got {}", args.len()))?;
    let outer = args.first().ok_or("missing ternary")?;
    let nested = bounded_ternary(
        outer,
        source,
        "$c1",
        "'A'",
        "$c1 ? 'A' : $c2 ? 'B' : 'X'",
        "$c2 ? 'B' : 'X'",
    )?;
    bounded_ternary(nested, source, "$c2", "'B'", "$c2 ? 'B' : 'X'", "'X'")?;
    expect_variable(args.get(1).ok_or("missing sibling")?, "$", "seed")
}

#[test]
fn paren_print_call_keeps_comma_sibling_out_of_ternary_else() -> TestResult {
    let source = "print($fh, $cond ? 'C' : 'D', $seed);";
    let ast = clean(source)?;
    let call = find(&ast, "FunctionCall").ok_or("missing print call")?;
    let NodeKind::FunctionCall { name, args } = &call.kind else {
        return Err("wrong print call".into());
    };
    require(name == "print", "wrong print call name")?;
    require(args.len() == 3, format!("print must retain three arguments, got {}", args.len()))?;
    expect_variable(args.first().ok_or("missing filehandle")?, "$", "fh")?;
    bounded_ternary(
        args.get(1).ok_or("missing ternary")?,
        source,
        "$cond",
        "'C'",
        "$cond ? 'C' : 'D'",
        "'D'",
    )?;
    expect_variable(args.get(2).ok_or("missing sibling")?, "$", "seed")
}

#[test]
fn statement_tie_keeps_comma_sibling_out_of_ternary_class() -> TestResult {
    let source = "tie(%h, $cond ? 'C' : 'D', $seed);";
    let ast = clean(source)?;
    let tie = find(&ast, "Tie").ok_or("missing tie")?;
    let NodeKind::Tie { variable, package, args } = &tie.kind else {
        return Err("wrong tie".into());
    };
    expect_variable(variable, "%", "h")?;
    require(text(source, tie) == Some(source.trim_end_matches(';')), "wrong tie span")?;
    let class = package.as_ref();
    bounded_ternary(class, source, "$cond", "'C'", "$cond ? 'C' : 'D'", "'D'")?;
    require(args.len() == 1, format!("tie must retain one list argument, got {}", args.len()))?;
    expect_variable(args.first().ok_or("missing sibling")?, "$", "seed")
}

#[test]
fn statement_tie_keeps_fat_arrow_sibling_out_of_ternary_class() -> TestResult {
    let source = "tie(%h, $cond ? 'C' : 'D', k => 'v');";
    let ast = clean(source)?;
    let tie = find(&ast, "Tie").ok_or("missing tie")?;
    let NodeKind::Tie { package, args, .. } = &tie.kind else {
        return Err("wrong tie".into());
    };
    bounded_ternary(package, source, "$cond", "'C'", "$cond ? 'C' : 'D'", "'D'")?;
    require(
        args.len() == 2,
        format!("tie must retain fat-arrow key and value, got {}", args.len()),
    )?;
    // The statement tie arm does not autoquote barewords before `=>` (matching
    // the expression-position arm), so the key stays an Identifier.
    let key = args.first().ok_or("missing fat-arrow key")?;
    require(
        matches!(&key.kind, NodeKind::Identifier { name } if name == "k"),
        format!("expected bareword key, got {:?}", key.kind),
    )?;
    require(
        text(source, args.get(1).ok_or("missing fat-arrow value")?) == Some("'v'"),
        "lost value",
    )
}

#[test]
fn controls_without_ternary_tail_collection_are_unchanged() -> TestResult {
    // Plain call without a ternary keeps its two arguments.
    let ast = clean("f('C', $seed);")?;
    let NodeKind::FunctionCall { name, args } =
        &find(&ast, "FunctionCall").ok_or("missing call")?.kind
    else {
        return Err("wrong call".into());
    };
    require(name == "f" && args.len() == 2, "plain call arguments changed")?;

    // Assignment RHS still owns its ternary entirely.
    let source = "my $x = $cond ? 'C' : 'D';";
    let ast = clean(source)?;
    let decl = find(&ast, "VariableDeclaration").ok_or("missing declaration")?;
    let NodeKind::VariableDeclaration { initializer: Some(init), .. } = &decl.kind else {
        return Err("missing initializer".into());
    };
    bounded_ternary(init, source, "$cond", "'C'", "$cond ? 'C' : 'D'", "'D'")?;

    // The expression-position tie boundary landed in #16155 still holds.
    let source = "my $o = tie(%h, $cond ? 'C' : 'D', $seed);";
    let ast = clean(source)?;
    let tie = find(&ast, "Tie").ok_or("missing tie")?;
    let NodeKind::Tie { package, args, .. } = &tie.kind else {
        return Err("wrong tie".into());
    };
    bounded_ternary(package, source, "$cond", "'C'", "$cond ? 'C' : 'D'", "'D'")?;
    require(args.len() == 1, format!("expression tie must retain one argument, got {}", args.len()))
}
