mod cpan_test_helpers;
use cpan_test_helpers::*;
use perl_parser_core::{Node, NodeKind, Parser};

fn no_with_following_declaration(
    directive: &str,
    module: &str,
    expected_args: &[&str],
) -> Result<(), String> {
    let source = format!("{directive}; my $x = 1;");
    let mut parser = Parser::new(&source);
    let ast = parser.parse().map_err(|error| format!("{source:?}: {error:?}"))?;
    if !parser.errors().is_empty() {
        return Err(format!("{source:?}: {:?}", parser.errors()));
    }
    let NodeKind::Program { statements } = &ast.kind else {
        return Err(format!("expected Program: {ast:?}"));
    };
    let [directive_node, declaration] = statements.as_slice() else {
        return Err(format!("expected exactly No and declaration for {source:?}: {statements:?}"));
    };
    if !matches!(&directive_node.kind, NodeKind::No { module: actual, args, .. }
        if actual == module && args == expected_args)
    {
        return Err(format!("incorrect directive ownership for {source:?}: {directive_node:?}"));
    }
    if directive_node.location.start != 0 || directive_node.location.end != directive.len() {
        return Err(format!("incomplete No span for {source:?}: {:?}", directive_node.location));
    }
    check_x_declaration(declaration)
}

fn check_x_declaration(node: &Node) -> Result<(), String> {
    let NodeKind::VariableDeclaration { declarator, variable, initializer, .. } = &node.kind else {
        return Err(format!("expected following declaration: {node:?}"));
    };
    if declarator != "my"
        || !matches!(&variable.kind, NodeKind::Variable { sigil, name } if sigil == "$" && name == "x")
        || !matches!(initializer.as_deref().map(|node| &node.kind), Some(NodeKind::Number { value }) if value == "1")
    {
        return Err(format!("following declaration changed: {node:?}"));
    }
    Ok(())
}

#[test]
fn no_qw_owns_arguments_and_preserves_next_statement() -> Result<(), String> {
    for words in [
        "qw(uninitialized numeric)",
        "qw[uninitialized numeric]",
        "qw{uninitialized numeric}",
        "qw<uninitialized numeric>",
        "qw/uninitialized numeric/",
        "qw|uninitialized numeric|",
        "qw!uninitialized numeric!",
        "qw#uninitialized numeric#",
        "qw (uninitialized numeric)",
        "qw [uninitialized numeric]",
        "qw\t{uninitialized numeric}",
        "qw\n<uninitialized numeric>",
        "qw # delimiter follows comment\n(uninitialized numeric)",
        "qw(uninitialized\nnumeric)",
        "qw(uninitialized\r\nnumeric)",
    ] {
        no_with_following_declaration(
            &format!("no warnings {words}"),
            "warnings",
            &["qw(uninitialized numeric)"],
        )?;
    }
    no_with_following_declaration("no warnings qw()", "warnings", &["qw()"])?;
    no_with_following_declaration("no warnings qw []", "warnings", &["qw()"])?;
    no_with_following_declaration(
        "no Example qw [foo #tag bar]",
        "Example",
        &["qw(foo #tag bar)"],
    )?;
    no_with_following_declaration(r"no Example qw(a\) b)", "Example", &[r"qw(a\) b)"])?;
    no_with_following_declaration(r"no Example qw(a\\)", "Example", &[r"qw(a\\)"])?;
    no_with_following_declaration(r"no Example qw/a\/ b/", "Example", &[r"qw(a\/ b)"])?;
    no_with_following_declaration("no Example qw((a b))", "Example", &["qw((a b))"])?;
    no_with_following_declaration(
        "no warnings 'once', qw(uninitialized numeric)",
        "warnings",
        &["'once'", "qw(uninitialized numeric)"],
    )
}

#[test]
fn no_qw_pair_values_remain_inside_directive() -> Result<(), String> {
    // Fat arrow is a comma in Perl: a trailing arrow is not a missing-value error.
    no_with_following_declaration("no Example qw(a) =>", "Example", &["qw(a)"])?;
    no_with_following_declaration("no Example qw(a b) => 1", "Example", &["qw(a b)", "1"])?;
    no_with_following_declaration(
        "no Example qw(a b) => { key => [1, 2] }, 'tail'",
        "Example",
        &["qw(a b)", "{", "key", "=>", "[", "1", ",", "2", "]", "}", "'tail'"],
    )?;
    no_with_following_declaration(
        "no Example qw(a) => sub { 1; 2; }, qw(b)",
        "Example",
        &["qw(a)", "sub", "{", "1", ";", "2", ";", "}", "qw(b)"],
    )
}

#[test]
fn no_argument_ownership_opposite_controls() -> Result<(), String> {
    no_with_following_declaration("no warnings 'all'", "warnings", &["'all'"])?;
    no_with_following_declaration(
        "no warnings ('uninitialized', 'numeric')",
        "warnings",
        &["'uninitialized'", ",", "'numeric'"],
    )?;
    no_with_following_declaration(
        "no if 1, warnings, 'numeric'",
        "if",
        &["1", "warnings", "'numeric'"],
    )?;
    let source = "no warnings; qw(uninitialized numeric); my $x = 1;";
    let mut parser = Parser::new(source);
    let ast = parser.parse().map_err(|error| format!("{error:?}"))?;
    if !parser.errors().is_empty() {
        return Err(format!("{:?}", parser.errors()));
    }
    let NodeKind::Program { statements } = &ast.kind else {
        return Err(format!("{ast:?}"));
    };
    let [no, words, declaration] = statements.as_slice() else {
        return Err(format!("{statements:?}"));
    };
    if !matches!(&no.kind, NodeKind::No { args, .. } if args.is_empty())
        || !matches!(&words.kind, NodeKind::ExpressionStatement { expression }
            if matches!(&expression.kind, NodeKind::ArrayLiteral { elements } if elements.len() == 2))
    {
        return Err(format!("semicolon ownership changed: {statements:?}"));
    }
    check_x_declaration(declaration)
}

#[test]
fn no_qw_truncated_delimiter_reports_error_and_recovers() -> Result<(), String> {
    for newline in ["\n", "\r\n"] {
        let source = format!("no warnings qw(uninitialized{newline}my $x = 1;");
        let mut parser = Parser::new(&source);
        let ast = parser.parse().map_err(|error| format!("{source:?}: {error:?}"))?;
        if parser.errors().is_empty() {
            return Err(format!("malformed source was clean: {source:?}"));
        }
        let NodeKind::Program { statements } = &ast.kind else {
            return Err(format!("{ast:?}"));
        };
        let last = statements.last().ok_or("lost all statements")?;
        check_x_declaration(last)?;
    }
    for source in [
        "no warnings qw(",
        "no warnings qw[uninitialized",
        "no warnings qw/uninitialized",
        r"no warnings qw(uninitialized\)",
        r"no warnings qw(uninitialized\\\)",
        r"no warnings qw/uninitialized\/",
        "no warnings qw((uninitialized numeric)",
    ] {
        let mut parser = Parser::new(source);
        let result = parser.parse();
        if result.is_ok() && parser.errors().is_empty() {
            return Err(format!("truncated source was clean: {source:?}"));
        }
    }
    // A final semicolon is optional; it must not be our malformed-input oracle.
    let mut parser = Parser::new("no warnings qw(uninitialized numeric)");
    let ast = parser.parse().map_err(|error| format!("{error:?}"))?;
    if !parser.errors().is_empty() {
        return Err(format!("{:?}", parser.errors()));
    }
    let NodeKind::Program { statements } = &ast.kind else {
        return Err(format!("{ast:?}"));
    };
    if !matches!(statements.as_slice(), [Node { kind: NodeKind::No { args, .. }, .. }] if args == &["qw(uninitialized numeric)"])
    {
        return Err(format!("lost valid EOF directive arguments: {statements:?}"));
    }
    Ok(())
}

// Tests for issue #2184: parse_no had a restrictive import item parser that
// rejected valid Perl. The fix applies the same depth-tracking slurp loop
// that parse_use uses to parse parenthesised argument lists.

#[test]
fn test_no_module_with_scalar_var_in_parens() {
    // `no MyModule ($var, 0)` — sigil-prefixed variable rejected by old code
    let source = r#"no MyModule ($var, 0);"#;
    assert_clean_parse(source);
}

#[test]
fn test_no_overload_with_backslash_ref() {
    // `no overload ('==' => \&func)` — backslash ref rejected by old code
    let source = r#"no overload ('==' => \&func);"#;
    assert_clean_parse(source);
}

#[test]
fn test_no_overload_multiple_ops() {
    let source = r#"no overload '+', '-', '*';"#;
    assert_clean_parse(source);
}

#[test]
fn test_no_overload_bare_cmp_key() {
    let source = r#"
no overload
    '""' => 'type'
  , cmp  => 'cmp';
"#;
    assert_clean_parse(source);
}

#[test]
fn test_no_feature_nested_parens() {
    // Nested parens inside the arg list must be depth-tracked
    let source = r#"no feature ('say', 'state');"#;
    assert_clean_parse(source);
}

#[test]
fn test_no_warnings_single_string() {
    let source = r#"no warnings 'all';"#;
    assert_clean_parse(source);
}

#[test]
fn test_no_strict_refs() {
    let source = r#"no strict 'refs';"#;
    assert_clean_parse(source);
}

#[test]
fn test_no_warnings_multiple_args_in_parens() {
    let source = r#"no warnings ('experimental', 'uninitialized');"#;
    assert_clean_parse(source);
}

#[test]
fn test_no_overload_fat_arrow_with_coderef() {
    // coderef value after fat arrow should be slurped without error
    let source = r#"no overload '""' => \&to_string;"#;
    assert_clean_parse(source);
}

#[test]
fn no_q_quotes_own_complete_spelling_and_following_statement() -> Result<(), String> {
    for quote in [
        "q(uninitialized)",
        "qq(uninitialized)",
        "q [uninitialized]",
        "qq{numeric}",
        "q<once>",
        "q#once#",
        "qq!once!",
        "q/uninitialized/",
        "qq|numeric|",
        "q()",
        "qq []",
        "q((nested))",
        "qq{a{b}c}",
        r"q(a\\)",
        r"q(a\)b)",
        r"qq/a\/b/",
        "q(λ雪)",
        "q # comment\n(once)",
        "qq # comment\r\n{numeric}",
        "qq(uninitialized\nnumeric)",
        "q(uninitialized\r\nnumeric)",
    ] {
        no_with_following_declaration(&format!("no Example {quote}"), "Example", &[quote])?;
    }
    no_with_following_declaration(
        "no Example 'once', q(uninitialized), qq(numeric), qw(foo bar)",
        "Example",
        &["'once'", "q(uninitialized)", "qq(numeric)", "qw(foo bar)"],
    )
}

#[test]
fn no_q_quotes_keep_nested_pair_values() -> Result<(), String> {
    no_with_following_declaration("no Example q(key) =>", "Example", &["q(key)"])?;
    no_with_following_declaration(
        "no Example q(key) => { qq(nested) => [1, 2] }, qq(tail)",
        "Example",
        &["q(key)", "{", "qq(nested)", "=>", "[", "1", ",", "2", "]", "}", "qq(tail)"],
    )?;
    no_with_following_declaration(
        "no Example qq(key) => sub { 1; 2; }, q(tail)",
        "Example",
        &["qq(key)", "sub", "{", "1", ";", "2", ";", "}", "q(tail)"],
    )
}

#[test]
fn no_q_quotes_reject_truncated_tokens() -> Result<(), String> {
    for source in [
        "no Example q(",
        "no Example qq[abc",
        r"no Example q(a\)",
        r"no Example qq/a\/",
        "no Example q((nested)",
    ] {
        let mut parser = Parser::new(source);
        let result = parser.parse();
        if result.is_ok() && parser.errors().is_empty() {
            return Err(format!("truncated source was clean: {source:?}"));
        }
    }
    Ok(())
}

#[test]
fn no_q_quote_boundaries_and_existing_argument_routes() -> Result<(), String> {
    no_with_following_declaration("no Example (q(a), qq(b))", "Example", &["q(a)", ",", "qq(b)"])?;
    no_with_following_declaration(
        "no if 1, Example, q(a), qq(b)",
        "if",
        &["1", "Example", "q(a)", "qq(b)"],
    )?;
    for quote in ["q(a)", "qq(b)"] {
        let source = format!("no Example; {quote}; my $x = 1;");
        let mut parser = Parser::new(&source);
        let ast = parser.parse().map_err(|error| format!("{source:?}: {error:?}"))?;
        if !parser.errors().is_empty() {
            return Err(format!("{:?}", parser.errors()));
        }
        let NodeKind::Program { statements } = &ast.kind else {
            return Err(format!("{ast:?}"));
        };
        let [no, string, declaration] = statements.as_slice() else {
            return Err(format!("{statements:?}"));
        };
        if !matches!(&no.kind, NodeKind::No { args, .. } if args.is_empty())
            || !matches!(&string.kind, NodeKind::ExpressionStatement { expression }
                if matches!(&expression.kind, NodeKind::String { value, .. } if value == quote))
        {
            return Err(format!("semicolon boundary changed: {statements:?}"));
        }
        check_x_declaration(declaration)?;
        let source = format!("no Example {quote}");
        let mut parser = Parser::new(&source);
        let ast = parser.parse().map_err(|error| format!("{error:?}"))?;
        let NodeKind::Program { statements } = &ast.kind else {
            return Err(format!("{ast:?}"));
        };
        if !parser.errors().is_empty()
            || !matches!(statements.as_slice(),
            [Node { kind: NodeKind::No { args, .. }, location, .. }] if args == &[quote] && location.end == source.len())
        {
            return Err(format!("valid EOF quote lost: {ast:?}, {:?}", parser.errors()));
        }
    }
    Ok(())
}

#[test]
fn no_q_quotes_in_raw_argument_routes_reject_truncation() -> Result<(), String> {
    let mut missed = Vec::new();
    for source in [
        r"no Example q(key) => q(a\)",
        r"no Example qq(key) => { nested => qq/a\/",
        r"no Example (q(a\)",
        r"no Example 'key' => q(a\)",
        r"no if 1, Example, q(a\)",
    ] {
        let mut parser = Parser::new(source);
        if parser.parse().is_ok() && parser.errors().is_empty() {
            missed.push(source);
        }
    }
    if !missed.is_empty() {
        return Err(format!("malformed quotes reported clean: {missed:?}"));
    }
    Ok(())
}
