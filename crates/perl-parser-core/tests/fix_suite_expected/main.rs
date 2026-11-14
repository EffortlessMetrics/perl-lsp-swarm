//! Consolidated integration tests for expected-token parser fixes (#5012 item 7).
//!
//! Former standalone targets under `tests/fix_expected_*.rs` are modules here so
//! they compile as one integration-test binary (`fix_suite_expected`).

#![allow(clippy::print_stderr)] // fix_expected_colon_tests prints sexp context on failure.

#[path = "../cpan_test_helpers/mod.rs"]
mod cpan_test_helpers;

mod fix_expected_colon;
mod fix_expected_colon_sprintf_qq_2721;
mod fix_expected_colon_tests;
mod fix_expected_identifier_package_keywords_tests;
mod fix_expected_identifier_tests;
mod fix_expected_left_brace;
mod fix_expected_left_brace_2399;
mod fix_expected_semicolon_for_loop;
