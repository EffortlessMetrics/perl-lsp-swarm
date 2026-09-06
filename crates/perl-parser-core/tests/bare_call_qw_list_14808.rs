//! Discriminating proof for #14808: an unbracketed, unparenthesized `qw`
//! list immediately after a bareword list operator must bind as one call.
//!
//! Perl flattens `qw` in list context, so `has qw(a b);` is the same call as
//! `has 'a', 'b';`. Before the fix, QuoteWords was not a `looks_like_bare_call`
//! argument start, so the parser emitted a bare Identifier and a sibling
//! ArrayLiteral instead of one FunctionCall.

mod cpan_test_helpers;
use cpan_test_helpers::*;

use perl_parser_core::{Node, NodeKind};
use perl_tdd_support::{must_some_with, must_with};

fn statements(source: &str) -> Vec<Node> {
    let ast = parse(source);
    let sexp = ast.to_sexp();
    must_with(
        match ast.into_parts().0 {
            NodeKind::Program { statements } => Ok(statements),
            kind => Err(format!("expected Program, got {}:\n{sexp}", kind.kind_name())),
        },
        "parse produced a Program",
    )
}

fn kind_names(source: &str) -> Vec<String> {
    let ast = parse(source);
    top_level_kinds(&ast).into_iter().map(str::to_string).collect()
}

fn unwrap_expr(stmt: Node) -> Node {
    let kind_name = stmt.kind.kind_name();
    must_with(
        match stmt.into_parts().0 {
            NodeKind::ExpressionStatement { expression } => Ok(*expression),
            _ => Err(format!("expected ExpressionStatement, got {kind_name}")),
        },
        "statement was an ExpressionStatement",
    )
}

fn string_payload(node: &Node) -> Option<&str> {
    match &node.kind {
        NodeKind::String { value, .. } => Some(value.as_str()),
        _ => None,
    }
}

fn named_call(source: &str, name: &str) -> (String, Vec<Node>) {
    let ast = parse(source);
    let mut found = Vec::new();
    collect_named_calls(&ast, name, &mut found);
    assert_eq!(
        found.len(),
        1,
        "expected exactly one FunctionCall named `{name}` in `{source}`, found {}:\n{}",
        found.len(),
        ast.to_sexp()
    );
    must_some_with(found.pop(), "exactly one matching FunctionCall")
}

fn collect_named_calls(node: &Node, name: &str, out: &mut Vec<(String, Vec<Node>)>) {
    if let NodeKind::FunctionCall { name: call_name, args } = &node.kind
        && call_name == name
    {
        out.push((call_name.clone(), args.clone()));
    }
    for child in node.children() {
        collect_named_calls(child, name, out);
    }
}

fn qw_word_payloads(source: &str) -> Vec<String> {
    let ast = parse(source);
    must_some_with(
        standalone_qw_words(&ast),
        format!("standalone qw ArrayLiteral in `{source}`:\n{}", ast.to_sexp()),
    )
}

fn standalone_qw_words(node: &Node) -> Option<Vec<String>> {
    if let NodeKind::ArrayLiteral { elements } = &node.kind {
        let words: Vec<String> =
            elements.iter().filter_map(|el| string_payload(el).map(str::to_string)).collect();
        if words.len() == elements.len() && !words.is_empty() {
            return Some(words);
        }
    }
    for child in node.children() {
        if let Some(words) = standalone_qw_words(child) {
            return Some(words);
        }
    }
    None
}

// ---------------------------------------------------------------------------
// QuoteWords after a lowercase bareword must bind as one call.
// ---------------------------------------------------------------------------

#[test]
fn bare_qw_after_has_is_one_flattened_call() {
    let source = "has qw(a b);";
    assert_clean_parse(source);

    let stmts = statements(source);
    assert_eq!(
        stmts.len(),
        1,
        "has qw(a b); must be one statement, not a bare Identifier plus an orphaned qw list:\n{:?}",
        kind_names(source)
    );

    let (name, args) = named_call(source, "has");
    assert_eq!(name, "has");
    let payloads: Vec<&str> = args.iter().filter_map(string_payload).collect();
    assert_eq!(
        payloads,
        qw_word_payloads("qw(a b);").iter().map(String::as_str).collect::<Vec<_>>(),
        "qw words must flatten into the call args, matching the standalone qw word payloads; args were {:?}",
        args.iter().map(|a| a.kind.kind_name()).collect::<Vec<_>>()
    );
}

#[test]
fn package_has_qw_does_not_orphan_the_list() {
    let source = "package App;\nhas qw(a b);\n";
    assert_clean_parse(source);
    let kinds = kind_names(source);
    assert_eq!(
        kinds,
        ["Package", "ExpressionStatement"],
        "package + has qw(a b) must not emit a sibling ArrayLiteral statement, got {kinds:?}"
    );
    let (_, args) = named_call(source, "has");
    assert_eq!(args.len(), 2, "expected two flattened qw words, got {} args", args.len());
}

#[test]
fn unknown_list_operator_croak_qw_binds() {
    let source = "croak qw(missing required);";
    assert_clean_parse(source);
    let (_, args) = named_call(source, "croak");
    let payloads: Vec<&str> = args.iter().filter_map(string_payload).collect();
    assert_eq!(payloads, ["missing", "required"]);
}

// ---------------------------------------------------------------------------
// Existing shapes that must not change.
// ---------------------------------------------------------------------------

#[test]
fn comma_separated_string_args_stay_a_flat_call() {
    let source = "has 'a', 'b';";
    assert_clean_parse(source);
    let (_, args) = named_call(source, "has");
    assert_eq!(args.len(), 2);
    assert!(
        args.iter().all(|arg| string_payload(arg).is_some()),
        "has 'a', 'b' must remain two string args, got {:?}",
        args.iter().map(|a| a.kind.kind_name()).collect::<Vec<_>>()
    );
}

#[test]
fn bracketed_qw_stays_an_index_expression() {
    let source = "has [qw(a b)];";
    assert_clean_parse(source);
    let mut stmts = statements(source);
    assert_eq!(stmts.len(), 1, "has [qw(a b)] must stay one statement");
    let expr = unwrap_expr(stmts.remove(0));
    let (op, left, right) = must_with(
        match &expr.kind {
            NodeKind::Binary { op, left, right } => Ok((op, left, right)),
            other => Err(format!(
                "has [qw(a b)] must remain Binary[], got {}:\n{}",
                other.kind_name(),
                expr.to_sexp()
            )),
        },
        "bracketed form is Binary[]",
    );
    assert_eq!(op, "[]");
    assert!(
        matches!(&left.kind, NodeKind::Identifier { name } if name == "has"),
        "bracketed form must keep Identifier(has) on the left, got {}",
        left.kind.kind_name()
    );
    assert!(
        matches!(&right.kind, NodeKind::ArrayLiteral { .. }),
        "bracketed qw must remain an ArrayLiteral index, got {}",
        right.kind.kind_name()
    );
}

#[test]
fn parenthesized_call_keeps_its_current_binding() {
    let source = "has(qw(a b));";
    assert_clean_parse(source);
    let (name, args) = named_call(source, "has");
    assert_eq!(name, "has");
    assert_eq!(
        args.len(),
        1,
        "parenthesized has(qw(a b)) must keep a single argument node, got {}:\n{}",
        args.len(),
        parse(source).to_sexp()
    );
    assert!(
        matches!(&args[0].kind, NodeKind::ArrayLiteral { .. }),
        "parenthesized qw argument must remain ArrayLiteral, got {}",
        args[0].kind.kind_name()
    );
}

// ---------------------------------------------------------------------------
// Negative controls: genuine standalone qw, and qw that is not immediately
// after the bareword, must not be absorbed.
// ---------------------------------------------------------------------------

#[test]
fn standalone_qw_statement_is_not_a_call() {
    let source = "qw(a b);";
    assert_clean_parse(source);
    let kinds = kind_names(source);
    assert_eq!(kinds, ["ExpressionStatement"]);
    let mut stmts = statements(source);
    assert_eq!(stmts.len(), 1);
    let expr = unwrap_expr(stmts.remove(0));
    assert!(
        matches!(expr.kind, NodeKind::ArrayLiteral { .. }),
        "standalone qw must remain ArrayLiteral, got {}",
        expr.kind.kind_name()
    );
}

#[test]
fn qw_after_a_finished_has_statement_stays_standalone() {
    let source = "has;\nqw(a b);\n";
    assert_clean_parse(source);
    let kinds = kind_names(source);
    assert_eq!(kinds.len(), 2, "has; then qw(a b); must remain two statements, got {kinds:?}");
    let mut stmts = statements(source);
    assert_eq!(stmts.len(), 2);
    let expr = unwrap_expr(stmts.remove(1));
    assert!(
        matches!(expr.kind, NodeKind::ArrayLiteral { .. }),
        "second statement must remain standalone qw, got {}",
        expr.kind.kind_name()
    );
}

#[test]
fn declared_sub_name_binds_qw_the_same_as_an_undeclared_bareword() {
    let source = "sub has {}\nhas qw(a b);\n";
    assert_clean_parse(source);
    let (_, args) = named_call(source, "has");
    let payloads: Vec<&str> = args.iter().filter_map(string_payload).collect();
    assert_eq!(payloads, ["a", "b"]);
}

fn call_arg_kinds(source: &str, name: &str) -> Vec<&'static str> {
    named_call(source, name).1.iter().map(|arg| arg.kind.kind_name()).collect()
}

fn call_string_payloads(source: &str, name: &str) -> Vec<String> {
    named_call(source, name).1.iter().filter_map(string_payload).map(str::to_string).collect()
}

// ---------------------------------------------------------------------------
// Edge cases that would still pass a naive "admit QuoteWords, keep ArrayLiteral"
// implementation, and opposite-direction controls for later qw arguments.
// ---------------------------------------------------------------------------

#[test]
fn qw_words_flatten_rather_than_remaining_one_array_arg() {
    let source = "has qw(a b);";
    assert_eq!(call_arg_kinds(source, "has"), ["String", "String"]);
    assert_eq!(call_string_payloads(source, "has"), ["a", "b"]);
}

#[test]
fn alternate_qw_delimiters_bind_and_flatten() {
    for source in ["has qw/a b/;", "has qw{a b};", "has qw[a b];", "has qw<a b>;"] {
        assert_clean_parse(source);
        assert_eq!(call_string_payloads(source, "has"), ["a", "b"], "source={source}");
    }
}

#[test]
fn empty_qw_is_a_zero_arg_call_not_two_statements() {
    let source = "has qw();";
    assert_clean_parse(source);
    assert_eq!(kind_names(source), ["ExpressionStatement"]);
    let (_, args) = named_call(source, "has");
    assert!(args.is_empty(), "has qw(); must be has() with no args, got {} args", args.len());
}

#[test]
fn comma_after_immediate_qw_still_collects_later_args() {
    let source = "has qw(a b), $x;";
    assert_clean_parse(source);
    assert_eq!(call_arg_kinds(source, "has"), ["String", "String", "Variable"]);
}

#[test]
fn newline_between_bareword_and_qw_still_binds() {
    let source = "has\nqw(a b);";
    assert_clean_parse(source);
    assert_eq!(call_string_payloads(source, "has"), ["a", "b"]);
}

#[test]
fn parenthesized_expression_bare_call_also_flattens_immediate_qw() {
    let source = "(has qw(a b));";
    assert_clean_parse(source);
    assert_eq!(call_string_payloads(source, "has"), ["a", "b"]);
}

#[test]
fn builtin_print_qw_already_binds_and_stays_on_the_builtin_arg_path() {
    // `print` is a builtin, so it already consumed `qw` as one argument before
    // this claim. Flattening that ArrayLiteral would restripe a working shape.
    let source = "print qw(a b);";
    assert_clean_parse(source);
    assert_eq!(kind_names(source), ["ExpressionStatement"]);
    let kinds = call_arg_kinds(source, "print");
    assert_eq!(
        kinds,
        ["ArrayLiteral"],
        "print qw(a b) must remain one ArrayLiteral arg on the builtin path, got {kinds:?}"
    );
}

#[test]
fn qw_after_a_comma_is_not_the_immediate_bind_shape() {
    // `has 'a', qw(b c)` already bound before this claim because the first arg
    // is a string. Flattening that later qw would change an existing shape.
    let source = "has 'a', qw(b c);";
    assert_clean_parse(source);
    let kinds = call_arg_kinds(source, "has");
    assert_eq!(
        kinds.last().copied(),
        Some("ArrayLiteral"),
        "later qw after a comma must remain one ArrayLiteral arg, got {kinds:?}"
    );
}

#[test]
fn single_qw_word_still_binds() {
    let source = "has qw(only);";
    assert_clean_parse(source);
    assert_eq!(call_string_payloads(source, "has"), ["only"]);
}

#[test]
fn word_or_after_flattened_qw_stays_outside_the_call() {
    let source = "has qw(a b) or die;";
    assert_clean_parse(source);
    assert_eq!(call_arg_kinds(source, "has"), ["String", "String"]);
    assert_eq!(call_string_payloads(source, "has"), ["a", "b"]);
}
