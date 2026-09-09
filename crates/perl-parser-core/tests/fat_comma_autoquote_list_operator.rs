//! A fat comma auto-quotes the bareword to its left in a parenthesis-free
//! list-operator call, exactly as it already does inside parentheses (#13604).
//!
//! `hook before => sub {...}` passes the *string* `before`, while
//! `hook(before, sub {...})` calls a sub of that name and passes its result.
//! Only the separator distinguishes them. Before this was fixed the
//! parenthesis-free spelling left a bare `Identifier`, so the two spellings of
//! one call disagreed and every consumer had to recover the distinction from
//! source bytes at tree-derived offsets — a second authority that byte offsets
//! cannot bind to a document.
//!
//! The ground truth for these expectations is `perl` itself; it is checked
//! against a live interpreter in `perl-semantic-analyzer`'s
//! `dancer2_hook_fat_comma_oracle`, which is where the claim about Perl
//! semantics is proven rather than assumed.

use perl_parser_core::{Node, NodeKind, Parser};
use perl_tdd_support::{must, must_some};

/// The first argument of the first `FunctionCall` named `name`, described as
/// `"String(text)"` or `"Identifier(text)"`.
fn first_argument_of(source: &str, name: &str) -> String {
    let mut parser = Parser::new(source);
    let ast = must(parser.parse());
    let mut found = None;
    collect(&ast, name, &mut found);
    must_some(found)
}

fn collect(node: &Node, wanted: &str, found: &mut Option<String>) {
    if found.is_some() {
        return;
    }
    if let NodeKind::FunctionCall { name, args } = &node.kind
        && name == wanted
    {
        *found = Some(match args.first().map(|arg| &arg.kind) {
            Some(NodeKind::String { value, .. }) => format!("String({value})"),
            Some(NodeKind::Identifier { name }) => format!("Identifier({name})"),
            other => format!("{other:?}"),
        });
        return;
    }
    for child in node.children() {
        collect(child, wanted, found);
    }
}

#[test]
fn a_fat_comma_quotes_the_bareword_without_parentheses() {
    assert_eq!(
        first_argument_of("hook before => sub { 1 };", "hook"),
        "String(before)",
        "`=>` auto-quotes the bareword to its left"
    );
}

/// The parenthesised form already quoted. The two spellings of one call must
/// not disagree — that divergence is the actual defect being fixed.
#[test]
fn both_spellings_of_one_call_agree() {
    assert_eq!(
        first_argument_of("hook before => sub { 1 };", "hook"),
        first_argument_of("hook(before => sub { 1 });", "hook"),
        "parenthesised and parenthesis-free calls must classify the operand identically"
    );
}

/// The separator alone decides. A comma calls `before()`, so the operand must
/// stay an `Identifier`: quoting it here would invent a literal Perl never
/// passes, which is the failure this change must not trade for the other one.
#[test]
fn a_comma_does_not_quote_the_bareword() {
    assert_eq!(
        first_argument_of("hook(before, sub { 1 });", "hook"),
        "Identifier(before)",
        "a comma leaves the bareword a call"
    );
}

/// Perl auto-quotes across trivia: the comment between the bareword and the
/// fat comma does not make the operand computed. The lexer drops the comment,
/// so the parser sees the separator directly and needs no trivia rule of its
/// own — this pins that.
#[test]
fn a_comment_before_the_fat_comma_still_quotes() {
    assert_eq!(
        first_argument_of("hook before # a note\n    => sub { 1 };", "hook"),
        "String(before)",
        "a comment does not stop `=>` auto-quoting"
    );
}

/// `__PACKAGE__` is the sharpest case: after `=>` Perl quotes the token to its
/// literal text rather than expanding it to the current package. The parser
/// must follow the separator here too instead of special-casing the token.
#[test]
fn the_dunder_package_token_is_quoted_after_a_fat_comma() {
    assert_eq!(
        first_argument_of("hook __PACKAGE__ => sub { 1 };", "hook"),
        "String(__PACKAGE__)",
        "`=>` quotes the token instead of expanding it"
    );
}

/// Only a bare identifier is rewritten. An operand that already carries a
/// value keeps its own node, so quoting cannot swallow a real expression.
#[test]
fn only_barewords_are_rewritten() {
    assert_eq!(
        first_argument_of("hook 'before' => sub { 1 };", "hook"),
        "String('before')",
        "an existing string literal is left exactly as parsed"
    );
    let variable = first_argument_of("hook $name => sub { 1 };", "hook");
    assert!(
        variable.starts_with("Some(Variable"),
        "a variable operand stays a variable, got {variable}"
    );
}
