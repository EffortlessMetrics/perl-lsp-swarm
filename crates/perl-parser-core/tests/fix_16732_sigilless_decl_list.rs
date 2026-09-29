//! #16732 — sigil-less items in lexical declaration lists.
//!
//! Real `perl -c` rejects `my (base) = @_;` with
//! `Can't declare constant item in "my"`. The native parser previously
//! accepted the bareword as a list slot, so `Parser::parse()` retained no
//! blocking diagnostic and `perllsp --check` answered `ok`.
//!
//! This file is the parser authority for that admission. It does not add a
//! second checker in code actions.

mod cpan_test_helpers;
use cpan_test_helpers::{assert_clean_parse, parse};
use perl_parser_core::{Node, NodeKind, Parser};
use perl_tdd_support::must;

const INVALID_MY_BASE: &str = "use strict; my (base) = @_;\n";
const VALID_MY_BASE: &str = "use strict; my ($base) = @_;\n";

fn parse_with_errors(source: &str) -> (Node, Vec<perl_parser_core::ParseError>) {
    let mut parser = Parser::new(source);
    let ast = must(parser.parse());
    (ast, parser.get_errors().to_vec())
}

fn find_variable<'a>(node: &'a Node, sigil: &str, name: &str) -> Option<&'a Node> {
    if let NodeKind::Variable { sigil: found_sigil, name: found_name } = &node.kind
        && found_sigil == sigil
        && found_name == name
    {
        return Some(node);
    }
    node.children().into_iter().find_map(|child| find_variable(child, sigil, name))
}

fn blocking_messages(errors: &[perl_parser_core::ParseError]) -> Vec<String> {
    errors.iter().filter(|error| error.blocks_clean_parse()).map(ToString::to_string).collect()
}

/// `assert_has_blocking_error` matches Debug/sexp text, which escapes the
/// quotes in `in "my"`. Display keeps the Perl wording intact.
fn assert_blocking_constant_item(source: &str, declarator: &str) {
    let (ast, errors) = parse_with_errors(source);
    let messages = blocking_messages(&errors);
    let expected = format!("Can't declare constant item in \"{declarator}\"");
    assert!(
        messages.iter().any(|message| message.contains(&expected)),
        "expected {expected:?}, got {messages:?}\n{}",
        ast.to_sexp()
    );
    assert!(
        ast.to_sexp().contains("ERROR"),
        "recovery must keep an Error node for source:\n{source}\n{}",
        ast.to_sexp()
    );
}

/// Independent syntax oracle. Side-effect-free `-c` only; skipped when `perl`
/// is not on PATH so the native proof does not depend on a host interpreter.
fn perl_c_status(source: &str) -> Option<bool> {
    let output = std::process::Command::new("perl").args(["-c", "-e", source]).output().ok()?;
    Some(output.status.success())
}

#[test]
fn perl_c_rejects_sigilless_my_list_and_accepts_the_sigil_control() {
    if let Some(ok) = perl_c_status(INVALID_MY_BASE) {
        assert!(!ok, "perl -c must reject `my (base) = @_;`");
    }
    if let Some(ok) = perl_c_status(VALID_MY_BASE) {
        assert!(ok, "perl -c must accept `my ($base) = @_;`");
    }
}

#[test]
fn my_sigilless_list_item_is_a_blocking_constant_item() {
    assert_blocking_constant_item(INVALID_MY_BASE, "my");
}

#[test]
fn our_and_state_sigilless_list_items_are_blocking() {
    assert_blocking_constant_item("our (base) = @_;\n", "our");
    assert_blocking_constant_item("use feature 'state'; state (base) = @_;\n", "state");
}

#[test]
fn numeric_and_string_list_items_are_constant_items() {
    assert_blocking_constant_item("my (1) = @_;\n", "my");
    assert_blocking_constant_item("my (\"base\") = @_;\n", "my");
}

#[test]
fn diagnostic_is_tied_to_the_offending_bareword() {
    let source = "my (base) = @_;\n";
    let (ast, errors) = parse_with_errors(source);
    let blocking: Vec<_> = errors.into_iter().filter(|error| error.blocks_clean_parse()).collect();
    assert!(
        blocking.iter().any(|error| error.to_string().contains("constant item")),
        "expected constant-item diagnostic, got {blocking:?}\n{}",
        ast.to_sexp()
    );
    let constant = blocking.iter().find(|error| error.to_string().contains("constant item"));
    let Some(constant) = constant else {
        panic!("constant-item diagnostic must exist");
    };
    let Some(start) = source.find("base") else {
        panic!("fixture contains base");
    };
    assert_eq!(constant.location(), Some(start), "diagnostic must point at the bareword, not `my`");
}

#[test]
fn mixed_list_keeps_valid_slots_and_flags_the_bareword() {
    let source = "my ($ok, base, $also) = @_;\n";
    let (ast, errors) = parse_with_errors(source);
    let messages = blocking_messages(&errors);
    assert!(
        messages.iter().any(|message| message.contains("constant item")),
        "expected constant-item diagnostic, got {messages:?}\n{}",
        ast.to_sexp()
    );
    assert!(
        find_variable(&ast, "$", "ok").is_some(),
        "valid $ok slot must remain: {}",
        ast.to_sexp()
    );
    assert!(
        find_variable(&ast, "$", "also").is_some(),
        "valid $also slot must remain: {}",
        ast.to_sexp()
    );
}

#[test]
fn nested_sigilless_item_is_rejected() {
    assert_blocking_constant_item("my ($a, (base)) = @_;\n", "my");
}

#[test]
fn recovery_keeps_later_independent_declaration() {
    let source = "my (base) = @_;\nmy $later = 1;\n";
    let (ast, errors) = parse_with_errors(source);
    assert!(
        errors.iter().any(|error| error.blocks_clean_parse()),
        "invalid declaration must stay blocking\n{}",
        ast.to_sexp()
    );
    assert!(
        find_variable(&ast, "$", "later").is_some(),
        "later independent declaration must remain usable: {}",
        ast.to_sexp()
    );
}

#[test]
fn valid_scalar_list_from_underscore_stays_clean() {
    assert_clean_parse(VALID_MY_BASE);
}

#[test]
fn valid_array_hash_undef_and_nested_lists_stay_clean() {
    assert_clean_parse("my (@arr, %hash) = @_;\n");
    assert_clean_parse("my ($a, undef, $c) = @_;\n");
    assert_clean_parse("my ($a, ($b, $c)) = (1, (2, 3));\n");
    assert_clean_parse("our ($Foo::bar);\n");
    assert_clean_parse("local ($a, $b) = @_;\n");
    assert_clean_parse("my ($x :lvalue);\n");
}

#[test]
fn valid_dollar_base_keeps_source_geometry() {
    let source = "my ($base) = @_;";
    let ast = parse(source);
    let Some(variable) = find_variable(&ast, "$", "base") else {
        panic!("$base must remain in the AST: {}", ast.to_sexp());
    };
    let Some(start) = source.find("$base") else {
        panic!("fixture contains $base");
    };
    let end = start + "$base".len();
    assert_eq!(
        (variable.location.start, variable.location.end),
        (start, end),
        "valid `$base` geometry must not move: {}",
        ast.to_sexp()
    );
}

#[test]
fn declaration_as_argument_uses_the_same_admission() {
    assert_blocking_constant_item("foo(my (base) = @_);\n", "my");
    assert_clean_parse("foo(my ($base) = @_);\n");
}
