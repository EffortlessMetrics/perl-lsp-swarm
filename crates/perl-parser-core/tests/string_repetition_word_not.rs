//! Structural Perl oracle: issue #13932, comment 5748477344 (Perl 5.32.1).
use perl_parser_core::error::{ParseError, RecoveryKind};
use perl_parser_core::{Node, NodeKind, Parser};

fn clean(source: &str) -> Result<Node, String> {
    let output = Parser::new(source).parse_with_recovery();
    if !output.diagnostics.is_empty() {
        return Err(format!("{source:?}: {:?}\n{}", output.diagnostics, output.ast.to_sexp()));
    }
    Ok(output.ast)
}

fn expression(ast: &Node) -> Result<&Node, String> {
    match &ast.kind {
        NodeKind::Program { statements } if statements.len() == 1 => {
            expression(statements.first().ok_or("missing statement")?)
        }
        NodeKind::ExpressionStatement { expression } => Ok(expression),
        _ => Err(format!("expected one expression statement: {}", ast.to_sexp())),
    }
}

fn shape(node: &Node) -> Result<String, String> {
    match &node.kind {
        NodeKind::Binary { op, left, right } => {
            Ok(format!("{op}({},{})", shape(left)?, shape(right)?))
        }
        NodeKind::Unary { op, operand } => Ok(format!("{op}({})", shape(operand)?)),
        NodeKind::Variable { sigil, name } => Ok(format!("{sigil}{name}")),
        NodeKind::Number { value } => Ok(value.clone()),
        NodeKind::MissingExpression => Ok("missing".into()),
        NodeKind::Identifier { name } => Ok(format!("id:{name}")),
        NodeKind::String { value, .. } => Ok(format!("str:{value}")),
        NodeKind::Goto { target, .. } => Ok(format!("goto({})", shape(target)?)),
        NodeKind::MethodCall { object, method, args } => Ok(format!(
            "method({},{},{})",
            shape(object)?,
            method,
            args.iter().map(shape).collect::<Result<Vec<_>, _>>()?.join(",")
        )),
        NodeKind::HashLiteral { pairs } => Ok(format!(
            "hash({})",
            pairs
                .iter()
                .map(|(k, v)| Ok(format!("{}:{}", shape(k)?, shape(v)?)))
                .collect::<Result<Vec<_>, String>>()?
                .join(",")
        )),
        NodeKind::Assignment { op, lhs, rhs } => {
            Ok(format!("{op}({},{})", shape(lhs)?, shape(rhs)?))
        }
        NodeKind::FunctionCall { name, args } => Ok(format!(
            "{name}({})",
            args.iter().map(shape).collect::<Result<Vec<_>, _>>()?.join(",")
        )),
        NodeKind::ArrayLiteral { elements } => Ok(format!(
            "list({})",
            elements.iter().map(shape).collect::<Result<Vec<_>, _>>()?.join(",")
        )),
        _ => Err(format!("unexpected expression: {}", node.to_sexp())),
    }
}

#[test]
fn repetition_word_not_association() -> Result<(), String> {
    for (source, expected) in [
        ("$s x not $n + 1;", "x($s,not(+($n,1)))"),
        ("$s x (not $n) + 1;", "+(x($s,not($n)),1)"),
        ("not $s x $n;", "not(x($s,$n))"),
        ("$s x not not $n + 1;", "x($s,not(not(+($n,1))))"),
        ("$s x not ($n) + 1;", "+(x($s,not($n)),1)"),
        ("$s x not not ($n) + 1;", "x($s,not(+(not($n),1)))"),
        ("$s x not $n, $z;", "x($s,not(list($n,$z)))"),
        ("$s x not ($n), $z;", "list(x($s,not($n)),$z)"),
        ("$s x not $n = $z, $q;", "x($s,not(list(=($n,$z),$q)))"),
        ("$s x not();", "x($s,not(list()))"),
        ("$s x not($n)**2;", "x($s,**(not($n),2))"),
        ("$s x not($n)->foo;", "x($s,method(not($n),foo,))"),
        ("$s x not not($n)**2;", "x($s,not(**(not($n),2)))"),
        ("f($s x not $n, $z);", "f(x($s,not(list($n,$z))))"),
        ("f($s x not($n), $z);", "f(x($s,not($n)),$z)"),
        ("$s x not => $n;", "hash(x($s,id:not):$n)"),
        ("$s x not $n, not $z, $q;", "x($s,not(list($n,not(list($z,$q)))))"),
        ("$s x not $n = not($z) + 1;", "x($s,not(=($n,+(not($z),1))))"),
        ("$s x !$n + 1;", "+(x($s,!($n)),1)"),
        ("$s x not $n and $ok;", "and(x($s,not($n)),$ok)"),
        ("$s x not $n or $ok;", "or(x($s,not($n)),$ok)"),
        ("$s x not $n xor $ok;", "xor(x($s,not($n)),$ok)"),
        ("($s x 2) and not $n;", "and(x($s,2),not($n))"),
    ] {
        let ast = clean(source)?;
        let actual = shape(expression(&ast)?)?;
        if actual != expected {
            return Err(format!("{source:?}: expected {expected}, got {actual}"));
        }
    }
    Ok(())
}

#[test]
fn repetition_word_not_source_spans() -> Result<(), String> {
    let source = "$s x not $n + 1;";
    let ast = clean(source)?;
    let repeat = expression(&ast)?;
    let NodeKind::Binary { op, left, right } = &repeat.kind else {
        return Err("missing repetition".into());
    };
    let NodeKind::Unary { operand, .. } = &right.kind else {
        return Err("missing word not".into());
    };
    if op != "x" {
        return Err(format!("wrong operator: {op}"));
    }
    for (node, expected) in [
        (repeat, "$s x not $n + 1"),
        (left.as_ref(), "$s"),
        (right.as_ref(), "not $n + 1"),
        (operand.as_ref(), "$n + 1"),
    ] {
        if source.get(node.location.start..node.location.end) != Some(expected) {
            return Err(format!("expected span {expected:?}, got {:?}", node.location));
        }
    }
    Ok(())
}

#[test]
fn canonical_not_boundary_without_repetition() -> Result<(), String> {
    for (source, expected) in [
        ("not $n + 1;", "not(+($n,1))"),
        ("not($n) + 1;", "+(not($n),1)"),
        ("not $n, not $z, $q;", "not(list($n,not(list($z,$q))))"),
        ("$n = not($z) + 1;", "=($n,+(not($z),1))"),
        ("not $n and $ok;", "and(not($n),$ok)"),
        ("f(not $n, $z);", "f(not(list($n,$z)))"),
        ("f(not($n), $z);", "f(not($n),$z)"),
        ("not => $n;", "hash(str:not:$n)"),
        ("$s x not goto END, $n;", "x($s,not(list(goto(id:END),$n)))"),
        ("not goto END, $n;", "not(list(goto(id:END),$n))"),
    ] {
        let ast = clean(source)?;
        let actual = shape(expression(&ast)?)?;
        if actual != expected {
            return Err(format!("{source:?}: expected {expected}, got {actual}"));
        }
    }
    Ok(())
}

#[test]
fn word_not_rejects_direct_postfix_continuations() -> Result<(), String> {
    // FC-WORD-NOT-POSTFIX-OVERADMIT. Oracle: perl 5.38.2 `-c` rejects each of
    // these (`not($n)[0]` → syntax error near `)[`; `not($n)++` → "Can't
    // modify not in postincrement"), so none may parse clean.
    for source in
        ["not($n)[0];", "not($n)++;", "not($n){k};", "$s x not($n)[0];", "$s x not($n)++;"]
    {
        let output = Parser::new(source).parse_with_recovery();
        if output.diagnostics.is_empty() {
            return Err(format!(
                "admitted invalid postfix after word not: {source:?} {}",
                output.ast.to_sexp()
            ));
        }
    }
    // Arrow chains on a parenthesized `not` result stay valid (oracle: syntax OK).
    for source in ["not($n)->foo;", "not($n)->[0];", "$s x not($n)->foo;"] {
        clean(source)?;
    }
    Ok(())
}

fn find_word_not(node: &Node) -> Option<&Node> {
    if matches!(&node.kind, NodeKind::Unary { op, .. } if op == "not") {
        return Some(node);
    }
    node.children().into_iter().find_map(find_word_not)
}

#[test]
fn nested_bare_word_not_scales_past_fifty() -> Result<(), String> {
    // FC-WORD-NOT-NEST-DEPTH. Perl accepts 50 nested bare `not` operators, so
    // the parser must too; the 128-frame budget still trips at 130 prefixes
    // (pinned by `word_not_depth_130_hits_limit` in guard_bypass_regression).
    let source = format!("{}1;", "not ".repeat(50));
    let ast = clean(&source)?;
    let mut depth = 0;
    let mut node = expression(&ast)?;
    while let NodeKind::Unary { operand, .. } = &node.kind {
        depth += 1;
        node = operand;
    }
    if depth != 50 {
        return Err(format!("expected 50 nested nots, found {depth}: {}", ast.to_sexp()));
    }
    if !matches!(&node.kind, NodeKind::Number { value } if value == "1") {
        return Err(format!("lost innermost operand: {}", ast.to_sexp()));
    }
    Ok(())
}

#[test]
fn parenthesized_word_not_span_includes_closer() -> Result<(), String> {
    // FC-WORD-NOT-SPAN-CLOSER. The simple-group path consumes `)` but returns
    // the inner location; the `not` span must still cover the closer.
    for (source, expected) in [
        ("$s x not($n) + 1;", "not($n)"),
        ("not(($n));", "not(($n))"),
        ("$s x not ($n) + 1;", "not ($n)"),
    ] {
        let ast = clean(source)?;
        let not = find_word_not(&ast).ok_or_else(|| format!("lost word not: {}", ast.to_sexp()))?;
        if source.get(not.location.start..not.location.end) != Some(expected) {
            return Err(format!(
                "wrong word-not span for {source:?}: expected {expected:?}, got {:?}",
                not.location
            ));
        }
    }
    Ok(())
}

fn has_repetition(node: &Node) -> bool {
    matches!(&node.kind, NodeKind::Binary { op, .. } if op == "x")
        || node.children().into_iter().any(has_repetition)
}

#[test]
fn x_name_contexts_remain_non_repetition() -> Result<(), String> {
    for source in
        ["x(not $n);", "Pkg::x(not $n);", "$obj->x(not $n);", "my %h = (x => not $n);", "$h{x};"]
    {
        let ast = clean(source)?;
        if has_repetition(&ast) {
            return Err(format!("invented repetition: {source}"));
        }
    }
    Ok(())
}

#[test]
fn malformed_not_rhs_recovers_following_statement() -> Result<(), String> {
    let output = Parser::new("$s x not ; $after = 1;").parse_with_recovery();
    if !output.diagnostics.iter().any(|error| {
        matches!(
            error,
            ParseError::Recovered { kind: RecoveryKind::MissingOperand, location: 5, .. }
        )
    }) {
        return Err(format!("missing malformed-not diagnostic: {:?}", output.diagnostics));
    }
    let NodeKind::Program { statements } = &output.ast.kind else {
        return Err("missing program".into());
    };
    if statements.len() != 2 {
        return Err(format!("lost recovery boundary: {}", output.ast.to_sexp()));
    }
    for (statement, expected) in statements.iter().zip(["x($s,not(missing))", "=($after,1)"]) {
        let actual = shape(expression(statement)?)?;
        if actual != expected {
            return Err(format!("expected {expected}, got {actual}"));
        }
    }
    Ok(())
}
