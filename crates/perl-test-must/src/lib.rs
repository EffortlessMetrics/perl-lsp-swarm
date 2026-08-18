#![deny(missing_docs)]

//! Assertion-boundary extraction helpers for Rust tests.
//!
//! `perl-test-must` is a dependency-free leaf crate for the cases where a test
//! scenario asserts that a [`Result`] or [`Option`] branch is impossible. The
//! helpers preserve the unexpected value, its type, and the invocation location
//! in the panic diagnostic.
//!
//! # Propagate setup failures with `?`
//!
//! Use ordinary `Result` propagation when a helper or test setup step should
//! return its failure to the caller:
//!
//! ```rust
//! # fn read_fixture() -> Result<&'static str, &'static str> { Ok("source") }
//! # fn setup() -> Result<&'static str, &'static str> {
//! let source = read_fixture()?;
//! Ok(source)
//! # }
//! # assert_eq!(setup(), Ok("source"));
//! ```
//!
//! # Assert scenario invariants with `must*`
//!
//! Use these helpers at the assertion boundary, where the scenario itself says
//! which branch must exist:
//!
//! ```rust
//! use perl_test_must::{must, must_err, must_some_with};
//!
//! let parsed: Result<i32, &str> = Ok(42);
//! assert_eq!(must(parsed), 42);
//!
//! let symbol = must_some_with(Some("Example"), "the fixture declares Example");
//! assert_eq!(symbol, "Example");
//!
//! let rejected: Result<(), &str> = Err("invalid fixture");
//! assert_eq!(must_err(rejected), "invalid fixture");
//! ```
//!
//! The `_with` variants preserve an `expect`-style explanation. All helpers use
//! [`track_caller`](https://doc.rust-lang.org/reference/attributes/codegen.html#the-track_caller-attribute)
//! so a failure points to the test invocation rather than this crate's internals.
//! Fully qualified type-name spelling is diagnostic evidence, not a stable ABI
//! or portable string contract.

use std::any::type_name;
use std::fmt::{self, Debug, Display};

/// Extracts the success value from a [`Result`].
///
/// Use this where the test scenario asserts that the operation must succeed. For
/// setup failures that should propagate, return `Result` from the caller and use
/// `?` instead.
///
/// `must` intentionally does not carry `#[must_use]`: asserting a side-effecting
/// `Result<(), E>` and discarding the extracted unit value is valid.
///
/// # Panics
///
/// Panics at the invocation site when `result` is [`Err`], including the error
/// type and [`Debug`] representation in the diagnostic.
///
/// # Examples
///
/// ```rust
/// use perl_test_must::must;
///
/// let result: Result<i32, &str> = Ok(42);
/// assert_eq!(must(result), 42);
/// ```
#[track_caller]
pub fn must<T, E: Debug>(result: Result<T, E>) -> T {
    match result {
        Ok(value) => value,
        Err(error) => panic_failure(
            "must",
            None,
            format_args!("unexpected Err<{}>: {error:?}", type_name::<E>()),
        ),
    }
}

/// Extracts the success value from a [`Result`] and retains assertion context.
///
/// This is the context-preserving counterpart to [`must`]. The supplied
/// [`Display`] value appears once in the failure diagnostic.
///
/// `must_with` intentionally does not carry `#[must_use]`: asserting a
/// side-effecting `Result<(), E>` and discarding the extracted unit value is
/// valid.
///
/// # Panics
///
/// Panics at the invocation site when `result` is [`Err`], including `context`,
/// the error type, and the error's [`Debug`] representation.
///
/// # Examples
///
/// ```rust
/// use perl_test_must::must_with;
///
/// let result: Result<&str, &str> = Ok("ready");
/// assert_eq!(must_with(result, "the fixture must load"), "ready");
/// ```
#[track_caller]
pub fn must_with<T, E: Debug>(result: Result<T, E>, context: impl Display) -> T {
    match result {
        Ok(value) => value,
        Err(error) => panic_failure(
            "must",
            Some(&context),
            format_args!("unexpected Err<{}>: {error:?}", type_name::<E>()),
        ),
    }
}

/// Extracts the present value from an [`Option`].
///
/// # Panics
///
/// Panics at the invocation site when `option` is [`None`], including the
/// expected payload type in the diagnostic.
///
/// # Examples
///
/// ```rust
/// use perl_test_must::must_some;
///
/// assert_eq!(must_some(Some("ready")), "ready");
/// ```
#[track_caller]
#[must_use]
pub fn must_some<T>(option: Option<T>) -> T {
    match option {
        Some(value) => value,
        None => {
            panic_failure("must_some", None, format_args!("unexpected None<{}>", type_name::<T>()))
        }
    }
}

/// Extracts the present value from an [`Option`] and retains assertion context.
///
/// This is the context-preserving counterpart to [`must_some`]. The supplied
/// [`Display`] value appears once in the failure diagnostic.
///
/// # Panics
///
/// Panics at the invocation site when `option` is [`None`], including `context`
/// and the expected payload type.
///
/// # Examples
///
/// ```rust
/// use perl_test_must::must_some_with;
///
/// let symbol = must_some_with(Some("Example"), "the fixture declares Example");
/// assert_eq!(symbol, "Example");
/// ```
#[track_caller]
#[must_use]
pub fn must_some_with<T>(option: Option<T>, context: impl Display) -> T {
    match option {
        Some(value) => value,
        None => panic_failure(
            "must_some",
            Some(&context),
            format_args!("unexpected None<{}>", type_name::<T>()),
        ),
    }
}

/// Extracts the error value from a [`Result`].
///
/// # Panics
///
/// Panics at the invocation site when `result` is [`Ok`], including the expected
/// error type, observed success type, and success value's [`Debug`]
/// representation.
///
/// # Examples
///
/// ```rust
/// use perl_test_must::must_err;
///
/// let result: Result<(), &str> = Err("rejected");
/// assert_eq!(must_err(result), "rejected");
/// ```
#[track_caller]
#[must_use]
pub fn must_err<T: Debug, E>(result: Result<T, E>) -> E {
    match result {
        Err(error) => error,
        Ok(value) => panic_failure(
            "must_err",
            None,
            format_args!(
                "expected Err<{}>, got Ok<{}>: {value:?}",
                type_name::<E>(),
                type_name::<T>()
            ),
        ),
    }
}

/// Extracts the error value from a [`Result`] and retains assertion context.
///
/// This is the context-preserving counterpart to [`must_err`]. The supplied
/// [`Display`] value appears once in the failure diagnostic.
///
/// # Panics
///
/// Panics at the invocation site when `result` is [`Ok`], including `context`,
/// the expected error type, observed success type, and success value's [`Debug`]
/// representation.
///
/// # Examples
///
/// ```rust
/// use perl_test_must::must_err_with;
///
/// let result: Result<(), &str> = Err("invalid input");
/// assert_eq!(
///     must_err_with(result, "the invalid fixture must be rejected"),
///     "invalid input"
/// );
/// ```
#[track_caller]
#[must_use]
pub fn must_err_with<T: Debug, E>(result: Result<T, E>, context: impl Display) -> E {
    match result {
        Err(error) => error,
        Ok(value) => panic_failure(
            "must_err",
            Some(&context),
            format_args!(
                "expected Err<{}>, got Ok<{}>: {value:?}",
                type_name::<E>(),
                type_name::<T>()
            ),
        ),
    }
}

#[track_caller]
#[expect(
    clippy::panic,
    reason = "This test assertion failure seam intentionally panics when its required branch is absent."
)]
fn panic_failure(
    helper: &'static str,
    context: Option<&dyn Display>,
    detail: fmt::Arguments<'_>,
) -> ! {
    match context {
        Some(context) => panic!("{helper}: {context}: {detail}"),
        None => panic!("{helper}: {detail}"),
    }
}

#[cfg(test)]
mod tests {
    use super::{must, must_err, must_err_with, must_some, must_some_with, must_with};

    #[test]
    fn must_unwraps_ok() {
        let result: Result<i32, &str> = Ok(42);
        assert_eq!(must(result), 42);
    }

    #[test]
    #[should_panic(expected = "must: unexpected Err")]
    fn must_panics_on_err() {
        let result: Result<i32, &str> = Err("oops");
        must(result);
    }

    #[test]
    fn must_with_unwraps_ok() {
        let result: Result<i32, &str> = Ok(42);
        assert_eq!(must_with(result, "fixture must load"), 42);
    }

    #[test]
    #[should_panic(expected = "must: fixture must load: unexpected Err")]
    fn must_with_panics_on_err_with_context() {
        let result: Result<i32, &str> = Err("oops");
        must_with(result, "fixture must load");
    }

    #[test]
    fn must_some_unwraps_some() {
        assert_eq!(must_some(Some(99)), 99);
    }

    #[test]
    #[should_panic(expected = "must_some: unexpected None")]
    fn must_some_panics_on_none() {
        let _ = must_some(Option::<i32>::None);
    }

    #[test]
    fn must_some_with_unwraps_some() {
        assert_eq!(must_some_with(Some(99), "fixture has a value"), 99);
    }

    #[test]
    #[should_panic(expected = "must_some: fixture has a value: unexpected None")]
    fn must_some_with_panics_on_none_with_context() {
        let _ = must_some_with(Option::<i32>::None, "fixture has a value");
    }

    #[test]
    fn must_err_unwraps_err() {
        let result: Result<i32, &str> = Err("expected error");
        assert_eq!(must_err(result), "expected error");
    }

    #[test]
    #[should_panic(expected = "must_err: expected Err")]
    fn must_err_panics_on_ok() {
        let result: Result<i32, &str> = Ok(1);
        let _ = must_err(result);
    }

    #[test]
    fn must_err_with_unwraps_err() {
        let result: Result<i32, &str> = Err("expected error");
        assert_eq!(must_err_with(result, "fixture must fail"), "expected error");
    }

    #[test]
    #[should_panic(expected = "must_err: fixture must fail: expected Err")]
    fn must_err_with_panics_on_ok_with_context() {
        let result: Result<i32, &str> = Ok(1);
        let _ = must_err_with(result, "fixture must fail");
    }
}
