#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]
//! Extended unit tests for the `perl-token` crate.
//!
//! Supplements `comprehensive_unit_tests.rs` with additional coverage for:
//! - Token span arithmetic and consistency
//! - Checked kind/span builders (geometry fields are sealed)
//! - Arc sharing across multiple clones and independent allocations
//! - TokenKind memory layout and size guarantees
//! - TokenKind Debug format specifics per variant
//! - Category-based grouping helpers
//! - Complex Perl idiom token sequences
//! - Edge cases: whitespace variants, escapes, high-bit ASCII, long text
//! - Iterator / collection patterns
//! - Reflexivity, symmetry, transitivity of PartialEq
#![deny(clippy::map_err_ignore)] // Cohort C0 activation (#12598): census-clean on all targets; new findings move the crate to C1.

use perl_token::{Token, TokenKind};
use std::sync::Arc;

// ===========================================================================
// Token span arithmetic
// ===========================================================================

#[test]
fn token_span_length_matches_text_len() {
    let cases: &[(&str, TokenKind)] = &[
        ("my", TokenKind::My),
        ("$", TokenKind::ScalarSigil),
        ("some_long_identifier_name", TokenKind::Identifier),
        ("s/foo/bar/g", TokenKind::Substitution),
        ("", TokenKind::Eof),
    ];
    for (text, kind) in cases {
        let tok = Token::new_checked(*kind, *text, 10, 10 + text.len()).expect("valid token");
        assert_eq!(tok.end() - tok.start(), tok.text.len(), "span mismatch for {text:?}");
    }
}

#[test]
fn token_span_adjacent_tokens_no_gap() {
    let a = Token::new_checked(TokenKind::My, "my", 0, 2).expect("valid token");
    let b = Token::new_checked(TokenKind::ScalarSigil, "$", 2, 3).expect("valid token");
    assert_eq!(a.end(), b.start());
}

#[test]
fn token_span_with_gap_for_whitespace() {
    let a = Token::new_checked(TokenKind::My, "my", 0, 2).expect("valid token");
    let b = Token::new_checked(TokenKind::ScalarSigil, "$", 3, 4).expect("valid token");
    assert_eq!(b.start() - a.end(), 1);
}

// ===========================================================================
// Checked builders (geometry is sealed)
// ===========================================================================

#[test]
fn token_with_kind_is_the_supported_kind_change() {
    let tok = Token::new_checked(TokenKind::Identifier, "eval", 0, 4).expect("valid token");
    let retyped = tok.with_kind(TokenKind::Eval).expect("kind change preserves span");
    assert_eq!(retyped.kind(), TokenKind::Eval);
    assert_eq!(retyped.start(), 0);
    assert_eq!(retyped.end(), 4);
}

#[test]
fn token_with_span_is_the_supported_geometry_change() {
    let tok = Token::new_checked(TokenKind::Number, "42", 0, 2).expect("valid token");
    let moved = tok.with_span(100, 102).expect("ordered span");
    assert_eq!(moved.start(), 100);
    assert_eq!(moved.end(), 102);
    assert_eq!(&*moved.text, "42");
}

#[test]
fn token_text_field_is_replaceable() {
    let mut tok = Token::new_checked(TokenKind::String, "old", 0, 3).expect("valid token");
    tok.text = Arc::from("new");
    assert_eq!(&*tok.text, "new");
}

// ===========================================================================
// Arc sharing — multiple clones
// ===========================================================================

#[test]
fn arc_strong_count_with_three_clones() {
    let original =
        Token::new_checked(TokenKind::Identifier, "shared_text", 0, 11).expect("valid token");
    let c1 = original.clone();
    let c2 = original.clone();
    let c3 = original.clone();
    assert_eq!(Arc::strong_count(&original.text), 4);
    assert!(Arc::ptr_eq(&c1.text, &c2.text));
    assert!(Arc::ptr_eq(&c2.text, &c3.text));
}

#[test]
fn arc_count_decreases_on_drop() {
    let original = Token::new_checked(TokenKind::Sub, "sub", 0, 3).expect("valid token");
    let arc_ref = original.text.clone();
    assert_eq!(Arc::strong_count(&arc_ref), 2);
    drop(original);
    assert_eq!(Arc::strong_count(&arc_ref), 1);
}

#[test]
fn independent_arc_allocations_are_equal_but_not_ptr_eq() {
    let a = Token::new_checked(TokenKind::Identifier, "foo", 0, 3).expect("valid token");
    let b = Token::new_checked(TokenKind::Identifier, "foo", 0, 3).expect("valid token");
    assert_eq!(a, b);
    // Different allocations: ptr_eq should be false
    assert!(!Arc::ptr_eq(&a.text, &b.text));
}

// ===========================================================================
// TokenKind — memory layout
// ===========================================================================

#[test]
fn token_kind_size_is_one_byte() {
    // Enum with < 256 variants should fit in a single byte
    assert_eq!(std::mem::size_of::<TokenKind>(), 1);
}

#[test]
fn token_kind_alignment_is_one() {
    assert_eq!(std::mem::align_of::<TokenKind>(), 1);
}

#[test]
fn option_token_kind_is_one_byte() {
    // With niche optimization, Option<TokenKind> should still be 1 byte
    assert_eq!(std::mem::size_of::<Option<TokenKind>>(), 1);
}

// ===========================================================================
// TokenKind — Debug format specifics
// ===========================================================================

#[test]
fn token_kind_debug_keywords() {
    let cases: &[(TokenKind, &str)] = &[
        (TokenKind::My, "My"),
        (TokenKind::Our, "Our"),
        (TokenKind::Local, "Local"),
        (TokenKind::State, "State"),
        (TokenKind::Sub, "Sub"),
        (TokenKind::If, "If"),
        (TokenKind::Elsif, "Elsif"),
        (TokenKind::Else, "Else"),
        (TokenKind::Unless, "Unless"),
        (TokenKind::While, "While"),
        (TokenKind::Until, "Until"),
        (TokenKind::For, "For"),
        (TokenKind::Foreach, "Foreach"),
        (TokenKind::Return, "Return"),
        (TokenKind::Package, "Package"),
        (TokenKind::Use, "Use"),
        (TokenKind::No, "No"),
        (TokenKind::Begin, "Begin"),
        (TokenKind::End, "End"),
        (TokenKind::Check, "Check"),
        (TokenKind::Init, "Init"),
        (TokenKind::Unitcheck, "Unitcheck"),
        (TokenKind::Eval, "Eval"),
        (TokenKind::Do, "Do"),
        (TokenKind::Given, "Given"),
        (TokenKind::When, "When"),
        (TokenKind::Default, "Default"),
        (TokenKind::Try, "Try"),
        (TokenKind::Catch, "Catch"),
        (TokenKind::Finally, "Finally"),
        (TokenKind::Continue, "Continue"),
        (TokenKind::Next, "Next"),
        (TokenKind::Last, "Last"),
        (TokenKind::Redo, "Redo"),
        (TokenKind::Goto, "Goto"),
        (TokenKind::Class, "Class"),
        (TokenKind::Method, "Method"),
        (TokenKind::Field, "Field"),
        (TokenKind::Format, "Format"),
        (TokenKind::Undef, "Undef"),
    ];
    for (kind, label) in cases {
        assert_eq!(format!("{kind:?}"), *label);
    }
}

#[test]
fn token_kind_debug_operators() {
    let cases: &[(TokenKind, &str)] = &[
        (TokenKind::Assign, "Assign"),
        (TokenKind::Plus, "Plus"),
        (TokenKind::Minus, "Minus"),
        (TokenKind::Star, "Star"),
        (TokenKind::Slash, "Slash"),
        (TokenKind::Percent, "Percent"),
        (TokenKind::Power, "Power"),
        (TokenKind::Arrow, "Arrow"),
        (TokenKind::FatArrow, "FatArrow"),
        (TokenKind::Dot, "Dot"),
        (TokenKind::Range, "Range"),
        (TokenKind::Ellipsis, "Ellipsis"),
        (TokenKind::Spaceship, "Spaceship"),
        (TokenKind::SmartMatch, "SmartMatch"),
        (TokenKind::DefinedOr, "DefinedOr"),
        (TokenKind::Increment, "Increment"),
        (TokenKind::Decrement, "Decrement"),
    ];
    for (kind, label) in cases {
        assert_eq!(format!("{kind:?}"), *label);
    }
}

#[test]
fn token_kind_debug_delimiters() {
    let cases: &[(TokenKind, &str)] = &[
        (TokenKind::LeftParen, "LeftParen"),
        (TokenKind::RightParen, "RightParen"),
        (TokenKind::LeftBrace, "LeftBrace"),
        (TokenKind::RightBrace, "RightBrace"),
        (TokenKind::LeftBracket, "LeftBracket"),
        (TokenKind::RightBracket, "RightBracket"),
        (TokenKind::Semicolon, "Semicolon"),
        (TokenKind::Comma, "Comma"),
    ];
    for (kind, label) in cases {
        assert_eq!(format!("{kind:?}"), *label);
    }
}

#[test]
fn token_kind_debug_literals() {
    let cases: &[(TokenKind, &str)] = &[
        (TokenKind::Number, "Number"),
        (TokenKind::String, "String"),
        (TokenKind::Regex, "Regex"),
        (TokenKind::Substitution, "Substitution"),
        (TokenKind::Transliteration, "Transliteration"),
        (TokenKind::QuoteSingle, "QuoteSingle"),
        (TokenKind::QuoteDouble, "QuoteDouble"),
        (TokenKind::QuoteWords, "QuoteWords"),
        (TokenKind::QuoteCommand, "QuoteCommand"),
        (TokenKind::HeredocStart, "HeredocStart"),
        (TokenKind::HeredocBody, "HeredocBody"),
        (TokenKind::FormatBody, "FormatBody"),
        (TokenKind::DataMarker, "DataMarker"),
        (TokenKind::DataBody, "DataBody"),
        (TokenKind::UnknownRest, "UnknownRest"),
        (TokenKind::HeredocDepthLimit, "HeredocDepthLimit"),
    ];
    for (kind, label) in cases {
        assert_eq!(format!("{kind:?}"), *label);
    }
}

#[test]
fn token_kind_debug_identifiers_and_special() {
    let cases: &[(TokenKind, &str)] = &[
        (TokenKind::Identifier, "Identifier"),
        (TokenKind::ScalarSigil, "ScalarSigil"),
        (TokenKind::ArraySigil, "ArraySigil"),
        (TokenKind::HashSigil, "HashSigil"),
        (TokenKind::SubSigil, "SubSigil"),
        (TokenKind::GlobSigil, "GlobSigil"),
        (TokenKind::Eof, "Eof"),
        (TokenKind::Unknown, "Unknown"),
    ];
    for (kind, label) in cases {
        assert_eq!(format!("{kind:?}"), *label);
    }
}

// ===========================================================================
// Token Debug format
// ===========================================================================

#[test]
fn token_debug_contains_all_fields() {
    let t = Token::new_checked(TokenKind::Number, "42", 10, 12).expect("valid token");
    let dbg = format!("{t:?}");
    assert!(dbg.contains("Number"), "missing kind in: {dbg}");
    assert!(dbg.contains("42"), "missing text in: {dbg}");
    assert!(dbg.contains("10"), "missing start in: {dbg}");
    assert!(dbg.contains("12"), "missing end in: {dbg}");
}

#[test]
fn token_debug_alternate_format() {
    let t = Token::new_checked(TokenKind::My, "my", 0, 2).expect("valid token");
    let dbg = format!("{t:#?}");
    // Alternate format should be multi-line
    assert!(dbg.contains('\n'), "alternate debug should be multi-line: {dbg}");
    assert!(dbg.contains("My"), "missing kind in: {dbg}");
}

// ===========================================================================
// PartialEq — transitivity and edge cases
// ===========================================================================

#[test]
fn token_eq_transitivity() {
    let a = Token::new_checked(TokenKind::Plus, "+", 5, 6).expect("valid token");
    let b = Token::new_checked(TokenKind::Plus, "+", 5, 6).expect("valid token");
    let c = Token::new_checked(TokenKind::Plus, "+", 5, 6).expect("valid token");
    assert_eq!(a, b);
    assert_eq!(b, c);
    assert_eq!(a, c);
}

#[test]
fn token_eq_reflexive_for_all_kinds() {
    let kinds = [
        TokenKind::My,
        TokenKind::Assign,
        TokenKind::LeftParen,
        TokenKind::Number,
        TokenKind::Identifier,
        TokenKind::Eof,
        TokenKind::Unknown,
        TokenKind::HeredocBody,
        TokenKind::Class,
        TokenKind::Try,
    ];
    for kind in &kinds {
        let tok = Token::new_checked(*kind, "x", 0, 1).expect("valid token");
        assert_eq!(tok, tok.clone());
    }
}

#[test]
fn token_ne_only_text_differs() {
    let a = Token::new_checked(TokenKind::Identifier, "foo", 0, 3).expect("valid token");
    let b = Token::new_checked(TokenKind::Identifier, "bar", 0, 3).expect("valid token");
    assert_ne!(a, b);
}

// ===========================================================================
// TokenKind — category grouping tests
// ===========================================================================

fn keyword_kinds() -> Vec<TokenKind> {
    vec![
        TokenKind::My,
        TokenKind::Our,
        TokenKind::Local,
        TokenKind::State,
        TokenKind::Sub,
        TokenKind::If,
        TokenKind::Elsif,
        TokenKind::Else,
        TokenKind::Unless,
        TokenKind::While,
        TokenKind::Until,
        TokenKind::For,
        TokenKind::Foreach,
        TokenKind::Return,
        TokenKind::Package,
        TokenKind::Use,
        TokenKind::No,
        TokenKind::Begin,
        TokenKind::End,
        TokenKind::Check,
        TokenKind::Init,
        TokenKind::Unitcheck,
        TokenKind::Eval,
        TokenKind::Do,
        TokenKind::Given,
        TokenKind::When,
        TokenKind::Default,
        TokenKind::Try,
        TokenKind::Catch,
        TokenKind::Finally,
        TokenKind::Continue,
        TokenKind::Next,
        TokenKind::Last,
        TokenKind::Redo,
        TokenKind::Goto,
        TokenKind::Class,
        TokenKind::Method,
        TokenKind::Field,
        TokenKind::Format,
        TokenKind::Undef,
    ]
}

fn delimiter_kinds() -> Vec<TokenKind> {
    vec![
        TokenKind::LeftParen,
        TokenKind::RightParen,
        TokenKind::LeftBrace,
        TokenKind::RightBrace,
        TokenKind::LeftBracket,
        TokenKind::RightBracket,
        TokenKind::Semicolon,
        TokenKind::Comma,
    ]
}

fn sigil_kinds() -> Vec<TokenKind> {
    vec![
        TokenKind::ScalarSigil,
        TokenKind::ArraySigil,
        TokenKind::HashSigil,
        TokenKind::SubSigil,
        TokenKind::GlobSigil,
    ]
}

#[test]
fn keyword_count_is_40() {
    assert_eq!(keyword_kinds().len(), 40);
}

#[test]
fn delimiter_count_is_8() {
    assert_eq!(delimiter_kinds().len(), 8);
}

#[test]
fn sigil_count_is_5() {
    assert_eq!(sigil_kinds().len(), 5);
}

#[test]
fn keywords_are_disjoint_from_delimiters() {
    for kw in keyword_kinds() {
        for delim in delimiter_kinds() {
            assert_ne!(kw, delim);
        }
    }
}

#[test]
fn sigils_are_disjoint_from_keywords() {
    for sigil in sigil_kinds() {
        for kw in keyword_kinds() {
            assert_ne!(sigil, kw);
        }
    }
}

// ===========================================================================
// TokenKind Copy chain
// ===========================================================================

#[test]
fn token_kind_copy_chain() {
    let a = TokenKind::Arrow;
    let b = a;
    let c = b;
    let d = c;
    assert_eq!(a, d);
}

#[test]
fn token_kind_copy_into_closure() {
    let kind = TokenKind::Sub;
    let f = move || kind;
    assert_eq!(f(), TokenKind::Sub);
    // Original is still usable (Copy)
    assert_eq!(kind, TokenKind::Sub);
}

// ===========================================================================
// Token::new — various Into<Arc<str>> conversions
// ===========================================================================

#[test]
fn token_new_from_static_str() {
    let t = Token::new_checked(TokenKind::Identifier, "static", 0, 6).expect("valid token");
    assert_eq!(&*t.text, "static");
}

#[test]
fn token_new_from_owned_string() {
    let s = "dynamic".to_string();
    let t = Token::new_checked(TokenKind::Identifier, s, 0, 7).expect("valid token");
    assert_eq!(&*t.text, "dynamic");
}

#[test]
fn token_new_from_boxed_str() {
    let boxed: Box<str> = "boxed".into();
    let arc: Arc<str> = Arc::from(boxed);
    let t = Token::new_checked(TokenKind::String, arc, 0, 5).expect("valid token");
    assert_eq!(&*t.text, "boxed");
}

// ===========================================================================
// Complex Perl idiom sequences
// ===========================================================================

#[test]
fn token_sequence_hash_access() {
    // $hash{key}
    let tokens = [
        Token::new_checked(TokenKind::ScalarSigil, "$", 0, 1).expect("valid token"),
        Token::new_checked(TokenKind::Identifier, "hash", 1, 5).expect("valid token"),
        Token::new_checked(TokenKind::LeftBrace, "{", 5, 6).expect("valid token"),
        Token::new_checked(TokenKind::Identifier, "key", 6, 9).expect("valid token"),
        Token::new_checked(TokenKind::RightBrace, "}", 9, 10).expect("valid token"),
    ];
    assert_eq!(tokens.len(), 5);
    assert_eq!(tokens[0].kind(), TokenKind::ScalarSigil);
    assert_eq!(tokens[2].kind(), TokenKind::LeftBrace);
    assert_eq!(tokens[4].kind(), TokenKind::RightBrace);
}

#[test]
fn token_sequence_array_slice() {
    // @array[0..2]
    let tokens = [
        Token::new_checked(TokenKind::ArraySigil, "@", 0, 1).expect("valid token"),
        Token::new_checked(TokenKind::Identifier, "array", 1, 6).expect("valid token"),
        Token::new_checked(TokenKind::LeftBracket, "[", 6, 7).expect("valid token"),
        Token::new_checked(TokenKind::Number, "0", 7, 8).expect("valid token"),
        Token::new_checked(TokenKind::Range, "..", 8, 10).expect("valid token"),
        Token::new_checked(TokenKind::Number, "2", 10, 11).expect("valid token"),
        Token::new_checked(TokenKind::RightBracket, "]", 11, 12).expect("valid token"),
    ];
    assert_eq!(tokens.len(), 7);
    assert_eq!(tokens[4].kind(), TokenKind::Range);
}

#[test]
fn token_sequence_if_elsif_else() {
    // if (...) { } elsif (...) { } else { }
    let tokens = vec![
        Token::new_checked(TokenKind::If, "if", 0, 2).expect("valid token"),
        Token::new_checked(TokenKind::LeftParen, "(", 3, 4).expect("valid token"),
        Token::new_checked(TokenKind::RightParen, ")", 4, 5).expect("valid token"),
        Token::new_checked(TokenKind::LeftBrace, "{", 6, 7).expect("valid token"),
        Token::new_checked(TokenKind::RightBrace, "}", 8, 9).expect("valid token"),
        Token::new_checked(TokenKind::Elsif, "elsif", 10, 15).expect("valid token"),
        Token::new_checked(TokenKind::LeftParen, "(", 16, 17).expect("valid token"),
        Token::new_checked(TokenKind::RightParen, ")", 17, 18).expect("valid token"),
        Token::new_checked(TokenKind::LeftBrace, "{", 19, 20).expect("valid token"),
        Token::new_checked(TokenKind::RightBrace, "}", 21, 22).expect("valid token"),
        Token::new_checked(TokenKind::Else, "else", 23, 27).expect("valid token"),
        Token::new_checked(TokenKind::LeftBrace, "{", 28, 29).expect("valid token"),
        Token::new_checked(TokenKind::RightBrace, "}", 30, 31).expect("valid token"),
    ];
    assert_eq!(tokens.len(), 13);
    assert_eq!(tokens[0].kind(), TokenKind::If);
    assert_eq!(tokens[5].kind(), TokenKind::Elsif);
    assert_eq!(tokens[10].kind(), TokenKind::Else);
}

#[test]
fn token_sequence_use_module() {
    // use strict;
    let tokens = [
        Token::new_checked(TokenKind::Use, "use", 0, 3).expect("valid token"),
        Token::new_checked(TokenKind::Identifier, "strict", 4, 10).expect("valid token"),
        Token::new_checked(TokenKind::Semicolon, ";", 10, 11).expect("valid token"),
    ];
    assert_eq!(tokens[0].kind(), TokenKind::Use);
    assert_eq!(&*tokens[1].text, "strict");
}

#[test]
fn token_sequence_fat_comma_pair() {
    // key => "value"
    let tokens = [
        Token::new_checked(TokenKind::Identifier, "key", 0, 3).expect("valid token"),
        Token::new_checked(TokenKind::FatArrow, "=>", 4, 6).expect("valid token"),
        Token::new_checked(TokenKind::String, "\"value\"", 7, 14).expect("valid token"),
    ];
    assert_eq!(tokens[1].kind(), TokenKind::FatArrow);
}

#[test]
fn token_sequence_ternary_operator() {
    // $x ? 1 : 0
    let tokens = [
        Token::new_checked(TokenKind::ScalarSigil, "$", 0, 1).expect("valid token"),
        Token::new_checked(TokenKind::Identifier, "x", 1, 2).expect("valid token"),
        Token::new_checked(TokenKind::Question, "?", 3, 4).expect("valid token"),
        Token::new_checked(TokenKind::Number, "1", 5, 6).expect("valid token"),
        Token::new_checked(TokenKind::Colon, ":", 7, 8).expect("valid token"),
        Token::new_checked(TokenKind::Number, "0", 9, 10).expect("valid token"),
    ];
    assert_eq!(tokens[2].kind(), TokenKind::Question);
    assert_eq!(tokens[4].kind(), TokenKind::Colon);
}

#[test]
fn token_sequence_regex_match() {
    // $str =~ /pattern/i
    let tokens = [
        Token::new_checked(TokenKind::ScalarSigil, "$", 0, 1).expect("valid token"),
        Token::new_checked(TokenKind::Identifier, "str", 1, 4).expect("valid token"),
        Token::new_checked(TokenKind::Match, "=~", 5, 7).expect("valid token"),
        Token::new_checked(TokenKind::Regex, "/pattern/i", 8, 18).expect("valid token"),
    ];
    assert_eq!(tokens[2].kind(), TokenKind::Match);
    assert_eq!(tokens[3].kind(), TokenKind::Regex);
}

#[test]
fn token_sequence_chained_arrow_deref() {
    // $obj->method->field
    let tokens = [
        Token::new_checked(TokenKind::ScalarSigil, "$", 0, 1).expect("valid token"),
        Token::new_checked(TokenKind::Identifier, "obj", 1, 4).expect("valid token"),
        Token::new_checked(TokenKind::Arrow, "->", 4, 6).expect("valid token"),
        Token::new_checked(TokenKind::Identifier, "method", 6, 12).expect("valid token"),
        Token::new_checked(TokenKind::Arrow, "->", 12, 14).expect("valid token"),
        Token::new_checked(TokenKind::Identifier, "field", 14, 19).expect("valid token"),
    ];
    let arrows: Vec<_> = tokens.iter().filter(|t| t.kind() == TokenKind::Arrow).collect();
    assert_eq!(arrows.len(), 2);
}

#[test]
fn token_sequence_defined_or_assign() {
    // $x //= "default"
    let tokens = [
        Token::new_checked(TokenKind::ScalarSigil, "$", 0, 1).expect("valid token"),
        Token::new_checked(TokenKind::Identifier, "x", 1, 2).expect("valid token"),
        Token::new_checked(TokenKind::DefinedOrAssign, "//=", 3, 6).expect("valid token"),
        Token::new_checked(TokenKind::String, "\"default\"", 7, 16).expect("valid token"),
    ];
    assert_eq!(tokens[2].kind(), TokenKind::DefinedOrAssign);
}

#[test]
fn token_sequence_try_catch_finally() {
    // try { } catch ($e) { } finally { }
    let tokens = vec![
        Token::new_checked(TokenKind::Try, "try", 0, 3).expect("valid token"),
        Token::new_checked(TokenKind::LeftBrace, "{", 4, 5).expect("valid token"),
        Token::new_checked(TokenKind::RightBrace, "}", 6, 7).expect("valid token"),
        Token::new_checked(TokenKind::Catch, "catch", 8, 13).expect("valid token"),
        Token::new_checked(TokenKind::LeftParen, "(", 14, 15).expect("valid token"),
        Token::new_checked(TokenKind::ScalarSigil, "$", 15, 16).expect("valid token"),
        Token::new_checked(TokenKind::Identifier, "e", 16, 17).expect("valid token"),
        Token::new_checked(TokenKind::RightParen, ")", 17, 18).expect("valid token"),
        Token::new_checked(TokenKind::LeftBrace, "{", 19, 20).expect("valid token"),
        Token::new_checked(TokenKind::RightBrace, "}", 21, 22).expect("valid token"),
        Token::new_checked(TokenKind::Finally, "finally", 23, 30).expect("valid token"),
        Token::new_checked(TokenKind::LeftBrace, "{", 31, 32).expect("valid token"),
        Token::new_checked(TokenKind::RightBrace, "}", 33, 34).expect("valid token"),
    ];
    assert_eq!(tokens[0].kind(), TokenKind::Try);
    assert_eq!(tokens[3].kind(), TokenKind::Catch);
    assert_eq!(tokens[10].kind(), TokenKind::Finally);
}

#[test]
fn token_sequence_class_method_perl538() {
    // class Foo { method bar { } }
    let tokens = [
        Token::new_checked(TokenKind::Class, "class", 0, 5).expect("valid token"),
        Token::new_checked(TokenKind::Identifier, "Foo", 6, 9).expect("valid token"),
        Token::new_checked(TokenKind::LeftBrace, "{", 10, 11).expect("valid token"),
        Token::new_checked(TokenKind::Method, "method", 12, 18).expect("valid token"),
        Token::new_checked(TokenKind::Identifier, "bar", 19, 22).expect("valid token"),
        Token::new_checked(TokenKind::LeftBrace, "{", 23, 24).expect("valid token"),
        Token::new_checked(TokenKind::RightBrace, "}", 25, 26).expect("valid token"),
        Token::new_checked(TokenKind::RightBrace, "}", 27, 28).expect("valid token"),
    ];
    assert_eq!(tokens[0].kind(), TokenKind::Class);
    assert_eq!(tokens[3].kind(), TokenKind::Method);
}

#[test]
fn token_sequence_heredoc() {
    // <<EOF\ncontent\nEOF
    let tokens = [
        Token::new_checked(TokenKind::HeredocStart, "<<EOF", 0, 5).expect("valid token"),
        Token::new_checked(TokenKind::HeredocBody, "content\n", 6, 14).expect("valid token"),
    ];
    assert_eq!(tokens[0].kind(), TokenKind::HeredocStart);
    assert_eq!(tokens[1].kind(), TokenKind::HeredocBody);
    assert!(tokens[1].text.contains('\n'));
}

#[test]
fn token_sequence_for_loop() {
    // for (my $i = 0; $i < 10; $i++) { }
    let tokens = vec![
        Token::new_checked(TokenKind::For, "for", 0, 3).expect("valid token"),
        Token::new_checked(TokenKind::LeftParen, "(", 4, 5).expect("valid token"),
        Token::new_checked(TokenKind::My, "my", 5, 7).expect("valid token"),
        Token::new_checked(TokenKind::ScalarSigil, "$", 8, 9).expect("valid token"),
        Token::new_checked(TokenKind::Identifier, "i", 9, 10).expect("valid token"),
        Token::new_checked(TokenKind::Assign, "=", 11, 12).expect("valid token"),
        Token::new_checked(TokenKind::Number, "0", 13, 14).expect("valid token"),
        Token::new_checked(TokenKind::Semicolon, ";", 14, 15).expect("valid token"),
        Token::new_checked(TokenKind::ScalarSigil, "$", 16, 17).expect("valid token"),
        Token::new_checked(TokenKind::Identifier, "i", 17, 18).expect("valid token"),
        Token::new_checked(TokenKind::Less, "<", 19, 20).expect("valid token"),
        Token::new_checked(TokenKind::Number, "10", 21, 23).expect("valid token"),
        Token::new_checked(TokenKind::Semicolon, ";", 23, 24).expect("valid token"),
        Token::new_checked(TokenKind::ScalarSigil, "$", 25, 26).expect("valid token"),
        Token::new_checked(TokenKind::Identifier, "i", 26, 27).expect("valid token"),
        Token::new_checked(TokenKind::Increment, "++", 27, 29).expect("valid token"),
        Token::new_checked(TokenKind::RightParen, ")", 29, 30).expect("valid token"),
        Token::new_checked(TokenKind::LeftBrace, "{", 31, 32).expect("valid token"),
        Token::new_checked(TokenKind::RightBrace, "}", 33, 34).expect("valid token"),
    ];
    assert_eq!(tokens[0].kind(), TokenKind::For);
    assert_eq!(tokens[15].kind(), TokenKind::Increment);
}

#[test]
fn token_sequence_while_with_loop_control() {
    // while (1) { next if $skip; last; }
    let tokens = vec![
        Token::new_checked(TokenKind::While, "while", 0, 5).expect("valid token"),
        Token::new_checked(TokenKind::LeftParen, "(", 6, 7).expect("valid token"),
        Token::new_checked(TokenKind::Number, "1", 7, 8).expect("valid token"),
        Token::new_checked(TokenKind::RightParen, ")", 8, 9).expect("valid token"),
        Token::new_checked(TokenKind::LeftBrace, "{", 10, 11).expect("valid token"),
        Token::new_checked(TokenKind::Next, "next", 12, 16).expect("valid token"),
        Token::new_checked(TokenKind::If, "if", 17, 19).expect("valid token"),
        Token::new_checked(TokenKind::ScalarSigil, "$", 20, 21).expect("valid token"),
        Token::new_checked(TokenKind::Identifier, "skip", 21, 25).expect("valid token"),
        Token::new_checked(TokenKind::Semicolon, ";", 25, 26).expect("valid token"),
        Token::new_checked(TokenKind::Last, "last", 27, 31).expect("valid token"),
        Token::new_checked(TokenKind::Semicolon, ";", 31, 32).expect("valid token"),
        Token::new_checked(TokenKind::RightBrace, "}", 33, 34).expect("valid token"),
    ];
    assert_eq!(tokens[5].kind(), TokenKind::Next);
    assert_eq!(tokens[10].kind(), TokenKind::Last);
}

#[test]
fn token_sequence_package_declaration() {
    // package Foo::Bar;
    let tokens = [
        Token::new_checked(TokenKind::Package, "package", 0, 7).expect("valid token"),
        Token::new_checked(TokenKind::Identifier, "Foo", 8, 11).expect("valid token"),
        Token::new_checked(TokenKind::DoubleColon, "::", 11, 13).expect("valid token"),
        Token::new_checked(TokenKind::Identifier, "Bar", 13, 16).expect("valid token"),
        Token::new_checked(TokenKind::Semicolon, ";", 16, 17).expect("valid token"),
    ];
    assert_eq!(tokens[0].kind(), TokenKind::Package);
    assert_eq!(tokens[2].kind(), TokenKind::DoubleColon);
}

// ===========================================================================
// Edge cases: whitespace, escapes, special chars
// ===========================================================================

#[test]
fn token_text_with_tab_characters() {
    let tok =
        Token::new_checked(TokenKind::String, "col1\tcol2\tcol3", 0, 14).expect("valid token");
    assert!(tok.text.contains('\t'));
}

#[test]
fn token_text_with_carriage_return() {
    let tok = Token::new_checked(TokenKind::String, "line\r\n", 0, 6).expect("valid token");
    assert!(tok.text.contains('\r'));
}

#[test]
fn token_text_with_only_whitespace() {
    let tok = Token::new_checked(TokenKind::String, "   \t\n  ", 0, 7).expect("valid token");
    assert_eq!(tok.text.trim(), "");
}

#[test]
fn token_text_with_backslash_sequences() {
    let tok =
        Token::new_checked(TokenKind::String, r#"hello\nworld\t!"#, 0, 15).expect("valid token");
    assert!(tok.text.contains('\\'));
}

#[test]
fn token_text_with_high_bit_ascii() {
    let tok = Token::new_checked(TokenKind::String, "\u{80}\u{FF}", 0, 4).expect("valid token");
    assert_eq!(tok.text.len(), 4); // 2 two-byte codepoints
}

#[test]
fn token_text_with_cjk_characters() {
    let text = "日本語テスト";
    let tok = Token::new_checked(TokenKind::String, text, 0, text.len()).expect("valid token");
    assert_eq!(&*tok.text, text);
    assert_eq!(tok.text.chars().count(), 6);
}

#[test]
fn token_text_with_emoji() {
    let text = "🦀🐪💎";
    let tok = Token::new_checked(TokenKind::String, text, 0, text.len()).expect("valid token");
    assert_eq!(&*tok.text, text);
}

#[test]
fn token_text_very_long_string() {
    let long = "x".repeat(100_000);
    let tok =
        Token::new_checked(TokenKind::String, long.as_str(), 0, 100_000).expect("valid token");
    assert_eq!(tok.text.len(), 100_000);
}

#[test]
fn token_with_single_char_text() {
    for ch in ['a', 'Z', '0', '_', '$', '@'] {
        let s = ch.to_string();
        let tok = Token::new_checked(TokenKind::Identifier, s.as_str(), 0, 1).expect("valid token");
        assert_eq!(tok.text.len(), 1);
    }
}

// ===========================================================================
// Iterator / collection patterns
// ===========================================================================

#[test]
fn filter_tokens_by_kind() {
    let tokens = [
        Token::new_checked(TokenKind::My, "my", 0, 2).expect("valid token"),
        Token::new_checked(TokenKind::ScalarSigil, "$", 3, 4).expect("valid token"),
        Token::new_checked(TokenKind::Identifier, "x", 4, 5).expect("valid token"),
        Token::new_checked(TokenKind::Assign, "=", 6, 7).expect("valid token"),
        Token::new_checked(TokenKind::My, "my", 8, 10).expect("valid token"),
        Token::new_checked(TokenKind::ScalarSigil, "$", 11, 12).expect("valid token"),
        Token::new_checked(TokenKind::Identifier, "y", 12, 13).expect("valid token"),
    ];
    let my_tokens: Vec<_> = tokens.iter().filter(|t| t.kind() == TokenKind::My).collect();
    assert_eq!(my_tokens.len(), 2);
}

#[test]
fn map_tokens_to_text() {
    let tokens = [
        Token::new_checked(TokenKind::Identifier, "foo", 0, 3).expect("valid token"),
        Token::new_checked(TokenKind::DoubleColon, "::", 3, 5).expect("valid token"),
        Token::new_checked(TokenKind::Identifier, "bar", 5, 8).expect("valid token"),
    ];
    let joined: String = tokens.iter().map(|t| t.text.as_ref()).collect();
    assert_eq!(joined, "foo::bar");
}

#[test]
fn partition_tokens_by_category() {
    let tokens = [
        Token::new_checked(TokenKind::My, "my", 0, 2).expect("valid token"),
        Token::new_checked(TokenKind::Plus, "+", 3, 4).expect("valid token"),
        Token::new_checked(TokenKind::If, "if", 5, 7).expect("valid token"),
        Token::new_checked(TokenKind::Minus, "-", 8, 9).expect("valid token"),
    ];
    let keywords = keyword_kinds();
    let (kw, non_kw): (Vec<_>, Vec<_>) = tokens.iter().partition(|t| keywords.contains(&t.kind()));
    assert_eq!(kw.len(), 2);
    assert_eq!(non_kw.len(), 2);
}

#[test]
fn collect_token_spans() {
    let tokens = [
        Token::new_checked(TokenKind::My, "my", 0, 2).expect("valid token"),
        Token::new_checked(TokenKind::ScalarSigil, "$", 3, 4).expect("valid token"),
        Token::new_checked(TokenKind::Identifier, "x", 4, 5).expect("valid token"),
    ];
    let spans: Vec<(usize, usize)> = tokens.iter().map(|t| (t.start(), t.end())).collect();
    assert_eq!(spans, vec![(0, 2), (3, 4), (4, 5)]);
}

#[test]
fn find_first_identifier() {
    let tokens = [
        Token::new_checked(TokenKind::My, "my", 0, 2).expect("valid token"),
        Token::new_checked(TokenKind::ScalarSigil, "$", 3, 4).expect("valid token"),
        Token::new_checked(TokenKind::Identifier, "first_id", 4, 12).expect("valid token"),
        Token::new_checked(TokenKind::Identifier, "second_id", 13, 22).expect("valid token"),
    ];
    let first = tokens.iter().find(|t| t.kind() == TokenKind::Identifier).map(|t| t.text.as_ref());
    assert_eq!(first, Some("first_id"), "should have found identifier 'first_id'");
}

#[test]
fn count_semicolons() {
    let tokens = [
        Token::new_checked(TokenKind::Identifier, "a", 0, 1).expect("valid token"),
        Token::new_checked(TokenKind::Semicolon, ";", 1, 2).expect("valid token"),
        Token::new_checked(TokenKind::Identifier, "b", 3, 4).expect("valid token"),
        Token::new_checked(TokenKind::Semicolon, ";", 4, 5).expect("valid token"),
        Token::new_checked(TokenKind::Identifier, "c", 6, 7).expect("valid token"),
        Token::new_checked(TokenKind::Semicolon, ";", 7, 8).expect("valid token"),
    ];
    let count = tokens.iter().filter(|t| t.kind() == TokenKind::Semicolon).count();
    assert_eq!(count, 3);
}

// ===========================================================================
// Compound assignment operators
// ===========================================================================

#[test]
fn compound_assign_operators_are_distinct() {
    let compounds = [
        TokenKind::PlusAssign,
        TokenKind::MinusAssign,
        TokenKind::StarAssign,
        TokenKind::SlashAssign,
        TokenKind::PercentAssign,
        TokenKind::DotAssign,
        TokenKind::AndAssign,
        TokenKind::OrAssign,
        TokenKind::XorAssign,
        TokenKind::PowerAssign,
        TokenKind::LeftShiftAssign,
        TokenKind::RightShiftAssign,
        TokenKind::LogicalAndAssign,
        TokenKind::LogicalOrAssign,
        TokenKind::DefinedOrAssign,
    ];
    for (i, a) in compounds.iter().enumerate() {
        for b in &compounds[i + 1..] {
            assert_ne!(a, b);
        }
    }
    assert_eq!(compounds.len(), 15);
}

// ===========================================================================
// Word operators vs symbolic operators
// ===========================================================================

#[test]
fn word_operators_are_distinct_from_symbolic() {
    assert_ne!(TokenKind::WordAnd, TokenKind::And);
    assert_ne!(TokenKind::WordOr, TokenKind::Or);
    assert_ne!(TokenKind::WordNot, TokenKind::Not);
}

#[test]
fn word_xor_has_no_symbolic_counterpart() {
    // BitwiseXor is ^, WordXor is xor — they're different concepts
    assert_ne!(TokenKind::WordXor, TokenKind::BitwiseXor);
}

// ===========================================================================
// Phase block tokens
// ===========================================================================

#[test]
fn phase_blocks_are_all_distinct() {
    let phases =
        [TokenKind::Begin, TokenKind::End, TokenKind::Check, TokenKind::Init, TokenKind::Unitcheck];
    for (i, a) in phases.iter().enumerate() {
        for b in &phases[i + 1..] {
            assert_ne!(a, b);
        }
    }
}

// ===========================================================================
// Special / error tokens
// ===========================================================================

#[test]
fn unknown_rest_token_for_budget_exceeded() {
    let tok = Token::new_checked(TokenKind::UnknownRest, "...remainder...", 500, 515)
        .expect("valid token");
    assert_eq!(tok.kind(), TokenKind::UnknownRest);
    assert_eq!(&*tok.text, "...remainder...");
}

#[test]
fn heredoc_depth_limit_token() {
    let tok =
        Token::new_checked(TokenKind::HeredocDepthLimit, "<<DEEP", 0, 6).expect("valid token");
    assert_eq!(tok.kind(), TokenKind::HeredocDepthLimit);
}

#[test]
fn data_marker_and_body() {
    let marker = Token::new_checked(TokenKind::DataMarker, "__END__", 0, 7).expect("valid token");
    let body =
        Token::new_checked(TokenKind::DataBody, "some trailing data", 8, 26).expect("valid token");
    assert_eq!(marker.kind(), TokenKind::DataMarker);
    assert_eq!(body.kind(), TokenKind::DataBody);
    assert_ne!(marker.kind(), body.kind());
}

#[test]
fn format_body_token() {
    let tok =
        Token::new_checked(TokenKind::FormatBody, "@<<<< $name\n.", 0, 13).expect("valid token");
    assert_eq!(tok.kind(), TokenKind::FormatBody);
}

// ===========================================================================
// Token in Option and Result contexts
// ===========================================================================

#[test]
fn option_token_some() {
    let maybe: Option<Token> =
        Some(Token::new_checked(TokenKind::Number, "42", 0, 2).expect("valid token"));
    assert!(maybe.is_some(), "expected Some");
    assert_eq!(maybe.as_ref().map(|t| t.kind()), Some(TokenKind::Number), "expected Number token");
}

#[test]
fn option_token_none() {
    let maybe: Option<Token> = None;
    assert!(maybe.is_none());
}

#[test]
fn vec_first_and_last() {
    let tokens = [
        Token::new_checked(TokenKind::Use, "use", 0, 3).expect("valid token"),
        Token::new_checked(TokenKind::Identifier, "strict", 4, 10).expect("valid token"),
        Token::new_checked(TokenKind::Semicolon, ";", 10, 11).expect("valid token"),
    ];
    if let Some(first) = tokens.first() {
        assert_eq!(first.kind(), TokenKind::Use);
    }
    if let Some(last) = tokens.last() {
        assert_eq!(last.kind(), TokenKind::Semicolon);
    }
}

// ===========================================================================
// Regression-style: ensure no accidental overlap between similar variants
// ===========================================================================

#[test]
fn match_vs_smart_match() {
    assert_ne!(TokenKind::Match, TokenKind::SmartMatch);
}

#[test]
fn not_vs_bitwise_not() {
    assert_ne!(TokenKind::Not, TokenKind::BitwiseNot);
}

#[test]
fn star_vs_glob_sigil() {
    assert_ne!(TokenKind::Star, TokenKind::GlobSigil);
}

#[test]
fn percent_vs_hash_sigil() {
    assert_ne!(TokenKind::Percent, TokenKind::HashSigil);
}

#[test]
fn bitwise_and_vs_sub_sigil() {
    assert_ne!(TokenKind::BitwiseAnd, TokenKind::SubSigil);
}

#[test]
fn slash_vs_defined_or() {
    assert_ne!(TokenKind::Slash, TokenKind::DefinedOr);
}

#[test]
fn less_vs_left_shift() {
    assert_ne!(TokenKind::Less, TokenKind::LeftShift);
}

#[test]
fn greater_vs_right_shift() {
    assert_ne!(TokenKind::Greater, TokenKind::RightShift);
}
