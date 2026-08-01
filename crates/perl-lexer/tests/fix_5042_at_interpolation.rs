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
// `@!`. `@` has only two punctuation forms -- the match-offset arrays `@+` and
// `@-` (covered further down) -- so every *other* `@` that is followed by
// neither an identifier, `{`, nor `$` must stay literal text.
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

#[test]
fn at_trailing_package_separator_stays_in_the_variable() -> R {
    // Verified against real perl 5.38.2: `print "@arr::"` prints the (empty)
    // package array `@arr::`, not `@arr` followed by literal "::" — so the
    // trailing separator belongs to the variable name.
    let parts = interp_parts("\"@arr::\"")?;
    assert_eq!(
        parts,
        vec![StringPart::Variable(Arc::from("@arr::"))],
        "a trailing '::' is part of the package-qualified name, got {parts:?}"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Non-ASCII scan boundaries in the `@` name loop
//
// The `@` identifier scan walks raw bytes and only decodes a `char` when it
// sees a byte >= 128, at which point `is_perl_identifier_continue(c)` decides
// whether the multi-byte character continues the name or ends it. Every other
// `@` test in this file uses either pure ASCII (never enters the byte >= 128
// arm) or a continuing character such as `é` (always takes the `true` side),
// so the `false` side of that predicate had no discriminator: an
// implementation that unconditionally consumed every byte >= 128 would still
// pass all of them.
// ---------------------------------------------------------------------------

// Verified against real perl 5.38.2:
//   our @arr = ("x","y"); print "@arr\x{20AC}"   # prints "x y€"
// so the euro sign terminates the array name and stays literal text.
#[test]
fn at_identifier_scan_stops_when_is_perl_identifier_continue_rejects_a_multibyte_char() -> R {
    let parts = interp_parts("\"@arr\u{20ac}\"")?;
    assert_eq!(
        parts,
        vec![StringPart::Variable(Arc::from("@arr")), StringPart::Literal(Arc::from("\u{20ac}")),],
        "'\u{20ac}' is not an identifier-continue character, so it must end \"@arr\" and stay \
         literal, got {parts:?}"
    );
    Ok(())
}

// The complementary positive: a multi-byte character that *is* an
// identifier-continue character keeps the scan going past the byte >= 128
// gate, so the name spans it. Together with the test above this pins both
// sides of the `is_perl_identifier_continue(c)` predicate rather than only
// the accepting side.
#[test]
fn at_identifier_scan_continues_through_an_accepted_multibyte_char_then_stops() -> R {
    let parts = interp_parts("\"@caf\u{00e9}s\u{20ac}\"")?;
    assert_eq!(
        parts,
        vec![
            StringPart::Variable(Arc::from("@caf\u{00e9}s")),
            StringPart::Literal(Arc::from("\u{20ac}")),
        ],
        "'é' continues the name and '\u{20ac}' ends it, got {parts:?}"
    );
    Ok(())
}

// The `@` arm opens a variable for anything `is_perl_identifier_start`
// accepts, which in this lexer deliberately includes emoji as well as
// XID_Start characters (see `ripr_seam_proof_peek_char.rs`, where `😀xy` is
// one identifier token). Every other `@` test uses an ASCII or XID_Start
// name, so an implementation that narrowed the gate to `is_xid_start` would
// pass all of them and only fail here.
#[test]
fn at_sigil_opens_a_variable_for_every_is_perl_identifier_start_char_including_emoji() -> R {
    let parts = interp_parts("\"@\u{1f600}xy\"")?;
    assert_eq!(
        parts,
        vec![StringPart::Variable(Arc::from("@\u{1f600}xy"))],
        "\"@😀xy\" should produce Variable(\"@😀xy\"), got {parts:?}"
    );
    Ok(())
}

// The `Some('{')` arm of the `@` sigil match must win over the identifier
// arm even when the braced expression starts with an identifier character.
// A wrong implementation that checked `is_perl_identifier_start` first would
// emit Literal("@") + ... instead of one Expression part.
#[test]
fn at_brace_arm_wins_over_the_identifier_arm_for_a_braced_name() -> R {
    let parts = interp_parts("\"@{main::arr}\"")?;
    assert_eq!(
        parts,
        vec![StringPart::Expression(Arc::from("@{main::arr}"))],
        "\"@{{main::arr}}\" must be one Expression part, got {parts:?}"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Regex match-offset special arrays — @+ and @-
//
// `@+` and `@-` are real Perl array variables holding the end/start offsets of
// the most recent successful match, and they DO interpolate in double-quoted
// strings. Verified against real perl 5.38.2:
//
//   perl -e '"foobar"=~/(o+)(b)/; print "@-\n"'   # prints "1 1 3"
//   perl -e '"foobar"=~/(o+)(b)/; print "@+\n"'   # prints "4 3 4"
//
// so the literal fallback must not claim them. `try_variable` already
// recognizes the same two forms at token level (the
// `(sigil == '@' || sigil == '%') && matches!(ch, '+' | '-')` branch in
// `lib.rs`), so the string scanner has to agree with it.
// ---------------------------------------------------------------------------

#[test]
fn at_plus_match_end_offsets_array_is_variable() -> R {
    let parts = interp_parts("\"@+\"")?;
    assert_eq!(
        parts,
        vec![StringPart::Variable(Arc::from("@+"))],
        "\"@+\" is the match-end-offset array and must interpolate, got {parts:?}"
    );
    Ok(())
}

#[test]
fn at_minus_match_start_offsets_array_is_variable() -> R {
    let parts = interp_parts("\"@-\"")?;
    assert_eq!(
        parts,
        vec![StringPart::Variable(Arc::from("@-"))],
        "\"@-\" is the match-start-offset array and must interpolate, got {parts:?}"
    );
    Ok(())
}

// Both offset arrays inside surrounding literal text: pins that the `+`/`-`
// arm emits exactly one Variable part each and hands everything else back to
// the literal bucket, rather than swallowing neighbouring characters.
#[test]
fn at_plus_and_at_minus_interpolate_inside_surrounding_literal_text() -> R {
    let parts = interp_parts("\"match at @+..@- end\"")?;
    assert_eq!(
        parts,
        vec![
            StringPart::Literal(Arc::from("match at ")),
            StringPart::Variable(Arc::from("@+")),
            StringPart::Literal(Arc::from("..")),
            StringPart::Variable(Arc::from("@-")),
            StringPart::Literal(Arc::from(" end")),
        ],
        "both offset arrays must interpolate as separate Variable parts, got {parts:?}"
    );
    Ok(())
}

// The `+`/`-` arm must consume exactly one character. An implementation that
// scanned a run of punctuation would emit Variable("@++") for this input.
#[test]
fn at_plus_consumes_exactly_one_sigil_character() -> R {
    let parts = interp_parts("\"@++\"")?;
    assert_eq!(
        parts,
        vec![StringPart::Variable(Arc::from("@+")), StringPart::Literal(Arc::from("+"))],
        "\"@++\" is `@+` followed by a literal `+`, got {parts:?}"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Array dereference — @$ref, @$$ref, @$Pkg::ref
//
// Verified against real perl 5.38.2:
//
//   perl -e 'our @a=(1,2); our $r=\@a; print "@$r"'                # "1 2"
//   perl -e 'our @a=(1,2); our $r=\@a; my $rr=\$r; print "@$$rr"'  # "1 2"
//
// so the `$` sigil run plus the dereferenced name is one interpolation unit,
// exactly like the `$#$ref` arm. Before this arm existed the `$` fell into the
// literal fallback and `"@$aref"` emitted Literal("@") + Variable("$aref"),
// splitting the `@` sigil away from its own interpolation unit.
// ---------------------------------------------------------------------------

#[test]
fn at_dollar_ref_is_a_single_array_deref_variable() -> R {
    let parts = interp_parts("\"@$aref\"")?;
    assert_eq!(
        parts,
        vec![StringPart::Variable(Arc::from("@$aref"))],
        "\"@$aref\" must be one Variable part, got {parts:?}"
    );
    Ok(())
}

#[test]
fn at_double_dollar_ref_chains_the_whole_sigil_run() -> R {
    let parts = interp_parts("\"@$$rr\"")?;
    assert_eq!(
        parts,
        vec![StringPart::Variable(Arc::from("@$$rr"))],
        "\"@$$rr\" must keep the whole sigil run in one Variable part, got {parts:?}"
    );
    Ok(())
}

// The deref arm reuses `consume_qualified_identifier_in_string`, so it folds
// `::` package segments for the same reason `@main::arr` and `$#main::array`
// do. An implementation that stopped at the first `:` would emit
// Variable("@$Foo") + Literal("::Bar::ref").
#[test]
fn at_dollar_ref_folds_package_qualifiers() -> R {
    let parts = interp_parts("\"@$Foo::Bar::ref\"")?;
    assert_eq!(
        parts,
        vec![StringPart::Variable(Arc::from("@$Foo::Bar::ref"))],
        "\"@$Foo::Bar::ref\" must be one Variable part, got {parts:?}"
    );
    Ok(())
}

// The deref arm must stop at the first non-identifier character and hand the
// rest back to the literal bucket, so a trailing subscript is not silently
// folded into the variable name. `@arr[...]` slice interpolation stays a
// documented non-goal for this PR.
#[test]
fn at_dollar_ref_stops_at_a_trailing_subscript() -> R {
    let parts = interp_parts("\"@$aref[0]\"")?;
    assert_eq!(
        parts,
        vec![StringPart::Variable(Arc::from("@$aref")), StringPart::Literal(Arc::from("[0]"))],
        "the `@$ref` arm must not consume the trailing subscript, got {parts:?}"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Package separators in array names: `::` and the old-style `'`
// ---------------------------------------------------------------------------

// The old-style apostrophe package separator still interpolates in perl
// 5.38.2 (with the "Old package separator used in string" deprecation
// warning): `@Foo::Bar=(1,2); print "@Foo'Bar"` prints "1 2". The lexer's own
// `is_perl_identifier_continue` already accepts `'` for the same reason, so
// the string scanner must not split the name here.
#[test]
fn at_apostrophe_package_separator_stays_in_one_variable() -> R {
    let parts = interp_parts("\"@Foo'Bar\"")?;
    assert_eq!(
        parts,
        vec![StringPart::Variable(Arc::from("@Foo'Bar"))],
        "\"@Foo'Bar\" should produce one Variable part, got {parts:?}"
    );
    Ok(())
}

// A `'` that does not begin a further name segment is literal text, so the
// separator handling must look ahead rather than accepting every apostrophe.
// Verified against real perl 5.38.2: `@foo=(1,2); print "@foo'"` prints "1 2'"
// and `print "@foo'9"` prints "1 2'9".
#[test]
fn at_trailing_apostrophe_is_literal_text() -> R {
    let parts = interp_parts("\"@foo'\"")?;
    assert_eq!(
        parts,
        vec![StringPart::Variable(Arc::from("@foo")), StringPart::Literal(Arc::from("'"))],
        "a trailing apostrophe must stay literal, got {parts:?}"
    );
    Ok(())
}

#[test]
fn at_apostrophe_before_a_digit_is_literal_text() -> R {
    let parts = interp_parts("\"@foo'9\"")?;
    assert_eq!(
        parts,
        vec![StringPart::Variable(Arc::from("@foo")), StringPart::Literal(Arc::from("'9"))],
        "an apostrophe followed by a digit must stay literal, got {parts:?}"
    );
    Ok(())
}

// A leading `::` names the array in package `main`. Verified against real perl
// 5.38.2: `@a=(1,2); print "@::a"` prints "1 2".
#[test]
fn at_leading_double_colon_is_a_main_package_array() -> R {
    let parts = interp_parts("\"@::a\"")?;
    assert_eq!(
        parts,
        vec![StringPart::Variable(Arc::from("@::a"))],
        "\"@::a\" should produce one Variable part, got {parts:?}"
    );
    Ok(())
}

// A lone `:` after `@` does NOT open the `::` package form, so `"@:x"` stays
// literal text and `x` is not folded into a variable name.
//
// Documented divergence: real perl 5.38.2 does consume `@:` as an
// interpolation unit — `@x=(7,8); print "[@:x]"` prints "[x]" and
// `print "[@:]"` prints "[]", i.e. it interpolates an always-empty array and
// leaves "x" literal. This arm deliberately recognizes only an identifier
// start or a `::` package prefix after `@`; a lone `:` falls into the literal
// bucket. Literal text is the conservative outcome (it asserts no variable),
// and the always-empty `@:` form is out of scope for #5042. This test pins the
// boundary so the `::` sub-arm cannot silently start claiming single colons.
#[test]
fn at_single_colon_stays_literal() -> R {
    let parts = interp_parts("\"@:x\"")?;
    assert_eq!(
        parts,
        vec![StringPart::Literal(Arc::from("@:x"))],
        "\"@:x\" must stay literal text, got {parts:?}"
    );
    Ok(())
}
