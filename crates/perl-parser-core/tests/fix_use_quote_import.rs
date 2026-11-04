mod cpan_test_helpers;
use cpan_test_helpers::*;

#[test]
fn test_use_overload_q_brace_multi_stmt() {
    assert_clean_parse(r#"use overload q{""} => sub { my $x = 1; $x };"#);
}

#[test]
fn test_use_overload_q_paren_multi_stmt() {
    assert_clean_parse(r#"use overload q("") => sub { my $x = 1; $x };"#);
}

#[test]
fn test_use_overload_q_bracket_multi_stmt() {
    assert_clean_parse(r#"use overload q[""] => sub { my $x = 1; $x };"#);
}

// Real-world from Regexp::Common
#[test]
fn test_regexp_common_pattern() {
    assert_clean_parse(
        r#"
use overload
    q{""} => sub {
        my ($self) = @_;
        my $pat = $self->{create}->($self, $self->{flags}, $self->{args});
        return $pat;
    };
"#,
    );
}

#[test]
fn use_quote_pair_values_preserve_spelling_and_reject_truncation() -> Result<(), String> {
    use perl_parser_core::{NodeKind, Parser};
    for quote in ["q(a)", "qq{b}", r"q(a\\)"] {
        let directive = format!("use Example 'key' => {quote}");
        let source = format!("{directive}; my $x = 1;");
        let mut parser = Parser::new(&source);
        let ast = parser.parse().map_err(|error| format!("{error:?}"))?;
        if !parser.errors().is_empty() {
            return Err(format!("{:?}", parser.errors()));
        }
        let NodeKind::Program { statements } = &ast.kind else {
            return Err(format!("{ast:?}"));
        };
        let [import, declaration] = statements.as_slice() else {
            return Err(format!("{statements:?}"));
        };
        if !matches!(&import.kind, NodeKind::Use { module, args, .. } if module == "Example" && args == &["'key'", quote])
            || import.location.start != 0
            || import.location.end != directive.len()
        {
            return Err(format!("quote pair ownership changed: {import:?}"));
        }
        let NodeKind::VariableDeclaration { variable, initializer, .. } = &declaration.kind else {
            return Err(format!("{declaration:?}"));
        };
        if !matches!(&variable.kind, NodeKind::Variable { sigil, name } if sigil == "$" && name == "x")
            || !matches!(initializer.as_deref().map(|node| &node.kind), Some(NodeKind::Number { value }) if value == "1")
        {
            return Err(format!("following declaration changed: {declaration:?}"));
        }
    }
    for source in [r"use Example 'key' => q(a\)", r"use Example 'key' => qq/a\/"] {
        let mut parser = Parser::new(source);
        if parser.parse().is_ok() && parser.errors().is_empty() {
            return Err(format!("malformed quote pair value was clean: {source:?}"));
        }
    }
    Ok(())
}
