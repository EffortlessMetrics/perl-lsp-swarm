#![warn(missing_docs)]
#![cfg_attr(clippy, allow(missing_docs))]

//! Safe unwrap replacements for tests.
//!
//! This crate provides panic-on-failure helpers that are safe to use in tests,
//! avoiding explicit `unwrap()` calls which are denied by clippy policy.
//!
//! # Overview
//!
//! Three extraction helpers cover the common branches, and each has a
//! context-bearing counterpart for preserving an `expect`-style explanation:
//! - [`must`] / [`must_with`] — extract the value from a `Result`, or panic with the error
//! - [`must_some`] / [`must_some_with`] — extract the value from an `Option`, or panic
//! - [`must_err`] / [`must_err_with`] — extract the error from a `Result`, or panic if `Ok`
//!
//! # Example
//!
//! ```rust
//! use perl_test_must::{must, must_err, must_some_with};
//!
//! let result: Result<i32, &str> = Ok(42);
//! assert_eq!(must(result), 42);
//!
//! let opt: Option<i32> = Some(7);
//! assert_eq!(must_some_with(opt, "the fixture contains an item"), 7);
//!
//! let err_result: Result<i32, &str> = Err("oops");
//! assert_eq!(must_err(err_result), "oops");
//! ```

// This crate provides test helpers that intentionally panic on failure.
// The must/must_some/must_err helper families are designed to panic in tests.
#![allow(clippy::panic)]

use std::any::type_name;
use std::fmt::{self, Debug, Display};

/// Extract the value from a `Result`, or panic with the error.
///
/// This is a test-only replacement for `unwrap` that is compliant
/// with the "no unwrap/expect" policy.
///
/// Note: `#[must_use]` is intentionally omitted. `must()` is frequently
/// called as an assertion (`must(fs::write(...))`) where the caller intentionally
/// discards the `()` return value. Adding `#[must_use]` would trigger ~373
/// spurious warnings across the workspace for those valid use cases.
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

/// Extract the value from a `Result`, or panic with the supplied context and error.
///
/// Use this when the test invariant needs an `expect`-style explanation in addition
/// to the unexpected error value. Like [`must`], this function intentionally omits
/// `#[must_use]` so `Result<(), E>` can be asserted for side effects.
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

/// Extract the value from an `Option`, or panic.
///
/// This is a test-only replacement for `unwrap` that is compliant
/// with the "no unwrap/expect" policy.
#[track_caller]
#[must_use]
pub fn must_some<T>(option: Option<T>) -> T {
    match option {
        Some(value) => value,
        None => panic_failure(
            "must_some",
            None,
            format_args!("unexpected None<{}>", type_name::<T>()),
        ),
    }
}

/// Extract the value from an `Option`, or panic with the supplied context.
///
/// Use this when the test invariant needs an `expect`-style explanation for why
/// the value must be present.
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

/// Extract the error from a `Result`, or panic if `Ok`.
///
/// This is a test-only replacement for `.unwrap_err()` that is compliant
/// with the "no unwrap/expect" policy.
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

/// Extract the error from a `Result`, or panic with the supplied context if `Ok`.
///
/// Use this when the test invariant needs an `expect_err`-style explanation for
/// why the operation must fail.
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
