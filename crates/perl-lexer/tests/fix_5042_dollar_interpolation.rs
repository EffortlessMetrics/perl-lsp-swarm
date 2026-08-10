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

// The dereferenced name is package-qualified, exactly like the `$#$ref` and
// `@$ref` arms. Verified against real perl 5.38.2:
//   $v = "deep"; $main::foo = \$v; print "$$main::foo";   # prints "deep"
// so `$$main::foo` is one deref of `$main::foo`, not Variable("$$main") plus
// the literal text "::foo".
#[test]
fn dollar_dollar_package_qualified_deref_is_one_variable() -> R {
    let parts = interpolated_parts(r#""$$main::foo""#).ok_or("no InterpolatedString")?;
    assert_eq!(
        parts,
        vec![StringPart::Variable(Arc::from("$$main::foo"))],
        "\"$$main::foo\" must be one Variable part, got {parts:?}"
    );
    Ok(())
}

// Same folding for a chained deref run and a multi-segment package qualifier.
#[test]
fn triple_dollar_multi_segment_package_deref_is_one_variable() -> R {
    let parts = interpolated_parts(r#""$$$Acme::Deep::Var""#).ok_or("no InterpolatedString")?;
    assert_eq!(
        parts,
        vec![StringPart::Variable(Arc::from("$$$Acme::Deep::Var"))],
        "\"$$$Acme::Deep::Var\" must be one Variable part, got {parts:?}"
    );
    Ok(())
}

// The `::` folding must not swallow a *lone* colon: `$$ref:tail` is the deref
// followed by literal text, mirroring the `$#$ref:tail` boundary pinned below.
#[test]
fn dollar_dollar_deref_stops_at_a_single_colon() -> R {
    let parts = interpolated_parts(r#""$$ref:tail""#).ok_or("no InterpolatedString")?;
    assert_eq!(
        parts,
        vec![StringPart::Variable(Arc::from("$$ref")), StringPart::Literal(Arc::from(":tail"))],
        "a lone ':' must end \"$$ref\", got {parts:?}"
    );
    Ok(())
}

// Bare "$$" (PID) must keep working even though $$foo is now a deref chain —
// the PID case only fires when no identifier follows the dollar run. Unlike
// `dollar_dollar_in_string_emits_pid_variable` above, this pins the boundary
// with a *following character*: the PID arm must stop after the second `$` and
// hand `;` back to the literal bucket rather than folding it into the variable.
#[test]
fn dollar_dollar_bare_still_emits_pid_variable() -> R {
    let parts = interpolated_parts(r#""pid=$$;""#).ok_or("no InterpolatedString")?;
    assert_eq!(
        parts,
        vec![
            StringPart::Literal(Arc::from("pid=")),
            StringPart::Variable(Arc::from("$$")),
            StringPart::Literal(Arc::from(";")),
        ]
    );
    Ok(())
}

// A punctuation special variable is NOT part of the dollar run. Verified
// against real perl 5.38.2: `print "[$$!]"` prints the PID followed by a
// literal `!` (e.g. "[32509!]"), so `$$!` is the PID plus literal text, not
// `$$` + the `$!` special variable and not a three-character variable.
#[test]
fn dollar_dollar_then_punctuation_is_pid_plus_literal_text() -> R {
    let parts = interpolated_parts(r#""$$!""#).ok_or("no InterpolatedString")?;
    assert_eq!(
        parts,
        vec![StringPart::Variable(Arc::from("$$")), StringPart::Literal(Arc::from("!"))]
    );
    Ok(())
}

// A pure sigil run longer than two is still ONE interpolation unit, not the
// PID followed by a stray literal `$`. Verified against real perl 5.38.2:
//   perl -e 'no strict; print "[$$]"'    # prints "[<pid>]"
//   perl -e 'no strict; print "[$$$]"'   # prints "[]"
//   perl -e 'no strict; print "[$$$$]"'  # prints "[]"
// The empty output for the longer runs shows perl parsed each as one deref
// unit; had it read `$$` + literal `$` the PID digits would have appeared.
// A single `self.advance()` in the fallback would emit
// [Variable("$$"), Literal("$")] here.
#[test]
fn triple_dollar_run_is_one_variable_not_pid_plus_literal_dollar() -> R {
    let parts = interpolated_parts(r#""$$$""#).ok_or("no InterpolatedString")?;
    assert_eq!(parts, vec![StringPart::Variable(Arc::from("$$$"))]);
    Ok(())
}

#[test]
fn quadruple_dollar_run_is_one_variable() -> R {
    let parts = interpolated_parts(r#""$$$$""#).ok_or("no InterpolatedString")?;
    assert_eq!(parts, vec![StringPart::Variable(Arc::from("$$$$"))]);
    Ok(())
}

// Digits do not start an identifier, but a `$` run in front of one is a scalar
// deref of that capture variable — not the PID followed by literal digits.
// Verified against real perl 5.38.2:
//   perl -W -e '"abc"=~/(a)/; print "$$1X"'
//   # one "uninitialized value" warning, prints just "X"
// i.e. `$$1` is one unit that interpolates empty and leaves "X" literal. A
// fallback that advanced once would emit [Variable("$$"), Literal("1X")].
#[test]
fn dollar_dollar_digit_is_a_capture_deref_not_pid_plus_digit() -> R {
    let parts = interpolated_parts(r#""$$1X""#).ok_or("no InterpolatedString")?;
    assert_eq!(
        parts,
        vec![StringPart::Variable(Arc::from("$$1")), StringPart::Literal(Arc::from("X"))]
    );
    Ok(())
}

// The digit run is consumed whole, for the same reason `"$10"` is capture
// group 10 rather than `$1` + "0".
#[test]
fn dollar_dollar_multi_digit_capture_deref_consumes_the_whole_digit_run() -> R {
    let parts = interpolated_parts(r#""$$12""#).ok_or("no InterpolatedString")?;
    assert_eq!(parts, vec![StringPart::Variable(Arc::from("$$12"))]);
    Ok(())
}

// The digit branch must also carry a longer sigil run, so it agrees with the
// identifier branch above (`$$$foo`) rather than dropping sigils.
#[test]
fn triple_dollar_digit_capture_deref_keeps_the_whole_sigil_run() -> R {
    let parts = interpolated_parts(r#""$$$9""#).ok_or("no InterpolatedString")?;
    assert_eq!(parts, vec![StringPart::Variable(Arc::from("$$$9"))]);
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

// ---------------------------------------------------------------------------
// Arrow forms that must NOT chain (boundary of the `->[` / `->{` branch)
//
// The deref-chain arm chains an arrow only when `[` or `{` follows. These pin
// the two other shapes a trailing arrow can take, both verified against real
// perl 5.38.2:
//
//   my $x=1; my $r=\$x;            print "$$r->"      # prints "1->"
//   my $ar=\@a; my $rr=\$ar;       print "$$rr->(1)"  # prints "ARRAY(0x..)->(1)"
//
// In both cases perl interpolates the deref and leaves the arrow as literal
// text, so neither may be classified as MethodCall.
// ---------------------------------------------------------------------------

#[test]
fn deref_chain_trailing_arrow_without_subscript_stays_literal() -> R {
    let parts = interpolated_parts("\"$$r->\"").ok_or("no InterpolatedString")?;
    assert_eq!(
        parts,
        vec![StringPart::Variable(Arc::from("$$r")), StringPart::Literal(Arc::from("->")),],
        "a trailing arrow with nothing after it must stay literal, got {parts:?}"
    );
    Ok(())
}

#[test]
fn deref_chain_arrow_paren_call_stays_literal() -> R {
    let parts = interpolated_parts("\"$$rr->(1)\"").ok_or("no InterpolatedString")?;
    assert_eq!(
        parts,
        vec![StringPart::Variable(Arc::from("$$rr")), StringPart::Literal(Arc::from("->(1)")),],
        "an arrow code-deref call does not interpolate and must stay literal, got {parts:?}"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// The punctuation special-variable set, exhaustively
//
// The `Some('?' | '!' | '@' | ...)` arm accepts 24 characters but only four
// of them (`!`, `@`, `?`, `|`) had a discriminator above. An implementation
// that shipped a shorter set — or that silently dropped the sigil for the
// untested members, which is exactly the #5042 bug — would still pass. Each
// character below is a real Perl special variable: every one of them was
// confirmed to interpolate under real perl 5.38.2 (`perl -e 'print "[$&]"'`
// and friends all produce the variable's value, never a literal `$`).
// ---------------------------------------------------------------------------

#[test]
fn every_punctuation_special_variable_in_the_match_arm_emits_one_variable_part() -> R {
    // The loop below covers 22 of the arm's 24 members. Two are deliberately
    // handled elsewhere rather than inline:
    //   - '\\' needs escaping to write inline, so it has its own test
    //     (`dollar_backslash_is_the_output_record_separator_...`) below;
    //   - ':' is context-dependent — `$:` is the variable but `$::` starts a
    //     package-qualified name — so it is covered by the `$:` vs `$::`
    //     section further down (`dollar_colon_in_string_emits_variable` and
    //     `dollar_double_colon_is_a_package_qualified_variable`).
    // '"' is not in the set at all: the closing delimiter wins, which
    // `is_perl_punctuation_variable_rejects_the_string_delimiter` pins.
    for punct in [
        '?', '!', '@', '&', '`', '\'', '.', '/', '|', '+', '-', '[', ']', '~', '=', '%', ',', ';',
        '>', '<', ')', '(',
    ] {
        let source = format!("\"${punct}\"");
        let expected = format!("${punct}");
        let parts = interpolated_parts(&source)
            .ok_or_else(|| format!("\"${punct}\" did not lex as an InterpolatedString"))?;
        assert_eq!(
            parts,
            vec![StringPart::Variable(Arc::from(expected.as_str()))],
            "\"${punct}\" must interpolate as Variable(\"${punct}\"), got {parts:?}"
        );
    }
    Ok(())
}

// `$\` is Perl's output record separator and it interpolates: the `$` sigil
// claims the backslash before it can start an escape sequence. Verified
// against real perl 5.38.2:
//   $\ = "!"; print "x$\ny";   # writes  x!ny
// so `"x$\ny"` is Literal("x") + Variable("$\") + Literal("ny") -- the `n` is
// ordinary text, not the tail of a `\n` escape. The comment on
// `dollar_backslash_in_string_emits_variable` above assumed this case was
// unreachable and tested `$|` instead, leaving the real `$\` shape unpinned.
#[test]
fn dollar_backslash_is_the_output_record_separator_not_the_start_of_an_escape() -> R {
    let parts = interpolated_parts("\"x$\\ny\"").ok_or("no InterpolatedString")?;
    assert_eq!(
        parts,
        vec![
            StringPart::Literal(Arc::from("x")),
            StringPart::Variable(Arc::from("$\\")),
            StringPart::Literal(Arc::from("ny")),
        ],
        "\"x$\\ny\" must interpolate $\\ and leave \"ny\" literal, got {parts:?}"
    );
    Ok(())
}

// The negative side of the same arm: a character that is *not* a Perl
// punctuation variable must fall through to the literal fallback and keep the
// sigil. Without this, an implementation that accepted every character after
// `$` would pass the exhaustive test above.
#[test]
fn a_character_outside_the_punctuation_arm_falls_through_to_the_literal_fallback() -> R {
    let parts = interpolated_parts("\"$*tail\"").ok_or("no InterpolatedString")?;
    assert_eq!(
        parts,
        vec![StringPart::Literal(Arc::from("$*tail"))],
        "'*' is not in the punctuation-variable set, so \"$*tail\" stays literal, got {parts:?}"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Scan boundaries for the digit, control and array-length arms
// ---------------------------------------------------------------------------

// The digit arm consumes a maximal run of ASCII digits and must stop at the
// first non-digit. Verified against real perl 5.38.2:
//   "abc" =~ /(a)(b)/; print "[$1a]"   # prints "[aa]"
// i.e. `$1` interpolates and the following `a` is literal text -- a digit
// variable never absorbs a trailing letter.
#[test]
fn dollar_digit_run_stops_at_the_first_non_digit_character() -> R {
    let parts = interpolated_parts("\"$1a\"").ok_or("no InterpolatedString")?;
    assert_eq!(
        parts,
        vec![StringPart::Variable(Arc::from("$1")), StringPart::Literal(Arc::from("a")),],
        "\"$1a\" must be Variable(\"$1\") + Literal(\"a\"), got {parts:?}"
    );
    Ok(())
}

// The digit arm is gated on `is_ascii_digit`, not on "any Unicode digit".
// `٣` (U+0663 ARABIC-INDIC DIGIT THREE) is `Nd` — a Unicode digit — but it is
// neither an ASCII digit nor an identifier-start character, so it must fall
// through to the literal fallback rather than being read as a capture-group
// variable. Perl agrees: `$٣` is not a capture variable. An implementation
// that used `char::is_numeric` here would emit Variable("$٣") instead.
#[test]
fn dollar_digit_arm_uses_is_ascii_digit_and_rejects_a_non_ascii_unicode_digit() -> R {
    let parts = interpolated_parts("\"$\u{0663}\"").ok_or("no InterpolatedString")?;
    assert_eq!(
        parts,
        vec![StringPart::Literal(Arc::from("$\u{0663}"))],
        "\"$٣\" must stay literal, got {parts:?}"
    );
    Ok(())
}

// The control-variable arm only absorbs an *uppercase* letter after `$^`.
// Verified against real perl 5.38.2: `print "$^w"` prints the value of `$^`
// (the format-top-name variable, "STDOUT_TOP") immediately followed by a
// literal "w" -- perl does not read `$^w` as one control variable. A wrong
// implementation that dropped the `is_ascii_uppercase` guard would produce
// Variable("$^w") and lose the literal.
#[test]
fn dollar_caret_followed_by_a_lowercase_letter_leaves_the_letter_literal() -> R {
    let parts = interpolated_parts("\"$^w\"").ok_or("no InterpolatedString")?;
    assert_eq!(
        parts,
        vec![StringPart::Variable(Arc::from("$^")), StringPart::Literal(Arc::from("w")),],
        "\"$^w\" must be Variable(\"$^\") + Literal(\"w\"), got {parts:?}"
    );
    Ok(())
}

// The `$#$ref` deref tail folds `::`-qualified package names the same way the
// bare `$#array` scan does. Verified against real perl 5.38.2:
//   our @a=(1,2,3); our $ref=\@a; print "$#$main::ref"   # prints 2
// so `main::ref` is one name; splitting it would leave a literal "::ref".
#[test]
fn dollar_hash_dollar_ref_folds_a_package_qualified_name() -> R {
    let parts = interpolated_parts("\"$#$main::ref\"").ok_or("no InterpolatedString")?;
    assert_eq!(
        parts,
        vec![StringPart::Variable(Arc::from("$#$main::ref"))],
        "\"$#$main::ref\" must be one Variable part, got {parts:?}"
    );
    Ok(())
}

// A single colon after `$#$ref` is not a package separator and must end the
// name, pinning the `else if ch == ':' && peek == ':'` guard rather than a
// looser "any colon continues" reading.
#[test]
fn dollar_hash_dollar_ref_stops_at_a_single_colon() -> R {
    let parts = interpolated_parts("\"$#$ref:tail\"").ok_or("no InterpolatedString")?;
    assert_eq!(
        parts,
        vec![StringPart::Variable(Arc::from("$#$ref")), StringPart::Literal(Arc::from(":tail")),],
        "a lone ':' must end \"$#$ref\", got {parts:?}"
    );
    Ok(())
}

// Same guard on the bare `$#array` scan: `$#arr:tail` is `$#arr` plus literal
// text, while `$#main::arr` (covered above) folds the separator.
#[test]
fn dollar_hash_bare_array_stops_at_a_single_colon() -> R {
    let parts = interpolated_parts("\"$#arr:tail\"").ok_or("no InterpolatedString")?;
    assert_eq!(
        parts,
        vec![StringPart::Variable(Arc::from("$#arr")), StringPart::Literal(Arc::from(":tail")),],
        "a lone ':' must end \"$#arr\", got {parts:?}"
    );
    Ok(())
}

// `$#{...}` takes the brace sub-arm even though `{` could otherwise look like
// the start of a subscript, and the whole thing stays one Variable part.
// This complements the `$#{$ref}` test above with a plain (non-sigil) inner
// expression, which is the shape that would fragment first if the brace
// sub-arm were dropped.
#[test]
fn dollar_hash_brace_arm_keeps_a_plain_inner_expression_in_one_variable() -> R {
    let parts = interpolated_parts("\"$#{arr}\"").ok_or("no InterpolatedString")?;
    assert_eq!(
        parts,
        vec![StringPart::Variable(Arc::from("$#{arr}"))],
        "\"$#{{arr}}\" must be one Variable part, got {parts:?}"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// $: (format line-break set) vs. $:: (package-qualified name)
// ---------------------------------------------------------------------------

// `$:` is a real Perl punctuation variable and interpolates. Verified against
// real perl 5.38.2: `$: = "S"; print "[$:]"` prints "[S]".
#[test]
fn dollar_colon_in_string_emits_variable() -> R {
    let parts = interpolated_parts(r#""$:""#).ok_or("no InterpolatedString")?;
    assert_eq!(parts, vec![StringPart::Variable(Arc::from("$:"))]);
    Ok(())
}

// A single `:` claims only itself — the following identifier stays literal.
// Verified against real perl 5.38.2: `$: = "S"; print "[$:foo]"` prints
// "[Sfoo]", i.e. `$:` then the literal "foo", not the variable `$:foo`.
#[test]
fn dollar_colon_then_identifier_leaves_literal_tail() -> R {
    let parts = interpolated_parts(r#""$:foo""#).ok_or("no InterpolatedString")?;
    assert_eq!(
        parts,
        vec![StringPart::Variable(Arc::from("$:")), StringPart::Literal(Arc::from("foo"))],
        "\"$:foo\" must be Variable(\"$:\") + Literal(\"foo\"), got {parts:?}"
    );
    Ok(())
}

// `$::foo` is `$main::foo`, NOT `$:` followed by the literal ":foo". Verified
// against real perl 5.38.2: `$foo = "P"; print "[$::foo]"` prints "[P]".
#[test]
fn dollar_double_colon_is_a_package_qualified_variable() -> R {
    let parts = interpolated_parts(r#""$::foo""#).ok_or("no InterpolatedString")?;
    assert_eq!(
        parts,
        vec![StringPart::Variable(Arc::from("$::foo"))],
        "\"$::foo\" must be one Variable part, got {parts:?}"
    );
    Ok(())
}

// A bare `$::` is itself a variable (the `main::` stash name), not `$:` plus a
// literal ':'. Verified against real perl 5.38.2: `print "[$::]"` warns
// "Use of uninitialized value $main::" and prints "[]".
#[test]
fn dollar_double_colon_bare_is_one_variable() -> R {
    let parts = interpolated_parts(r#""$::""#).ok_or("no InterpolatedString")?;
    assert_eq!(
        parts,
        vec![StringPart::Variable(Arc::from("$::"))],
        "\"$::\" must be one Variable part, got {parts:?}"
    );
    Ok(())
}

// Three colons: perl folds the leading `::` pair and hands the odd colon back
// to the literal text. Verified against real perl 5.38.2: `print "[$:::foo]"`
// warns about `$main::` and prints "[:foo]".
#[test]
fn dollar_triple_colon_folds_only_the_leading_pair() -> R {
    let parts = interpolated_parts(r#""$:::foo""#).ok_or("no InterpolatedString")?;
    assert_eq!(
        parts,
        vec![StringPart::Variable(Arc::from("$::")), StringPart::Literal(Arc::from(":foo"))],
        "\"$:::foo\" must be Variable(\"$::\") + Literal(\":foo\"), got {parts:?}"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// #5428 — plain-identifier `$foo->bar` is literal, not a MethodCall.
//
// Real perl does not interpolate a bare method call inside a double-quoted
// string. `"$foo->bar"` interpolates `$foo` and prints `->bar` as literal
// text — the method is never called. Only arrow *subscripts* (`->[]`, `->{}`,
// `->()`) genuinely interpolate. This is the plain-identifier counterpart of
// `deref_chain_arrow_method_call_stays_literal` above; #5235 fixed the
// `$$`-deref-chain half and deliberately left this one for #5428.
//
// Verified against real perl 5.38.2:
//   package Foo; sub new { bless {}, shift } sub bar { "METHOD" }
//   package main; my $o = Foo->new; print "$o->bar"
//   # prints "Foo=HASH(0x..)->bar", never "METHOD"
// ---------------------------------------------------------------------------

#[test]
fn plain_identifier_arrow_method_call_stays_literal() -> R {
    let parts = interpolated_parts(r#""$foo->bar""#).ok_or("no InterpolatedString")?;
    assert_eq!(
        parts,
        vec![StringPart::Variable(Arc::from("$foo")), StringPart::Literal(Arc::from("->bar")),],
        "a bare arrow method after a plain identifier must stay literal, got {parts:?}"
    );
    Ok(())
}

#[test]
fn plain_identifier_arrow_method_call_with_args_stays_literal() -> R {
    // `$foo->method(arg)` — the `(arg)` is part of the (uncalled) method,
    // so the whole `->method(arg)` stays literal.
    let parts = interpolated_parts(r#""$foo->method(x)""#).ok_or("no InterpolatedString")?;
    assert_eq!(
        parts,
        vec![
            StringPart::Variable(Arc::from("$foo")),
            StringPart::Literal(Arc::from("->method(x)")),
        ],
        "a bare arrow method with args must stay literal, got {parts:?}"
    );
    Ok(())
}

#[test]
fn plain_identifier_arrow_subscripts_still_interpolate() -> R {
    // Positive guards: the fix must NOT over-correct. Arrow subscripts DO
    // interpolate and must keep their MethodCall classification.
    let parts = interpolated_parts(r#""$ar->[1]""#).ok_or("no InterpolatedString")?;
    assert_eq!(
        parts,
        vec![StringPart::Variable(Arc::from("$ar")), StringPart::MethodCall(Arc::from("->[1]")),],
        "an array arrow subscript must still interpolate, got {parts:?}"
    );

    let parts = interpolated_parts(r#""$hr->{k}""#).ok_or("no InterpolatedString")?;
    assert_eq!(
        parts,
        vec![StringPart::Variable(Arc::from("$hr")), StringPart::MethodCall(Arc::from("->{k}")),],
        "a hash arrow subscript must still interpolate, got {parts:?}"
    );

    let parts = interpolated_parts(r#""$fn->(1)""#).ok_or("no InterpolatedString")?;
    assert_eq!(
        parts,
        vec![StringPart::Variable(Arc::from("$fn")), StringPart::MethodCall(Arc::from("->(1)")),],
        "a coderef arrow call must still interpolate, got {parts:?}"
    );
    Ok(())
}

#[test]
fn plain_identifier_trailing_arrow_without_subscript_stays_literal() -> R {
    // `$foo->` at end of string: the arrow has no subscript or identifier, so
    // it stays literal (mirrors `deref_chain_trailing_arrow_without_subscript`
    // for the plain-identifier arm).
    let parts = interpolated_parts(r#""$foo->""#).ok_or("no InterpolatedString")?;
    assert_eq!(
        parts,
        vec![StringPart::Variable(Arc::from("$foo")), StringPart::Literal(Arc::from("->")),],
        "a trailing arrow with no subscript must stay literal, got {parts:?}"
    );
    Ok(())
}
