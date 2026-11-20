//! #8316 — grammar-neutral CPAN Coro block-call parsing and recovery.
//!
//! Coro code uses imported functions with block arguments (`async { ... }`,
//! `Coro::async { ... }`, postfix args, and related forms). These must stay
//! ordinary function calls without Coro-specific AST nodes or global keywords.

mod cpan_test_helpers;

use cpan_test_helpers::{assert_clean_parse, parse, top_level_kinds};
use perl_lexer::{LEXER_KEYWORDS, is_lexer_keyword};
use perl_parser_core::{Node, NodeKind};
use proptest::prelude::*;

fn walk(node: &Node, visit: &mut impl FnMut(&Node)) {
    visit(node);
    for child in node.children() {
        walk(child, visit);
    }
}

fn source_text<'a>(source: &'a str, node: &Node) -> Option<&'a str> {
    source.get(node.location.start..node.location.end)
}

fn find_function_calls<'a>(node: &'a Node, name: &str, out: &mut Vec<&'a Node>) {
    if let NodeKind::FunctionCall { name: found, .. } = &node.kind
        && found == name
    {
        out.push(node);
    }
    for child in node.children() {
        find_function_calls(child, name, out);
    }
}

fn block_call_shape(source: &str, call: &Node) -> Result<(String, Vec<String>), String> {
    let NodeKind::FunctionCall { name, args } = &call.kind else {
        return Err(format!("expected FunctionCall, got {}", call.kind.kind_name()));
    };
    let mut arg_texts = Vec::new();
    for arg in args {
        arg_texts.push(
            source_text(source, arg)
                .ok_or_else(|| format!("arg missing source text for {name}"))?
                .to_string(),
        );
    }
    Ok((name.clone(), arg_texts))
}

fn assert_block_call(
    source: &str,
    expected_name: &str,
    expected_args: &[&str],
) -> Result<(), String> {
    let ast = parse(source);
    let mut calls = Vec::new();
    find_function_calls(&ast, expected_name, &mut calls);
    assert_eq!(
        calls.len(),
        1,
        "expected one `{expected_name}` call in:\n{source}\n{}",
        ast.to_sexp()
    );
    let (name, args) = block_call_shape(source, calls[0])?;
    assert_eq!(name, expected_name);
    assert_eq!(
        args,
        expected_args.iter().map(|s| (*s).to_string()).collect::<Vec<_>>(),
        "unexpected args for `{expected_name}` in:\n{source}"
    );
    Ok(())
}

fn find_named_sub<'a>(node: &'a Node, name: &str) -> Option<&'a Node> {
    if matches!(&node.kind, NodeKind::Subroutine { name: Some(found), .. } if found == name) {
        return Some(node);
    }
    node.children().into_iter().find_map(|child| find_named_sub(child, name))
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

fn assert_no_coro_specific_ast_nodes(source: &str) {
    let ast = parse(source);
    let sexp = ast.to_sexp().to_lowercase();
    for forbidden in ["corocall", "coroasync", "coroblock", "yieldstmt", "corosub"] {
        assert!(
            !sexp.contains(forbidden),
            "AST must not introduce Coro-specific node {forbidden:?} for:\n{source}\n{sexp}"
        );
    }
}

#[test]
fn focused_fixture_file_parses_coro_block_call_shapes() {
    let source = include_str!("../../../test_corpus/parser/coro_block_call_shapes.pl");
    assert_clean_parse(source);
    assert_no_coro_specific_ast_nodes(source);
}

#[test]
fn method_call_with_subref_argument_stays_valid_perl() -> Result<(), String> {
    let source = "$obj->async(sub { work() });";
    assert_clean_parse(source);
    let ast = parse(source);
    let mut method_calls = 0usize;
    walk(&ast, &mut |node| {
        if let NodeKind::MethodCall { method, args, .. } = &node.kind
            && method == "async"
        {
            method_calls += 1;
            assert_eq!(args.len(), 1, "subref must be the method argument");
        }
    });
    assert_eq!(method_calls, 1, "{}", ast.to_sexp());
    Ok(())
}

#[test]
fn nested_block_call_argument() -> Result<(), String> {
    let source = "async { async { inner() } };";
    let ast = parse(source);
    let mut calls = Vec::new();
    find_function_calls(&ast, "async", &mut calls);
    assert_eq!(calls.len(), 2, "nested block call must retain inner and outer calls");
    let NodeKind::Program { statements } = &ast.kind else {
        return Err("expected program".to_string());
    };
    let NodeKind::ExpressionStatement { expression } = &statements[0].kind else {
        return Err("expected outer expression".to_string());
    };
    let (name, args) = block_call_shape(source, expression)?;
    assert_eq!(name, "async");
    assert_eq!(args.len(), 1);
    assert_eq!(args[0], "{ async { inner() } }");
    Ok(())
}

#[test]
fn hypothetical_coro_sub_stays_unimplemented() {
    let source = "coro sub worker { 1 }";
    let ast = parse(source);
    let sexp = ast.to_sexp();
    assert!(
        !sexp.contains("CoroSub") && !sexp.contains("coro_sub"),
        "hypothetical core syntax must not introduce dedicated nodes: {sexp}"
    );
}

#[test]
fn async_lexer_keyword_mutation_guard() {
    assert!(
        !is_lexer_keyword("async"),
        "adding async to LEXER_KEYWORDS would break block-call parsing (#8316)"
    );
}

#[test]
fn imported_async_block_call_is_ordinary_function_call() -> Result<(), String> {
    assert_block_call("async { work() };", "async", &["{ work() }"])?;
    assert_no_coro_specific_ast_nodes("async { work() };");
    Ok(())
}

#[test]
fn async_block_with_postfix_argument() -> Result<(), String> {
    assert_block_call("async { work($_[0]) } $arg;", "async", &["{ work($_[0]) }", "$arg"])?;
    assert_no_coro_specific_ast_nodes("async { work($_[0]) } $arg;");
    Ok(())
}

#[test]
fn qualified_coro_async_block_call() -> Result<(), String> {
    assert_block_call("Coro::async { work() };", "Coro::async", &["{ work() }"])?;
    assert_no_coro_specific_ast_nodes("Coro::async { work() };");
    Ok(())
}

#[test]
fn async_pool_block_call() -> Result<(), String> {
    assert_block_call("async_pool { work() };", "async_pool", &["{ work() }"])?;
    assert_no_coro_specific_ast_nodes("async_pool { work() };");
    Ok(())
}

#[test]
fn assignment_from_async_block_call() -> Result<(), String> {
    let source = "my $worker = async { $self->_worker_loop($i) };";
    let ast = parse(source);
    assert_eq!(top_level_kinds(&ast), vec!["VariableDeclaration"], "{}", ast.to_sexp());
    let mut calls = Vec::new();
    find_function_calls(&ast, "async", &mut calls);
    assert_eq!(calls.len(), 1, "{}", ast.to_sexp());
    let (_, args) = block_call_shape(source, calls[0])?;
    assert_eq!(args.len(), 1);
    assert!(args[0].starts_with('{'), "first arg must be block: {:?}", args);
    assert_no_coro_specific_ast_nodes(source);
    Ok(())
}

#[test]
fn real_world_corpus_worker_spawn_snippet() -> Result<(), String> {
    let source = r#"my $worker = async {
                $self->_worker_loop($i);
            };"#;
    assert_clean_parse(source);
    assert_block_call(
        source,
        "async",
        &["{\n                $self->_worker_loop($i);\n            }"],
    )?;
    Ok(())
}

#[test]
fn coro_related_names_stay_ordinary_identifiers() -> Result<(), String> {
    assert_clean_parse("sub async { 1 }");
    assert_clean_parse("sub coro { 1 }");
    assert_clean_parse("sub yield { 1 }");
    assert_clean_parse("sub cede { 1 }");
    assert_clean_parse("sub schedule { 1 }");
    assert_clean_parse("package async;");
    assert_clean_parse("package coro;");
    assert_clean_parse("my %h = (async => 1, coro => 2, yield => 3);");
    assert_clean_parse("async: while (0) { last async; }");
    assert_clean_parse(r#"my $s = "async";"#);
    Ok(())
}

#[test]
fn coro_names_are_not_lexer_keywords() {
    for name in ["coro", "yield", "cede", "schedule", "async_pool"] {
        assert!(
            !is_lexer_keyword(name),
            "{name} must remain a bare identifier for block-call parsing (#8316)"
        );
        assert!(!LEXER_KEYWORDS.contains(&name), "{name} must not appear in LEXER_KEYWORDS");
    }
}

#[test]
fn core_syntax_guard_has_no_coro_specific_node_kinds() {
    let sample = "async { 1 }; Coro::async { 2 }; async_pool { 3 };";
    assert_no_coro_specific_ast_nodes(sample);
    let ast = parse(sample);
    let mut kind_names = Vec::new();
    walk(&ast, &mut |node| kind_names.push(node.kind.kind_name()));
    for kind in kind_names {
        assert!(
            !kind.contains("Coro") && !kind.eq_ignore_ascii_case("Yield"),
            "unexpected Coro-specific NodeKind {kind:?}"
        );
    }
}

#[test]
fn malformed_async_block_recovery_stays_bounded() -> Result<(), String> {
    let cases: &[(&str, bool)] = &[
        ("async { 1 };\n", true),
        ("async { foo() };\n", true),
        ("async { foo(\n", false),
        ("async {\n", false),
        ("Coro::async {\n", false),
    ];
    for (prefix, expect_later_sub) in cases {
        let trailing = "sub later_ok { 1 }\n";
        let combined = format!("{prefix}{trailing}");
        let ast = parse(&combined);
        let found = find_named_sub(&ast, "later_ok").is_some();
        assert_eq!(
            found,
            *expect_later_sub,
            "later_ok discoverability for prefix {prefix:?}:\n{combined}\n{}",
            ast.to_sexp()
        );
        let sexp = ast.to_sexp();
        if !*expect_later_sub {
            assert!(
                sexp.contains("ERROR") || sexp.contains("Missing"),
                "malformed prefix should leave a bounded error signal: {prefix:?}\n{sexp}"
            );
        }
    }
    Ok(())
}

#[test]
fn incomplete_async_block_does_not_swallow_following_statement() {
    let source = "async { foo() }\nmy $after = 1;\n";
    let ast = parse(source);
    assert!(
        find_variable(&ast, "$", "after").is_some(),
        "following statement must not be swallowed: {}",
        ast.to_sexp()
    );
}

proptest! {
    #[test]
    fn block_call_seeds_with_postfix_and_comments(
        body in r#"[a-z_]{2,8}"#,
        arg in r#"\$[a-z][a-z0-9_]{0,5}"#,
    ) {
        prop_assume!(body != "s" && body != "tr" && body != "m" && body != "q");
        let source = format!("async {{ {body}() }} {arg}; # trailing");
        prop_assume!(source.is_ascii());
        assert_block_call(&source, "async", &[&format!("{{ {body}() }}"), &arg])
            .map_err(TestCaseError::fail)?;
    }
}
