//! Regression tests for string interpolation edge cases in double-quoted strings.
//!
//! Covers two confirmed bugs fixed in parse_double_quoted_string:
//!
//! 1. `$` sigil loss: `$` consumed but not emitted when followed by a character
//!    that is neither an identifier start nor `{` (e.g., `"$!"` produced
//!    `Literal("!")` — the `$` was dropped entirely).
//!
//! 2. `@` array interpolation: `@arr` inside `"..."` was treated as literal text
//!    instead of being recognized as a `StringPart::Variable("@arr")`.

use perl_lexer::{PerlLexer, StringPart, Token, TokenType};
use perl_tdd_support::must_some;
use std::sync::Arc;

type R = Result<(), Box<dyn std::error::Error>>;

fn tokens(input: &str) -> Vec<Token> {
    PerlLexer::new(input).collect_tokens()
}

fn significant(input: &str) -> Vec<Token> {
    tokens(input)
        .into_iter()
        .filter(|t| {
            !matches!(t.token_type, TokenType::Whitespace | TokenType::Newline | TokenType::EOF)
        })
        .collect()
}

fn interp_parts(input: &str) -> Result<Vec<StringPart>, Box<dyn std::error::Error>> {
    let toks = significant(input);
    let tok = must_some(toks.into_iter().next());
    match tok.token_type {
        TokenType::InterpolatedString(parts) => Ok(parts),
        TokenType::StringLiteral => Ok(vec![]),
        other => Err(format!("expected InterpolatedString or StringLiteral, got {other:?}").into()),
    }
}

// ---------------------------------------------------------------------------
// Bug fix: $ sigil not dropped when followed by non-identifier character
// ---------------------------------------------------------------------------

#[test]
fn dollar_followed_by_punct_preserved_as_literal() -> R {
    let parts = interp_parts("\"$!\"")?;
    assert_eq!(
        parts,
        vec![StringPart::Literal(Arc::from("$!"))],
        "\"$!\" should produce Literal(\"$!\"), got {parts:?}"
    );
    Ok(())
}

#[test]
fn dollar_followed_by_space_preserved_as_literal() -> R {
    let parts = interp_parts("\"$ \"")?;
    assert!(
        parts.iter().any(|p| matches!(p, StringPart::Literal(s) if s.contains('$'))),
        "\"$ \" should keep the $ in a Literal part, got {parts:?}"
    );
    Ok(())
}

#[test]
fn dollar_followed_by_digit_preserved_as_literal() -> R {
    // $1..$9 are backreference variables; they start with a digit which is not
    // is_perl_identifier_start, so they previously dropped the '$'.
    let parts = interp_parts("\"$1\"")?;
    assert!(
        parts.iter().any(|p| matches!(p, StringPart::Literal(s) if s.contains('$'))),
        "\"$1\" should keep $ in a Literal part, got {parts:?}"
    );
    Ok(())
}

#[test]
fn dollar_followed_by_identifier_remains_variable() -> R {
    let parts = interp_parts("\"$name\"")?;
    assert_eq!(
        parts,
        vec![StringPart::Variable(Arc::from("$name"))],
        "\"$name\" should produce Variable(\"$name\"), got {parts:?}"
    );
    Ok(())
}

#[test]
fn dollar_brace_expr_remains_expression() -> R {
    let parts = interp_parts("\"${expr}\"")?;
    assert_eq!(
        parts,
        vec![StringPart::Expression(Arc::from("${expr}"))],
        "\"${{expr}}\" should produce Expression(\"${{expr}}\"), got {parts:?}"
    );
    Ok(())
}

#[test]
fn mixed_literal_variable_dollar_punct() -> R {
    // "hello $name$!" — variable followed by $ not followed by identifier
    let parts = interp_parts("\"hello $name$!\"")?;
    let has_var =
        parts.iter().any(|p| matches!(p, StringPart::Variable(s) if s.as_ref() == "$name"));
    let has_dollar_bang =
        parts.iter().any(|p| matches!(p, StringPart::Literal(s) if s.contains("$!")));
    assert!(has_var, "should contain Variable(\"$name\"), got {parts:?}");
    assert!(has_dollar_bang, "should contain Literal with \"$!\", got {parts:?}");
    Ok(())
}

// ---------------------------------------------------------------------------
// Bug fix: @ array sigil recognized in interpolated strings
// ---------------------------------------------------------------------------

#[test]
fn at_followed_by_identifier_is_variable() -> R {
    let parts = interp_parts("\"@arr\"")?;
    assert_eq!(
        parts,
        vec![StringPart::Variable(Arc::from("@arr"))],
        "\"@arr\" should produce Variable(\"@arr\"), got {parts:?}"
    );
    Ok(())
}

#[test]
fn at_underscore_array_is_variable() -> R {
    let parts = interp_parts("\"@_\"")?;
    assert_eq!(
        parts,
        vec![StringPart::Variable(Arc::from("@_"))],
        "\"@_\" (argument array) should produce Variable(\"@_\"), got {parts:?}"
    );
    Ok(())
}

#[test]
fn at_brace_expr_is_expression() -> R {
    let parts = interp_parts("\"@{arr}\"")?;
    assert_eq!(
        parts,
        vec![StringPart::Expression(Arc::from("@{arr}"))],
        "\"@{{arr}}\" should produce Expression(\"@{{arr}}\"), got {parts:?}"
    );
    Ok(())
}

#[test]
fn at_followed_by_punct_stays_literal() -> R {
    let parts = interp_parts("\"@!\"")?;
    assert!(
        parts.iter().any(|p| matches!(p, StringPart::Literal(s) if s.contains('@'))),
        "\"@!\" should keep @ as a literal (no identifier follows), got {parts:?}"
    );
    Ok(())
}

#[test]
fn mixed_scalar_and_array_interpolation() -> R {
    // "Hello $name, you have @items items"
    let parts = interp_parts("\"Hello $name, you have @items items\"")?;
    let has_scalar =
        parts.iter().any(|p| matches!(p, StringPart::Variable(s) if s.as_ref() == "$name"));
    let has_array =
        parts.iter().any(|p| matches!(p, StringPart::Variable(s) if s.as_ref() == "@items"));
    assert!(has_scalar, "should contain Variable(\"$name\"), got {parts:?}");
    assert!(has_array, "should contain Variable(\"@items\"), got {parts:?}");
    Ok(())
}

#[test]
fn array_interpolation_followed_by_text() -> R {
    // "count: @arr items"
    let parts = interp_parts("\"count: @arr items\"")?;
    let has_array =
        parts.iter().any(|p| matches!(p, StringPart::Variable(s) if s.as_ref() == "@arr"));
    let has_suffix =
        parts.iter().any(|p| matches!(p, StringPart::Literal(s) if s.contains("items")));
    assert!(has_array, "should contain Variable(\"@arr\"), got {parts:?}");
    assert!(has_suffix, "should contain Literal with 'items', got {parts:?}");
    Ok(())
}

#[test]
fn array_interpolation_disabled_keeps_at_as_literal_text() -> R {
    use perl_lexer::{LexerConfig, PerlLexer};
    let config = LexerConfig { parse_interpolation: false, ..Default::default() };
    let toks: Vec<Token> = PerlLexer::with_config("\"@arr\"", config)
        .collect_tokens()
        .into_iter()
        .filter(|t| {
            !matches!(t.token_type, TokenType::Whitespace | TokenType::Newline | TokenType::EOF)
        })
        .collect();
    let tok = must_some(toks.first());
    // With interpolation disabled, @arr is retained as literal text — no Variable part
    let only_literals = match &tok.token_type {
        TokenType::InterpolatedString(parts) => {
            parts.iter().all(|p| matches!(p, StringPart::Literal(_)))
        }
        TokenType::StringLiteral => true,
        _ => false,
    };
    assert!(
        only_literals,
        "with parse_interpolation:false, @arr should be literal text only, got {:?}",
        tok.token_type
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Regression guard: existing passing interpolation patterns still work
// ---------------------------------------------------------------------------

#[test]
fn plain_string_only_literal_parts() -> R {
    // A double-quoted string with no $ or @ produces only Literal parts
    // (the lexer emits InterpolatedString([Literal]) even for plain text).
    let toks = significant("\"hello\"");
    let tok = toks.first().ok_or("no token")?;
    let only_literals = match &tok.token_type {
        TokenType::InterpolatedString(parts) => {
            parts.iter().all(|p| matches!(p, StringPart::Literal(_)))
        }
        TokenType::StringLiteral => true,
        _ => false,
    };
    assert!(
        only_literals,
        "plain string should contain only Literal parts, got {:?}",
        tok.token_type
    );
    Ok(())
}

#[test]
fn escape_sequence_still_captured() -> R {
    // "\n" should not panic or corrupt the parser
    let toks = significant("\"\\n\"");
    assert!(!toks.is_empty(), "escape sequence should produce at least one token");
    Ok(())
}

#[test]
fn complex_interpolation_chain_unchanged() -> R {
    // "$obj->method()[0]" style is not double-quoted but inside a double-quoted string
    // "$arr->[0]" should still produce Variable + MethodCall
    let parts = interp_parts("\"$arr->[0]\"")?;
    let has_var =
        parts.iter().any(|p| matches!(p, StringPart::Variable(s) if s.as_ref() == "$arr"));
    assert!(has_var, "should contain Variable(\"$arr\"), got {parts:?}");
    Ok(())
}
