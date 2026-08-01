//! Public behavior proof for the shared qualified-identifier scan used by the
//! `$`/`@` interpolation arms of `parse_double_quoted_string`.
//!
//! `consume_qualified_identifier_in_string` is `pub(crate)`, so its unit tests
//! live beside it. This file proves the same seam through the crate's *public*
//! surface: every production call site is reachable from `PerlLexer::new` plus
//! `collect_tokens`, and each of the scan's four branches (the conditional `'`
//! package separator, identifier-continue, the `::` package pair, and the
//! terminating break) is observable in the resulting `StringPart` boundaries.
//!
//! Each call site is driven separately so that a regression in any one of them
//! fails on its own row rather than being masked by the others.

use perl_lexer::{PerlLexer, StringPart, TokenType};
use std::sync::Arc;

type R = Result<(), Box<dyn std::error::Error>>;

/// Lex `input` and return the parts of its single interpolated string token.
fn interpolated_parts(input: &str) -> Result<Vec<StringPart>, Box<dyn std::error::Error>> {
    let token = PerlLexer::new(input)
        .collect_tokens()
        .into_iter()
        .find(|t| matches!(t.token_type, TokenType::InterpolatedString(_)))
        .ok_or_else(|| format!("no InterpolatedString token for {input:?}"))?;

    match token.token_type {
        TokenType::InterpolatedString(parts) => Ok(parts),
        other => Err(format!("expected InterpolatedString for {input:?}, got {other:?}").into()),
    }
}

/// Assert that `input` lexes to exactly one `Variable` part spelled `expected`.
fn assert_single_variable(input: &str, expected: &str) -> R {
    let parts = interpolated_parts(input)?;
    if parts != vec![StringPart::Variable(Arc::from(expected))] {
        return Err(format!("{input:?} must be one Variable({expected:?}), got {parts:?}").into());
    }
    Ok(())
}

/// The `::`-folding branch, exercised once per production call site.
///
/// The four call sites are the `$#array`, `@array`, `@$ref` and `$$ref` arms.
/// Every row here reaches the scan loop through a different arm, so a call site
/// that stopped folding `::` — or stopped calling the helper at all — fails
/// here even though the other three still pass.
#[test]
fn every_call_site_folds_package_separators_through_the_shared_scan() -> R {
    let cases = [
        // (input, expected single Variable part, which production arm)
        (r#""$#main::array""#, "$#main::array", "$#array arm"),
        (r#""@main::arr""#, "@main::arr", "@array arm"),
        (r#""@$main::ref""#, "@$main::ref", "@$ref deref arm"),
        (r#""$$main::rt""#, "$$main::rt", "$$ref deref arm"),
    ];

    for (input, expected, arm) in cases {
        assert_single_variable(input, expected).map_err(|e| format!("{arm}: {e}"))?;
    }
    Ok(())
}

/// The terminating-break branch: a *lone* `:` is not a package separator, so
/// the scan must stop before it and hand the rest back as literal text.
///
/// This is the discriminator against a scan that treats any `:` as a separator
/// — such an implementation would pass the folding test above but fail here.
#[test]
fn every_call_site_stops_the_shared_scan_at_a_lone_colon() -> R {
    let cases = [
        (r#""$#arr:tail""#, "$#arr", "$#array arm"),
        (r#""@arr:tail""#, "@arr", "@array arm"),
        (r#""@$ref:tail""#, "@$ref", "@$ref deref arm"),
        (r#""$$ref:tail""#, "$$ref", "$$ref deref arm"),
    ];

    for (input, head, arm) in cases {
        let parts = interpolated_parts(input)?;
        let expected =
            vec![StringPart::Variable(Arc::from(head)), StringPart::Literal(Arc::from(":tail"))];
        if parts != expected {
            return Err(format!(
                "{arm}: {input:?} must stop the scan at the lone ':', got {parts:?}"
            )
            .into());
        }
    }
    Ok(())
}

/// The identifier-continue branch across the character classes the helper
/// accepts but a plain ASCII loop would not: non-ASCII identifier characters
/// and the old-style `'` package separator.
///
/// Real perl 5.38.2 resolves both forms to the array itself:
/// `@Foo::Bar = (7,8); print "@Foo'Bar"` prints `7 8`.
#[test]
fn the_shared_scan_accepts_non_ascii_and_apostrophe_identifier_characters() -> R {
    assert_single_variable(r#""@café""#, "@café")?;
    assert_single_variable(r#""@Foo'Bar""#, "@Foo'Bar")?;
    assert_single_variable(r#""$#café""#, "$#café")?;
    Ok(())
}

/// The negative half of the `'` branch, observed through the public surface.
///
/// A `'` is only a package separator when a further name segment follows it.
/// Real perl 5.38.2: `@foo=(1,2); print "@foo'"` prints `1 2'` and
/// `print "@foo'9"` prints `1 2'9` — the apostrophe stays literal text. An
/// implementation that folded `'` unconditionally (which
/// `is_perl_identifier_continue` alone would do) still passes the positive
/// `@Foo'Bar` row above but fails here.
#[test]
fn the_shared_scan_leaves_a_non_separating_apostrophe_as_literal_text() -> R {
    let cases = [
        (r#""@foo'""#, "@foo", "'"),
        (r#""@foo'9""#, "@foo", "'9"),
        (r#""$#foo'9""#, "$#foo", "'9"),
    ];

    for (input, head, tail) in cases {
        let parts = interpolated_parts(input)?;
        let expected =
            vec![StringPart::Variable(Arc::from(head)), StringPart::Literal(Arc::from(tail))];
        if parts != expected {
            return Err(format!(
                "{input:?} must leave the non-separating apostrophe literal, got {parts:?}"
            )
            .into());
        }
    }
    Ok(())
}

/// A qualified name must not absorb a following subscript: the scan stops at
/// `[`, leaving the subscript to its own part.
///
/// Real perl 5.38.2: `@a = (10,20,30); $main::ar = \@a; print "$$main::ar[1]"`
/// prints `20`, so the subscript binds to the deref chain but is a separate
/// lexical unit, classified `ArraySlice` by this arm.
#[test]
fn the_shared_scan_stops_before_a_trailing_subscript() -> R {
    let parts = interpolated_parts(r#""$$main::ar[1]""#)?;
    let expected = vec![
        StringPart::Variable(Arc::from("$$main::ar")),
        StringPart::ArraySlice(Arc::from("[1]")),
    ];
    if parts != expected {
        return Err(format!("qualified deref must not absorb \"[1]\", got {parts:?}").into());
    }
    Ok(())
}
