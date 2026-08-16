//! Focused contract tests for context-preserving assertion helpers.

use std::any::Any;
use std::fmt;
use std::panic::catch_unwind;

use perl_test_must::{must_err_with, must_some_with, must_with};

#[derive(Debug)]
struct IndexedSymbol;

struct UnexpectedValue;

impl fmt::Debug for UnexpectedValue {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("unexpected-value-7")
    }
}

struct ExpectedError;

#[test]
fn must_with_renders_context_error_and_type_once() -> Result<(), String> {
    let payload = catch_unwind(|| {
        must_with::<i32, &str>(Err("boom"), "fixture config must be valid")
    });
    let message = panic_message(payload.err().ok_or("expected must_with to panic")?)?;

    assert_eq!(occurrences(&message, "must:"), 1, "message was: {message}");
    assert_eq!(
        occurrences(&message, "fixture config must be valid"),
        1,
        "message was: {message}"
    );
    assert_eq!(occurrences(&message, "unexpected Err<&str>"), 1, "message was: {message}");
    assert_eq!(occurrences(&message, "\"boom\""), 1, "message was: {message}");
    Ok(())
}

#[test]
fn must_some_with_renders_context_and_payload_type_once() -> Result<(), String> {
    let payload = catch_unwind(|| {
        must_some_with(Option::<IndexedSymbol>::None, "indexed symbol must exist")
    });
    let message = panic_message(payload.err().ok_or("expected must_some_with to panic")?)?;

    assert_eq!(occurrences(&message, "must_some:"), 1, "message was: {message}");
    assert_eq!(
        occurrences(&message, "indexed symbol must exist"),
        1,
        "message was: {message}"
    );
    assert_eq!(occurrences(&message, "unexpected None<"), 1, "message was: {message}");
    assert_eq!(occurrences(&message, "IndexedSymbol"), 1, "message was: {message}");
    Ok(())
}

#[test]
fn must_err_with_renders_context_types_and_value_once() -> Result<(), String> {
    let payload = catch_unwind(|| {
        must_err_with::<UnexpectedValue, ExpectedError>(
            Ok(UnexpectedValue),
            "invalid fixture must be rejected",
        )
    });
    let message = panic_message(payload.err().ok_or("expected must_err_with to panic")?)?;

    assert_eq!(occurrences(&message, "must_err:"), 1, "message was: {message}");
    assert_eq!(
        occurrences(&message, "invalid fixture must be rejected"),
        1,
        "message was: {message}"
    );
    assert_eq!(occurrences(&message, "expected Err<"), 1, "message was: {message}");
    assert_eq!(occurrences(&message, "ExpectedError"), 1, "message was: {message}");
    assert_eq!(occurrences(&message, "got Ok<"), 1, "message was: {message}");
    assert_eq!(occurrences(&message, "UnexpectedValue"), 1, "message was: {message}");
    assert_eq!(occurrences(&message, "unexpected-value-7"), 1, "message was: {message}");
    Ok(())
}

#[test]
fn context_variants_accept_borrowed_values_and_format_arguments() -> Result<(), String> {
    let owned = String::from("borrowed");
    let context_subject = String::from("fixture");

    let result: Result<&str, &str> = Ok(owned.as_str());
    assert_eq!(
        must_with(result, format_args!("{context_subject} must load")),
        "borrowed"
    );

    assert_eq!(
        must_some_with(
            Some(owned.as_str()),
            format_args!("{context_subject} contains an item")
        ),
        "borrowed"
    );

    let error_result: Result<&str, &str> = Err(owned.as_str());
    assert_eq!(
        must_err_with(
            error_result,
            format_args!("{context_subject} must reject invalid input")
        ),
        "borrowed"
    );
    Ok(())
}

#[test]
fn must_with_accepts_unit_success_as_a_side_effect_assertion() {
    must_with::<(), &str>(Ok(()), "side effect completed");
}

fn occurrences(message: &str, needle: &str) -> usize {
    message.match_indices(needle).count()
}

fn panic_message(payload: Box<dyn Any + Send>) -> Result<String, String> {
    if let Some(message) = payload.downcast_ref::<String>() {
        return Ok(message.clone());
    }

    if let Some(message) = payload.downcast_ref::<&'static str>() {
        return Ok((*message).to_owned());
    }

    Err(String::from("panic payload was not a string"))
}
