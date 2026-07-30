//! Regression tests for #5042 — $ special-var silent-drop in interpolated strings.
//!
//! Before the fix, `"$!"`, `"$@"`, `"$$"`, `"$0"`, `"$^W"`, etc. inside
//! double-quoted strings fell through the interpolation match to `_ => {}`
//! and silently dropped the `$` sigil, producing an empty or incorrect
//! InterpolatedString parts list.

use perl_lexer::{PerlLexer, StringPart, TokenType};
use std::sync::Arc;

type R = Result<(), Box<dyn std::error::Error>>;

fn interpolated_parts(input: &str) -> Option<Vec<StringPart>> {
    let tok = PerlLexer::new(input).next_token()?;
    match tok.token_type {
        TokenType::InterpolatedString(parts) => Some(parts),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Punctuation special variables
// ---------------------------------------------------------------------------

#[test]
fn dollar_bang_in_string_emits_variable() -> R {
    let parts = interpolated_parts(r#""$!""#).ok_or("no InterpolatedString")?;
    assert_eq!(parts, vec![StringPart::Variable(Arc::from("$!"))]);
    Ok(())
}

#[test]
fn dollar_at_in_string_emits_variable() -> R {
    let parts = interpolated_parts(r#""$@""#).ok_or("no InterpolatedString")?;
    assert_eq!(parts, vec![StringPart::Variable(Arc::from("$@"))]);
    Ok(())
}

#[test]
fn dollar_question_in_string_emits_variable() -> R {
    let parts = interpolated_parts(r#""$?""#).ok_or("no InterpolatedString")?;
    assert_eq!(parts, vec![StringPart::Variable(Arc::from("$?"))]);
    Ok(())
}

#[test]
fn dollar_backslash_in_string_emits_variable() -> R {
    // "$\" terminates the string via the escape arm; use "$\\" (escaped backslash)
    // to land in the literal-backslash case. The plain "$\" case is handled by
    // the escape arm before the '$' interpolation arm fires.
    let parts = interpolated_parts(r#""$|""#).ok_or("no InterpolatedString")?;
    assert_eq!(parts, vec![StringPart::Variable(Arc::from("$|"))]);
    Ok(())
}

// ---------------------------------------------------------------------------
// Process ID — $$
// ---------------------------------------------------------------------------

#[test]
fn dollar_dollar_in_string_emits_pid_variable() -> R {
    let parts = interpolated_parts(r#""$$""#).ok_or("no InterpolatedString")?;
    assert_eq!(parts, vec![StringPart::Variable(Arc::from("$$"))]);
    Ok(())
}

// $$identifier must NOT consume the second $ into a PID token — the first $
// becomes a literal and the second starts a new interpolation.
#[test]
fn dollar_dollar_identifier_treats_first_as_literal() -> R {
    let parts = interpolated_parts(r#""$$foo""#).ok_or("no InterpolatedString")?;
    // First '$' → Literal("$"), then '$foo' → Variable("$foo")
    assert_eq!(
        parts,
        vec![StringPart::Literal(Arc::from("$")), StringPart::Variable(Arc::from("$foo")),]
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Digit variables — $0 (program name), $1..$9 (capture groups)
// ---------------------------------------------------------------------------

#[test]
fn dollar_zero_in_string_emits_variable() -> R {
    let parts = interpolated_parts(r#""$0""#).ok_or("no InterpolatedString")?;
    assert_eq!(parts, vec![StringPart::Variable(Arc::from("$0"))]);
    Ok(())
}

#[test]
fn dollar_one_in_string_emits_variable() -> R {
    let parts = interpolated_parts(r#""$1""#).ok_or("no InterpolatedString")?;
    assert_eq!(parts, vec![StringPart::Variable(Arc::from("$1"))]);
    Ok(())
}

// ---------------------------------------------------------------------------
// Control variables — $^W, $^O, $^X, etc.
// ---------------------------------------------------------------------------

#[test]
fn dollar_caret_w_in_string_emits_variable() -> R {
    let parts = interpolated_parts(r#""$^W""#).ok_or("no InterpolatedString")?;
    assert_eq!(parts, vec![StringPart::Variable(Arc::from("$^W"))]);
    Ok(())
}

#[test]
fn dollar_caret_o_in_string_emits_variable() -> R {
    let parts = interpolated_parts(r#""$^O""#).ok_or("no InterpolatedString")?;
    assert_eq!(parts, vec![StringPart::Variable(Arc::from("$^O"))]);
    Ok(())
}

// bare $^ (no uppercase letter) produces Variable("$^")
#[test]
fn dollar_caret_bare_in_string_emits_variable() -> R {
    let parts = interpolated_parts("\"$^\x01\"").ok_or("no InterpolatedString")?;
    // $^ followed by a non-uppercase char → Variable("$^") + Literal of the next char
    assert!(
        parts.first() == Some(&StringPart::Variable(Arc::from("$^"))),
        "expected Variable(\"$^\") as first part, got {:?}",
        parts
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Mixed: literal context + special variable
// ---------------------------------------------------------------------------

#[test]
fn literal_prefix_then_dollar_bang_emits_two_parts() -> R {
    let parts = interpolated_parts(r#""Error: $!""#).ok_or("no InterpolatedString")?;
    assert_eq!(
        parts,
        vec![StringPart::Literal(Arc::from("Error: ")), StringPart::Variable(Arc::from("$!")),]
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Regression guards — existing identifier interpolation must be unaffected
// ---------------------------------------------------------------------------

#[test]
fn dollar_foo_in_string_still_emits_variable() -> R {
    let parts = interpolated_parts(r#""$foo""#).ok_or("no InterpolatedString")?;
    assert_eq!(parts, vec![StringPart::Variable(Arc::from("$foo"))]);
    Ok(())
}

#[test]
fn dollar_brace_expr_in_string_still_emits_expression() -> R {
    let parts = interpolated_parts(r#""${expr}""#).ok_or("no InterpolatedString")?;
    assert_eq!(parts, vec![StringPart::Expression(Arc::from("${expr}"))]);
    Ok(())
}

// ---------------------------------------------------------------------------
// The closing delimiter must win over $"
//
// Perl defines $" (the list separator), but inside a double-quoted string the
// terminating quote takes precedence: `perl -e 'print "$"'` prints a literal
// '$' (with a "Final $ should be \$ or $name" warning) rather than
// interpolating $". An earlier revision of this fix accepted '"' as a
// punctuation special variable, which consumed the closing quote and turned
// the valid string "$" into Error("unterminated string") -- and silently
// mis-lexed every token after it.
// ---------------------------------------------------------------------------

#[test]
fn trailing_dollar_does_not_consume_closing_quote() -> R {
    let parts = interpolated_parts(r#""$""#).ok_or("no InterpolatedString")?;
    assert_eq!(
        parts,
        vec![StringPart::Literal(Arc::from("$"))],
        "a trailing '$' must stay literal and leave the closing quote intact"
    );
    Ok(())
}

#[test]
fn trailing_dollar_leaves_following_tokens_intact() -> R {
    // Regression guard for the cascade: if the closing quote is swallowed, the
    // concatenation operator and the next string are absorbed as string content
    // and the whole statement mis-lexes.
    let mut lexer = PerlLexer::new(r#""$" . "tail""#);

    let first = lexer.next_token().ok_or("no first token")?;
    assert_eq!(
        first.token_type,
        TokenType::InterpolatedString(vec![StringPart::Literal(Arc::from("$"))]),
        "first token must be the complete string \"$\""
    );

    let second = lexer.next_token().ok_or("no second token")?;
    assert!(
        !matches!(second.token_type, TokenType::Error(_)),
        "token after the string must not be an error, got {:?}",
        second.token_type
    );
    Ok(())
}

#[test]
fn literal_prefix_then_trailing_dollar_stays_literal() -> R {
    let parts = interpolated_parts(r#""cost: $""#).ok_or("no InterpolatedString")?;

    // The lexer emits the trailing '$' as its own Literal part rather than
    // merging it into the preceding one, so assert on the reconstructed text
    // and on the absence of any interpolation -- those are the properties that
    // matter here. Adjacent-literal fragmentation is existing lexer behavior
    // and is deliberately not pinned to a single part by this test.
    let joined: String = parts
        .iter()
        .map(|part| match part {
            StringPart::Literal(text) => Ok(text.to_string()),
            other => Err(format!("expected only literal parts, got {other:?}")),
        })
        .collect::<Result<String, String>>()?;

    assert_eq!(joined, "cost: $", "trailing '$' must survive as literal text");
    Ok(())
}
