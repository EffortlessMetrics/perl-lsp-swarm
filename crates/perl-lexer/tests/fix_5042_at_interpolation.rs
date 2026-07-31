//! Regression tests for #5042 — `@` sigil interpolation in double-quoted
//! strings.
//!
//! Ported from PR #5355 (branch claude/inspiring-babbage-83z94q), which added
//! the `'@' if self.config.parse_interpolation` arm to `parse_double_quoted_string`.
//! Issue #5042's headline claim is the `@` sigil, so #5235 (the `$`-only fix)
//! cannot close the issue without this arm.

use perl_lexer::{LexerConfig, PerlLexer, StringPart, Token, TokenType};
use perl_tdd_support::must_some;
use std::sync::Arc;

type R = Result<(), Box<dyn std::error::Error>>;

fn significant(input: &str) -> Vec<Token> {
    PerlLexer::new(input)
        .collect_tokens()
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
// @arr, @_, @{expr}
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

// Verified against real perl 5.38.2: `perl -e 'print "@!"'` prints a literal
// `@!` -- `@` has no punctuation-variable forms the way `$` does, so a `@`
// not followed by an identifier or `{` must stay literal text.
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
    let parts = interp_parts("\"count: @arr items\"")?;
    let has_array =
        parts.iter().any(|p| matches!(p, StringPart::Variable(s) if s.as_ref() == "@arr"));
    let has_suffix =
        parts.iter().any(|p| matches!(p, StringPart::Literal(s) if s.contains("items")));
    assert!(has_array, "should contain Variable(\"@arr\"), got {parts:?}");
    assert!(has_suffix, "should contain Literal with 'items', got {parts:?}");
    Ok(())
}

// Non-ASCII (UTF-8) identifier continuation after '@', mirroring the
// UTF-8 fast-path fallback the '$' arm already exercises for `$café`
// (see tokenizer_edge_case_tests.rs). The '@' arm ports the same
// byte>=128 -> is_perl_identifier_continue fallback loop, but had no
// direct test forcing that branch: every existing `@`-interpolation
// test uses pure-ASCII names, so the non-ASCII continuation path was
// unexercised.
#[test]
fn at_followed_by_unicode_identifier_is_variable() -> R {
    let parts = interp_parts("\"@caf\u{00e9}\"")?;
    assert_eq!(
        parts,
        vec![StringPart::Variable(Arc::from("@caf\u{00e9}"))],
        "\"@café\" should produce Variable(\"@café\"), got {parts:?}"
    );
    Ok(())
}

// Trailing '@' with nothing after it (current_char() is None once the
// sigil is consumed) must fall into the literal fallback arm rather than
// panicking or losing the sigil -- mirrors the existing bare-trailing-'$'
// regression guard in fix_5042_dollar_interpolation.rs.
#[test]
fn at_followed_by_nothing_stays_literal() -> R {
    let parts = interp_parts("\"abc@\"")?;
    let joined: String = parts
        .iter()
        .map(|part| match part {
            StringPart::Literal(text) => Ok(text.to_string()),
            other => Err(format!("expected only literal parts, got {other:?}")),
        })
        .collect::<Result<String, String>>()?;
    assert_eq!(joined, "abc@", "trailing '@' in \"abc@\" must survive as literal text");
    Ok(())
}

#[test]
fn array_interpolation_disabled_keeps_at_as_literal_text() -> R {
    let config = LexerConfig { parse_interpolation: false, ..Default::default() };
    let toks: Vec<Token> = PerlLexer::with_config("\"@arr\"", config)
        .collect_tokens()
        .into_iter()
        .filter(|t| {
            !matches!(t.token_type, TokenType::Whitespace | TokenType::Newline | TokenType::EOF)
        })
        .collect();
    let tok = must_some(toks.first());
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
// Package-qualified array names (review finding on PR #5235)
//
// Verified against real perl 5.38.2:
//   our @arr = ("x", "y"); print "@main::arr"   # prints "x y"
// so `::` segments belong to the interpolated variable, not to trailing
// literal text. The `$#` arm already folds `::`; this pins the `@` arm to
// the same rule.
// ---------------------------------------------------------------------------

#[test]
fn at_package_qualified_array_emits_single_variable() -> R {
    let parts = interp_parts("\"@main::arr\"")?;
    assert_eq!(
        parts,
        vec![StringPart::Variable(Arc::from("@main::arr"))],
        "\"@main::arr\" should be one Variable, not Variable(\"@main\") + Literal(\"::arr\"), \
         got {parts:?}"
    );
    Ok(())
}

#[test]
fn at_deeply_qualified_array_emits_single_variable() -> R {
    let parts = interp_parts("\"@Foo::Bar::baz\"")?;
    assert_eq!(
        parts,
        vec![StringPart::Variable(Arc::from("@Foo::Bar::baz"))],
        "multi-segment package qualifiers should stay in one Variable, got {parts:?}"
    );
    Ok(())
}

#[test]
fn at_single_trailing_colon_is_not_a_qualifier() -> R {
    // One colon is not a package separator: it must terminate the variable and
    // stay as literal text.
    let parts = interp_parts("\"@arr:tail\"")?;
    assert_eq!(
        parts,
        vec![StringPart::Variable(Arc::from("@arr")), StringPart::Literal(Arc::from(":tail")),],
        "a single ':' must end the variable, got {parts:?}"
    );
    Ok(())
}
