//! Package-local assertion-boundary helpers.
//!
//! These replace the swarm-only `perl-tdd-support` / `perl-test-must`
//! dev-dependency (`#8771`) so the packaged test population resolves outside
//! this workspace. They are deliberately narrow: only the two behaviors this
//! package's tests actually use are reproduced, with the same
//! `#[track_caller]` and type-name diagnostics as the shared helper.
//!
//! Use these only at an assertion boundary, where the scenario itself says
//! which branch must exist. Prefer `?` propagation for setup failures.

// Each test binary includes this file directly and uses a subset of it.
#![allow(dead_code)]

// `clippy::panic` is denied package-wide for production code. Each helper below
// carries a narrow, documented exception: panicking at an asserted-impossible
// branch is the entire contract of an assertion-boundary helper.

use std::any::type_name;
use std::fmt::Debug;

/// Extracts the success value from a [`Result`].
///
/// # Panics
///
/// Panics at the invocation site when `result` is [`Err`], naming the error
/// type and its [`Debug`] representation.
#[allow(clippy::panic, reason = "test-only assertion boundary (#8771)")]
#[track_caller]
pub fn must<T, E: Debug>(result: Result<T, E>) -> T {
    match result {
        Ok(value) => value,
        Err(error) => {
            panic!("must: unexpected Err<{}>: {error:?}", type_name::<E>())
        }
    }
}

/// Extracts the error value from a [`Result`].
///
/// # Panics
///
/// Panics at the invocation site when `result` is [`Ok`], naming the expected
/// error type, the success type, and the success value's [`Debug`]
/// representation — the same diagnostic shape as the shared helper.
#[allow(clippy::panic, reason = "test-only assertion boundary (#8771)")]
#[track_caller]
pub fn must_err<T: Debug, E>(result: Result<T, E>) -> E {
    match result {
        Ok(value) => {
            panic!(
                "must_err: expected Err<{}>, got Ok<{}>: {value:?}",
                type_name::<E>(),
                type_name::<T>()
            )
        }
        Err(error) => error,
    }
}
