#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]
#![deny(clippy::map_err_ignore)] // Cohort C0 activation (#12598): census-clean on all targets; new findings move the crate to C1.
use perl_token::{Token, TokenKind, TokenRef, TokenSpan, TokenSpanError};
use std::error::Error;
use std::sync::Arc;

type TestResult = Result<(), Box<dyn Error>>;

fn ordered_span(start: usize, end: usize) -> TokenSpan {
    TokenSpan::try_new(start, end).expect("ordered span")
}

#[test]
fn token_ref_new_exposes_borrowed_fields() -> TestResult {
    let source = "my $name";
    let token = TokenRef::new_checked(TokenKind::My, &source[0..2], 0, 2)?;

    assert_eq!(token.kind(), TokenKind::My);
    assert_eq!(token.text, "my");
    assert_eq!(token.len(), 2);
    assert!(!token.is_empty());
    assert_eq!(token.span(), ordered_span(0, 2));
    assert_eq!(token.display_name(), "'my'");
    Ok(())
}

#[test]
fn token_ref_to_owned_token_is_explicit() -> TestResult {
    let borrowed = TokenRef::new_checked(TokenKind::Identifier, "value", 8, 13)?;
    let owned = borrowed.to_owned_token();

    assert_eq!(owned.kind(), TokenKind::Identifier);
    assert_eq!(&*owned.text, "value");
    assert_eq!(owned.span(), ordered_span(8, 13));
    assert_eq!(owned.display_name(), "identifier");
    Ok(())
}

#[test]
fn externally_mutated_token_ref_text_cannot_create_invalid_owned_token() -> TestResult {
    let mut borrowed = TokenRef::new_checked(TokenKind::UnknownRest, "", 40, 96)?;
    borrowed.text = "mutated payload";

    let owned = borrowed.to_owned_token();

    assert_eq!(owned.kind(), TokenKind::UnknownRest);
    assert_eq!(owned.span(), ordered_span(40, 96));
    assert!(owned.text.is_empty());
    assert!(owned.is_geometry_only());
    Ok(())
}

#[test]
fn externally_mutated_token_ref_equal_width_payload_stays_geometry_only() -> TestResult {
    let equal_width_payload = "x".repeat(56);
    let mut borrowed = TokenRef::new_checked(TokenKind::UnknownRest, "", 40, 96)?;
    borrowed.text = &equal_width_payload;

    let owned = borrowed.to_owned_token();

    assert_eq!(owned.kind(), TokenKind::UnknownRest);
    assert_eq!(owned.span(), ordered_span(40, 96));
    assert!(owned.text.is_empty());
    assert!(owned.is_geometry_only());
    Ok(())
}

#[test]
fn payload_bearing_unknown_rest_round_trips_through_token_ref() -> TestResult {
    let payload = "remainder";
    let borrowed = TokenRef::new_checked(TokenKind::UnknownRest, payload, 40, 49)?;

    let owned = borrowed.to_owned_token();

    assert_eq!(owned.kind(), TokenKind::UnknownRest);
    assert_eq!(owned.span(), ordered_span(40, 49));
    assert_eq!(&*owned.text, payload);
    assert!(!owned.is_geometry_only());
    Ok(())
}

#[test]
fn token_from_token_ref_matches_constructor() -> TestResult {
    let borrowed = TokenRef::new_checked(TokenKind::Number, "42", 20, 22)?;
    let from_impl: Token = borrowed.into();
    let constructed = Token::new_checked(TokenKind::Number, "42", 20, 22)?;

    assert_eq!(from_impl, constructed);
    Ok(())
}

#[test]
fn token_as_ref_token_roundtrips_without_changing_span() -> TestResult {
    let token = Token::new_checked(TokenKind::String, "\"abc\"", 4, 9)?;
    let borrowed = token.as_ref_token();

    assert_eq!(borrowed.kind(), TokenKind::String);
    assert_eq!(borrowed.text, "\"abc\"");
    assert_eq!(borrowed.span(), token.span());

    let rebuilt = borrowed.to_owned_token();
    assert_eq!(rebuilt, token);
    Ok(())
}

#[test]
fn token_ref_does_not_touch_existing_arc_token_storage() -> TestResult {
    let text: Arc<str> = Arc::from("shared");
    let token = Token::new_checked(TokenKind::Identifier, text.clone(), 0, 6)?;
    let strong_before = Arc::strong_count(&text);

    let borrowed = token.as_ref_token();
    assert_eq!(borrowed.text, "shared");
    assert_eq!(Arc::strong_count(&text), strong_before);
    Ok(())
}

#[test]
fn token_ref_new_checked_rejects_malformed_span() -> TestResult {
    let err = match TokenRef::new_checked(TokenKind::Unknown, "x", 10, 5) {
        Ok(_) => return Err("TokenRef must not saturate reversed spans".into()),
        Err(err) => err,
    };
    assert_eq!(err, TokenSpanError::EndBeforeStart { start: 10, end: 5 });
    Ok(())
}

#[test]
fn token_ref_is_empty_for_zero_length_span() -> TestResult {
    let borrowed = TokenRef::new_checked(TokenKind::Eof, "", 8, 8)?;
    assert_eq!(borrowed.len(), 0);
    assert!(borrowed.is_empty());
    assert_eq!(borrowed.span(), ordered_span(8, 8));
    Ok(())
}

#[test]
fn token_ref_try_new_rejects_end_before_start() -> TestResult {
    let err = match TokenRef::try_new(TokenKind::Identifier, "x", 10, 3) {
        Ok(_) => return Err("TokenRef::try_new should reject end < start".into()),
        Err(err) => err,
    };
    assert_eq!(err, TokenSpanError::EndBeforeStart { start: 10, end: 3 });
    Ok(())
}

#[test]
fn token_ref_new_checked_rejects_empty_non_eof() -> TestResult {
    let err = match TokenRef::new_checked(TokenKind::Identifier, "", 5, 5) {
        Ok(_) => return Err("TokenRef::new_checked should reject empty non-EOF spans".into()),
        Err(err) => err,
    };
    assert_eq!(err, TokenSpanError::EmptySpanNotAllowed { kind: TokenKind::Identifier, at: 5 });
    Ok(())
}

#[test]
fn token_ref_new_checked_allows_empty_unknown() -> TestResult {
    let token = TokenRef::new_checked(TokenKind::Unknown, "", 21, 21)?;
    assert_eq!(token.kind(), TokenKind::Unknown);
    assert_eq!(token.text, "");
    assert_eq!(token.span(), ordered_span(21, 21));
    assert!(token.is_empty());
    Ok(())
}

#[test]
fn token_ref_new_checked_allows_empty_eof() -> TestResult {
    let token = TokenRef::new_checked(TokenKind::Eof, "", 12, 12)?;
    assert_eq!(token.kind(), TokenKind::Eof);
    assert_eq!(token.span(), ordered_span(12, 12));
    assert!(token.is_empty());
    Ok(())
}
