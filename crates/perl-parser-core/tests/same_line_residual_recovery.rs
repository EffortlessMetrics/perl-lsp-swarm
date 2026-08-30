//! Regression proof that invalid same-line residue cannot masquerade as a clean parse.

mod cpan_test_helpers;

use cpan_test_helpers::assert_clean_parse;
use perl_parser_core::error::{ParseError, RecoveryKind, RecoverySite};
use perl_parser_core::{Node, NodeKind, Parser};
use std::process::Command;

fn has_recovery_node(node: &Node) -> bool {
    if matches!(node.kind, NodeKind::Error { .. } | NodeKind::MissingExpression) {
        return true;
    }

    node.children().into_iter().any(has_recovery_node)
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

fn contains_word_operator_with_goto(node: &Node, expected_op: &str) -> bool {
    if let NodeKind::Binary { op, left, right } = &node.kind {
        if op == expected_op && (contains_goto(left) || contains_goto(right)) {
            return true;
        }
    }
    node.children().into_iter().any(|child| contains_word_operator_with_goto(child, expected_op))
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
    assert_clean_parse(source);
    assert_no_same_line_residual(source)
}

#[test]
fn command_line_ne_wrapper_does_not_hide_following_residue() -> Result<(), String> {
    let source = "-ne print 1 2;";
    assert_same_line_residual_at(source, "2")
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
        assert_clean_parse(source);
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
        if statements.len() != 2 || !contains_word_operator_with_goto(&statements[0], operator) {
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
        if statements.len() != 2 || !contains_word_operator_with_goto(&statements[0], operator) {
            return Err(format!(
                "same-line {operator} goto {target} must keep its Goto RHS and later print statement:\n{}",
                output.ast.to_sexp()
            ));
        }
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
        assert_clean_parse(source);
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
        let has_control_flow_goto = contains_word_operator_with_goto(&statements[0], operator);
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
    assert_clean_parse(source);
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
