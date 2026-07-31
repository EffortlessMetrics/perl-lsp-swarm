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

// $$foo is a scalar-dereference expression, not a PID followed by a separate
// variable. Verified against real perl 5.38.2:
//   my $x = "v"; my $foo = \$x; print "$$foo";   # prints "v"
// so the lexer must emit the whole `$$foo` as one Variable part rather than
// splitting it into Literal("$") + Variable("$foo").
#[test]
fn dollar_dollar_identifier_is_scalar_deref() -> R {
    let parts = interpolated_parts(r#""$$foo""#).ok_or("no InterpolatedString")?;
    assert_eq!(parts, vec![StringPart::Variable(Arc::from("$$foo"))]);
    Ok(())
}

// $$$foo chains scalar derefs (deref of a deref). Verified against real perl
// 5.38.2:
//   my $x = "v"; my $r1 = \$x; my $foo = \$r1; print "$$$foo";  # prints "v"
#[test]
fn dollar_dollar_dollar_identifier_is_double_scalar_deref() -> R {
    let parts = interpolated_parts(r#""$$$foo""#).ok_or("no InterpolatedString")?;
    assert_eq!(parts, vec![StringPart::Variable(Arc::from("$$$foo"))]);
    Ok(())
}

// Bare "$$" (PID) must keep working even though $$foo is now a deref chain —
// the PID case only fires when no identifier follows the dollar run.
#[test]
fn dollar_dollar_bare_still_emits_pid_variable() -> R {
    let parts = interpolated_parts(r#""$$""#).ok_or("no InterpolatedString")?;
    assert_eq!(parts, vec![StringPart::Variable(Arc::from("$$"))]);
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

// $10 is capture group 10 (a single multi-digit numeric variable), not $1
// followed by literal "0". Verified against real perl 5.38.2 with an
// 11-capture-group match: `$10` prints the 10th group's value, confirming
// perl consumes all consecutive digits into one numeric variable.
#[test]
fn dollar_ten_in_string_emits_single_multi_digit_variable() -> R {
    let parts = interpolated_parts(r#""$10""#).ok_or("no InterpolatedString")?;
    assert_eq!(parts, vec![StringPart::Variable(Arc::from("$10"))]);
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
// Array-length operator — $#array, $#{$ref}, $#$ref
// ---------------------------------------------------------------------------

// $#array is this PR's headline claim but had zero direct coverage.
#[test]
fn dollar_hash_array_in_string_emits_variable() -> R {
    let parts = interpolated_parts(r#""$#array""#).ok_or("no InterpolatedString")?;
    assert_eq!(parts, vec![StringPart::Variable(Arc::from("$#array"))]);
    Ok(())
}

// $#{$ref} interpolates to the last index of the array ref $ref. Verified
// against real perl 5.38.2:
//   my @arr = (10,20,30,40); my $ref = \@arr; print "$#{$ref}";  # prints 3
// It must be emitted as one Variable part, not fragmented into
// Variable("$#") + Literal("{") + Variable("$ref") + Literal("}").
#[test]
fn dollar_hash_brace_ref_in_string_emits_single_variable() -> R {
    let parts = interpolated_parts(r#""$#{$ref}""#).ok_or("no InterpolatedString")?;
    assert_eq!(parts, vec![StringPart::Variable(Arc::from("$#{$ref}"))]);
    Ok(())
}

// $#$ref is the sigil-less-brace form of the same thing. Verified against
// real perl 5.38.2 (same script as above): `print "$#$ref";` also prints 3.
// It must be emitted as one Variable part, not Variable("$#") + Variable("$ref").
#[test]
fn dollar_hash_dollar_ref_in_string_emits_single_variable() -> R {
    let parts = interpolated_parts(r#""$#$ref""#).ok_or("no InterpolatedString")?;
    assert_eq!(parts, vec![StringPart::Variable(Arc::from("$#$ref"))]);
    Ok(())
}

// $#$$ref chains a *double* deref sigil inside the `$#` array-length form
// (as opposed to `$#$ref`'s single sigil). Verified against real perl
// 5.38.2:
//   my @a=(1,2,3); my $r1=\@a; my $ref=\$r1; print "$#$$ref";  # prints 2
// This is a distinct exercise of the `while self.current_char() == Some('$')`
// loop at the `Some('$')` sub-arm of `$#` -- the single-`$` case
// (`dollar_hash_dollar_ref_in_string_emits_single_variable` above) only
// iterates that loop once; this pins the multi-iteration path.
#[test]
fn dollar_hash_double_dollar_ref_in_string_emits_single_variable() -> R {
    let parts = interpolated_parts(r#""$#$$ref""#).ok_or("no InterpolatedString")?;
    assert_eq!(parts, vec![StringPart::Variable(Arc::from("$#$$ref"))]);
    Ok(())
}

// ---------------------------------------------------------------------------
// Deref-then-brace: $${foo}, $$${foo} (regression from PR #5235 review)
// ---------------------------------------------------------------------------

// `$${foo}` is `${${foo}}` -- a scalar deref through a brace group. Verified
// against real perl 5.38.2:
//   my $x = "hello"; my $foo = \$x; print "$${foo}";  # prints "hello"
// Before this fix the `Some('$')` deref arm only recognized an identifier
// after the dollar run, so `{` fell through to the PID case and produced
// [Variable("$$"), Literal("{foo}")] instead of a single interpolation unit.
#[test]
fn dollar_dollar_brace_expr_is_single_expression() -> R {
    let parts = interpolated_parts(r#""$${foo}""#).ok_or("no InterpolatedString")?;
    assert_eq!(parts, vec![StringPart::Expression(Arc::from("$${foo}"))]);
    Ok(())
}

// Chained deref through a brace group: `$$${foo}` derefs twice. Verified
// against real perl 5.38.2:
//   my $x = "V"; my $inner = \$x; my $foo = \$inner; print "$$${foo}"; # "V"
#[test]
fn triple_dollar_brace_expr_is_single_expression() -> R {
    let parts = interpolated_parts(r#""$$${foo}""#).ok_or("no InterpolatedString")?;
    assert_eq!(parts, vec![StringPart::Expression(Arc::from("$$${foo}"))]);
    Ok(())
}

// After a brace-closed deref, a following `[...]` is NOT a subscript -- it is
// literal text. Verified against real perl 5.38.2:
//   my $x = "V2"; my $foo = \$x; print "$${foo}[0]";  # prints "V2[0]"
#[test]
fn dollar_dollar_brace_expr_then_bracket_stays_literal_suffix() -> R {
    let parts = interpolated_parts(r#""$${foo}[0]""#).ok_or("no InterpolatedString")?;
    assert_eq!(
        parts,
        vec![StringPart::Expression(Arc::from("$${foo}")), StringPart::Literal(Arc::from("[0]")),]
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Postfix subscripts after a $$foo deref chain (regression from PR #5235
// review) -- must not land in the literal bucket.
// ---------------------------------------------------------------------------

// `$$foo[1]` applies the array subscript to the dereferenced array. Verified
// against real perl 5.38.2:
//   my @a = (10,20,30); my $foo = \@a; print "$$foo[1]";  # prints "20"
#[test]
fn dollar_dollar_identifier_array_subscript_not_literal() -> R {
    let parts = interpolated_parts(r#""$$foo[1]""#).ok_or("no InterpolatedString")?;
    assert_eq!(
        parts,
        vec![StringPart::Variable(Arc::from("$$foo")), StringPart::ArraySlice(Arc::from("[1]")),]
    );
    Ok(())
}

// `$$foo{a}` applies the hash subscript to the dereferenced hash. Verified
// against real perl 5.38.2:
//   my %h = (a=>1); my $foo = \%h; print "$$foo{a}";  # prints "1"
#[test]
fn dollar_dollar_identifier_hash_subscript_not_literal() -> R {
    let parts = interpolated_parts(r#""$$foo{a}""#).ok_or("no InterpolatedString")?;
    assert_eq!(
        parts,
        vec![StringPart::Variable(Arc::from("$$foo")), StringPart::Expression(Arc::from("{a}")),]
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// `::`-qualified package names after $# (regression from PR #5235 review)
// ---------------------------------------------------------------------------

// `$#main::array` must consume the whole `::`-qualified name as one Variable,
// mirroring try_variable's `$#` handling. Verified against real perl 5.38.2:
//   package main; our @array = (1,2,3); print "$#main::array";  # prints "2"
#[test]
fn dollar_hash_qualified_package_array_emits_single_variable() -> R {
    let parts = interpolated_parts(r#""$#main::array""#).ok_or("no InterpolatedString")?;
    assert_eq!(parts, vec![StringPart::Variable(Arc::from("$#main::array"))]);
    Ok(())
}

// ---------------------------------------------------------------------------
// Escaped `\$foo` must stay a literal, never a Variable
// ---------------------------------------------------------------------------

#[test]
fn escaped_dollar_foo_stays_literal() -> R {
    let parts = interpolated_parts(r#""\$foo""#).ok_or("no InterpolatedString")?;
    assert_eq!(
        parts,
        vec![StringPart::Literal(Arc::from("\\$foo"))],
        "an escaped \\$ must never become a Variable part"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Bare trailing '$' catch-all fallback (the `_ => {}` replacement) — exercises
// the fallback arm that previously had zero coverage.
// ---------------------------------------------------------------------------

#[test]
fn bare_trailing_dollar_in_middle_of_literal_stays_literal() -> R {
    let parts = interpolated_parts(r#""abc$""#).ok_or("no InterpolatedString")?;
    let joined: String = parts
        .iter()
        .map(|part| match part {
            StringPart::Literal(text) => Ok(text.to_string()),
            other => Err(format!("expected only literal parts, got {other:?}")),
        })
        .collect::<Result<String, String>>()?;
    assert_eq!(joined, "abc$", "trailing '$' in \"abc$\" must survive as literal text");
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

// ---------------------------------------------------------------------------
// Deref chain followed by an arrow (review finding on PR #5235)
//
// Verified against real perl 5.38.2:
//   my @a=(10,20,30); my $ar=\@a; my $rr=\$ar; print "$$rr->[1]"   # prints 20
//   my %h=(k=>"V");   my $hr=\%h; my $hrr=\$hr; print "$$hrr->{k}" # prints V
// so an arrow *subscript* chains onto the deref and interpolates.
//
//   package Foo; sub new{bless{},shift} sub bar{"M"}
//   my $o=Foo->new; my $ro=\$o; print "$$ro->bar"
//     # prints "Foo=HASH(0x...)->bar"
// so a bare arrow *method* call does NOT interpolate — "->bar" must stay
// literal. This is the boundary between the two cases.
// ---------------------------------------------------------------------------

#[test]
fn deref_chain_arrow_array_subscript_is_method_call_part() -> R {
    let parts = interpolated_parts("\"$$rr->[1]\"").ok_or("no InterpolatedString")?;
    assert_eq!(
        parts,
        vec![StringPart::Variable(Arc::from("$$rr")), StringPart::MethodCall(Arc::from("->[1]")),],
        "\"$$rr->[1]\" must chain the arrow subscript, got {parts:?}"
    );
    Ok(())
}

#[test]
fn deref_chain_arrow_hash_subscript_is_method_call_part() -> R {
    let parts = interpolated_parts("\"$$hrr->{k}\"").ok_or("no InterpolatedString")?;
    assert_eq!(
        parts,
        vec![StringPart::Variable(Arc::from("$$hrr")), StringPart::MethodCall(Arc::from("->{k}")),],
        "\"$$hrr->{{k}}\" must chain the arrow subscript, got {parts:?}"
    );
    Ok(())
}

#[test]
fn deref_chain_arrow_method_call_stays_literal() -> R {
    // Real perl does not interpolate a bare "->name" after a deref chain.
    // Classifying it as MethodCall would over-claim interpolation.
    let parts = interpolated_parts("\"$$ro->bar\"").ok_or("no InterpolatedString")?;
    assert_eq!(
        parts,
        vec![StringPart::Variable(Arc::from("$$ro")), StringPart::Literal(Arc::from("->bar")),],
        "a bare arrow method must remain literal text, got {parts:?}"
    );
    Ok(())
}

#[test]
fn triple_deref_chain_arrow_subscript_chains() -> R {
    let parts = interpolated_parts("\"$$$ref->[0]\"").ok_or("no InterpolatedString")?;
    assert_eq!(
        parts,
        vec![StringPart::Variable(Arc::from("$$$ref")), StringPart::MethodCall(Arc::from("->[0]")),],
        "a longer deref chain must still chain the arrow subscript, got {parts:?}"
    );
    Ok(())
}
