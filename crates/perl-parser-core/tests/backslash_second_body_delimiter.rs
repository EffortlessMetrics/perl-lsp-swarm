//! #18305: a backslash delimiter closes the second body instead of escaping it.
//! Native Perl 5.40.1 accepts these forms; ordinary escaped delimiters still escape.

use perl_lexer::{PerlLexer, TokenType};
use perl_parser_core::syntax::quote::{
    SubstitutionError, TransliterationError, extract_substitution_parts_strict,
    extract_transliteration_parts, extract_transliteration_parts_strict,
};
use perl_parser_core::{NodeKind, Parser};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn assert_parser_parts(
    operator: &str,
    search: &str,
    replacement: &str,
    modifiers: &str,
) -> TestResult {
    let source = format!("{operator}; my $after = 7;");
    let mut parser = Parser::new(&source);
    let ast = parser.parse()?;
    assert!(parser.errors().is_empty(), "{source:?}: {:?}", parser.errors());
    let NodeKind::Program { statements } = &ast.kind else {
        return Err("expected program".into());
    };
    let [first, following] = statements.as_slice() else {
        return Err(format!("lost neighboring declaration for {source:?}: {statements:?}").into());
    };
    let expression = match &first.kind {
        NodeKind::ExpressionStatement { expression } => expression.as_ref(),
        _ => first,
    };
    let actual = match &expression.kind {
        NodeKind::Substitution { pattern, replacement, modifiers, .. }
            if operator.starts_with('s') =>
        {
            (pattern.as_str(), replacement.as_str(), modifiers.as_str())
        }
        NodeKind::Transliteration { search, replace, modifiers, .. }
            if operator.starts_with("tr") || operator.starts_with('y') =>
        {
            (search.as_str(), replace.as_str(), modifiers.as_str())
        }
        _ => return Err(format!("wrong operator for {source:?}: {expression:?}").into()),
    };
    assert_eq!(actual, (search, replacement, modifiers), "{source:?}");
    assert_eq!(expression.location.start, 0, "{source:?}");
    assert_eq!(expression.location.end, operator.len(), "{source:?}");
    let NodeKind::VariableDeclaration { variable, initializer, .. } = &following.kind else {
        return Err(format!("lost following declaration for {source:?}: {following:?}").into());
    };
    assert!(matches!(&variable.kind, NodeKind::Variable { sigil, name }
        if sigil == "$" && name == "after"));
    assert!(matches!(initializer.as_deref().map(|node| &node.kind),
        Some(NodeKind::Number { value }) if value == "7"));
    assert_eq!(following.location.start, operator.len() + 2, "{source:?}");
    let variable_start = source.find("$after").ok_or("missing fixture variable")?;
    assert_eq!(variable.location.start, variable_start, "{source:?}");
    assert_eq!(variable.location.end, variable_start + "$after".len(), "{source:?}");
    Ok(())
}

fn assert_lexer_boundaries(prefix: &str, modifier: &str) -> TestResult {
    for bodies in [r"\a\b\", r"\a\\", r"\\\", r"{a}\b\", r"{a}\\"] {
        let operator = format!("{prefix}{bodies}{modifier}");
        let source = format!("{operator}; my $after = 7;");
        let mut lexer = PerlLexer::new(&source);
        let token = lexer.next_token().ok_or("missing operator token")?;
        assert_eq!(
            token.token_type,
            if prefix == "s" { TokenType::Substitution } else { TokenType::Transliteration },
            "{source:?}"
        );
        assert_eq!(token.text.as_ref(), operator, "{source:?}");
        assert_eq!((token.start, token.end), (0, operator.len()), "{source:?}");
        let separator = lexer.next_token().ok_or("missing neighboring semicolon")?;
        assert_eq!(separator.token_type, TokenType::Semicolon, "{source:?}");
        assert_eq!((separator.start, separator.end), (operator.len(), operator.len() + 1));
    }
    Ok(())
}

#[test]
fn substitution_lexer_replacement_closes_at_backslash() -> TestResult {
    assert_lexer_boundaries("s", "g")
}

#[test]
fn tr_lexer_replacement_closes_at_backslash() -> TestResult {
    assert_lexer_boundaries("tr", "d")
}

#[test]
fn y_lexer_replacement_closes_at_backslash() -> TestResult {
    assert_lexer_boundaries("y", "d")
}

fn assert_parser_boundaries(prefix: &str, modifier: &str) -> TestResult {
    for (bodies, search, replacement) in [
        (r"\a\b\", "a", "b"),
        (r"\a\\", "a", ""),
        (r"\\\", "", ""),
        (r"{a}\b\", "a", "b"),
        (r"{a}\\", "a", ""),
        (r#"{a}\"x\"#, "a", "\"x"),
    ] {
        assert_parser_parts(&format!("{prefix}{bodies}{modifier}"), search, replacement, modifier)?;
    }
    Ok(())
}

#[test]
fn substitution_parser_retains_payload_modifiers_and_neighbor() -> TestResult {
    assert_parser_boundaries("s", "g")
}

#[test]
fn tr_parser_retains_payload_modifiers_and_neighbor() -> TestResult {
    assert_parser_boundaries("tr", "d")
}

#[test]
fn y_parser_retains_payload_modifiers_and_neighbor() -> TestResult {
    assert_parser_boundaries("y", "d")
}

#[test]
fn backslash_closer_precedes_quoted_lookahead_into_neighboring_source() -> TestResult {
    for first_body in [r"s{a}\", r"s\a\"] {
        for quote in ['\'', '"'] {
            let operator = format!("{first_body}{quote}x\\");
            let valid_suffix = format!(" + \\\\{quote}y{quote}; my $after = 7;");
            let malformed_suffix = format!("\\y{quote}\\; my $after = 7;");
            for suffix in [&valid_suffix, &malformed_suffix] {
                let source = format!("{operator}{suffix}");
                let mut lexer = PerlLexer::new(&source);
                let token = lexer.next_token().ok_or("missing quoted replacement")?;
                assert_eq!(token.token_type, TokenType::Substitution, "{source:?}");
                assert_eq!(token.text.as_ref(), operator, "{source:?}");
                assert_eq!((token.start, token.end), (0, operator.len()), "{source:?}");
            }

            // Native Perl accepts this adjacent reference expression. Its quotes
            // and backslashes must not become part of the preceding substitution.
            let source = format!("{operator}{valid_suffix}");
            let mut parser = Parser::new(&source);
            let ast = parser.parse()?;
            assert!(parser.errors().is_empty(), "{source:?}: {:?}", parser.errors());
            let NodeKind::Program { statements } = &ast.kind else {
                return Err("expected program with adjacent expression".into());
            };
            let [first, following] = statements.as_slice() else {
                return Err(format!("lost adjacent source: {statements:?}").into());
            };
            let expression = match &first.kind {
                NodeKind::ExpressionStatement { expression } => expression.as_ref(),
                _ => first,
            };
            let NodeKind::Binary { op, left, .. } = &expression.kind else {
                return Err(format!("lost addition: {expression:?}").into());
            };
            assert_eq!(op, "+");
            assert!(matches!(&left.kind,
                NodeKind::Substitution { pattern, replacement, modifiers, .. }
                if pattern == "a" && replacement == &format!("{quote}x") && modifiers.is_empty()));
            assert_eq!((left.location.start, left.location.end), (0, operator.len()));
            assert!(matches!(&following.kind,
                NodeKind::VariableDeclaration { variable, .. }
                if matches!(&variable.kind, NodeKind::Variable { sigil, name }
                    if sigil == "$" && name == "after")));
            assert_eq!(following.location.start, source.find("my $after").ok_or("missing suffix")?);
        }
    }
    Ok(())
}

#[test]
fn strict_extractors_stop_at_backslash_before_modifiers() -> TestResult {
    for (bodies, search, replacement) in [
        (r"\a\b\", "a", "b"),
        (r"\a\\", "a", ""),
        (r"\\\", "", ""),
        (r"{a}\b\", "a", "b"),
        (r"{a}\\", "a", ""),
    ] {
        let substitution = format!("s{bodies}g");
        assert_eq!(
            extract_substitution_parts_strict(&substitution),
            Ok((search.into(), replacement.into(), "g".into())),
            "{substitution:?}"
        );
        for prefix in ["tr", "y"] {
            let transliteration = format!("{prefix}{bodies}d");
            assert_eq!(
                extract_transliteration_parts_strict(&transliteration),
                Ok((search.into(), replacement.into(), "d".into())),
                "{transliteration:?}"
            );
        }
    }
    Ok(())
}

#[test]
fn non_strict_transliteration_extractor_stops_at_backslash() {
    for prefix in ["tr", "y"] {
        for (bodies, search, replacement) in [
            (r"\a\b\", "a", "b"),
            (r"\a\\", "a", ""),
            (r"\\\", "", ""),
            (r"{a}\b\", "a", "b"),
            (r"{a}\\", "a", ""),
        ] {
            let operator = format!("{prefix}{bodies}d");
            assert_eq!(
                extract_transliteration_parts(&operator),
                (search.into(), replacement.into(), "d".into()),
                "{operator:?}"
            );
        }
    }
}

#[test]
fn ordinary_escaped_and_paired_delimiters_remain_intact() -> TestResult {
    for prefix in ["s", "tr", "y"] {
        for (bodies, search, replacement) in [
            (r"/a/b\/c/", "a", r"b\/c"),
            (r"/a\/b/c/", r"a\/b", "c"),
            (r"{a}{b\}c}", "a", r"b\}c"),
            (r"{a}/b\/c/", "a", r"b\/c"),
            (r"[a](b)", "a", "b"),
        ] {
            assert_parser_parts(&format!("{prefix}{bodies}"), search, replacement, "")?;
        }
    }
    assert_eq!(
        extract_transliteration_parts(r"tr/a/b\/c/d"),
        ("a".into(), r"b\/c".into(), "d".into())
    );
    Ok(())
}

#[test]
fn missing_replacement_closers_remain_errors() {
    for bodies in [r"\a\b", r"{a}\b", r"/a/b", r"{a}{b", r"{a}/b\"] {
        assert_eq!(
            extract_substitution_parts_strict(&format!("s{bodies}")),
            Err(SubstitutionError::MissingClosingDelimiter)
        );
        for prefix in ["tr", "y"] {
            assert_eq!(
                extract_transliteration_parts_strict(&format!("{prefix}{bodies}")),
                Err(TransliterationError::MissingClosingDelimiter)
            );
        }
        for prefix in ["s", "tr", "y"] {
            let source = format!("{prefix}{bodies}; my $after = 7;");
            let mut parser = Parser::new(&source);
            assert!(
                parser.parse().is_err() || !parser.errors().is_empty(),
                "unterminated replacement accepted: {source:?}"
            );
        }
    }
    assert_eq!(
        extract_substitution_parts_strict(r"s\a\"),
        Err(SubstitutionError::MissingReplacement)
    );
    assert_eq!(
        extract_transliteration_parts_strict(r"tr\a\"),
        Err(TransliterationError::MissingReplacement)
    );
}
