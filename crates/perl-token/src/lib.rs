//! Perl token definitions shared across the parser ecosystem.
//!
//! This crate defines [`Token`] and [`TokenKind`], the fundamental types that
//! flow from the lexer (`perl-lexer`) into the parser (`perl-parser-core`).
//! Downstream crates re-export these types so consumers rarely need to depend
//! on `perl-token` directly.
//!
//! # Examples
//!
//! Create and inspect tokens:
//!
//! ```rust
//! use perl_token::{Token, TokenKind};
//!
//! // Create a keyword token for `my`
//! let token = Token::new_checked(TokenKind::My, "my", 0, 2)?;
//! assert_eq!(token.kind(), TokenKind::My);
//! assert_eq!(&*token.text, "my");
//! assert_eq!(token.start(), 0);
//! assert_eq!(token.end(), 2);
//!
//! // Create a numeric literal token
//! let num = Token::new_checked(TokenKind::Number, "42", 7, 9)?;
//! assert_eq!(num.kind(), TokenKind::Number);
//! assert_eq!(&*num.text, "42");
//! # Ok::<(), perl_token::TokenSpanError>(())
//! ```
//!
//! Use [`TokenKind::display_name`] for user-facing error messages:
//!
//! ```rust
//! use perl_token::TokenKind;
//!
//! assert_eq!(TokenKind::LeftBrace.display_name(), "'{'");
//! assert_eq!(TokenKind::Identifier.display_name(), "identifier");
//! assert_eq!(TokenKind::Eof.display_name(), "end of input");
//! ```
//!
//! # Evolution policy (#2898)
//!
//! | Type | Disposition |
//! |------|-------------|
//! | [`TokenKind`] | closed / exhaustive public enum |
//! | [`Token`], [`TokenRef`], [`TokenSpan`], [`TokenSpanError`], [`TokenCategory`], [`TokenKindMetadata`] | `#[non_exhaustive]` |
//! | `TokenOrigin`, `TokenStatus` | not types in this crate |
//!
//! ```compile_fail
//! use perl_token::TokenOrigin;
//! ```
//!
//! ```compile_fail
//! use perl_token::TokenStatus;
//! ```
//!
//! Crate-private construction after proven invariants (`from_ordered`,
//! `from_valid_parts`) is not part of the public API:
//!
//! ```compile_fail
//! use perl_token::TokenSpan;
//! let _ = TokenSpan::from_ordered(0, 1);
//! ```
//!
//! ```compile_fail
//! use perl_token::{Token, TokenKind};
//! use std::sync::Arc;
//! let _ = Token::from_valid_parts(TokenKind::Identifier, Arc::from("x"), 0, 1);
//! ```

#![warn(missing_docs)]
#![cfg_attr(test, allow(clippy::expect_used, clippy::unwrap_used, clippy::panic))]
#![deny(clippy::map_err_ignore)] // Cohort C0 activation (#12598): census-clean on all targets; new findings move the crate to C1.

mod kind;
mod span;
mod token;

pub use kind::{
    DELIMITER_SPELLINGS, KEYWORD_SPELLINGS, OPERATOR_SPELLINGS, SIGIL_SPELLINGS, TokenCategory,
    TokenKind, TokenKindMetadata,
};
pub use span::{TokenSpan, TokenSpanError};
pub use token::{Token, TokenRef};

#[cfg(test)]
mod tests {
    use super::*;

    // --- TokenSpan ---

    #[test]
    fn token_span_new_and_accessors() {
        let span = TokenSpan::try_new(5, 10).expect("ordered span");
        assert_eq!(span.start(), 5);
        assert_eq!(span.end(), 10);
        assert_eq!(span.len(), 5);
        assert!(!span.is_empty());
        assert_eq!(span.range(), 5..10);
    }

    #[test]
    fn token_span_is_empty_when_zero_length() {
        let span = TokenSpan::try_new(3, 3).expect("ordered span");
        assert!(span.is_empty());
        assert_eq!(span.len(), 0);
    }

    #[test]
    fn token_span_try_new_ok() -> Result<(), TokenSpanError> {
        let span = TokenSpan::try_new(0, 5)?;
        assert_eq!(span.start(), 0);
        assert_eq!(span.end(), 5);
        Ok(())
    }

    #[test]
    fn token_span_try_new_end_before_start_errors() {
        assert_eq!(
            TokenSpan::try_new(10, 5),
            Err(TokenSpanError::EndBeforeStart { start: 10, end: 5 })
        );
    }

    #[test]
    fn token_span_error_display_end_before_start() {
        let err = TokenSpanError::EndBeforeStart { start: 10, end: 5 };
        let msg = err.to_string();
        assert!(msg.contains("10"));
        assert!(msg.contains("5"));
    }

    #[test]
    fn token_span_error_display_empty_span_not_allowed() {
        let err = TokenSpanError::EmptySpanNotAllowed { kind: TokenKind::Identifier, at: 7 };
        let msg = err.to_string();
        assert!(msg.contains("Identifier"));
        assert!(msg.contains("7"));
    }

    #[test]
    fn token_span_error_display_text_length_mismatch() {
        let err = TokenSpanError::TextLengthMismatch { text_len: 5, span_len: 1, start: 0, end: 1 };
        let msg = err.to_string();
        assert!(msg.contains("5"));
        assert!(msg.contains("1"));
        assert!(msg.contains("0"));
    }

    // --- Token ---

    #[test]
    fn token_new_stores_fields() {
        let tok = Token::new_checked(TokenKind::My, "my", 0, 2).expect("valid token");
        assert_eq!(tok.kind(), TokenKind::My);
        assert_eq!(&*tok.text, "my");
        assert_eq!(tok.start(), 0);
        assert_eq!(tok.end(), 2);
        assert_eq!(tok.as_parts(), (TokenKind::My, "my", 0, 2));
    }

    #[test]
    fn token_new_checked_rejects_inverted_span() {
        assert_eq!(
            Token::new_checked(TokenKind::Identifier, "x", 9, 4),
            Err(TokenSpanError::EndBeforeStart { start: 9, end: 4 })
        );
    }

    #[test]
    fn token_len_and_is_empty() {
        let tok = Token::new_checked(TokenKind::Identifier, "foo", 10, 13).expect("valid token");
        assert_eq!(tok.len(), 3);
        assert!(!tok.is_empty());

        let eof = Token::eof_at(8);
        assert_eq!(eof.len(), 0);
        assert!(eof.is_empty());
    }

    #[test]
    fn token_span_and_range() {
        let tok = Token::new_checked(TokenKind::Number, "42", 5, 7).expect("valid token");
        assert_eq!(tok.span(), TokenSpan::try_new(5, 7).expect("ordered span"));
        assert_eq!(tok.range(), 5..7);
    }

    #[test]
    fn token_try_new_allows_ordered_spans() -> Result<(), TokenSpanError> {
        let tok = Token::try_new(TokenKind::Identifier, "name", 4, 8)?;
        assert_eq!(tok.kind(), TokenKind::Identifier);
        assert_eq!(&*tok.text, "name");
        assert_eq!(tok.span(), TokenSpan::try_new(4, 8).expect("ordered span"));
        Ok(())
    }

    #[test]
    fn token_try_new_rejects_end_before_start() {
        assert_eq!(
            Token::try_new(TokenKind::Identifier, "x", 10, 5),
            Err(TokenSpanError::EndBeforeStart { start: 10, end: 5 })
        );
    }

    #[test]
    fn token_new_checked_rejects_empty_non_eof() {
        assert_eq!(
            Token::new_checked(TokenKind::Identifier, "", 5, 5),
            Err(TokenSpanError::EmptySpanNotAllowed { kind: TokenKind::Identifier, at: 5 })
        );
    }

    #[test]
    fn token_new_checked_allows_empty_eof() -> Result<(), TokenSpanError> {
        let tok = Token::new_checked(TokenKind::Eof, "", 5, 5)?;
        assert_eq!(tok.kind(), TokenKind::Eof);
        assert_eq!(tok.start(), 5);
        Ok(())
    }

    #[test]
    fn token_new_checked_allows_empty_unknown() -> Result<(), TokenSpanError> {
        let tok = Token::new_checked(TokenKind::Unknown, "", 6, 6)?;
        assert_eq!(tok.kind(), TokenKind::Unknown);
        assert_eq!(tok.start(), 6);
        assert!(tok.is_empty());
        Ok(())
    }

    #[test]
    fn token_eof_at() {
        let eof = Token::eof_at(42);
        assert_eq!(eof.kind(), TokenKind::Eof);
        assert_eq!(eof.start(), 42);
        assert_eq!(eof.end(), 42);
        assert!(eof.is_empty());
    }

    #[test]
    fn token_unknown_at_rejects_inverted_span() {
        assert_eq!(
            Token::unknown_at("?", 5, 3),
            Err(TokenSpanError::EndBeforeStart { start: 5, end: 3 })
        );
    }

    #[test]
    fn token_new_checked_allows_geometry_only_unknown_rest() {
        // The budget-stop recovery representation: empty text over a non-empty
        // span. The payload-free geometry is the signal the parser's typed
        // `lexer_budget_exhausted` stop cause is keyed on (#14158); it must
        // construct instead of falling through to a silent `Eof`.
        let tok = Token::new_checked(TokenKind::UnknownRest, "", 12, 70_013)
            .expect("geometry-only UnknownRest is a legal recovery representation");
        assert_eq!(tok.kind(), TokenKind::UnknownRest);
        assert_eq!(tok.start(), 12);
        assert_eq!(tok.end(), 70_013);
        assert!(tok.is_geometry_only());
    }

    #[test]
    fn token_new_checked_still_rejects_wrong_width_non_empty_text() {
        // The width contract stays in force for every payload-bearing token.
        assert_eq!(
            Token::new_checked(TokenKind::UnknownRest, "a", 12, 70_013),
            Err(TokenSpanError::TextLengthMismatch {
                text_len: 1,
                span_len: 70_001,
                start: 12,
                end: 70_013
            })
        );
    }

    #[test]
    fn token_unknown_rest_at_builds_payload_free_geometry() {
        let tok = Token::unknown_rest_at(7, 9).expect("non-empty span");
        assert!(tok.is_geometry_only());
        assert!(tok.text.is_empty());
        // Empty and reversed spans are rejected: geometry-only still requires
        // a real span to identify the unparsed remainder.
        assert_eq!(
            Token::unknown_rest_at(7, 7),
            Err(TokenSpanError::EmptySpanNotAllowed {
                kind: TokenKind::UnknownRest,
                at: 7
            })
        );
        assert_eq!(
            Token::unknown_rest_at(9, 7),
            Err(TokenSpanError::EndBeforeStart { start: 9, end: 7 })
        );
    }

    #[test]
    fn token_ref_to_owned_token_preserves_geometry_only_unknown_rest() {
        let view =
            TokenRef::new_checked(TokenKind::UnknownRest, "", 12, 70_013).expect("legal view");
        assert!(view.is_geometry_only());
        let owned = view.to_owned_token();
        assert_eq!(owned.kind(), TokenKind::UnknownRest);
        assert!(owned.is_geometry_only());
    }

    #[test]
    fn token_try_new_rejects_empty_non_eof() {
        assert_eq!(
            Token::try_new(TokenKind::Identifier, "", 5, 5),
            Err(TokenSpanError::EmptySpanNotAllowed { kind: TokenKind::Identifier, at: 5 })
        );
    }

    #[test]
    fn token_with_kind() {
        let tok = Token::new_checked(TokenKind::Identifier, "sub", 0, 3).expect("valid token");
        let retyped = tok.with_kind(TokenKind::Sub).expect("kind change preserves span");
        assert_eq!(retyped.kind(), TokenKind::Sub);
        assert_eq!(&*retyped.text, "sub");
        assert_eq!(retyped.start(), 0);
        assert_eq!(retyped.end(), 3);
    }

    #[test]
    fn token_with_kind_rejects_empty_identifier() {
        let eof = Token::eof_at(9);
        assert_eq!(
            eof.with_kind(TokenKind::Identifier),
            Err(TokenSpanError::EmptySpanNotAllowed { kind: TokenKind::Identifier, at: 9 })
        );
    }

    #[test]
    fn token_ref_with_kind_rejects_empty_identifier() {
        let eof = TokenRef::new_checked(TokenKind::Eof, "", 9, 9).expect("empty eof");
        assert_eq!(
            eof.with_kind(TokenKind::Identifier),
            Err(TokenSpanError::EmptySpanNotAllowed { kind: TokenKind::Identifier, at: 9 })
        );
    }

    #[test]
    fn token_with_span_ok() -> Result<(), TokenSpanError> {
        let tok = Token::new_checked(TokenKind::String, "hello", 0, 5).expect("valid token");
        let moved = tok.with_span(10, 15)?;
        assert_eq!(moved.start(), 10);
        assert_eq!(moved.end(), 15);
        Ok(())
    }

    #[test]
    fn token_with_span_rejects_empty_non_eof() {
        let tok = Token::new_checked(TokenKind::String, "hello", 0, 5).expect("valid token");
        assert_eq!(
            tok.with_span(10, 10),
            Err(TokenSpanError::EmptySpanNotAllowed { kind: TokenKind::String, at: 10 })
        );
    }

    #[test]
    fn token_display_name_delegates_to_kind() {
        let tok = Token::new_checked(TokenKind::LeftBrace, "{", 0, 1).expect("valid token");
        assert_eq!(tok.display_name(), "'{'");
    }

    #[test]
    fn token_as_ref_token_round_trip() {
        let tok = Token::new_checked(TokenKind::Sub, "sub", 0, 3).expect("valid token");
        let tok_ref = tok.as_ref_token();
        assert_eq!(tok_ref.kind(), TokenKind::Sub);
        assert_eq!(tok_ref.text, "sub");
        assert_eq!(tok_ref.start(), 0);
        assert_eq!(tok_ref.end(), 3);

        let owned: Token = tok_ref.into();
        assert_eq!(owned.kind(), TokenKind::Sub);
        assert_eq!(&*owned.text, "sub");
    }

    // --- TokenRef ---

    #[test]
    fn token_ref_new_checked_rejects_inverted_span() {
        assert_eq!(
            TokenRef::new_checked(TokenKind::Identifier, "x", 9, 4),
            Err(TokenSpanError::EndBeforeStart { start: 9, end: 4 })
        );
    }

    #[test]
    fn token_ref_accessors() {
        let r = TokenRef::new_checked(TokenKind::Number, "99", 4, 6).expect("valid token");
        assert_eq!(r.len(), 2);
        assert!(!r.is_empty());
        assert_eq!(r.span(), TokenSpan::try_new(4, 6).expect("ordered span"));
        assert_eq!(r.display_name(), "number");
    }

    #[test]
    fn token_ref_try_new_allows_ordered_spans() -> Result<(), TokenSpanError> {
        let r = TokenRef::try_new(TokenKind::Number, "99", 4, 6)?;
        assert_eq!(r.kind(), TokenKind::Number);
        assert_eq!(r.text, "99");
        assert_eq!(r.span(), TokenSpan::try_new(4, 6)?);
        Ok(())
    }

    #[test]
    fn token_ref_to_owned_token() {
        let r = TokenRef::new_checked(TokenKind::Identifier, "foo", 1, 4).expect("valid token");
        let owned = r.to_owned_token();
        assert_eq!(owned.kind(), TokenKind::Identifier);
        assert_eq!(&*owned.text, "foo");
    }

    // --- TokenKind::from_keyword ---

    #[test]
    fn from_keyword_recognises_perl_keywords() {
        assert_eq!(TokenKind::from_keyword("my"), Some(TokenKind::My));
        assert_eq!(TokenKind::from_keyword("sub"), Some(TokenKind::Sub));
        assert_eq!(TokenKind::from_keyword("if"), Some(TokenKind::If));
        assert_eq!(TokenKind::from_keyword("elsif"), Some(TokenKind::Elsif));
        assert_eq!(TokenKind::from_keyword("else"), Some(TokenKind::Else));
        assert_eq!(TokenKind::from_keyword("while"), Some(TokenKind::While));
        assert_eq!(TokenKind::from_keyword("for"), Some(TokenKind::For));
        assert_eq!(TokenKind::from_keyword("foreach"), Some(TokenKind::Foreach));
        assert_eq!(TokenKind::from_keyword("return"), Some(TokenKind::Return));
        assert_eq!(TokenKind::from_keyword("package"), Some(TokenKind::Package));
        assert_eq!(TokenKind::from_keyword("use"), Some(TokenKind::Use));
        assert_eq!(TokenKind::from_keyword("BEGIN"), Some(TokenKind::Begin));
        assert_eq!(TokenKind::from_keyword("END"), Some(TokenKind::End));
        assert_eq!(TokenKind::from_keyword("eval"), Some(TokenKind::Eval));
        assert_eq!(TokenKind::from_keyword("class"), Some(TokenKind::Class));
        assert_eq!(TokenKind::from_keyword("defer"), Some(TokenKind::Defer));
        assert_eq!(TokenKind::from_keyword("and"), Some(TokenKind::WordAnd));
        assert_eq!(TokenKind::from_keyword("or"), Some(TokenKind::WordOr));
        assert_eq!(TokenKind::from_keyword("not"), Some(TokenKind::WordNot));
        assert_eq!(TokenKind::from_keyword("xor"), Some(TokenKind::WordXor));
        assert_eq!(TokenKind::from_keyword("cmp"), Some(TokenKind::StringCompare));
    }

    #[test]
    fn from_keyword_unknown_returns_none() {
        assert_eq!(TokenKind::from_keyword("MY"), None);
        assert_eq!(TokenKind::from_keyword("Sub"), None);
        assert_eq!(TokenKind::from_keyword("unknown"), None);
        assert_eq!(TokenKind::from_keyword(""), None);
    }

    // --- TokenKind::from_operator ---

    #[test]
    fn from_operator_recognises_operators() {
        assert_eq!(TokenKind::from_operator("="), Some(TokenKind::Assign));
        assert_eq!(TokenKind::from_operator("+"), Some(TokenKind::Plus));
        assert_eq!(TokenKind::from_operator("**"), Some(TokenKind::Power));
        assert_eq!(TokenKind::from_operator("->"), Some(TokenKind::Arrow));
        assert_eq!(TokenKind::from_operator("=>"), Some(TokenKind::FatArrow));
        assert_eq!(TokenKind::from_operator("<=>"), Some(TokenKind::Spaceship));
        assert_eq!(TokenKind::from_operator("//="), Some(TokenKind::DefinedOrAssign));
        assert_eq!(TokenKind::from_operator("..."), Some(TokenKind::Ellipsis));
        assert_eq!(TokenKind::from_operator("~~"), Some(TokenKind::SmartMatch));
    }

    #[test]
    fn from_operator_unknown_returns_none() {
        assert_eq!(TokenKind::from_operator(""), None);
        assert_eq!(TokenKind::from_operator("xyz"), None);
    }

    // --- TokenKind::from_delimiter ---

    #[test]
    fn from_delimiter_recognises_all() {
        assert_eq!(TokenKind::from_delimiter("("), Some(TokenKind::LeftParen));
        assert_eq!(TokenKind::from_delimiter(")"), Some(TokenKind::RightParen));
        assert_eq!(TokenKind::from_delimiter("{"), Some(TokenKind::LeftBrace));
        assert_eq!(TokenKind::from_delimiter("}"), Some(TokenKind::RightBrace));
        assert_eq!(TokenKind::from_delimiter("["), Some(TokenKind::LeftBracket));
        assert_eq!(TokenKind::from_delimiter("]"), Some(TokenKind::RightBracket));
        assert_eq!(TokenKind::from_delimiter(";"), Some(TokenKind::Semicolon));
        assert_eq!(TokenKind::from_delimiter(","), Some(TokenKind::Comma));
        assert_eq!(TokenKind::from_delimiter("x"), None);
    }

    // --- TokenKind::from_sigil ---

    #[test]
    fn from_sigil_recognises_all() {
        assert_eq!(TokenKind::from_sigil("$"), Some(TokenKind::ScalarSigil));
        assert_eq!(TokenKind::from_sigil("@"), Some(TokenKind::ArraySigil));
        assert_eq!(TokenKind::from_sigil("%"), Some(TokenKind::HashSigil));
        assert_eq!(TokenKind::from_sigil("&"), Some(TokenKind::SubSigil));
        assert_eq!(TokenKind::from_sigil("*"), Some(TokenKind::GlobSigil));
        assert_eq!(TokenKind::from_sigil("!"), None);
    }

    // --- TokenKind::category ---

    #[test]
    fn category_keyword_variants() {
        assert_eq!(TokenKind::My.category(), TokenCategory::Keyword);
        assert_eq!(TokenKind::Sub.category(), TokenCategory::Keyword);
        assert_eq!(TokenKind::Defer.category(), TokenCategory::Keyword);
    }

    #[test]
    fn category_operator_variants() {
        assert_eq!(TokenKind::Plus.category(), TokenCategory::Operator);
        assert_eq!(TokenKind::Spaceship.category(), TokenCategory::Operator);
        assert_eq!(TokenKind::WordAnd.category(), TokenCategory::Operator);
    }

    #[test]
    fn category_delimiter_variants() {
        assert_eq!(TokenKind::LeftParen.category(), TokenCategory::Delimiter);
        assert_eq!(TokenKind::Comma.category(), TokenCategory::Delimiter);
    }

    #[test]
    fn category_literal_variants() {
        assert_eq!(TokenKind::Number.category(), TokenCategory::Literal);
        assert_eq!(TokenKind::HeredocStart.category(), TokenCategory::Literal);
        assert_eq!(TokenKind::DataMarker.category(), TokenCategory::Literal);
    }

    #[test]
    fn category_identifier_variants() {
        assert_eq!(TokenKind::Identifier.category(), TokenCategory::Identifier);
        assert_eq!(TokenKind::ScalarSigil.category(), TokenCategory::Identifier);
        assert_eq!(TokenKind::GlobSigil.category(), TokenCategory::Identifier);
    }

    #[test]
    fn category_special_variants() {
        assert_eq!(TokenKind::Eof.category(), TokenCategory::Special);
        assert_eq!(TokenKind::Unknown.category(), TokenCategory::Special);
    }

    // --- TokenKind::display_name ---

    #[test]
    fn display_name_selected_variants() {
        assert_eq!(TokenKind::LeftBrace.display_name(), "'{'");
        assert_eq!(TokenKind::RightBrace.display_name(), "'}'");
        assert_eq!(TokenKind::Identifier.display_name(), "identifier");
        assert_eq!(TokenKind::Eof.display_name(), "end of input");
        assert_eq!(TokenKind::Number.display_name(), "number");
        assert_eq!(TokenKind::Sub.display_name(), "'sub'");
        assert_eq!(TokenKind::Semicolon.display_name(), "';'");
        assert_eq!(TokenKind::HeredocStart.display_name(), "heredoc (<<)");
        assert_eq!(TokenKind::DataMarker.display_name(), "data marker (__DATA__ or __END__)");
    }

    // --- TokenKind::all / metadata_count ---

    #[test]
    fn all_returns_132_variants() {
        assert_eq!(TokenKind::all().len(), 132);
        assert_eq!(TokenKind::metadata_count(), 132);
    }

    #[test]
    fn metadata_round_trips_through_kind() {
        let m = TokenKind::Sub.metadata();
        assert_eq!(m.category, TokenCategory::Keyword);
        assert_eq!(m.display_name, "'sub'");
    }

    // --- TokenKind role predicates ---

    #[test]
    fn is_assignment_operator_returns_true_for_assign_variants() {
        assert!(TokenKind::Assign.is_assignment_operator());
        assert!(TokenKind::PlusAssign.is_assignment_operator());
        assert!(TokenKind::MinusAssign.is_assignment_operator());
        assert!(TokenKind::StarAssign.is_assignment_operator());
        assert!(TokenKind::SlashAssign.is_assignment_operator());
        assert!(TokenKind::PercentAssign.is_assignment_operator());
        assert!(TokenKind::DotAssign.is_assignment_operator());
        assert!(TokenKind::AndAssign.is_assignment_operator());
        assert!(TokenKind::OrAssign.is_assignment_operator());
        assert!(TokenKind::XorAssign.is_assignment_operator());
        assert!(TokenKind::PowerAssign.is_assignment_operator());
        assert!(TokenKind::LeftShiftAssign.is_assignment_operator());
        assert!(TokenKind::RightShiftAssign.is_assignment_operator());
        assert!(TokenKind::LogicalAndAssign.is_assignment_operator());
        assert!(TokenKind::LogicalOrAssign.is_assignment_operator());
        assert!(TokenKind::DefinedOrAssign.is_assignment_operator());
    }

    #[test]
    fn is_assignment_operator_returns_false_for_non_assign() {
        assert!(!TokenKind::Plus.is_assignment_operator());
        assert!(!TokenKind::Equal.is_assignment_operator());
        assert!(!TokenKind::Identifier.is_assignment_operator());
    }

    #[test]
    fn is_logical_operator_returns_true_for_logical_variants() {
        assert!(TokenKind::And.is_logical_operator());
        assert!(TokenKind::Or.is_logical_operator());
        assert!(TokenKind::Not.is_logical_operator());
        assert!(TokenKind::DefinedOr.is_logical_operator());
        assert!(TokenKind::WordAnd.is_logical_operator());
        assert!(TokenKind::WordOr.is_logical_operator());
        assert!(TokenKind::WordNot.is_logical_operator());
        assert!(TokenKind::WordXor.is_logical_operator());
    }

    #[test]
    fn is_logical_operator_returns_false_for_non_logical() {
        assert!(!TokenKind::Plus.is_logical_operator());
        assert!(!TokenKind::Assign.is_logical_operator());
        assert!(!TokenKind::Identifier.is_logical_operator());
    }

    #[test]
    fn is_open_delimiter_returns_true_for_open_delimiters() {
        assert!(TokenKind::LeftParen.is_open_delimiter());
        assert!(TokenKind::LeftBrace.is_open_delimiter());
        assert!(TokenKind::LeftBracket.is_open_delimiter());
    }

    #[test]
    fn is_open_delimiter_returns_false_for_non_open() {
        assert!(!TokenKind::RightParen.is_open_delimiter());
        assert!(!TokenKind::Semicolon.is_open_delimiter());
        assert!(!TokenKind::Plus.is_open_delimiter());
    }

    #[test]
    fn is_quote_like_returns_true_for_quote_variants() {
        assert!(TokenKind::Regex.is_quote_like());
        assert!(TokenKind::Substitution.is_quote_like());
        assert!(TokenKind::Transliteration.is_quote_like());
        assert!(TokenKind::QuoteSingle.is_quote_like());
        assert!(TokenKind::QuoteDouble.is_quote_like());
        assert!(TokenKind::QuoteWords.is_quote_like());
        assert!(TokenKind::QuoteCommand.is_quote_like());
        assert!(TokenKind::HeredocStart.is_quote_like());
    }

    #[test]
    fn is_quote_like_returns_false_for_non_quote() {
        assert!(!TokenKind::String.is_quote_like());
        assert!(!TokenKind::Identifier.is_quote_like());
        assert!(!TokenKind::LeftParen.is_quote_like());
    }

    #[test]
    fn is_recovery_boundary_returns_true_for_boundaries() {
        assert!(TokenKind::Semicolon.is_recovery_boundary());
        assert!(TokenKind::RightParen.is_recovery_boundary());
        assert!(TokenKind::RightBrace.is_recovery_boundary());
        assert!(TokenKind::RightBracket.is_recovery_boundary());
        assert!(TokenKind::Eof.is_recovery_boundary());
    }

    #[test]
    fn is_recovery_boundary_returns_false_for_non_boundary() {
        assert!(!TokenKind::Plus.is_recovery_boundary());
        assert!(!TokenKind::Identifier.is_recovery_boundary());
        assert!(!TokenKind::LeftParen.is_recovery_boundary());
    }

    // --- TokenRef::new_checked branches ---

    #[test]
    fn token_ref_new_checked_rejects_end_before_start() {
        assert_eq!(
            TokenRef::new_checked(TokenKind::Identifier, "x", 10, 3),
            Err(TokenSpanError::EndBeforeStart { start: 10, end: 3 })
        );
    }

    #[test]
    fn token_ref_new_checked_allows_empty_eof() -> Result<(), Box<dyn std::error::Error>> {
        let tok = TokenRef::new_checked(TokenKind::Eof, "", 7, 7)?;
        assert_eq!(tok.kind(), TokenKind::Eof);
        assert_eq!(tok.start(), 7);
        assert!(tok.is_empty());
        Ok(())
    }

    #[test]
    fn token_ref_new_checked_allows_empty_unknown() -> Result<(), Box<dyn std::error::Error>> {
        let tok = TokenRef::new_checked(TokenKind::Unknown, "", 3, 3)?;
        assert_eq!(tok.kind(), TokenKind::Unknown);
        assert_eq!(tok.start(), 3);
        assert!(tok.is_empty());
        Ok(())
    }

    #[test]
    fn token_ref_new_checked_rejects_empty_non_eof() {
        assert_eq!(
            TokenRef::new_checked(TokenKind::Identifier, "", 5, 5),
            Err(TokenSpanError::EmptySpanNotAllowed { kind: TokenKind::Identifier, at: 5 })
        );
    }

    #[test]
    fn token_ref_new_checked_rejects_text_length_mismatch() {
        assert_eq!(
            TokenRef::new_checked(TokenKind::Identifier, "hello", 0, 1),
            Err(TokenSpanError::TextLengthMismatch { text_len: 5, span_len: 1, start: 0, end: 1 })
        );
    }
}
