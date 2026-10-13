//! Regression coverage for truncated postfix dereferences and recovered slice spans (#14174).

use perl_parser_core::{Node, NodeKind, ParseError, Parser, error::RecoveryKind};

fn contains_truncated_chain(diagnostics: &[ParseError]) -> bool {
    diagnostics.iter().any(|diagnostic| {
        matches!(diagnostic, ParseError::Recovered { kind: RecoveryKind::TruncatedChain, .. })
    })
}

#[test]
fn every_truncated_postfix_dereference_sigil_records_recovery() -> Result<(), String> {
    let mut missing = Vec::new();
    for suffix in ["$", "@", "%", "&", "*", "$#"] {
        let source = format!("my $value = $ref->{suffix}");
        let mut parser = Parser::new(&source);
        let output = parser.parse_with_recovery();

        if !contains_truncated_chain(&output.diagnostics) {
            missing.push((source, output.diagnostics, output.ast.to_sexp()));
        }
    }
    if !missing.is_empty() {
        return Err(format!("truncated postfix cases without recovery: {missing:?}"));
    }
    Ok(())
}

#[test]
fn truncated_recovery_span_covers_the_consumed_arrow_and_sigil() -> Result<(), String> {
    for suffix in ["$", "@", "%", "&", "*", "$#"] {
        let source = format!("$ref->{suffix}");
        let mut parser = Parser::new(&source);
        let output = parser.parse_with_recovery();
        let Some(ParseError::Recovered { location, .. }) = output.diagnostics.first() else {
            return Err(format!(
                "{source}: expected a recovery diagnostic, got {:?}",
                output.diagnostics
            ));
        };
        if *location != source.len() {
            return Err(format!(
                "{source}: recovery location {location} must sit after the consumed `->{suffix}`"
            ));
        }
        let error = first_error(&output.ast).ok_or_else(|| format!("{source}: no error node"))?;
        if error.location.end != source.len() {
            return Err(format!(
                "{source}: error span {:?} must include the suffix",
                error.location
            ));
        }
    }
    Ok(())
}

#[test]
fn braced_dynamic_method_call_is_not_a_truncated_chain() -> Result<(), String> {
    // `$obj->${method}()` is valid Perl (perl -c accepts it); the lexer emits a
    // standalone `$` before `{`, which must not be mistaken for `->$*` truncation.
    for source in ["$obj->${method}();", "$obj->${ $name }(1, 2);", "$obj->${method};"] {
        let mut parser = Parser::new(source);
        let output = parser.parse_with_recovery();
        if contains_truncated_chain(&output.diagnostics) {
            return Err(format!(
                "{source}: valid dynamic method call reported as truncated: {:?}",
                output.diagnostics
            ));
        }
        if first_error(&output.ast).is_some() {
            return Err(format!("{source}: unexpected Error node in {}", output.ast.to_sexp()));
        }
    }
    Ok(())
}

#[test]
fn braced_dynamic_method_expression_stays_traversable() -> Result<(), String> {
    // The method-name expression inside `->${ ... }` must remain a child node so
    // strict-vars and usage analysis still see `$name`; reducing the call to
    // `MethodCall { method: "${ $name }" }` text would drop it (review finding).
    let source = "$obj->${ $name }(1, 2);";
    let mut parser = Parser::new(source);
    let output = parser.parse_with_recovery();
    let name_node = find_variable(&output.ast, "name").ok_or_else(|| {
        format!("{source}: method expression `$name` missing from {}", output.ast.to_sexp())
    })?;
    let expected_start = source.find("$name").ok_or("fixture must contain $name")?;
    if name_node.location.start != expected_start {
        return Err(format!(
            "{source}: `$name` span starts at {} not {expected_start}",
            name_node.location.start
        ));
    }
    Ok(())
}

fn find_variable<'a>(node: &'a Node, wanted: &str) -> Option<&'a Node> {
    if let NodeKind::Variable { name, .. } = &node.kind
        && name == wanted
    {
        return Some(node);
    }
    node.children().into_iter().find_map(|child| find_variable(child, wanted))
}

fn first_error(node: &Node) -> Option<&Node> {
    if matches!(node.kind, NodeKind::Error { .. }) {
        return Some(node);
    }
    node.children().into_iter().find_map(first_error)
}

#[test]
fn recovered_slices_contain_their_selectors_and_preserve_following_declaration()
-> Result<(), String> {
    for (expression, expected_operator, selector_text) in [
        ("@$ref{'alpha'", "hash_slice", "'alpha'"),
        ("$ref->@[0", "->@[]", "0"),
        ("$ref->@{'alpha'", "hash_slice", "'alpha'"),
        ("$ref->%{'alpha'", "->%{}", "'alpha'"),
    ] {
        let source = format!("my @values = {expression};\nmy $next = 1;\n");
        let mut parser = Parser::new(&source);
        let output = parser.parse_with_recovery();
        if output.diagnostics.is_empty() {
            return Err(format!("{source}: missing delimiter must produce a diagnostic"));
        }
        let NodeKind::Program { statements } = &output.ast.kind else {
            return Err(format!("{source}: expected Program, got {}", output.ast.to_sexp()));
        };
        let first =
            statements.first().ok_or_else(|| format!("{source}: missing first statement"))?;
        let NodeKind::VariableDeclaration { initializer: Some(slice), .. } = &first.kind else {
            return Err(format!("{source}: recovered declaration missing: {}", first.to_sexp()));
        };
        let (target, selector) = match &slice.kind {
            NodeKind::HashSlice { target, keys } if expected_operator == "hash_slice" => {
                (target, keys)
            }
            NodeKind::Binary { op, left, right }
                if expected_operator != "hash_slice" && op == expected_operator =>
            {
                (left, right)
            }
            _ => return Err(format!("{source}: wrong recovered selector: {}", slice.to_sexp())),
        };
        let receiver = if expression.starts_with('@') {
            let NodeKind::Unary { op, operand } = &target.kind else {
                return Err(format!("{source}: missing array dereference receiver"));
            };
            if op != "@{}" {
                return Err(format!("{source}: wrong receiver operator {op}"));
            }
            operand
        } else {
            target
        };
        if !matches!(&receiver.kind, NodeKind::Variable { sigil, name } if sigil == "$" && name == "ref")
        {
            return Err(format!("{source}: wrong receiver: {}", receiver.to_sexp()));
        }
        let selector_matches = match &selector.kind {
            NodeKind::Number { value } => selector_text == "0" && value == "0",
            NodeKind::String { value, interpolated } => {
                selector_text == "'alpha'" && value == "'alpha'" && !interpolated
            }
            _ => false,
        };
        if !selector_matches {
            return Err(format!("{source}: wrong selector value: {}", selector.to_sexp()));
        }
        let selector_start = source.find(selector_text).ok_or("fixture selector missing")?;
        if selector.location.start != selector_start
            || selector.location.end != selector_start + selector_text.len()
        {
            return Err(format!(
                "{source}: selector has wrong source span: {:?}",
                selector.location
            ));
        }
        for child in [target, selector] {
            if slice.location.start > child.location.start
                || slice.location.end < child.location.end
            {
                return Err(format!(
                    "{source}: slice span {:?} must contain child span {:?}",
                    slice.location, child.location
                ));
            }
        }
        let next =
            statements.get(1).ok_or_else(|| format!("{source}: following statement lost"))?;
        let NodeKind::VariableDeclaration {
            declarator, variable, initializer: Some(value), ..
        } = &next.kind
        else {
            return Err(format!("{source}: following declaration lost: {}", next.to_sexp()));
        };
        if declarator != "my"
            || !matches!(&variable.kind, NodeKind::Variable { sigil, name } if sigil == "$" && name == "next")
            || !matches!(&value.kind, NodeKind::Number { value } if value == "1")
        {
            return Err(format!("{source}: following declaration changed: {}", next.to_sexp()));
        }
        let next_start = source.find("my $next").ok_or("fixture declaration missing")?;
        let value_start = source.find("1;").ok_or("fixture initializer missing")?;
        if statements.len() != 2
            || next.location.start != next_start
            || variable.location.start != next_start + "my ".len()
            || value.location.start != value_start
            || value.location.end != value_start + 1
        {
            return Err(format!(
                "{source}: following declaration structure or spans changed: {}",
                output.ast.to_sexp()
            ));
        }
    }
    Ok(())
}
