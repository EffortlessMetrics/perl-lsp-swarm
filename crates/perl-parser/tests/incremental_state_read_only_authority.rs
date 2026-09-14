//! Gate-run proof that `IncrementalState`'s compatibility view grants reads
//! only (#13774).
//!
//! These contracts were previously written as `compile_fail` doctests on
//! `IncrementalState`. They proved nothing, for two independent reasons. No
//! gate, workflow, or `justfile` recipe ever ran `cargo test --doc` at all;
//! and the `incremental` module is behind a non-default feature, so even a
//! `--doc` route would leave them uncompiled unless it named that feature.
//!
//! (`crates/perl-parser/Cargo.toml` also sets `[lib] doctest = false`, but
//! measured on the pinned 1.95.0 toolchain that is *not* why they were inert:
//! the field only removes doctests from the default `cargo test` selection,
//! and an explicit `--doc` collects them regardless.)
//!
//! They live here instead because `unit_parser_stack_full` names this target
//! explicitly with `--features incremental`, so a mutation that reintroduces
//! mutation authority fails a check that actually runs.

#![cfg(feature = "incremental")]

use perl_parser::incremental::IncrementalState;
use static_assertions::assert_not_impl_any;

type TestResult = Result<(), Box<dyn std::error::Error>>;

// `IncrementalState` derefs to its read view so legacy field-style reads keep
// working, and that is the whole compatibility surface. A `DerefMut` impl
// would hand every caller `&mut` access to `source`, `tokens`,
// `lex_checkpoints`, the line index, and the parser output independently of
// one another — exactly the generation atomicity the private `read_view`
// field exists to hold. Reintroducing one breaks the build of this target.
//
// This is strictly stronger than the two `compile_fail` doctests it replaces:
// those named `state.source.push_str(..)` and `state.tokens.clear()`
// individually, whereas the absent impl is the single mechanism that denies
// every such call at once.
assert_not_impl_any!(IncrementalState: std::ops::DerefMut);

/// The legacy field reads the compatibility view exists for still agree with
/// the accessors.
///
/// Migrated from the one passing doctest on `IncrementalState`, which was
/// inert for the same reason the `compile_fail` pair was.
#[test]
fn legacy_field_reads_agree_with_read_only_accessors() -> TestResult {
    let state = IncrementalState::new("my $x = 1;".to_string());

    if state.source.len() != state.source().len() {
        return Err("source field read disagrees with source() accessor".into());
    }
    if state.tokens.len() != state.tokens().len() {
        return Err("tokens field read disagrees with tokens() accessor".into());
    }
    if state.lex_checkpoints.len() != state.lex_checkpoints().len() {
        return Err("lex_checkpoints field read disagrees with lex_checkpoints() accessor".into());
    }

    Ok(())
}
