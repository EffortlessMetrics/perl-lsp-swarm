//! Boundary-sensitive regression coverage for keyword-shaped hash keys.

use perl_parser_core::{Node, NodeKind, Parser};

type TestResult = Result<(), String>;

fn parse_clean(source: &str) -> Result<Node, String> {
    let mut parser = Parser::new(source);
    let ast = parser.parse().map_err(|error| format!("failed to parse {source:?}: {error}"))?;
    if !parser.errors().is_empty() {
        return Err(format!("unexpected parser errors for {source:?}: {:?}", parser.errors()));
    }
    let sexp = ast.to_sexp();
    if sexp.contains("ERROR") {
        return Err(format!("unexpected ERROR node for {source:?}: {sexp}"));
    }
    Ok(ast)
}

fn contains_static_subscript(node: &Node, expected_op: &str, expected_key: &str) -> bool {
    if let NodeKind::Binary { op, right, .. } = &node.kind
        && op == expected_op
        && matches!(&right.kind, NodeKind::Identifier { name } if name == expected_key)
    {
        return true;
    }
    node.children()
        .into_iter()
        .any(|child| contains_static_subscript(child, expected_op, expected_key))
}

fn contains_loop_control(node: &Node, expected_op: &str, expected_label: Option<&str>) -> bool {
    if let NodeKind::LoopControl { op, label } = &node.kind
        && op == expected_op
        && label.as_deref() == expected_label
    {
        return true;
    }
    node.children()
        .into_iter()
        .any(|child| contains_loop_control(child, expected_op, expected_label))
}

fn contains_return(node: &Node) -> bool {
    matches!(&node.kind, NodeKind::Return { .. })
        || node.children().into_iter().any(contains_return)
}

#[test]
fn terminal_control_words_are_static_arrow_hash_keys() -> TestResult {
    for key in ["next", "last", "redo", "return"] {
        let source = format!("$hash->{{{key}}};");
        let ast = parse_clean(&source)?;
        if !contains_static_subscript(&ast, "->{}", key) {
            return Err(format!(
                "{key:?} was not retained as a static arrow-hash key: {}",
                ast.to_sexp()
            ));
        }
    }
    Ok(())
}

#[test]
fn terminal_control_words_are_static_direct_hash_keys() -> TestResult {
    for key in ["next", "last", "redo", "return"] {
        let source = format!("$hash{{{key}}};");
        let ast = parse_clean(&source)?;
        if !contains_static_subscript(&ast, "{}", key) {
            return Err(format!(
                "{key:?} was not retained as a static direct-hash key: {}",
                ast.to_sexp()
            ));
        }
    }
    Ok(())
}

#[test]
fn comma_followed_control_words_remain_executable_expressions() -> TestResult {
    for (op, source) in [
        ("next", "sub f { my %hash; my $value; while (1) { my $x = $hash{next, $value}; } }"),
        ("last", "sub f { my %hash; my $value; while (1) { my $x = $hash{last, $value}; } }"),
        ("redo", "sub f { my %hash; my $value; while (1) { my $x = $hash{redo, $value}; } }"),
    ] {
        let ast = parse_clean(source)?;
        if !contains_loop_control(&ast, op, None) {
            return Err(format!(
                "comma-followed {op:?} stopped being executable loop control: {}",
                ast.to_sexp()
            ));
        }
    }

    let source = "sub f { my %hash; my $value; my $x = $hash{return, $value}; return 99; }";
    let ast = parse_clean(source)?;
    if !contains_return(&ast) {
        return Err(format!("comma-followed return stopped being executable: {}", ast.to_sexp()));
    }
    Ok(())
}

#[test]
fn ordinary_executable_control_forms_keep_their_ast() -> TestResult {
    let next_ast = parse_clean("next OUTER;")?;
    if !contains_loop_control(&next_ast, "next", Some("OUTER")) {
        return Err(format!("labeled next stopped being loop control: {}", next_ast.to_sexp()));
    }

    let return_ast = parse_clean("return $value;")?;
    if !contains_return(&return_ast) {
        return Err(format!(
            "valued return stopped being a return expression: {}",
            return_ast.to_sexp()
        ));
    }
    Ok(())
}
