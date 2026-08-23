//! Public contract tests for `perl-test-must`.

use std::fmt;
use std::panic::{UnwindSafe, catch_unwind};

use perl_test_must::{must, must_err, must_err_with, must_some, must_some_with, must_with};

struct DiagnosticError;

impl fmt::Debug for DiagnosticError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("diagnostic-error")
    }
}

#[derive(Debug)]
struct MissingItem;

struct ExpectedError;

struct UnexpectedOk;

impl fmt::Debug for UnexpectedOk {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("unexpected-ok-value")
    }
}

#[test]
fn non_copy_and_borrowed_values_preserve_the_generic_contract() {
    let owned = String::from("owned value");
    let result: Result<String, &str> = Ok(owned);
    assert_eq!(must(result), "owned value");

    let borrowed_owner = String::from("borrowed value");
    let borrowed = borrowed_owner.as_str();
    let context_owner = String::from("fixture context");
    let context = context_owner.as_str();

    assert_eq!(must::<&str, &str>(Ok(borrowed)), borrowed);
    assert_eq!(must_with::<&str, &str>(Ok(borrowed), context), borrowed);
    assert_eq!(must_some(Some(borrowed)), borrowed);
    assert_eq!(must_some_with(Some(borrowed), context), borrowed);
    assert_eq!(must_err::<&str, &str>(Err(borrowed)), borrowed);
    assert_eq!(must_err_with::<&str, &str>(Err(borrowed), context), borrowed);
}

#[test]
fn unit_results_remain_valid_side_effect_assertions() {
    must::<(), &str>(Ok(()));
    must_with::<(), &str>(Ok(()), "side effect must succeed");
}

#[test]
fn nested_result_and_option_extraction_composes() {
    let nested: Result<Option<String>, &str> = Ok(Some(String::from("value")));
    assert_eq!(must_some(must(nested)), "value");
}

#[test]
fn must_failure_reports_semantic_clauses_once() -> Result<(), String> {
    let message = panic_text(|| {
        must::<(), DiagnosticError>(Err(DiagnosticError));
    })?;

    assert_eq!(occurrences(&message, "must:"), 1, "message was: {message}");
    assert_eq!(occurrences(&message, "unexpected Err<"), 1, "message was: {message}");
    assert_eq!(occurrences(&message, "DiagnosticError"), 1, "message was: {message}");
    assert_eq!(occurrences(&message, "diagnostic-error"), 1, "message was: {message}");
    Ok(())
}

#[test]
fn must_some_with_failure_reports_context_and_type_once() -> Result<(), String> {
    let message = panic_text(|| {
        let _ = must_some_with(Option::<MissingItem>::None, "indexed symbol must exist");
    })?;

    assert_eq!(occurrences(&message, "must_some:"), 1, "message was: {message}");
    assert_eq!(occurrences(&message, "indexed symbol must exist"), 1, "message was: {message}");
    assert_eq!(occurrences(&message, "unexpected None<"), 1, "message was: {message}");
    assert_eq!(occurrences(&message, "MissingItem"), 1, "message was: {message}");
    Ok(())
}

#[test]
fn must_err_with_failure_reports_context_types_and_value_once() -> Result<(), String> {
    let message = panic_text(|| {
        let _ = must_err_with::<UnexpectedOk, ExpectedError>(
            Ok(UnexpectedOk),
            "invalid fixture must be rejected",
        );
    })?;

    assert_eq!(occurrences(&message, "must_err:"), 1, "message was: {message}");
    assert_eq!(
        occurrences(&message, "invalid fixture must be rejected"),
        1,
        "message was: {message}"
    );
    assert_eq!(occurrences(&message, "expected Err<"), 1, "message was: {message}");
    assert_eq!(occurrences(&message, "ExpectedError"), 1, "message was: {message}");
    assert_eq!(occurrences(&message, "got Ok<"), 1, "message was: {message}");
    assert_eq!(occurrences(&message, "UnexpectedOk"), 1, "message was: {message}");
    assert_eq!(occurrences(&message, "unexpected-ok-value"), 1, "message was: {message}");
    Ok(())
}

#[test]
fn context_accepts_format_arguments_without_new_bounds() {
    let subject = String::from("fixture");
    let value: Result<&str, &str> = Ok("ready");

    assert_eq!(must_with(value, format_args!("{subject} must load")), "ready");
}

fn occurrences(message: &str, needle: &str) -> usize {
    message.match_indices(needle).count()
}

fn panic_text(operation: impl FnOnce() + UnwindSafe) -> Result<String, String> {
    let payload =
        catch_unwind(operation).err().ok_or_else(|| String::from("expected operation to panic"))?;

    if let Some(message) = payload.downcast_ref::<String>() {
        return Ok(message.clone());
    }

    if let Some(message) = payload.downcast_ref::<&'static str>() {
        return Ok((*message).to_owned());
    }

    Err(String::from("panic payload was not a string"))
}
