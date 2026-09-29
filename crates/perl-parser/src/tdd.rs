//! Compatibility re-export of TDD support modules.
//!
//! Keep the module boundary explicit so adding a public item to
//! `perl-tdd-support` does not silently publish it through `perl-parser`.

/// Basic TDD helpers and generators.
pub use perl_tdd_support::tdd::tdd_basic;
/// TDD workflow state and actions.
pub use perl_tdd_support::tdd::tdd_workflow;
/// Test-case generation and coverage helpers.
pub use perl_tdd_support::tdd::test_generator;
/// Test discovery and execution helpers.
pub use perl_tdd_support::tdd::test_runner;
