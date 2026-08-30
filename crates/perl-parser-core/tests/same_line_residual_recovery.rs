//! Regression proof that invalid same-line residue cannot masquerade as a clean parse.

mod cpan_test_helpers;

use cpan_test_helpers::{assert_clean_parse, assert_no_blocking_diagnostics};
use perl_parser_core::error::{ParseError, RecoveryKind, RecoverySite};
use perl_parser_core::{Node, NodeKind, Parser};
use std::process::Command;

fn assert_valid_case(source: &str) {
    assert_clean_parse(source);
    assert_no_blocking_diagnostics(source);
}

fn has_recovery_node(node: &Node) -> bool {
    if matches!(node.kind, NodeKind::Error { .. } | NodeKind::MissingExpression) {
        return true;
    }

    node.children().into_iter().any(has_recovery_node)
}

fn has_unrecovered_blocking_diagnostic(diagnostics: &[ParseError]) -> bool {
    diagnostics
        .iter()
        .any(|error| error.blocks_clean_parse() && !is_benign_bare_goto_recovery(error))
}

fn is_benign_bare_goto_recovery(error: &ParseError) -> bool {
    matches!(
        error,
        ParseError::Recovered {
            site: RecoverySite::InfixRhs,
            kind: RecoveryKind::MissingOperand,
            ..
        }
    )
}

#[test]
fn unexpected_recovered_blocking_diagnostic_is_not_silenced() -> Result<(), String> {
    let benign = ParseError::Recovered {
        site: RecoverySite::InfixRhs,
        kind: RecoveryKind::MissingOperand,
        location: 0,
    };
    let unexpected = ParseError::Recovered {
        site: RecoverySite::ArgList,
        kind: RecoveryKind::InsertedCloser,
        location: 0,
    };

    if has_unrecovered_blocking_diagnostic(&[benign]) {
        return Err("the intended bare-goto recovery was treated as unexpected".to_string());
    }
    if !has_unrecovered_blocking_diagnostic(&[unexpected]) {
        return Err("an unexpected recovered blocking diagnostic was silenced".to_string());
    }
    Ok(())
}

fn find_assignment<'a>(node: &'a Node, expected_op: &str) -> Option<&'a Node> {
    if matches!(&node.kind, NodeKind::Assignment { op, .. } if op == expected_op) {
        return Some(node);
    }

    node.children().into_iter().find_map(|child| find_assignment(child, expected_op))
}

fn contains_goto(node: &Node) -> bool {
    matches!(node.kind, NodeKind::Goto { .. }) || node.children().into_iter().any(contains_goto)
}

fn first_expression(ast: &Node) -> Option<&Node> {
    let NodeKind::Program { statements } = &ast.kind else {
        return None;
    };
    let statement = statements.first()?;
    let NodeKind::ExpressionStatement { expression } = &statement.kind else {
        return None;
    };
    Some(expression)
}

fn first_expression_has_direct_word_operator_goto_rhs(ast: &Node, expected_op: &str) -> bool {
    matches!(
        first_expression(ast),
        Some(expression)
            if matches!(&expression.kind, NodeKind::Binary { op, right, .. }
                if op == expected_op && matches!(right.kind, NodeKind::Goto { .. }))
    )
}

fn first_expression_has_direct_word_operator_unary_goto_rhs(
    ast: &Node,
    expected_op: &str,
    unary_op: &str,
) -> bool {
    matches!(
        first_expression(ast),
        Some(expression)
            if matches!(&expression.kind, NodeKind::Binary { op, right, .. }
                if op == expected_op
                    && matches!(&right.kind, NodeKind::Unary { op, operand }
                        if op == unary_op && matches!(operand.kind, NodeKind::Goto { .. })))
    )
}

fn is_postfix_goto(node: &Node) -> bool {
    matches!(
        &node.kind,
        NodeKind::MethodCall { object, method, args }
            if method == "foo"
                && args.is_empty()
                && matches!(&object.kind, NodeKind::Identifier { name } if name == "goto")
    )
}

fn or_rhs_is_unary_postfix_goto(node: &Node, expected_op: &str) -> bool {
    matches!(
        &node.kind,
        NodeKind::Binary { op, right, .. }
            if op == "or"
                && matches!(
                    &right.kind,
                    NodeKind::Unary { op, operand }
                        if op == expected_op && is_postfix_goto(operand)
                )
    )
}

fn assert_non_clean(source: &str) -> Result<(), String> {
    let output = Parser::new(source).parse_with_recovery();
    if output.diagnostics.is_empty() && !has_recovery_node(&output.ast) {
        return Err(format!(
            "invalid Perl returned a clean native parse:\nsource={source:?}\nast={}\ndiagnostics={:?}",
            output.ast.to_sexp(),
            output.diagnostics
        ));
    }
    Ok(())
}

fn assert_same_line_residual_at(source: &str, token: &str) -> Result<(), String> {
    let output = Parser::new(source).parse_with_recovery();
    let expected = source
        .find(token)
        .ok_or_else(|| format!("test token {token:?} is absent from {source:?}"))?;
    let locations: Vec<usize> = output
        .diagnostics
        .iter()
        .filter_map(|error| {
            matches!(
                error,
                ParseError::Recovered {
                    site: RecoverySite::Statement,
                    kind: RecoveryKind::UnexpectedSameLineResidue,
                    ..
                }
            )
            .then_some(error)
            .and_then(|error| match error {
                ParseError::Recovered { location, .. } => Some(*location),
                _ => None,
            })
        })
        .collect();

    if locations != [expected] {
        return Err(format!(
            "same-line residual must identify exactly the first unconsumed token:\nsource={source:?}\nexpected={expected}\nlocations={locations:?}\ndiagnostics={:?}",
            output.diagnostics
        ));
    }
    Ok(())
}

fn assert_no_same_line_residual(source: &str) -> Result<(), String> {
    let output = Parser::new(source).parse_with_recovery();
    if output.diagnostics.iter().any(|error| {
        matches!(
            error,
            ParseError::Recovered {
                site: RecoverySite::Statement,
                kind: RecoveryKind::UnexpectedSameLineResidue,
                ..
            }
        )
    }) {
        return Err(format!(
            "guarded parser boundary must not be mislabeled as same-line residue:\nsource={source:?}\nast={}\ndiagnostics={:?}",
            output.ast.to_sexp(),
            output.diagnostics
        ));
    }
    Ok(())
}

fn perl_compile_accepts(source: &str) -> Result<bool, String> {
    let path = std::env::var("PATH").unwrap_or_default();
    let output = Command::new("perl")
        .args(["-c", "-e", source])
        .env_clear()
        .env("PATH", path)
        .env("LC_ALL", "C")
        .env("LANG", "C")
        .env_remove("PERL5LIB")
        .env_remove("PERL5OPT")
        .env_remove("PERL_LOCAL_LIB_ROOT")
        .env_remove("PERL_LOCAL_LIB_PREFIX")
        .output()
        .map_err(|error| format!("real-Perl oracle unavailable: {error}"))?;
    Ok(output.status.success())
}

#[test]
fn invalid_same_line_residue_is_not_clean() -> Result<(), String> {
    for (source, token) in [
        ("use strict; my $x = 1 print \"hi\";", "print"),
        ("use strict; my $x = 1; 1 2;", "2"),
        ("$value x = 3;", "x ="),
    ] {
        assert_non_clean(source)?;
        assert_same_line_residual_at(source, token)?;
    }
    Ok(())
}

#[test]
fn command_line_ne_wrapper_is_not_same_line_residue() -> Result<(), String> {
    let source = "-ne print;";
    assert_valid_case(source);
    assert_no_same_line_residual(source)
}

#[test]
fn command_line_ne_wrapper_does_not_hide_following_residue() -> Result<(), String> {
    for (source, token) in [("-ne print 1 2;", "2"), ("-ne foo 1;", "foo")] {
        assert_same_line_residual_at(source, token)?;
    }
    Ok(())
}

#[test]
fn spaced_repetition_tokens_are_not_rewritten_to_x_assign() -> Result<(), String> {
    let source = "$value x = 3;";
    let output = Parser::new(source).parse_with_recovery();
    if find_assignment(&output.ast, "x=").is_some() {
        return Err(format!("spaced x = must not be normalized to x=:\n{}", output.ast.to_sexp()));
    }
    if output.diagnostics.is_empty() && !has_recovery_node(&output.ast) {
        return Err(format!(
            "spaced x = must expose its invalid residual token:\n{}",
            output.ast.to_sexp()
        ));
    }
    Ok(())
}

#[test]
fn valid_same_line_statement_boundaries_remain_clean() -> Result<(), String> {
    for source in [
        "use strict; my $x = 1; print \"hi\";",
        "copy($from, $to) or goto fail;",
        "my $x = 1; $x += 2;",
        "foo($x, $y); bar($z);",
    ] {
        assert_valid_case(source);
    }
    Ok(())
}

#[test]
fn cross_line_or_goto_stays_one_control_flow_expression() -> Result<(), String> {
    for (operator, target) in [("or", "fail_or"), ("and", "fail_and"), ("xor", "fail_xor")] {
        let source = format!("copy($from, $to)\n    {operator} goto {target};\nprint \"ok\";\n");
        let output = Parser::new(&source).parse_with_recovery();
        let statements = match &output.ast.kind {
            NodeKind::Program { statements } => statements,
            kind => return Err(format!("expected a program, got {}", kind.kind_name())),
        };

        if output.diagnostics.iter().any(ParseError::blocks_clean_parse) {
            return Err(format!(
                "valid {operator} continuation became blocking: {:?}\nast={}",
                output.diagnostics,
                output.ast.to_sexp()
            ));
        }
        if statements.len() != 2
            || !first_expression_has_direct_word_operator_goto_rhs(&output.ast, operator)
        {
            return Err(format!(
                "{operator}/goto must remain one binary control-flow expression with a later print statement:\n{}",
                output.ast.to_sexp()
            ));
        }
    }
    Ok(())
}

#[test]
fn same_line_word_operator_goto_variants_keep_their_control_flow_rhs() -> Result<(), String> {
    for (operator, target) in [
        ("or", "fail_or"),
        ("and", "fail_and"),
        ("xor", "fail_xor"),
        ("or", "&fail_sub"),
        ("or", "$dynamic_target"),
    ] {
        let source = format!("copy($from, $to) {operator} goto {target}; print \"ok\";");
        let output = Parser::new(&source).parse_with_recovery();
        let statements = match &output.ast.kind {
            NodeKind::Program { statements } => statements,
            kind => return Err(format!("expected a program, got {}", kind.kind_name())),
        };

        if output.diagnostics.iter().any(ParseError::blocks_clean_parse) {
            return Err(format!(
                "valid same-line {operator} goto {target} became blocking: {:?}\nast={}",
                output.diagnostics,
                output.ast.to_sexp()
            ));
        }
        if statements.len() != 2
            || !first_expression_has_direct_word_operator_goto_rhs(&output.ast, operator)
        {
            return Err(format!(
                "same-line {operator} goto {target} must keep its Goto RHS and later print statement:\n{}",
                output.ast.to_sexp()
            ));
        }
    }
    Ok(())
}

#[test]
fn bare_and_unary_word_goto_forms_match_perl_boundaries() -> Result<(), String> {
    for source in ["foo or goto;", "foo and goto;", "foo xor goto;"] {
        if !perl_compile_accepts(source)? {
            return Err(format!("real Perl rejected valid bare goto form: {source:?}"));
        }
        let output = Parser::new(source).parse_with_recovery();
        if has_unrecovered_blocking_diagnostic(&output.diagnostics) {
            return Err(format!(
                "bare goto form became blocking: source={source:?}, diagnostics={:?}",
                output.diagnostics
            ));
        }
    }

    for (source, operator, unary_op) in [
        ("foo or not goto fail;", "or", "not"),
        ("foo or !goto fail;", "or", "!"),
        ("foo or +goto fail;", "or", "+"),
    ] {
        if !perl_compile_accepts(source)? {
            return Err(format!("real Perl rejected valid unary goto form: {source:?}"));
        }
        let output = Parser::new(source).parse_with_recovery();
        if output.diagnostics.iter().any(ParseError::blocks_clean_parse)
            || !first_expression_has_direct_word_operator_unary_goto_rhs(
                &output.ast,
                operator,
                unary_op,
            )
        {
            return Err(format!(
                "unary goto must remain one clean word-operator control-flow expression:\nsource={source:?}\ndiagnostics={:?}\nast={}",
                output.diagnostics,
                output.ast.to_sexp()
            ));
        }
    }

    for operator in ["or", "and", "xor"] {
        let source = format!("foo {operator} -goto fail; print \"ok\";");
        if !perl_compile_accepts(&source)? {
            return Err(format!("real Perl rejected valid unary-minus goto form: {source:?}"));
        }
        let output = Parser::new(&source).parse_with_recovery();
        if output.diagnostics.iter().any(ParseError::blocks_clean_parse)
            || !first_expression_has_direct_word_operator_unary_goto_rhs(&output.ast, operator, "-")
        {
            return Err(format!(
                "unary-minus goto must remain one clean word-operator control-flow expression:\nsource={source:?}\ndiagnostics={:?}\nast={}",
                output.diagnostics,
                output.ast.to_sexp()
            ));
        }
    }
    Ok(())
}

#[test]
fn bare_word_goto_forms_preserve_control_flow_rhs() -> Result<(), String> {
    for (source, operator) in
        [("foo or goto;", "or"), ("foo and goto;", "and"), ("foo xor goto;", "xor")]
    {
        if !perl_compile_accepts(source)? {
            return Err(format!("real Perl rejected valid bare goto form: {source:?}"));
        }
        let output = Parser::new(source).parse_with_recovery();
        let has_missing_target_recovery = output.diagnostics.iter().any(|error| {
            matches!(
                error,
                ParseError::Recovered {
                    site: RecoverySite::InfixRhs,
                    kind: RecoveryKind::MissingOperand,
                    ..
                }
            )
        });
        let missing_target_is_word_operator_rhs = match &output.ast.kind {
            NodeKind::Program { statements } => matches!(
                statements.first().map(|statement| &statement.kind),
                Some(NodeKind::ExpressionStatement { expression })
                    if matches!(
                        &expression.kind,
                        NodeKind::Binary { op, right, .. }
                            if op == operator
                                && matches!(
                                    &right.kind,
                                    NodeKind::Goto { target, .. }
                                        if matches!(target.kind, NodeKind::MissingExpression)
                                )
                    )
            ),
            _ => false,
        };
        if has_unrecovered_blocking_diagnostic(&output.diagnostics)
            || !has_missing_target_recovery
            || !missing_target_is_word_operator_rhs
        {
            return Err(format!(
                "bare {operator} goto must recover a MissingExpression Goto RHS with an InfixRhs/MissingOperand marker:\nsource={source:?}\ndiagnostics={:?}\nast={}",
                output.diagnostics,
                output.ast.to_sexp()
            ));
        }
    }
    Ok(())
}

#[test]
fn unary_goto_postfix_arrow_forms_match_perl_boundaries() -> Result<(), String> {
    for (source, unary_op) in [
        ("foo or +goto->foo; print \"ok\";", "+"),
        ("foo or !goto->foo; print \"ok\";", "!"),
        ("foo or not goto->foo; print \"ok\";", "not"),
        ("foo or -goto->foo; print \"ok\";", "-"),
    ] {
        if !perl_compile_accepts(source)? {
            return Err(format!("real Perl rejected valid postfix-arrow form: {source:?}"));
        }
        let output = Parser::new(source).parse_with_recovery();
        if !output.diagnostics.is_empty() {
            return Err(format!(
                "postfix-arrow goto form must remain diagnostic-free: source={source:?}, diagnostics={:?}\nast={}",
                output.diagnostics,
                output.ast.to_sexp()
            ));
        }
        let statements = match &output.ast.kind {
            NodeKind::Program { statements } => statements,
            _ => {
                return Err(format!("postfix-arrow source did not produce a program: {source:?}"));
            }
        };
        let first_expression = match statements.first().map(|statement| &statement.kind) {
            Some(NodeKind::ExpressionStatement { expression }) => expression,
            _ => {
                return Err(format!("postfix-arrow source lost its first expression: {source:?}"));
            }
        };
        let second_expression = match statements.get(1).map(|statement| &statement.kind) {
            Some(NodeKind::ExpressionStatement { expression }) => expression,
            _ => {
                return Err(format!("postfix-arrow source lost its trailing print: {source:?}"));
            }
        };
        let first_is_or = matches!(
            &first_expression.kind,
            NodeKind::Binary { op, .. } if op == "or"
        );
        let second_is_print = matches!(
            &second_expression.kind,
            NodeKind::FunctionCall { name, args } if name == "print" && !args.is_empty()
        );
        if !first_is_or
            || !or_rhs_is_unary_postfix_goto(first_expression, unary_op)
            || !second_is_print
            || contains_goto(first_expression)
        {
            return Err(format!(
                "postfix-arrow goto form must preserve the word operator, unary postfix call, and trailing statement:\nsource={source:?}\nast={}",
                output.ast.to_sexp()
            ));
        }
    }
    Ok(())
}

#[test]
fn unary_word_goto_forms_do_not_hide_trailing_residue() -> Result<(), String> {
    for (source, token) in [
        ("foo or not goto fail; 1 2;", "2"),
        ("foo or !goto fail; 1 2;", "2"),
        ("foo or +goto fail; 1 2;", "2"),
    ] {
        if perl_compile_accepts(source)? {
            return Err(format!("real Perl unexpectedly accepted residue: {source:?}"));
        }
        assert_same_line_residual_at(source, token)?;
    }
    Ok(())
}

#[test]
fn valid_low_precedence_and_directive_continuations_do_not_gain_residual_errors()
-> Result<(), String> {
    for source in [
        "no warnings qw(uninitialized numeric); my $x = 1;",
        "open my $fh, '<', $path or die $!;",
        "print $fh \"message\" or die $!;",
        "$ok = do_work() if $enabled;",
    ] {
        assert_valid_case(source);
    }
    Ok(())
}

#[test]
fn malformed_guard_boundaries_are_not_claimed_as_same_line_residue() -> Result<(), String> {
    for source in ["use strict qw(", "no warnings qw(", "my $x = <<'END';\nunterminated\n"] {
        assert_no_same_line_residual(source)?;
    }
    Ok(())
}

#[test]
fn real_perl_oracle_agrees_on_supported_continuations_and_residue() -> Result<(), String> {
    let valid_sources = [
        ("copy($from, $to) or goto fail; print \"ok\";", Some(("or", true))),
        ("copy($from, $to) and goto fail; print \"ok\";", Some(("and", true))),
        ("copy($from, $to) xor goto &fail_sub; print \"ok\";", Some(("xor", true))),
        ("copy($from, $to) or goto $dynamic_target; print \"ok\";", Some(("or", true))),
        ("foo or goto => 1; print \"ok\";", Some(("or", false))),
        ("foo or (goto => 1); print \"ok\";", Some(("or", false))),
        ("foo or (goto => 1, next => 2); print \"ok\";", Some(("or", false))),
        ("foo or goto => 1, bar => 2; print \"ok\";", Some(("or", false))),
        ("foo and (goto => 1); print \"ok\";", Some(("and", false))),
        ("foo xor (goto => 1); print \"ok\";", Some(("xor", false))),
        ("foo or not goto fail; print \"ok\";", Some(("or", true))),
        ("foo or !goto fail; print \"ok\";", Some(("or", true))),
        ("foo or +goto fail; print \"ok\";", Some(("or", true))),
    ];
    for (source, expected) in valid_sources {
        if !perl_compile_accepts(source)? {
            return Err(format!("real Perl rejected a supported continuation: {source:?}"));
        }
        let output = Parser::new(source).parse_with_recovery();
        if output.diagnostics.iter().any(ParseError::blocks_clean_parse) {
            return Err(format!(
                "Rust parser rejected a real-Perl-supported continuation:\nsource={source:?}\ndiagnostics={:?}",
                output.diagnostics
            ));
        }
        let statements = match &output.ast.kind {
            NodeKind::Program { statements } => statements,
            kind => return Err(format!("expected a program, got {}", kind.kind_name())),
        };
        let Some((operator, contains_control_flow_goto)) = expected else {
            continue;
        };
        if statements.len() != 2 {
            return Err(format!(
                "real-Perl-supported {operator} continuation must preserve the later statement:\n{}",
                output.ast.to_sexp()
            ));
        }
        let has_control_flow_goto =
            first_expression_has_direct_word_operator_goto_rhs(&output.ast, operator)
                || ["not", "!", "+"].into_iter().any(|unary_op| {
                    first_expression_has_direct_word_operator_unary_goto_rhs(
                        &output.ast,
                        operator,
                        unary_op,
                    )
                });
        if has_control_flow_goto != contains_control_flow_goto {
            return Err(format!(
                "{operator} fat-arrow/control-flow classification disagreed with the expected AST:\n{}",
                output.ast.to_sexp()
            ));
        }
    }

    for source in ["my $x = 1 print \"hi\";", "my $x = 1 2;", "$value x = 3;"] {
        if perl_compile_accepts(source)? {
            return Err(format!("real Perl unexpectedly accepted invalid residue: {source:?}"));
        }
        assert_non_clean(source)?;
    }
    Ok(())
}

#[test]
fn fat_arrow_goto_bareword_is_not_consumed_as_control_flow() -> Result<(), String> {
    let source = "foo or goto => 1; print \"ok\";";
    if !perl_compile_accepts(source)? {
        return Err("real Perl rejected the fat-arrow goto bareword regression".to_string());
    }

    let output = Parser::new(source).parse_with_recovery();
    if output.diagnostics.iter().any(ParseError::blocks_clean_parse) {
        return Err(format!(
            "fat-arrow goto was incorrectly treated as control flow:\ndiagnostics={:?}\nast={}",
            output.diagnostics,
            output.ast.to_sexp()
        ));
    }
    let statements = match &output.ast.kind {
        NodeKind::Program { statements } => statements,
        kind => return Err(format!("expected a program, got {}", kind.kind_name())),
    };
    if statements.len() != 2 || !output.ast.to_sexp().contains("binary_or") {
        return Err(format!(
            "fat-arrow goto must remain in the word-operator expression with a later print statement:\n{}",
            output.ast.to_sexp()
        ));
    }
    Ok(())
}

#[test]
fn valid_class_method_source_does_not_gain_a_false_semicolon_error() -> Result<(), String> {
    let source = concat!(
        "use v5.40;\n",
        "use feature 'class';\n",
        "no warnings 'experimental::class';\n",
        "class C { method m { 1 } }\n",
    );
    assert_valid_case(source);
    Ok(())
}

#[test]
fn cross_line_missing_terminator_remains_non_clean() -> Result<(), String> {
    let source = "my $x = 1\nprint \"hi\";";
    assert_non_clean(source)?;
    let output = Parser::new(source).parse_with_recovery();
    let expected = source.find("print").ok_or("missing test token")?;
    let locations: Vec<usize> = output
        .diagnostics
        .iter()
        .filter_map(|error| match error {
            ParseError::Recovered {
                site: RecoverySite::Statement,
                kind: RecoveryKind::InferredSemicolon,
                location,
            } => Some(*location),
            _ => None,
        })
        .collect();
    if locations != [expected] {
        return Err(format!(
            "cross-line recovery changed identity/location: expected [{expected}], got {locations:?}"
        ));
    }
    Ok(())
}

#[test]
fn same_line_residual_recovery_is_deterministic() -> Result<(), String> {
    let source = "use strict; my $x = 1 print \"hi\";";
    let first = Parser::new(source).parse_with_recovery();
    let second = Parser::new(source).parse_with_recovery();

    if first.ast.to_sexp() != second.ast.to_sexp()
        || format!("{:?}", first.diagnostics) != format!("{:?}", second.diagnostics)
    {
        return Err(format!(
            "same-line residual recovery changed between identical parses:\nfirst={} {:?}\nsecond={} {:?}",
            first.ast.to_sexp(),
            first.diagnostics,
            second.ast.to_sexp(),
            second.diagnostics
        ));
    }
    if first.diagnostics.is_empty() && !has_recovery_node(&first.ast) {
        return Err("deterministic clean acceptance is still incorrect".to_string());
    }
    Ok(())
}
