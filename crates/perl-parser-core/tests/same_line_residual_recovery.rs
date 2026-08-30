//! Regression proof that invalid same-line residue cannot masquerade as a clean parse.

mod cpan_test_helpers;

use cpan_test_helpers::assert_clean_parse;
use perl_parser_core::error::{ParseError, RecoveryKind, RecoverySite};
use perl_parser_core::{Node, NodeKind, Parser};

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

    node.children()
        .into_iter()
        .find_map(|child| find_assignment(child, expected_op))
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
fn spaced_repetition_tokens_are_not_rewritten_to_x_assign() -> Result<(), String> {
    let source = "$value x = 3;";
    let output = Parser::new(source).parse_with_recovery();
    if find_assignment(&output.ast, "x=").is_some() {
        return Err(format!(
            "spaced x = must not be normalized to x=:\n{}",
            output.ast.to_sexp()
        ));
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
