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
    for child in node.children() {
        if contains_static_subscript(child, expected_op, expected_key) {
            return true;
        }
    }
    false
}

fn contains_keyword_slice(node: &Node, expected_keys: &[&str]) -> bool {
    if let NodeKind::HashSlice { keys, .. } = &node.kind
        && let NodeKind::ArrayLiteral { elements } = &keys.kind
        && elements.len() == expected_keys.len()
        && elements.iter().zip(expected_keys).all(|(element, expected)| {
            matches!(&element.kind, NodeKind::Identifier { name } if name == expected)
        })
    {
        return true;
    }
    for child in node.children() {
        if contains_keyword_slice(child, expected_keys) {
            return true;
        }
    }
    false
}

fn contains_loop_control(node: &Node, expected_op: &str, expected_label: &str) -> bool {
    if let NodeKind::LoopControl { op, label } = &node.kind
        && op == expected_op
        && label.as_deref() == Some(expected_label)
    {
        return true;
    }
    for child in node.children() {
        if contains_loop_control(child, expected_op, expected_label) {
            return true;
        }
    }
    false
}

fn contains_return(node: &Node) -> bool {
    if matches!(&node.kind, NodeKind::Return { .. }) {
        return true;
    }
    for child in node.children() {
        if contains_return(child) {
            return true;
        }
    }
    false
}

#[test]
fn boundary_delimited_control_words_are_static_arrow_hash_keys() -> TestResult {
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
fn boundary_delimited_control_words_are_static_direct_hash_keys() -> TestResult {
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
fn comma_delimited_control_words_are_static_hash_slice_keys() -> TestResult {
    let expected = ["next", "last", "redo", "return"];
    let ast = parse_clean("@hash{next, last, redo, return};")?;
    if !contains_keyword_slice(&ast, &expected) {
        return Err(format!(
            "control words were not retained as a static hash-slice key list: {}",
            ast.to_sexp()
        ));
    }
    Ok(())
}

#[test]
fn executable_loop_control_and_return_forms_keep_their_ast() -> TestResult {
    let next_ast = parse_clean("next OUTER;")?;
    if !contains_loop_control(&next_ast, "next", "OUTER") {
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
