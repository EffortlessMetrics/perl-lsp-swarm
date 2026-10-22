use perl_lexer::{PerlLexer, TokenType};

type R = Result<(), Box<dyn std::error::Error>>;

fn assert_spans_are_valid(input: &str) {
    let mut lexer = PerlLexer::new(input);
    let mut token_count = 0usize;

    while let Some(token) = lexer.next_token() {
        assert!(input.is_char_boundary(token.start), "token.start must be a char boundary");
        assert!(input.is_char_boundary(token.end), "token.end must be a char boundary");
        assert!(token.start <= token.end, "token start must not exceed end");
        assert!(token.end <= input.len(), "token end must be within the input");
        assert_eq!(
            token.text.as_ref(),
            &input[token.start..token.end],
            "token text must match its byte span"
        );

        token_count += 1;
        assert!(token_count <= 64, "lexer should terminate for short regression cases");

        if matches!(token.token_type, TokenType::EOF) {
            break;
        }
    }
}

#[test]
fn unicode_and_bom_prefixes_do_not_panic_and_have_valid_spans() -> R {
    let cases = [
        "¡<<'",             // original regression shape
        "\u{FEFF}my $x=1;", // UTF-8 BOM at file start
        "👩‍💻<<'END'",        // ZWJ emoji prefix before heredoc-like bytes
        "🏳️‍🌈q'abc'",         // variation selector + joiner before quote-like bytes
        "🧑🏽‍💻\"x\"",          // skin tone modifier + ZWJ
    ];

    for case in cases {
        assert_spans_are_valid(case);
    }

    Ok(())
}

#[test]
fn unicode_regression_case_no_panic() -> R {
    let input = "¡<<'";
    let mut lexer = PerlLexer::new(input);
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| lexer.next_token()));
    assert!(result.is_ok(), "Lexer should not panic on Unicode input: {input:?}");
    Ok(())
}
// #18307: the byte before `*` can be inside a Unicode operand. These
// expectations describe the complete public token stream, including recovery.
fn assert_typeglob_guard_tokens(input: &str, expected: &[(TokenType, &str)]) -> R {
    let mut lexer = PerlLexer::new(input);
    let mut offset = 0;
    for (kind, text) in expected {
        while input.as_bytes().get(offset).is_some_and(u8::is_ascii_whitespace) {
            offset += 1;
        }
        let token = lexer.next_token().ok_or_else(|| format!("missing {text:?} in {input:?}"))?;
        assert_eq!(&token.token_type, kind, "{input:?}: {token:?}");
        assert_eq!(token.text.as_ref(), *text, "{input:?}");
        assert_eq!((token.start, token.end), (offset, offset + text.len()), "{input:?}");
        assert!(input.is_char_boundary(token.start) && input.is_char_boundary(token.end));
        assert_eq!(input.get(token.start..token.end), Some(*text));
        offset += text.len();
    }
    while input.as_bytes().get(offset).is_some_and(u8::is_ascii_whitespace) {
        offset += 1;
    }
    assert_eq!(offset, input.len(), "expectations must cover {input:?}");
    let eof = lexer.next_token();
    assert!(matches!(eof.as_ref().map(|t| &t.token_type), Some(TokenType::EOF)));
    if let Some(eof) = eof {
        assert_eq!((eof.start, eof.end, eof.text.as_ref()), (input.len(), input.len(), ""));
    }
    assert!(lexer.next_token().is_none(), "EOF must be terminal for {input:?}");
    Ok(())
}

fn semicolon_quote_kind(quote: char) -> TokenType {
    match quote {
        '"' => TokenType::InterpolatedString(vec![perl_lexer::StringPart::Literal(";".into())]),
        '\'' => TokenType::StringLiteral,
        _ => TokenType::QuoteCommand,
    }
}

#[test]
fn unicode_operands_before_multiplication_preserve_semicolon_strings() -> R {
    // Perl 5.40.1 accepts these names with `use utf8`, including a
    // four-byte letter and continuation-only combining mark / digit tails.
    for name in ["$a", "$é", "$变量", "$𐐀", "$a\u{0301}", "$a١", "$Foo::é", "é"] {
        for quote in ['"', '\'', '`'] {
            for gap in [" ", "", "\t", "\r\n"] {
                let quoted = format!("{quote};{quote}");
                let input = format!("{name}{gap}*{gap}{quoted};my $after=1;");
                assert_typeglob_guard_tokens(
                    &input,
                    &[
                        (TokenType::Identifier(name.into()), name),
                        (TokenType::Operator("*".into()), "*"),
                        (semicolon_quote_kind(quote), &quoted),
                        (TokenType::Semicolon, ";"),
                        (TokenType::Keyword("my".into()), "my"),
                        (TokenType::Identifier("$after".into()), "$after"),
                        (TokenType::Operator("=".into()), "="),
                        (TokenType::Number("1".into()), "1"),
                        (TokenType::Semicolon, ";"),
                    ],
                )?;
            }
        }
    }
    Ok(())
}

#[test]
fn ordinary_operand_endings_before_multiplication_stay_operands() -> R {
    for prefix in [
        vec![(TokenType::Number("1".into()), "1")],
        vec![
            (TokenType::LeftParen, "("),
            (TokenType::Identifier("$a".into()), "$a"),
            (TokenType::RightParen, ")"),
        ],
        vec![
            (TokenType::Identifier("$a".into()), "$a"),
            (TokenType::LeftBracket, "["),
            (TokenType::Number("0".into()), "0"),
            (TokenType::RightBracket, "]"),
        ],
        vec![(TokenType::StringLiteral, "'x'")],
    ] {
        let operand: String = prefix.iter().map(|(_, text)| *text).collect();
        for quote in ['"', '\'', '`'] {
            let quoted = format!("{quote};{quote}");
            let input = format!("{operand}*{quoted};");
            let mut expected = prefix.clone();
            expected.extend([
                (TokenType::Operator("*".into()), "*"),
                (semicolon_quote_kind(quote), &quoted),
                (TokenType::Semicolon, ";"),
            ]);
            assert_typeglob_guard_tokens(&input, &expected)?;
        }
    }
    Ok(())
}

#[test]
fn punctuation_typeglobs_keep_the_quote_and_adjacent_statement() -> R {
    for alias in ["*alias", "*é"] {
        for quote in ['"', '\'', '`'] {
            let punctuation = quote.to_string();
            let input = format!("{alias} = *{quote};my $after=1;");
            assert_typeglob_guard_tokens(
                &input,
                &[
                    (TokenType::Identifier(alias.into()), alias),
                    (TokenType::Operator("=".into()), "="),
                    (TokenType::Identifier("*".into()), "*"),
                    (TokenType::Operator(punctuation.as_str().into()), &punctuation),
                    (TokenType::Semicolon, ";"),
                    (TokenType::Keyword("my".into()), "my"),
                    (TokenType::Identifier("$after".into()), "$after"),
                    (TokenType::Operator("=".into()), "="),
                    (TokenType::Number("1".into()), "1"),
                    (TokenType::Semicolon, ";"),
                ],
            )?;
        }
    }
    Ok(())
}

#[test]
fn malformed_multiplication_quotes_retain_line_bounded_errors() -> R {
    use perl_parser_core::{TokenKind, TokenStream};
    for name in ["$a", "$é", "$a\u{0301}"] {
        for quote in ['"', '\'', '`'] {
            let unclosed = format!("{quote};");
            for tail in ["", "\nmy $after=1;"] {
                let input = format!("{name} * {unclosed}{tail}");
                let mut expected = vec![
                    (TokenType::Identifier(name.into()), name),
                    (TokenType::Operator("*".into()), "*"),
                    (TokenType::Error("unterminated string".into()), unclosed.as_str()),
                ];
                if !tail.is_empty() {
                    expected.extend([
                        (TokenType::Keyword("my".into()), "my"),
                        (TokenType::Identifier("$after".into()), "$after"),
                        (TokenType::Operator("=".into()), "="),
                        (TokenType::Number("1".into()), "1"),
                        (TokenType::Semicolon, ";"),
                    ]);
                }
                assert_typeglob_guard_tokens(&input, &expected)?;
                let mut stream = TokenStream::new(&input);
                assert_eq!(stream.next()?.text.as_ref(), name);
                assert_eq!(stream.next()?.text.as_ref(), "*");
                let error = stream.next()?;
                assert_eq!(error.kind(), TokenKind::Unknown);
                assert_eq!(error.text.as_ref(), unclosed);
                assert_eq!((error.start(), error.end()), (name.len() + 3, name.len() + 5));
                if !tail.is_empty() {
                    assert_eq!(stream.next()?.text.as_ref(), "my");
                }
            }
        }
    }
    Ok(())
}

#[test]
fn unicode_multiplication_reaches_the_parser_without_recovery() -> R {
    use perl_parser_core::{Node, NodeKind, Parser};
    fn count_multiplication(node: &Node) -> usize {
        assert!(!matches!(node.kind, NodeKind::Error { .. }));
        usize::from(matches!(&node.kind, NodeKind::Binary { op, .. } if op == "*"))
            + node.children().into_iter().map(count_multiplication).sum::<usize>()
    }
    for name in ["$a", "$é", "$变量", "$𐐀", "$a\u{0301}"] {
        for quote in ['"', '\'', '`'] {
            let input = format!("use utf8;my $x={name}*{quote};{quote};my $after=1;");
            let mut parser = Parser::new(&input);
            let ast = parser.parse()?;
            assert!(parser.errors().is_empty(), "{input:?}: {:?}", parser.errors());
            assert_eq!(count_multiplication(&ast), 1, "{input:?}");
            assert_eq!(ast.children().len(), 3, "both declarations must survive: {input:?}");
        }
    }
    Ok(())
}
