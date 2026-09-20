//! Exact production-path proof for contextual `x=` on declaration expressions.

mod cpan_test_helpers;

use cpan_test_helpers::{assert_clean_parse, parse};
use perl_parser_core::syntax::error::{ParseError, RecoveryKind, RecoverySite};
use perl_parser_core::{Node, NodeKind, Parser};

fn find_assignment<'a>(node: &'a Node, expected_op: &str) -> Option<&'a Node> {
    if matches!(&node.kind, NodeKind::Assignment { op, .. } if op == expected_op) {
        return Some(node);
    }

    node.children().into_iter().find_map(|child| find_assignment(child, expected_op))
}

fn find_binary<'a>(node: &'a Node, expected_op: &str) -> Option<&'a Node> {
    if matches!(&node.kind, NodeKind::Binary { op, .. } if op == expected_op) {
        return Some(node);
    }

    node.children().into_iter().find_map(|child| find_binary(child, expected_op))
}

fn find_variable_declaration(node: &Node) -> Option<&Node> {
    // Both declaration topologies count: a parenthesized list (`my ($x, $y)`)
    // parses as `VariableListDeclaration`, which carries the same declared
    // names as its singular sibling.
    if matches!(
        node.kind,
        NodeKind::VariableDeclaration { .. } | NodeKind::VariableListDeclaration { .. }
    ) {
        return Some(node);
    }

    node.children().into_iter().find_map(find_variable_declaration)
}

fn find_named_call<'a>(node: &'a Node, expected_name: &str) -> Option<&'a Node> {
    if matches!(&node.kind, NodeKind::FunctionCall { name, .. } if name == expected_name) {
        return Some(node);
    }

    node.children().into_iter().find_map(|child| find_named_call(child, expected_name))
}

fn find_missing_expression(node: &Node) -> Option<&Node> {
    if matches!(node.kind, NodeKind::MissingExpression) {
        return Some(node);
    }

    node.children().into_iter().find_map(find_missing_expression)
}

fn find_variable<'a>(node: &'a Node, expected_name: &str) -> Option<&'a Node> {
    if matches!(&node.kind, NodeKind::Variable { name, .. } if name == expected_name) {
        return Some(node);
    }

    node.children().into_iter().find_map(|child| find_variable(child, expected_name))
}

fn find_comma_array<'a>(node: &'a Node, first: &Node) -> Option<&'a Node> {
    if let NodeKind::ArrayLiteral { elements } = &node.kind
        && elements.len() == 2
        && elements[0].location == first.location
    {
        return Some(node);
    }

    node.children().into_iter().find_map(|child| find_comma_array(child, first))
}

fn source_slice<'a>(source: &'a str, node: &Node) -> Result<&'a str, String> {
    source
        .get(node.location.start..node.location.end)
        .ok_or_else(|| format!("invalid node span {:?} for {source:?}", node.location))
}

#[test]
fn list_declaration_accepts_repetition_assignment() -> Result<(), String> {
    let source = "my ($x, $y) x= 3;";
    assert_clean_parse(source);
    let ast = parse(source);
    let assignment = find_assignment(&ast, "x=")
        .ok_or_else(|| format!("expected declaration x= assignment:\n{}", ast.to_sexp()))?;
    let NodeKind::Assignment { lhs, rhs, .. } = &assignment.kind else {
        return Err(format!("expected Assignment, got {:?}", assignment.kind));
    };

    if find_variable_declaration(lhs).is_none() {
        return Err(format!("x= lhs lost declaration topology: {:?}", lhs.kind));
    }
    if source_slice(source, lhs)? != "my ($x, $y)" {
        return Err(format!("unexpected declaration lhs span: {:?}", lhs.location));
    }
    if !matches!(&rhs.kind, NodeKind::Number { value } if value == "3") {
        return Err(format!("expected numeric x= rhs, got {:?}", rhs.kind));
    }
    if source_slice(source, assignment)? != "my ($x, $y) x= 3" {
        return Err(format!("unexpected full assignment span: {:?}", assignment.location));
    }
    // A trailing comma still belongs to the surrounding comma expression,
    // not to the repetition RHS: the assignment spans exactly
    // `my ($x, $y) x= 3` with a numeric RHS, sits first in the two-element
    // comma array, and `$z` survives after it. A finisher that parsed the
    // RHS at comma precedence would swallow `, $z` and fail the span pin.
    let trailing = "my ($x, $y) x= 3, $z;";
    assert_clean_parse(trailing);
    let trailing_ast = parse(trailing);
    let trailing_assignment = find_assignment(&trailing_ast, "x=").ok_or_else(|| {
        format!("trailing comma lost declaration x=:\n{}", trailing_ast.to_sexp())
    })?;
    if source_slice(trailing, trailing_assignment)? != "my ($x, $y) x= 3" {
        return Err(format!("comma leaked into the repetition RHS:\n{}", trailing_ast.to_sexp()));
    }
    let NodeKind::Assignment { rhs, .. } = &trailing_assignment.kind else {
        return Err(format!("expected Assignment, got {:?}", trailing_assignment.kind));
    };
    if !matches!(&rhs.kind, NodeKind::Number { value } if value == "3") {
        return Err(format!("expected numeric repetition rhs, got {:?}", rhs.kind));
    }
    let comma = find_comma_array(&trailing_ast, trailing_assignment).ok_or_else(|| {
        format!("expected the two-element comma array:\n{}", trailing_ast.to_sexp())
    })?;
    let NodeKind::ArrayLiteral { elements } = &comma.kind else {
        return Err(format!("expected ArrayLiteral, got {:?}", comma.kind));
    };
    let z = find_variable(&trailing_ast, "z")
        .ok_or_else(|| format!("trailing comma lost $z:\n{}", trailing_ast.to_sexp()))?;
    if !matches!(&elements[1].kind, NodeKind::Variable { name, .. } if name == "z") {
        return Err(format!("expected $z second in the comma array:\n{}", trailing_ast.to_sexp()));
    }
    if z.location.start < trailing_assignment.location.end {
        return Err(format!("$z overlaps the repetition assignment:\n{}", trailing_ast.to_sexp()));
    }
    Ok(())
}

#[test]
fn call_argument_declaration_accepts_repetition_assignment() -> Result<(), String> {
    let source = "f(my $v x= 3);";
    assert_clean_parse(source);
    let ast = parse(source);
    let call =
        find_named_call(&ast, "f").ok_or_else(|| format!("expected f call:\n{}", ast.to_sexp()))?;
    let assignment = find_assignment(call, "x=")
        .ok_or_else(|| format!("expected x= inside f argument:\n{}", ast.to_sexp()))?;
    let NodeKind::Assignment { lhs, rhs, .. } = &assignment.kind else {
        return Err(format!("expected Assignment, got {:?}", assignment.kind));
    };

    if find_variable_declaration(lhs).is_none() {
        return Err(format!("call-argument x= lhs lost declaration: {:?}", lhs.kind));
    }
    if source_slice(source, lhs)? != "my $v" {
        return Err(format!("unexpected call declaration lhs span: {:?}", lhs.location));
    }
    if !matches!(&rhs.kind, NodeKind::Number { value } if value == "3") {
        return Err(format!("expected numeric call-argument rhs, got {:?}", rhs.kind));
    }
    if source_slice(source, assignment)? != "my $v x= 3" {
        return Err(format!("unexpected call-argument assignment span: {:?}", assignment.location));
    }
    Ok(())
}

#[test]
fn declaration_repetition_assignment_keeps_rhs_right_associative() -> Result<(), String> {
    let source = "f(my $v x= $count = 2);";
    assert_clean_parse(source);
    let ast = parse(source);
    let call =
        find_named_call(&ast, "f").ok_or_else(|| format!("expected f call:\n{}", ast.to_sexp()))?;
    let assignment = find_assignment(call, "x=")
        .ok_or_else(|| format!("expected outer x= assignment:\n{}", ast.to_sexp()))?;
    let NodeKind::Assignment { rhs, .. } = &assignment.kind else {
        return Err(format!("expected Assignment, got {:?}", assignment.kind));
    };
    if !matches!(&rhs.kind, NodeKind::Assignment { op, .. } if op == "=") {
        return Err(format!("expected ordinary assignment on x= rhs, got {:?}", rhs.kind));
    }
    Ok(())
}

#[test]
fn call_argument_list_preserves_surrounding_arguments() -> Result<(), String> {
    // Neighboring arguments survive around the declaration assignment: three
    // args in order, the middle one the exact `my $v x= 3` assignment.
    let source = "f($a, my $v x= 3, $b);";
    assert_clean_parse(source);
    let ast = parse(source);
    let call =
        find_named_call(&ast, "f").ok_or_else(|| format!("expected f call:\n{}", ast.to_sexp()))?;
    let NodeKind::FunctionCall { args, .. } = &call.kind else {
        return Err(format!("expected FunctionCall, got {:?}", call.kind));
    };
    if args.len() != 3 {
        return Err(format!("expected three call arguments:\n{}", ast.to_sexp()));
    }
    if !matches!(&args[0].kind, NodeKind::Variable { name, .. } if name == "a") {
        return Err(format!("expected $a first:\n{}", ast.to_sexp()));
    }
    if !matches!(&args[2].kind, NodeKind::Variable { name, .. } if name == "b") {
        return Err(format!("expected $b last:\n{}", ast.to_sexp()));
    }
    let NodeKind::Assignment { lhs, rhs, op } = &args[1].kind else {
        return Err(format!("expected middle x= assignment, got {:?}", args[1].kind));
    };
    if op != "x=" {
        return Err(format!("expected x= operator, got {op}"));
    }
    if find_variable_declaration(lhs).is_none() {
        return Err(format!("middle argument x= lhs lost declaration: {:?}", lhs.kind));
    }
    if source_slice(source, &args[1])? != "my $v x= 3" {
        return Err(format!("unexpected middle argument span: {:?}", args[1].location));
    }
    if !matches!(&rhs.kind, NodeKind::Number { value } if value == "3") {
        return Err(format!("expected numeric middle rhs, got {:?}", rhs.kind));
    }
    Ok(())
}

#[test]
fn ordinary_declaration_and_repetition_controls_remain_distinct() -> Result<(), String> {
    for source in ["f(my $v = 3);", "my ($c, $d) = (1, 2);"] {
        assert_clean_parse(source);
        let ast = parse(source);
        if find_assignment(&ast, "x=").is_some() {
            return Err(format!("ordinary declaration assignment became x=:\n{}", ast.to_sexp()));
        }
        // Ordinary `=` keeps its established declaration-with-initializer
        // shape: call-argument and list declarations carry the RHS as an
        // initializer, never as an `=` assignment node, so assert that shape
        // directly instead of searching for an assignment that cannot exist.
        let initialized = find_variable_declaration(&ast).is_some_and(|node| match &node.kind {
            NodeKind::VariableDeclaration { initializer, .. }
            | NodeKind::VariableListDeclaration { initializer, .. } => initializer.is_some(),
            _ => false,
        });
        if !initialized {
            return Err(format!("expected ordinary declaration initializer:\n{}", ast.to_sexp()));
        }
    }

    // A bare `x` after a statement-level list declaration has no binary
    // continuation over declarations in this parser (the statement layer only
    // continues declarations on `,`/`=>` or word operators), so it must simply
    // keep parsing without normalizing into `x=`. That architecture predates
    // this claim and is unchanged by it.
    let source = "my ($a, $b) x 3;";
    assert_clean_parse(source);
    let ast = parse(source);
    if find_assignment(&ast, "x=").is_some() {
        return Err(format!("ordinary binary x became x=:\n{}", ast.to_sexp()));
    }
    if find_variable_declaration(&ast).is_none() {
        return Err(format!("ordinary x control lost declaration:\n{}", ast.to_sexp()));
    }
    // Inside call arguments the declaration flows through the binary chain,
    // so a bare `x` stays an ordinary repetition operator there.
    let source = "f(my $w x 3);";
    assert_clean_parse(source);
    let ast = parse(source);
    if find_assignment(&ast, "x=").is_some() {
        return Err(format!("ordinary binary x became x=:\n{}", ast.to_sexp()));
    }
    if find_binary(&ast, "x").is_none() {
        return Err(format!("expected ordinary binary x:\n{}", ast.to_sexp()));
    }
    if find_variable_declaration(&ast).is_none() {
        return Err(format!("ordinary x control lost declaration:\n{}", ast.to_sexp()));
    }
    Ok(())
}

#[test]
fn trivia_separated_declaration_x_equals_is_never_normalized() -> Result<(), String> {
    // Statement level: the leftover `x = 3` parses as an ordinary second
    // statement assignment with no diagnostics (same-line leftover
    // enforcement belongs to statement termination, not the operator). Pin
    // that exact shape so the test cannot pass vacuously on some future
    // unrelated acceptance.
    let source = "my ($x, $y) x = 3;";
    let output = Parser::new(source).parse_with_recovery();
    if find_assignment(&output.ast, "x=").is_some() {
        return Err(format!(
            "trivia-separated declaration x = became x=:\n{}",
            output.ast.to_sexp()
        ));
    }
    let NodeKind::Program { statements, .. } = &output.ast.kind else {
        return Err(format!("expected program root, got {:?}", output.ast.kind));
    };
    if statements.len() != 2 {
        return Err(format!(
            "expected the leftover `x = 3` to parse as a second statement, got {}",
            output.ast.to_sexp()
        ));
    }
    if find_assignment(&output.ast, "=").is_none() {
        return Err(format!(
            "expected the leftover to stay an ordinary assignment:\n{}",
            output.ast.to_sexp()
        ));
    }
    if !output.diagnostics.is_empty() {
        return Err(format!(
            "expected no diagnostics for the clean split, got {:?}",
            output.diagnostics
        ));
    }
    // Call arguments: a split `x` / `=` cannot form the operator, so the
    // argument errors at the `)` boundary with exactly two boundary
    // diagnostics. Pin both so the proof cannot pass on a silent drop.
    for source in ["f(my $v x\n= 3);", "f(my $v x # gap\n= 3);"] {
        let output = Parser::new(source).parse_with_recovery();
        if find_assignment(&output.ast, "x=").is_some() {
            return Err(format!(
                "trivia-separated declaration x = became x=:\n{}",
                output.ast.to_sexp()
            ));
        }
        if !matches!(
            output.diagnostics.as_slice(),
            [
                ParseError::UnexpectedToken { expected, found, location: 8 },
                ParseError::UnexpectedToken { expected: expected2, found: found2, location: 8 },
            ] if expected == "')'" && found == "identifier"
                && expected2 == "statement" && found2 == "identifier"
        ) {
            return Err(format!(
                "expected exactly the two `)`-boundary diagnostics at 8, got {:?}",
                output.diagnostics
            ));
        }
    }
    Ok(())
}

#[test]
fn broken_foreach_header_does_not_poison_later_repetition() -> Result<(), String> {
    // A broken iterator header must not poison the iterator-context flag:
    // the header error is diagnosed, the flag is restored, and a later
    // valid list declaration still grows its `x=` tail with the follower
    // statement surviving. Both `foreach` and `for` spellings route through
    // the guarded sites.
    for (source, error_at) in [
        ("foreach my ($x $y); my ($a, $b) x= 3; $w = 1;", 15),
        ("for my ($x $y); my ($a, $b) x= 3; $w = 1;", 11),
    ] {
        let output = Parser::new(source).parse_with_recovery();
        if !matches!(
            output.diagnostics.as_slice(),
            [
                ParseError::SyntaxError { message, location },
                ParseError::UnexpectedToken { expected, found, location: location2 },
            ] if message == "Expected comma or closing parenthesis in variable list"
                && *location == error_at
                && expected == "statement"
                && found == "identifier"
                && *location2 == error_at
        ) {
            return Err(format!(
                "expected the bad-header diagnostics at {error_at}, got {:?}",
                output.diagnostics
            ));
        }
        let assignment = find_assignment(&output.ast, "x=").ok_or_else(|| {
            format!("poisoned flag dropped the later x= tail:\n{}", output.ast.to_sexp())
        })?;
        let NodeKind::Assignment { lhs, rhs, .. } = &assignment.kind else {
            return Err(format!("expected Assignment, got {:?}", assignment.kind));
        };
        if find_variable_declaration(lhs).is_none() {
            return Err(format!("later x= lhs lost declaration topology: {:?}", lhs.kind));
        }
        if !matches!(&rhs.kind, NodeKind::Number { value } if value == "3") {
            return Err(format!("expected numeric later x= rhs, got {:?}", rhs.kind));
        }
        if find_assignment(&output.ast, "=").is_none() {
            return Err(format!("follower statement did not survive:\n{}", output.ast.to_sexp()));
        }
    }
    Ok(())
}

#[test]
fn loop_headers_reject_declaration_repetition_tail() -> Result<(), String> {
    // `foreach` iterator targets are not assignment expressions: they must
    // never grow an `x=` tail even though they parse through the same
    // list-declaration branch. Pin the exact recovery diagnostics so the
    // rejection cannot pass silently.
    let source = "foreach my ($x, $y) x= 3 (@items) {}";
    let output = Parser::new(source).parse_with_recovery();
    if find_assignment(&output.ast, "x=").is_some() {
        return Err(format!("foreach iterator target grew an x= tail:\n{}", output.ast.to_sexp()));
    }
    if !matches!(
        output.diagnostics.as_slice(),
        [
            ParseError::UnexpectedToken { expected, found, location: 20 },
            ParseError::UnexpectedToken { expected: expected2, found: found2, location: 21 },
        ] if expected == "'('" && found == "identifier"
            && expected2 == "statement" && found2 == "'='"
    ) {
        return Err(format!(
            "expected exactly the two iterator-boundary diagnostics, got {:?}",
            output.diagnostics
        ));
    }
    // C-style `for` initializers are assignment-capable, so the tail fires
    // there with the same declaration-over-assignment shape as statements.
    let source = "for (my ($x, $y) x= 3; $c; $d) {}";
    assert_clean_parse(source);
    let ast = parse(source);
    let assignment = find_assignment(&ast, "x=")
        .ok_or_else(|| format!("expected C-style init x= assignment:\n{}", ast.to_sexp()))?;
    let NodeKind::Assignment { lhs, rhs, .. } = &assignment.kind else {
        return Err(format!("expected Assignment, got {:?}", assignment.kind));
    };
    if find_variable_declaration(lhs).is_none() {
        return Err(format!("C-style x= lhs lost declaration topology: {:?}", lhs.kind));
    }
    if source_slice(source, lhs)? != "my ($x, $y)" {
        return Err(format!("unexpected C-style declaration lhs span: {:?}", lhs.location));
    }
    if !matches!(&rhs.kind, NodeKind::Number { value } if value == "3") {
        return Err(format!("expected numeric C-style x= rhs, got {:?}", rhs.kind));
    }
    Ok(())
}

#[test]
fn call_argument_declaration_recovers_missing_repetition_rhs() -> Result<(), String> {
    let source = "f(my $v x=);";
    let output = Parser::new(source).parse_with_recovery();
    let call = find_named_call(&output.ast, "f")
        .ok_or_else(|| format!("expected recovered f call:\n{}", output.ast.to_sexp()))?;
    let assignment = find_assignment(call, "x=")
        .ok_or_else(|| format!("expected recovered x= argument:\n{}", output.ast.to_sexp()))?;
    let missing = find_missing_expression(assignment)
        .ok_or_else(|| format!("expected missing rhs:\n{}", output.ast.to_sexp()))?;
    let operator_start = source.find("x=").ok_or("expected x= operator")?;

    if missing.location.start != operator_start || missing.location.end != operator_start {
        return Err(format!("unexpected missing-rhs span: {:?}", missing.location));
    }
    if !matches!(
        output.diagnostics.as_slice(),
        [ParseError::Recovered {
            site: RecoverySite::InfixRhs,
            kind: RecoveryKind::MissingOperand,
            location,
        }] if *location == operator_start
    ) {
        return Err(format!(
            "expected one exact missing-rhs recovery at {operator_start}, got {:?}",
            output.diagnostics
        ));
    }
    Ok(())
}
