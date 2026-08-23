//! Lexer-owned contract for heredoc openers, queued bodies, and resumption.
//!
//! The public token stream currently exposes the whole opener token and a body
//! event with authoritative `start..end` geometry. Quoting/interpolation/command
//! metadata is not yet carried by `HeredocSpec`; #6727 owns that representation
//! follow-up. Ordinary trailing-whitespace terminator rejection also remains a
//! named production gap rather than a passing conformance row in this suite.

use std::fmt::Debug;

use perl_lexer::{PerlLexer, Token, TokenType};

type R<T = ()> = Result<T, Box<dyn std::error::Error>>;

fn missing(message: impl Into<String>) -> Box<dyn std::error::Error> {
    std::io::Error::other(message.into()).into()
}

fn require(condition: bool, message: impl Into<String>) -> R {
    if condition { Ok(()) } else { Err(missing(message)) }
}

fn require_eq<T>(actual: &T, expected: &T, context: impl Into<String>) -> R
where
    T: Debug + PartialEq + ?Sized,
{
    if actual == expected {
        Ok(())
    } else {
        Err(missing(format!("{}: expected {expected:?}, got {actual:?}", context.into())))
    }
}

fn next(lexer: &mut PerlLexer<'_>, message: &'static str) -> R<Token> {
    lexer.next_token().ok_or_else(|| missing(message))
}

fn collect_remaining(lexer: &mut PerlLexer<'_>) -> Vec<Token> {
    let mut tokens = Vec::new();
    while let Some(token) = lexer.next_token() {
        let eof = matches!(&token.token_type, TokenType::EOF);
        tokens.push(token);
        if eof {
            break;
        }
    }
    tokens
}

fn body_tokens(tokens: &[Token]) -> Vec<&Token> {
    tokens.iter().filter(|token| matches!(&token.token_type, TokenType::HeredocBody(_))).collect()
}

fn body_slices<'a>(source: &'a str, tokens: &[Token]) -> Vec<&'a str> {
    body_tokens(tokens).into_iter().filter_map(|token| source.get(token.start..token.end)).collect()
}

fn token_kind(token_type: &TokenType) -> &'static str {
    match token_type {
        TokenType::Keyword(_) => "Keyword",
        TokenType::Identifier(_) => "Identifier",
        TokenType::Number(_) => "Number",
        TokenType::Operator(_) => "Operator",
        TokenType::StringLiteral | TokenType::InterpolatedString(_) => "StringLiteral",
        TokenType::Semicolon => "Semicolon",
        TokenType::Comma => "Comma",
        TokenType::LeftParen => "LeftParen",
        TokenType::RightParen => "RightParen",
        TokenType::EOF => "EOF",
        _ => "other",
    }
}

fn require_token_at<'a>(tokens: &'a [Token], start: usize, text: &str) -> R<&'a Token> {
    tokens
        .iter()
        .find(|token| token.start == start && token.text.as_ref() == text)
        .ok_or_else(|| missing(format!("missing token {text:?} at byte {start}")))
}

fn assert_clean_terminal(tokens: &[Token]) -> R {
    require(
        tokens.iter().all(|token| !token.token_type.is_recovery_token()),
        "token stream contains a recovery token",
    )?;
    require_eq(
        &tokens.iter().filter(|token| matches!(&token.token_type, TokenType::EOF)).count(),
        &1,
        "terminal EOF count",
    )?;
    let terminal = tokens.last().ok_or_else(|| missing("token stream is empty"))?;
    require(matches!(&terminal.token_type, TokenType::EOF), "last token is not EOF")
}

fn assert_eof_at_source_end(source: &str, tokens: &[Token]) -> R {
    let eof = tokens.last().ok_or_else(|| missing("token stream must end with EOF"))?;
    require(matches!(&eof.token_type, TokenType::EOF), "token stream does not end with EOF")?;
    require_eq(&(eof.start, eof.end), &(source.len(), source.len()), "EOF geometry")
}

fn assert_exact_segment(
    source: &str,
    tokens: &[&Token],
    expected: &[(&str, &str, usize, usize)],
) -> R {
    require_eq(&tokens.len(), &expected.len(), "exact segment token count")?;
    for (token, &(text, kind, start, end)) in tokens.iter().zip(expected) {
        require_eq(token.text.as_ref(), text, format!("token text at byte {start}"))?;
        require_eq(token_kind(&token.token_type), kind, format!("token kind for {text:?}"))?;
        require_eq(
            &(token.start, token.end),
            &(start, end),
            format!("token geometry for {text:?}"),
        )?;
        require_eq(&source.get(start..end), &Some(text), format!("source slice for {text:?}"))?;
    }
    Ok(())
}

fn assert_token_projection(source: &str, tokens: &[&Token], expected: &[(&str, &str)]) -> R {
    require_eq(&tokens.len(), &expected.len(), "token projection length")?;
    let mut cursor = 0;
    for (token, &(text, kind)) in tokens.iter().zip(expected) {
        let relative_start = source
            .get(cursor..)
            .and_then(|suffix| suffix.find(text))
            .ok_or_else(|| missing(format!("missing projected source text {text:?}")))?;
        let start = cursor + relative_start;
        let end = start + text.len();
        require_eq(token.text.as_ref(), text, format!("projected token text at byte {start}"))?;
        require_eq(
            token_kind(&token.token_type),
            kind,
            format!("projected token kind for {text:?}"),
        )?;
        require_eq(
            &(token.start, token.end),
            &(start, end),
            format!("projected token geometry for {text:?}"),
        )?;
        require_eq(
            &source.get(start..end),
            &Some(text),
            format!("projected source slice for {text:?}"),
        )?;
        cursor = end;
    }
    Ok(())
}

fn assert_assignment_at(
    source: &str,
    tokens: &[Token],
    start: usize,
    variable: &str,
    value: &str,
) -> R {
    let variable_text = format!("${variable}");
    let variable_start = start + 3;
    let variable_end = variable_start + variable_text.len();
    let value_start = variable_end + 3;
    let value_end = value_start + value.len();
    let statement_end = value_end + 1;
    let expected = [
        ("my", "Keyword", start, start + 2),
        (variable_text.as_str(), "Identifier", variable_start, variable_end),
        ("=", "Operator", variable_end + 1, variable_end + 2),
        (value, "Number", value_start, value_end),
        (";", "Semicolon", value_end, statement_end),
    ];
    let statement = tokens
        .iter()
        .filter(|token| {
            token.start >= start && token.end <= statement_end && token.start < token.end
        })
        .collect::<Vec<_>>();
    assert_exact_segment(source, &statement, &expected)
}

fn assert_body_contract(
    source: &str,
    tokens: &[Token],
    body: &Token,
    body_start: usize,
    expected_body: &str,
    terminator_line: &str,
    trailing_start: usize,
) -> R {
    require_eq(&body.start, &body_start, "heredoc body start")?;
    require_eq(&body.end, &(body_start + expected_body.len()), "heredoc body end")?;
    require_eq(
        &source.get(body.start..body.end),
        &Some(expected_body),
        "heredoc body source slice",
    )?;
    require(
        source.is_char_boundary(body.start) && source.is_char_boundary(body.end),
        "heredoc body range is not on UTF-8 boundaries",
    )?;
    require_eq(
        &source.get(body.end..trailing_start),
        &Some(terminator_line),
        "heredoc terminator source slice",
    )?;
    require_eq(
        &(body.end + terminator_line.len()),
        &trailing_start,
        "heredoc terminator boundary",
    )?;
    assert_eof_at_source_end(source, tokens)
}

#[test]
fn opener_forms_prove_geometry_queue_label_body_and_terminal_state() -> R {
    let cases = [
        ("<<EOF", "EOF", "body\n"),
        ("<<'EOF'", "EOF", "body\n"),
        ("<<\"EOF\"", "EOF", "body\n"),
        ("<<`CMD`", "CMD", "body\n"),
        ("<<~END", "END", "  body\n"),
        ("<<\\RAW", "RAW", "body\n"),
        ("<<Δ", "Δ", "body\n"),
    ];

    for (opener, terminator, expected_body) in cases {
        let source = if opener.starts_with("<<~") {
            format!("{opener};\n  body\n  {terminator}\nmy $x = 1;\n")
        } else {
            format!("{opener};\nbody\n{terminator}\nmy $x = 1;\n")
        };
        let mut lexer = PerlLexer::with_body_tokens(&source);
        let opener_token = next(&mut lexer, "missing heredoc opener token")?;

        require(
            matches!(&opener_token.token_type, TokenType::HeredocStart),
            format!("{opener:?} must produce HeredocStart, got {:?}", opener_token.token_type),
        )?;
        require_eq(opener_token.text.as_ref(), opener, "heredoc opener text")?;
        require_eq(
            &(opener_token.start, opener_token.end),
            &(0, opener.len()),
            "heredoc opener geometry",
        )?;
        require_eq(
            &source.get(opener_token.start..opener_token.end),
            &Some(opener),
            "heredoc opener source slice",
        )?;
        require(source.is_char_boundary(opener_token.end), "opener ends inside UTF-8")?;

        let suffix = collect_remaining(&mut lexer);
        let bodies = body_tokens(&suffix);
        require_eq(&bodies.len(), &1, format!("body count for {opener:?}"))?;
        let body = bodies.first().copied().ok_or_else(|| missing("missing queued heredoc body"))?;
        let body_start = opener.len() + 2;
        let trailing_start =
            source.find("my $x = 1;").ok_or_else(|| missing("missing trailing statement"))?;
        let terminator_line = if opener.starts_with("<<~") {
            format!("  {terminator}\n")
        } else {
            format!("{terminator}\n")
        };
        require(body.text.is_empty(), "heredoc body token text must remain opaque")?;
        assert_body_contract(
            &source,
            &suffix,
            body,
            body_start,
            expected_body,
            &terminator_line,
            trailing_start,
        )?;
        assert_assignment_at(&source, &suffix, trailing_start, "x", "1")?;
        assert_clean_terminal(&suffix)?;
        require(lexer.next_token().is_none(), "lexer emitted tokens after terminal EOF")?;
    }
    Ok(())
}

#[test]
fn false_openers_are_inert_and_preserve_clean_continuation() -> R {
    let cases: &[(&str, &[(&str, &str)])] = &[
        (
            "my $cmp = 1 < 2;\n",
            &[
                ("my", "Keyword"),
                ("$cmp", "Identifier"),
                ("=", "Operator"),
                ("1", "Number"),
                ("<", "Operator"),
                ("2", "Number"),
                (";", "Semicolon"),
            ],
        ),
        (
            "my $shift = 1 << 2;\n",
            &[
                ("my", "Keyword"),
                ("$shift", "Identifier"),
                ("=", "Operator"),
                ("1", "Number"),
                ("<<", "Operator"),
                ("2", "Number"),
                (";", "Semicolon"),
            ],
        ),
        (
            "my $paren = (1 << index($text, 'x'));\n",
            &[
                ("my", "Keyword"),
                ("$paren", "Identifier"),
                ("=", "Operator"),
                ("(", "LeftParen"),
                ("1", "Number"),
                ("<<", "Operator"),
                ("index", "Identifier"),
                ("(", "LeftParen"),
                ("$text", "Identifier"),
                (",", "Comma"),
                ("'x'", "StringLiteral"),
                (")", "RightParen"),
                (")", "RightParen"),
                (";", "Semicolon"),
            ],
        ),
        (
            "my $vars = $left << $right;\n",
            &[
                ("my", "Keyword"),
                ("$vars", "Identifier"),
                ("=", "Operator"),
                ("$left", "Identifier"),
                ("<<", "Operator"),
                ("$right", "Identifier"),
                (";", "Semicolon"),
            ],
        ),
        (
            "my $single = '<<EOF';\n",
            &[
                ("my", "Keyword"),
                ("$single", "Identifier"),
                ("=", "Operator"),
                ("'<<EOF'", "StringLiteral"),
                (";", "Semicolon"),
            ],
        ),
        (
            "my $double = \"<<EOF\";\n",
            &[
                ("my", "Keyword"),
                ("$double", "Identifier"),
                ("=", "Operator"),
                ("\"<<EOF\"", "StringLiteral"),
                (";", "Semicolon"),
            ],
        ),
        (
            "my $angle = '<';\n",
            &[
                ("my", "Keyword"),
                ("$angle", "Identifier"),
                ("=", "Operator"),
                ("'<'", "StringLiteral"),
                (";", "Semicolon"),
            ],
        ),
        // `collect_tokens` omits comment trivia, so the complete public-token projection is empty.
        ("# documentation says <<EOF\n", &[]),
    ];
    let continuation =
        concat!("my $after = 7;\n", "print <<REAL;\n", "body\n", "REAL\n", "my $tail = 9;\n",);

    for &(prefix, expected_prefix) in cases {
        let source = format!("{prefix}{continuation}");
        let tokens = PerlLexer::with_body_tokens(&source).collect_tokens();
        let valid_opener_start = source
            .find("<<REAL")
            .ok_or_else(|| missing("fixture is missing the valid heredoc opener"))?;
        let valid_body_start = source
            .find("body\n")
            .ok_or_else(|| missing("fixture is missing the valid heredoc body"))?;
        let after_start = source
            .find("my $after = 7;")
            .ok_or_else(|| missing("fixture is missing the continuation assignment"))?;
        let tail_start = source
            .find("my $tail = 9;")
            .ok_or_else(|| missing("fixture is missing the trailing assignment"))?;

        let before_valid =
            tokens.iter().filter(|token| token.start < valid_opener_start).collect::<Vec<_>>();
        require(
            before_valid.iter().all(|token| {
                !matches!(&token.token_type, TokenType::HeredocStart | TokenType::HeredocBody(_))
            }),
            format!("false opener queued or emitted a heredoc for {prefix:?}"),
        )?;
        require(
            before_valid.iter().all(|token| !token.token_type.is_recovery_token()),
            format!("false opener emitted recovery for {prefix:?}"),
        )?;

        let prefix_tokens = before_valid
            .iter()
            .copied()
            .filter(|token| token.start < prefix.len() && token.start < token.end)
            .collect::<Vec<_>>();
        assert_token_projection(prefix, &prefix_tokens, expected_prefix)?;

        assert_assignment_at(&source, &tokens, after_start, "after", "7")?;
        let opener = require_token_at(&tokens, valid_opener_start, "<<REAL")?;
        require(
            matches!(&opener.token_type, TokenType::HeredocStart),
            format!("later valid heredoc was not recognized after {prefix:?}"),
        )?;
        let bodies = body_tokens(&tokens);
        require_eq(&bodies.len(), &1, format!("later valid heredoc body count after {prefix:?}"))?;
        let body =
            bodies.first().copied().ok_or_else(|| missing("missing later valid heredoc body"))?;
        require_eq(
            &(body.start, body.end),
            &(valid_body_start, valid_body_start + "body\n".len()),
            format!("later valid heredoc body geometry after {prefix:?}"),
        )?;
        require_eq(
            &source.get(body.start..body.end),
            &Some("body\n"),
            format!("later valid heredoc body source slice after {prefix:?}"),
        )?;
        require_eq(
            &source.get(body.end..tail_start),
            &Some("REAL\n"),
            format!("later valid heredoc terminator gap after {prefix:?}"),
        )?;
        require(
            tokens
                .iter()
                .filter(|token| !matches!(&token.token_type, TokenType::HeredocBody(_)))
                .all(|token| token.end <= body.end || token.start >= tail_start),
            format!("ordinary token overlaps the REAL terminator after {prefix:?}"),
        )?;
        assert_assignment_at(&source, &tokens, tail_start, "tail", "9")?;
        assert_eof_at_source_end(&source, &tokens)?;
        assert_clean_terminal(&tokens)?;
    }
    Ok(())
}

#[test]
fn body_token_policy_is_explicit_and_preserves_the_same_resume_point() -> R {
    let source = "print <<EOF;\nbody\nEOF\nmy $x = 1;\n";

    let ordinary = PerlLexer::new(source).collect_tokens();
    require(
        ordinary.iter().any(|token| matches!(&token.token_type, TokenType::HeredocStart)),
        "ordinary mode omitted the heredoc opener",
    )?;
    require(body_tokens(&ordinary).is_empty(), "ordinary mode emitted heredoc bodies")?;
    let ordinary_my = ordinary
        .iter()
        .find(|token| token.text.as_ref() == "my")
        .ok_or_else(|| missing("ordinary mode did not resume after heredoc"))?;

    let with_bodies = PerlLexer::with_body_tokens(source).collect_tokens();
    require(
        with_bodies.iter().any(|token| matches!(&token.token_type, TokenType::HeredocStart)),
        "body-token mode omitted the heredoc opener",
    )?;
    let bodies = body_tokens(&with_bodies);
    require_eq(&bodies.len(), &1, "body-token mode body count")?;
    let body = bodies
        .first()
        .copied()
        .ok_or_else(|| missing("body-token mode omitted the heredoc body"))?;
    require_eq(&source.get(body.start..body.end), &Some("body\n"), "body source slice")?;
    require(body.text.is_empty(), "heredoc body token text must remain opaque")?;

    let with_bodies_my = with_bodies
        .iter()
        .find(|token| token.text.as_ref() == "my")
        .ok_or_else(|| missing("body-token mode did not resume after heredoc"))?;
    require_eq(&ordinary_my.start, &with_bodies_my.start, "resume token start")?;
    require_eq(&ordinary_my.end, &with_bodies_my.end, "resume token end")?;
    let trailing_start =
        source.find("my $x = 1;").ok_or_else(|| missing("missing trailing statement"))?;
    assert_assignment_at(source, &ordinary, trailing_start, "x", "1")?;
    assert_assignment_at(source, &with_bodies, trailing_start, "x", "1")?;
    assert_eof_at_source_end(source, &ordinary)?;
    assert_eof_at_source_end(source, &with_bodies)?;
    assert_clean_terminal(&ordinary)?;
    assert_clean_terminal(&with_bodies)
}

#[test]
fn multiple_mixed_heredocs_are_consumed_in_fifo_order() -> R {
    let source = concat!(
        "print <<A, <<'B', <<~C;\n",
        "first\n",
        "A\n",
        "second\n",
        "B\n",
        "  third\n",
        "  C\n",
        "my $after = 1;\n",
    );
    let tokens = PerlLexer::with_body_tokens(source).collect_tokens();

    let openers = tokens
        .iter()
        .filter(|token| matches!(&token.token_type, TokenType::HeredocStart))
        .map(|token| token.text.as_ref())
        .collect::<Vec<_>>();
    require_eq(&openers, &vec!["<<A", "<<'B'", "<<~C"], "FIFO opener order")?;
    let bodies = body_tokens(&tokens);
    let first_body_start = "print <<A, <<'B', <<~C;\n".len();
    let second_body_start = first_body_start + "first\nA\n".len();
    let third_body_start = second_body_start + "second\nB\n".len();
    let expected_ranges = vec![
        (first_body_start, first_body_start + "first\n".len()),
        (second_body_start, second_body_start + "second\n".len()),
        (third_body_start, third_body_start + "  third\n".len()),
    ];
    let actual_ranges = bodies.iter().map(|body| (body.start, body.end)).collect::<Vec<_>>();
    require_eq(&actual_ranges, &expected_ranges, "FIFO body ranges")?;
    require_eq(
        &body_slices(source, &tokens),
        &vec!["first\n", "second\n", "  third\n"],
        "FIFO body source slices",
    )?;
    require(bodies.iter().all(|body| body.text.is_empty()), "FIFO body tokens must remain opaque")?;
    let first = bodies.first().ok_or_else(|| missing("missing first FIFO body"))?;
    let second = bodies.get(1).ok_or_else(|| missing("missing second FIFO body"))?;
    let third = bodies.get(2).ok_or_else(|| missing("missing third FIFO body"))?;
    require_eq(&source.get(first.end..second.start), &Some("A\n"), "first terminator")?;
    require_eq(&source.get(second.end..third.start), &Some("B\n"), "second terminator")?;

    let trailing_start =
        source.find("my $after = 1;").ok_or_else(|| missing("missing FIFO trailing statement"))?;
    require_eq(&source.get(third.end..trailing_start), &Some("  C\n"), "third terminator")?;
    assert_assignment_at(source, &tokens, trailing_start, "after", "1")?;
    assert_eof_at_source_end(source, &tokens)?;
    assert_clean_terminal(&tokens)
}

#[test]
fn exact_terminators_cover_lf_crlf_cr_and_indentation() -> R {
    let cases = [
        ("print <<EOF;\nbody\nEOF\nmy $x = 1;\n", "body\n", "print <<EOF;\n".len()),
        ("print <<EOF;\r\nbody\r\nEOF\r\nmy $x = 1;\r\n", "body\r\n", "print <<EOF;\r\n".len()),
        ("print <<EOF;\rbody\rEOF\rmy $x = 1;\r", "body\r", "print <<EOF;\r".len()),
        ("print <<~EOF;\n  body\n\tEOF\nmy $x = 1;\n", "  body\n", "print <<~EOF;\n".len()),
    ];

    for (source, expected_body, body_start) in cases {
        let tokens = PerlLexer::with_body_tokens(source).collect_tokens();
        let bodies = body_tokens(&tokens);
        require_eq(&bodies.len(), &1, format!("line-ending heredoc body count for {source:?}"))?;
        let body =
            bodies.first().copied().ok_or_else(|| missing("missing line-ending heredoc body"))?;
        let trailing_start =
            source.find("my $x = 1;").ok_or_else(|| missing("missing trailing statement"))?;
        let terminator_line = match source {
            source if source.contains("\r\n") => "EOF\r\n",
            source if source.contains('\r') => "EOF\r",
            source if source.contains("\tEOF") => "\tEOF\n",
            _ => "EOF\n",
        };
        assert_body_contract(
            source,
            &tokens,
            body,
            body_start,
            expected_body,
            terminator_line,
            trailing_start,
        )?;
        assert_assignment_at(source, &tokens, trailing_start, "x", "1")?;
        assert_clean_terminal(&tokens)?;
    }
    Ok(())
}

#[test]
fn an_empty_body_is_anchored_at_the_terminator_boundary() -> R {
    let source = "print <<EOF;\nEOF\nmy $x = 1;\n";
    let tokens = PerlLexer::with_body_tokens(source).collect_tokens();
    let bodies = body_tokens(&tokens);
    require_eq(&bodies.len(), &1, "empty heredoc body count")?;
    let body = bodies.first().copied().ok_or_else(|| missing("missing empty heredoc body"))?;
    let expected_offset = source
        .find('\n')
        .map(|offset| offset + 1)
        .ok_or_else(|| missing("fixture is missing the opener newline"))?;

    let trailing_start =
        source.find("my $x = 1;").ok_or_else(|| missing("missing trailing statement"))?;
    assert_body_contract(source, &tokens, body, expected_offset, "", "EOF\n", trailing_start)?;
    require_eq(
        &(body.start, body.end),
        &(expected_offset, expected_offset),
        "empty heredoc body anchor",
    )?;
    require(body.text.is_empty(), "empty heredoc body token text is not empty")?;
    assert_assignment_at(source, &tokens, trailing_start, "x", "1")?;
    assert_clean_terminal(&tokens)
}

#[test]
fn unterminated_body_emits_bounded_unknown_rest_then_terminal_eof() -> R {
    let source = "print <<EOF;\nbody without terminator\n";
    let mut lexer = PerlLexer::with_body_tokens(source);
    let tokens = collect_remaining(&mut lexer);
    let unknown_index = tokens
        .iter()
        .position(|token| matches!(&token.token_type, TokenType::UnknownRest))
        .ok_or_else(|| missing("unterminated heredoc did not emit UnknownRest"))?;
    let unknown = tokens.get(unknown_index).ok_or_else(|| missing("missing UnknownRest token"))?;

    require_eq(
        &source.get(unknown.start..unknown.end),
        &Some("body without terminator\n"),
        "unterminated body source slice",
    )?;
    require_eq(&unknown.end, &source.len(), "unterminated body end")?;
    require(
        matches!(
            tokens.get(unknown_index + 1).map(|token| &token.token_type),
            Some(TokenType::EOF)
        ),
        "UnknownRest is not followed by EOF",
    )?;
    require_eq(&tokens.len(), &(unknown_index + 2), "unterminated terminal token count")?;
    require(lexer.next_token().is_none(), "lexer emitted tokens after terminal EOF")
}
