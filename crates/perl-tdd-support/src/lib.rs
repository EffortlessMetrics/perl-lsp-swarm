//! Test-driven development helpers and generators for Perl.
//!
//! This crate provides tools to support TDD workflows when working with Perl code,
//! including test generation, execution runners, and validation utilities for
//! Perl parser and LSP development.
//!
//! # Overview
//!
//! The TDD support crate offers:
//! - Test case generators for Perl syntax patterns
//! - Test execution runners with result capture
//! - Basic TDD workflow helpers for parser development
//! - Utilities for validating parser behavior against expected outcomes
//!
//! # Example
//!
//! ```no_run
//! use perl_tdd_support::tdd_basic;
//!
//! // Use TDD helpers to validate parser behavior
//! // (specific APIs depend on tdd module implementation)
//! ```

#![deny(unsafe_code)]
#![deny(unreachable_pub)]
#![warn(rust_2018_idioms)]
#![warn(missing_docs)]
// This crate provides test helpers that intentionally panic on failure.
// The must/must_some/must_err helpers are designed to panic in tests.
#![allow(clippy::panic)]
#![allow(
    clippy::print_stderr,
    reason = "test support crate intentionally emits narrative scenario output"
)]
#![deny(clippy::map_err_ignore)] // Cohort C0 activation (#12598): census-clean on all targets; new findings move the crate to C1.
#![allow(
    clippy::too_many_lines,
    clippy::module_name_repetitions,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    clippy::cast_possible_wrap,
    clippy::must_use_candidate,
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    clippy::wildcard_imports,
    clippy::enum_glob_use,
    clippy::match_same_arms,
    clippy::if_not_else,
    clippy::struct_excessive_bools,
    clippy::items_after_statements,
    clippy::return_self_not_must_use,
    clippy::unused_self,
    clippy::only_used_in_recursion,
    clippy::items_after_test_module,
    clippy::while_let_loop,
    clippy::single_range_in_vec_init,
    clippy::arc_with_non_send_sync,
    clippy::needless_range_loop,
    clippy::result_large_err,
    clippy::if_same_then_else,
    clippy::should_implement_trait,
    clippy::manual_flatten,
    clippy::needless_raw_string_hashes,
    clippy::single_char_pattern,
    clippy::uninlined_format_args
)]

pub use perl_parser_core::{Node, NodeKind, SourceLocation};
pub use perl_parser_core::{ParseError, ParseResult, error, parser};
pub use perl_parser_core::{Parser, ast, position};

/// Test-driven development helpers and generators.
pub mod tdd;

/// BDD-style scenario helper for narrative test logs.
pub mod bdd;

pub use bdd::BddScenario;

pub use tdd::tdd_basic;
pub use tdd::tdd_workflow;
pub use tdd::test_generator;
/// Test execution and TDD support functionality.
pub use tdd::test_runner;

/// Safe unwrap replacements for tests.
/// Re-exported from `perl-test-must` for backward compatibility.
///
/// Both families are re-exported. The `_with` variants are the
/// context-preserving counterparts: when a call site migrates away from
/// `.expect("…")`, `must_with` / `must_some_with` / `must_err_with` keep the
/// explanation in the panic diagnostic, while the bare helpers drop it
/// ([#14291]). Re-exporting only the bare three made the correct form
/// unreachable through this import path.
///
/// [#14291]: https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/14291
pub use perl_test_must::{must, must_err, must_err_with, must_some, must_some_with, must_with};

/// Independent old-generation edit transaction model for source-equivalence
/// proof ([#7344]).
///
/// Applies an ordered edit transaction to an immutable predecessor source
/// without calling parser or production edit-application code, so a
/// differential harness can compare production incremental source state
/// against an oracle that cannot reproduce the production applicator's own
/// defects.
///
/// [#7344]: https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/7344
pub mod reference_edit;

pub use reference_edit::{
    REFERENCE_EDIT_COORDINATE_MODEL_ID, ReferenceByteMapSegment, ReferenceEdit, ReferenceEditError,
    ReferenceEditResult, ReferenceEditTransaction, ReferenceSourceState,
};

/// Typed skip for symlink-creating tests on Windows sessions without
/// `SeCreateSymbolicLinkPrivilege` (os error 1314).
pub mod symlink_privilege;

pub use symlink_privilege::{SymlinkTestDecision, classify_symlink_error, symlink_test_decision};

/// CI Guardrail Ignored Test Monitoring and Governance.
pub mod governance;

/// Windows symlink capability helpers for tests that exercise reparse-point
/// semantics without requiring Developer Mode ([#12567]).
///
/// Windows-only by construction: every helper wraps a `std::os::windows::fs`
/// API and none has a non-Windows stub. On Unix targets tests use
/// [`std::os::unix::fs::symlink`] directly under `#[cfg(unix)]` ([#12567]),
/// so both this module and the crate-root re-export are gated behind
/// `#[cfg(windows)]`; unconditional imports of these names are a compile
/// error on other targets.
#[cfg(windows)]
pub mod windows_fs;

#[cfg(windows)]
pub use windows_fs::{try_create_dir_symlink, try_create_file_symlink};
