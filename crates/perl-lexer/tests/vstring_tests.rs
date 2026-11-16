//! Tests for v-string (version string) tokenization.
//!
//! V-strings are a Perl literal form like `v5.26.0` or `v5.10` commonly used
//! in `use`, `require`, and version declarations.

use perl_lexer::{PerlLexer, TokenType};

type R = Result<(), Box<dyn std::error::Error>>;

/// Collect only the significant (non-whitespace, non-newline, non-EOF) tokens.
fn significant(input: &str) -> Vec<perl_lexer::Token> {
    PerlLexer::new(input)
        .collect_tokens()
        .into_iter()
        .filter(|t| {
            !matches!(t.token_type, TokenType::Whitespace | TokenType::Newline | TokenType::EOF)
        })
        .collect()
}

// ===========================================================================
// Basic v-string recognition
// ===========================================================================

#[test]
fn test_vstring_three_part_version() -> R {
    let toks = significant("v5.26.0");
    assert_eq!(toks.len(), 1, "expected single token, got: {:?}", toks);
    assert!(
        matches!(&toks[0].token_type, TokenType::Version(v) if v.as_ref() == "v5.26.0"),
        "expected Version(v5.26.0), got: {:?}",
        toks[0].token_type
    );
    assert_eq!(toks[0].text.as_ref(), "v5.26.0");
    assert_eq!(toks[0].start, 0);
    assert_eq!(toks[0].end, 7);
    Ok(())
}

#[test]
fn test_vstring_two_part_version() -> R {
    let toks = significant("v5.10");
    assert_eq!(toks.len(), 1, "expected single token, got: {:?}", toks);
    assert!(
        matches!(&toks[0].token_type, TokenType::Version(v) if v.as_ref() == "v5.10"),
        "expected Version(v5.10), got: {:?}",
        toks[0].token_type
    );
    Ok(())
}

#[test]
fn test_vstring_four_part_version() -> R {
    let toks = significant("v1.2.3.4");
    assert_eq!(toks.len(), 1, "expected single token, got: {:?}", toks);
    assert!(
        matches!(&toks[0].token_type, TokenType::Version(v) if v.as_ref() == "v1.2.3.4"),
        "expected Version(v1.2.3.4), got: {:?}",
        toks[0].token_type
    );
    Ok(())
}

// ===========================================================================
// V-strings in context
// ===========================================================================

#[test]
fn test_vstring_in_use_statement() -> R {
    let toks = significant("use v5.26;");
    // Should have: use, v5.26, ;  -- but v5.26 needs a dot, so it qualifies
    // Actually "use" is a keyword, "v5.26" is a v-string, ";" is semicolon
    let version_tok = toks.iter().find(|t| matches!(&t.token_type, TokenType::Version(_)));
    assert!(
        version_tok.is_some(),
        "expected a Version token in 'use v5.26;', got: {:?}",
        toks.iter().map(|t| format!("{:?}", t.token_type)).collect::<Vec<_>>()
    );
    let vt = version_tok.ok_or("no version token")?;
    assert!(
        matches!(&vt.token_type, TokenType::Version(v) if v.as_ref() == "v5.26"),
        "expected Version(v5.26), got: {:?}",
        vt.token_type
    );
    Ok(())
}

#[test]
fn test_vstring_in_require_statement() -> R {
    let toks = significant("require v5.10.0;");
    let version_tok = toks.iter().find(|t| matches!(&t.token_type, TokenType::Version(_)));
    assert!(
        version_tok.is_some(),
        "expected a Version token in 'require v5.10.0;', got: {:?}",
        toks.iter().map(|t| format!("{:?}", t.token_type)).collect::<Vec<_>>()
    );
    Ok(())
}

#[test]
fn test_vstring_in_assignment() -> R {
    // $VERSION = v1.2.3;
    let toks = significant("$VERSION = v1.2.3;");
    let version_tok = toks.iter().find(|t| matches!(&t.token_type, TokenType::Version(_)));
    assert!(
        version_tok.is_some(),
        "expected a Version token in '$VERSION = v1.2.3;', got: {:?}",
        toks.iter().map(|t| format!("{:?}", t.token_type)).collect::<Vec<_>>()
    );
    Ok(())
}

// ===========================================================================
// Edge cases: things that should NOT be v-strings
// ===========================================================================

#[test]
fn test_bare_v_is_identifier() -> R {
    // Just "v" by itself is an identifier
    let toks = significant("v");
    assert_eq!(toks.len(), 1);
    assert!(
        matches!(&toks[0].token_type, TokenType::Identifier(_) | TokenType::Keyword(_)),
        "expected identifier for bare 'v', got: {:?}",
        toks[0].token_type
    );
    Ok(())
}

#[test]
fn test_v_followed_by_alpha_is_identifier() -> R {
    // "var" should be a normal identifier, not a v-string
    let toks = significant("var");
    assert_eq!(toks.len(), 1);
    assert!(
        matches!(&toks[0].token_type, TokenType::Identifier(_) | TokenType::Keyword(_)),
        "expected identifier for 'var', got: {:?}",
        toks[0].token_type
    );
    Ok(())
}

#[test]
fn test_v_digits_no_dot_is_vstring() -> R {
    // "v5" without a dot is a valid Perl v-string meaning chr(5)
    let toks = significant("v5");
    assert_eq!(toks.len(), 1, "expected single token, got: {:?}", toks);
    assert!(
        matches!(&toks[0].token_type, TokenType::Version(v) if v.as_ref() == "v5"),
        "expected Version(v5) for bare v-string, got: {:?}",
        toks[0].token_type
    );
    Ok(())
}

#[test]
fn test_v_digits_underscore_is_identifier() -> R {
    // "v5_test" should be a normal identifier
    let toks = significant("v5_test");
    assert_eq!(toks.len(), 1);
    assert!(
        matches!(&toks[0].token_type, TokenType::Identifier(_) | TokenType::Keyword(_)),
        "expected identifier for 'v5_test', got: {:?}",
        toks[0].token_type
    );
    Ok(())
}

#[test]
fn test_v_digits_alpha_is_identifier() -> R {
    // "v5x" should be a normal identifier
    let toks = significant("v5x");
    assert_eq!(toks.len(), 1);
    assert!(
        matches!(&toks[0].token_type, TokenType::Identifier(_) | TokenType::Keyword(_)),
        "expected identifier for 'v5x', got: {:?}",
        toks[0].token_type
    );
    Ok(())
}

// ===========================================================================
// Span correctness
// ===========================================================================

#[test]
fn test_vstring_span_in_statement() -> R {
    // "use v5.26.0;"
    //  0123456789...
    let toks = significant("use v5.26.0;");
    let version_tok = toks
        .iter()
        .find(|t| matches!(&t.token_type, TokenType::Version(_)))
        .ok_or("no version token found")?;
    assert_eq!(version_tok.start, 4, "v-string should start at byte 4");
    assert_eq!(version_tok.end, 11, "v-string should end at byte 11");
    Ok(())
}

// ===========================================================================
// Dot-terminated v-string: trailing dot not consumed
// ===========================================================================

#[test]
fn test_vstring_trailing_dot_not_consumed() -> R {
    // "v5.26." -- the trailing dot (not followed by digit) should NOT be part of v-string
    let toks = significant("v5.26.");
    // Should get: Version("v5.26"), then some token for "."
    let version_tok = toks.iter().find(|t| matches!(&t.token_type, TokenType::Version(_)));
    assert!(
        version_tok.is_some(),
        "expected Version token for 'v5.26.', got: {:?}",
        toks.iter().map(|t| format!("{:?}", t.token_type)).collect::<Vec<_>>()
    );
    let vt = version_tok.ok_or("no version token")?;
    assert!(
        matches!(&vt.token_type, TokenType::Version(v) if v.as_ref() == "v5.26"),
        "expected Version(v5.26), got: {:?}",
        vt.token_type
    );
    Ok(())
}

#[test]
fn test_vstring_large_version_numbers() -> R {
    let toks = significant("v536.100.200");
    assert_eq!(toks.len(), 1);
    assert!(
        matches!(&toks[0].token_type, TokenType::Version(v) if v.as_ref() == "v536.100.200"),
        "expected Version(v536.100.200), got: {:?}",
        toks[0].token_type
    );
    Ok(())
}

// ===========================================================================
// Autoquote gate: `vstring_is_autoquoted` (lib.rs `vstring_is_autoquoted`)
//
// A bareword shaped like a v-string in an autoquote position is a string key
// or name, not a v-string. Each test below names the brace-gate seam it
// discriminates: the whitespace walks, the byte before `{`, the sigil /
// closer arms, the identifier-name walk, the arrow-suffixed name arm, and
// the fallthroughs that leave a genuine v-string intact.
// ===========================================================================

/// Find the token carrying exactly `text` among the significant tokens.
fn token_with_text<'a>(toks: &'a [perl_lexer::Token], text: &str) -> Option<&'a perl_lexer::Token> {
    toks.iter().find(|t| t.text.as_ref() == text)
}

/// `$h{v5}` — the headline hash-subscript case: the name walk stops at the
/// `$` sigil (`name_start > 0` boundary), so the brace gate reports
/// autoquoted and the key stays an identifier.
#[test]
fn hash_subscript_bareword_after_sigiled_variable_is_identifier() -> R {
    let toks = significant("print $h{v5};");
    let key = token_with_text(&toks, "v5").ok_or("no 'v5' token in 'print $h{v5};'")?;
    assert!(
        matches!(&key.token_type, TokenType::Identifier(t) if t.as_ref() == "v5"),
        "hash key 'v5' must stay an identifier, got: {:?}",
        key.token_type
    );
    assert_eq!(key.start, 9, "key must span its exact bytes");
    assert_eq!(key.end, 11);
    Ok(())
}

/// `$h {v5}` — whitespace between the variable and the brace: the backward
/// whitespace walk before `{` must run (above the `inner > 0` boundary) and
/// still land on the identifier+sigil shape.
#[test]
fn whitespace_before_subscript_brace_still_autoquotes_bareword() -> R {
    let toks = significant("$h {v5}");
    let key = token_with_text(&toks, "v5").ok_or("no 'v5' token in '$h {v5}'")?;
    assert!(
        matches!(&key.token_type, TokenType::Identifier(t) if t.as_ref() == "v5"),
        "'$h {{v5}}' is still a subscript; 'v5' must stay an identifier, got: {:?}",
        key.token_type
    );
    assert_eq!(key.start, 4);
    assert_eq!(key.end, 6);
    Ok(())
}

/// `$my_hash{v5}` — the name walk must consume alphanumeric *and* underscore
/// name bytes (the `is_ascii_alphanumeric() || b'_'` predicate) before
/// stopping at the sigil.
#[test]
fn underscored_hash_name_walk_stops_at_sigil_and_autoquotes() -> R {
    let toks = significant("$my_hash{v5}");
    let key = token_with_text(&toks, "v5").ok_or("no 'v5' token in '$my_hash{{v5}}'")?;
    assert!(
        matches!(&key.token_type, TokenType::Identifier(t) if t.as_ref() == "v5"),
        "key of '$my_hash{{v5}}' must stay an identifier, got: {:?}",
        key.token_type
    );
    Ok(())
}

/// `@ARGV{v5}` — a hash slice on a multi-character `@`-sigiled name: the
/// name walk runs several iterations before hitting the sigil, which is the
/// below/equal/above boundary of the walk loop.
#[test]
fn hash_slice_bareword_after_at_array_is_identifier() -> R {
    let toks = significant("@ARGV{v5}");
    let key = token_with_text(&toks, "v5").ok_or("no 'v5' token in '@ARGV{{v5}}'")?;
    assert!(
        matches!(&key.token_type, TokenType::Identifier(t) if t.as_ref() == "v5"),
        "hash-slice key 'v5' must stay an identifier, got: {:?}",
        key.token_type
    );
    Ok(())
}

/// `$h{a}{v5}` — a `{` directly after a closing `}` (chained subscript):
/// the closer arm of the brace gate must fire.
#[test]
fn chained_hash_subscript_after_closing_brace_is_identifier() -> R {
    let toks = significant("$h{a}{v5}");
    let key = token_with_text(&toks, "v5").ok_or("no 'v5' token in '$h{{a}}{{v5}}'")?;
    assert!(
        matches!(&key.token_type, TokenType::Identifier(t) if t.as_ref() == "v5"),
        "chained-subscript key 'v5' must stay an identifier, got: {:?}",
        key.token_type
    );
    Ok(())
}

/// `$h->[0]{v5}` — a `{` directly after a closing `]`: the array-closer arm
/// must fire, and the same shape with intervening whitespace exercises the
/// whitespace walk landing on the closer.
#[test]
fn hash_subscript_after_array_closer_is_identifier() -> R {
    for input in ["$h->[0]{v5}", "$h->[0] {v5}"] {
        let toks = significant(input);
        let key =
            token_with_text(&toks, "v5").ok_or_else(|| format!("no 'v5' token in {input:?}"))?;
        assert!(
            matches!(&key.token_type, TokenType::Identifier(t) if t.as_ref() == "v5"),
            "'{input}' keys 'v5' after ']' must stay identifiers, got: {:?}",
            key.token_type
        );
    }
    Ok(())
}

/// `@{v5}` and `%{v5}` — dereference braces directly after a `@` or `%`
/// sigil: the sigil arms of the brace gate must fire so the deref name stays
/// an identifier. (`${v5}` is consumed wholesale by the variable scanner, so
/// the `$` arm is shadowed there; `@`/`%` are the reachable spellings.)
#[test]
fn deref_brace_bareword_after_sigil_is_identifier() -> R {
    for input in ["@{v5}", "%{v5}"] {
        let toks = significant(input);
        let name =
            token_with_text(&toks, "v5").ok_or_else(|| format!("no 'v5' token in {input:?}"))?;
        assert!(
            matches!(&name.token_type, TokenType::Identifier(t) if t.as_ref() == "v5"),
            "'{input}' deref name 'v5' must stay an identifier, got: {:?}",
            name.token_type
        );
    }
    Ok(())
}

/// `${v5}` — the symbolic-dereference spelling is consumed by the variable
/// scanner as one identifier token, so no bareword v-string candidate ever
/// appears inside it.
#[test]
fn symbolic_deref_braces_are_one_variable_token() -> R {
    let toks = significant("${v5}");
    assert_eq!(toks.len(), 1, "expected one token, got: {:?}", toks);
    assert!(
        matches!(&toks[0].token_type, TokenType::Identifier(t) if t.as_ref() == "${v5}"),
        "'${{v5}}' must be one variable token, got: {:?}",
        toks[0].token_type
    );
    Ok(())
}

/// `$h->{v5}` — the arrow arm: a `-` `>` immediately before the brace (the
/// `inner >= 2` boundary) means subscript, so the key stays an identifier.
#[test]
fn arrow_hash_subscript_bareword_is_identifier() -> R {
    let toks = significant("$h->{v5}");
    let key = token_with_text(&toks, "v5").ok_or("no 'v5' token in '$h->{{v5}}'")?;
    assert!(
        matches!(&key.token_type, TokenType::Identifier(t) if t.as_ref() == "v5"),
        "arrow-subscript key 'v5' must stay an identifier, got: {:?}",
        key.token_type
    );
    assert_eq!(key.start, 5);
    assert_eq!(key.end, 7);
    Ok(())
}

/// `$r->x{v5}` — an identifier name directly before `{` that is itself
/// preceded by `->`: the arrow-suffixed-name arm must fire.
#[test]
fn brace_after_arrow_suffixed_name_is_identifier() -> R {
    let toks = significant("$r->x{v5}");
    let key = token_with_text(&toks, "v5").ok_or("no 'v5' token in '$r->x{{v5}}'")?;
    assert!(
        matches!(&key.token_type, TokenType::Identifier(t) if t.as_ref() == "v5"),
        "subscript of an arrow-call result: 'v5' must stay an identifier, got: {:?}",
        key.token_type
    );
    Ok(())
}

/// `sub f { v5 }` — a brace whose preceding identifier carries neither a
/// sigil nor an arrow (a bare named block, not a subscript) is NOT an
/// autoquote position: the body keeps its genuine v-string.
#[test]
fn bare_sub_block_body_keeps_vstring() -> R {
    let toks = significant("sub f { v5 }");
    let body = token_with_text(&toks, "v5").ok_or("no 'v5' token in 'sub f {{ v5 }}'")?;
    assert!(
        matches!(&body.token_type, TokenType::Version(t) if t.as_ref() == "v5"),
        "bare block body 'v5' is a genuine v-string, got: {:?}",
        body.token_type
    );
    assert_eq!(body.start, 8);
    assert_eq!(body.end, 10);
    Ok(())
}

/// `{ v5 }` — a brace at the very start of input: the backward walk hits the
/// `inner > 0` boundary (below), so the gate cannot classify and the final
/// fallthrough leaves the genuine v-string.
#[test]
fn block_brace_at_input_start_keeps_vstring() -> R {
    let toks = significant("{ v5 }");
    let body = token_with_text(&toks, "v5").ok_or("no 'v5' token in '{{ v5 }}'")?;
    assert!(
        matches!(&body.token_type, TokenType::Version(t) if t.as_ref() == "v5"),
        "body of a leading bare block 'v5' is a genuine v-string, got: {:?}",
        body.token_type
    );
    Ok(())
}

/// `; { v5 }` — a brace preceded by `;`: the backward walk skips the
/// whitespace and lands on a byte that is neither closer, sigil, arrow, nor
/// identifier (the `_ => {{}}` fallthrough), so the v-string survives.
#[test]
fn block_brace_after_statement_semicolon_keeps_vstring() -> R {
    let toks = significant("; { v5 }");
    let body = token_with_text(&toks, "v5").ok_or("no 'v5' token in '; {{ v5 }}'")?;
    assert!(
        matches!(&body.token_type, TokenType::Version(t) if t.as_ref() == "v5"),
        "body of a semicolon-preceded bare block 'v5' is a genuine v-string, got: {:?}",
        body.token_type
    );
    Ok(())
}

/// Regression anchor for the call-site guard: a sub NAME that looks like a
/// v-string stays an identifier (`sub v5 {{ }}` declares, not versions).
#[test]
fn sub_named_like_a_vstring_is_identifier() -> R {
    let toks = significant("sub v5 { }");
    let name = token_with_text(&toks, "v5").ok_or("no 'v5' token in 'sub v5 {{ }}'")?;
    assert!(
        matches!(&name.token_type, TokenType::Identifier(t) if t.as_ref() == "v5"),
        "sub name 'v5' must stay an identifier, got: {:?}",
        name.token_type
    );
    Ok(())
}
