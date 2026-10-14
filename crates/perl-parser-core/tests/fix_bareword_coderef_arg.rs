//! Regression proof for the comp parse_recovery cluster (#4730):
//! upstream `t/comp/proto.t` calls a declared sub with a reference first
//! argument in list-operator form (`testing \&no_proto, undef;`). The
//! bare-call admission check had no `Backslash` arm, so the call lost its
//! argument uptake and left same-line residue.

mod cpan_test_helpers;

use cpan_test_helpers::{assert_clean_parse, assert_no_blocking_diagnostics};
use perl_parser_core::{Node, NodeKind, Parser};
use perl_tdd_support::{must_some_with, must_with};

fn find_function_call<'a>(node: &'a Node, name: &str) -> Option<(&'a str, Vec<String>)> {
    match &node.kind {
        NodeKind::FunctionCall { name: call_name, args } if call_name == name => Some((
            call_name.as_str(),
            args.iter().map(|arg| arg.kind.kind_name().to_string()).collect(),
        )),
        _ => node.children().into_iter().find_map(|child| find_function_call(child, name)),
    }
}

fn assert_parses_as_call(source: &str, name: &str, expected_args: usize) -> Vec<String> {
    let mut parser = Parser::new(source);
    let ast = must_with(parser.parse(), format!("parse failed for {source:?}"));
    let found = find_function_call(&ast, name);
    let (found_name, arg_kinds) = must_some_with(
        found,
        format!("expected FunctionCall {name} for source:\n{source}\n\nsexp:\n{}", ast.to_sexp()),
    );
    assert_eq!(found_name, name);
    assert_eq!(
        arg_kinds.len(),
        expected_args,
        "expected {expected_args} args for source:\n{source}\n\nsexp:\n{}",
        ast.to_sexp()
    );
    arg_kinds
}

#[test]
fn declared_bareword_call_with_coderef_arg_parses_as_two_arg_call() {
    let source = "sub testing { } sub no_proto { } testing \\&no_proto, undef;";
    let arg_kinds = assert_parses_as_call(source, "testing", 2);
    assert_eq!(
        arg_kinds[0], "Unary",
        "first arg should be the reference expression: {arg_kinds:?}"
    );
    assert_clean_parse(source);
    assert_no_blocking_diagnostics(source);
}

#[test]
fn bareword_call_reference_arg_forms_parse() {
    assert_parses_as_call("sub foo { } foo \\&x;", "foo", 1);
    assert_parses_as_call("sub foo { } foo \\@a;", "foo", 1);
    assert_parses_as_call("sub foo { } foo \\\\$x;", "foo", 1);
}

#[test]
fn repeated_backslashes_with_operands_are_call_arguments() {
    for source in [
        r"sub foo { } foo \\$x;",
        r"sub foo { } foo \\\$x;",
        r"sub foo { } sub cb { } foo \\&cb;",
        r"sub foo { } sub cb { } foo \\\&cb;",
    ] {
        let arg_kinds = assert_parses_as_call(source, "foo", 1);
        assert_eq!(arg_kinds[0], "Unary");
        assert_clean_parse(source);
        assert_no_blocking_diagnostics(source);
    }
}

#[test]
fn explicit_reference_operands_remain_clean() {
    // Real Perl accepts these operands, including empty parenthesized lists.
    for source in [
        r"my $ref = \undef;",
        r"my $ref = \();",
        r"my $ref = \{};",
        r"my $ref = \[];",
        r"my $ref = \\();",
    ] {
        assert_clean_parse(source);
        assert_no_blocking_diagnostics(source);
    }
}

#[test]
fn bareword_call_other_args_still_parse() {
    assert_clean_parse("sub foo { } foo $x, undef;");
    assert_clean_parse("sub foo { } foo &x;");
    assert_clean_parse("sub foo { } foo %h;");
}

#[test]
fn uppercase_declared_call_with_reference_arg_parses() {
    assert_parses_as_call("sub T { } T \\&cb;", "T", 1);
}

#[test]
fn operandless_backslash_is_not_a_call_argument() {
    // Perl rejects each sibling: nesting references does not supply an operand.
    for source in [
        r"sub foo { } foo \;",
        r"sub foo { } foo \\;",
        r"sub foo { } foo \\\;",
        r"sub foo { } foo \\",
        r"sub foo { } foo(\\);",
        r"my $ref = \;",
    ] {
        let mut parser = Parser::new(source);
        let ast = must_with(parser.parse(), format!("parse failed for {source:?}"));
        assert!(
            ast.to_sexp().contains("ERROR") || !parser.get_errors().is_empty(),
            "operandless backslash must stay an error for {source:?}, got:\n{}",
            ast.to_sexp()
        );
    }
}

#[test]
fn uppercase_indirect_and_concat_forms_are_not_bare_calls() {
    // Negative pins: these must not be re-read as bare calls by the new arm.
    for (source, excluded_owner) in [("print STDOUT \\$x;", "STDOUT"), ("$z = foo . \\$x;", "foo")]
    {
        let mut parser = Parser::new(source);
        let ast = must_with(parser.parse(), format!("parse failed for {source:?}"));
        assert!(
            find_function_call(&ast, excluded_owner).is_none(),
            "unexpected FunctionCall {excluded_owner} for source:\n{source}\n\nsexp:\n{}",
            ast.to_sexp()
        );
        assert_clean_parse(source);
        assert_no_blocking_diagnostics(source);
    }
}

#[test]
fn lowercase_filehandle_form_is_a_pinned_limitation() {
    // Known limitation, shared with the pre-existing sigil arm: real Perl
    // reads `print myfh \$x;` as print-to-filehandle, while this parser
    // currently reads a nested call `print(myfh(\$x))`. Pinning the shape
    // keeps the ambiguity explicit instead of silently widening the
    // indirect-object admission set in this claim.
    let source = "print myfh \\$x;";
    let mut parser = Parser::new(source);
    let ast = must_with(parser.parse(), format!("parse failed for {source:?}"));
    let sexp = ast.to_sexp();
    assert!(
        sexp.contains("myfh"),
        "limitation pin: expected the nested-call reading to remain visible:\n{sexp}"
    );
    assert_clean_parse(source);
}
