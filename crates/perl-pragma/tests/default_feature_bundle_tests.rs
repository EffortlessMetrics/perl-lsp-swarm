//! Spec-alignment tests for the Perl `:default` feature bundle.
//!
//! In Perl, a file outside any `use VERSION` / `use feature` declaration still
//! has the `:default` feature bundle enabled — `indirect`, `multidimensional`,
//! `bareword_filehandles`, `apostrophe_as_package_separator`, and `smartmatch`.
//! These default-on features are not free-standing extras: `use vX.Y` bundles
//! work by *disabling* them (for example `use v5.36` turns off `indirect` and
//! `multidimensional`; `use v5.38` additionally turns off
//! `bareword_filehandles`; `use v5.42` turns off
//! `apostrophe_as_package_separator` and `smartmatch`).
//!
//! These tests pin the baseline ([`PragmaState::default`]) and the way the
//! version bundles compose against it.
//!
//! Reference: <https://perldoc.perl.org/feature#FEATURE-BUNDLES>

use perl_ast::SourceLocation;
use perl_ast::ast::{Node, NodeKind};
use perl_pragma::{PragmaState, PragmaTracker};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn loc(start: usize, end: usize) -> SourceLocation {
    SourceLocation { start, end }
}

fn use_node(module: &str, args: &[&str], start: usize, end: usize) -> Node {
    Node {
        kind: NodeKind::Use {
            module: module.to_string(),
            args: args.iter().map(|s| s.to_string()).collect(),
            has_filter_risk: false,
        },
        location: loc(start, end),
    }
}

fn no_node(module: &str, args: &[&str], start: usize, end: usize) -> Node {
    Node {
        kind: NodeKind::No {
            module: module.to_string(),
            args: args.iter().map(|s| s.to_string()).collect(),
            has_filter_risk: false,
        },
        location: loc(start, end),
    }
}

fn function_call(name: &str, start: usize, end: usize) -> Node {
    Node {
        kind: NodeKind::FunctionCall { name: name.to_string(), args: vec![] },
        location: loc(start, end),
    }
}

fn program(stmts: Vec<Node>) -> Node {
    let end = stmts.last().map_or(0, |n| n.location.end);
    Node { kind: NodeKind::Program { statements: stmts }, location: loc(0, end) }
}

/// Effective state after the last statement in the program.
fn final_state(stmts: Vec<Node>) -> PragmaState {
    let ast = program(stmts);
    let map = PragmaTracker::build(&ast);
    PragmaTracker::final_state(&map)
}

const DEFAULT_BUNDLE: &[&str] = &[
    "indirect",
    "multidimensional",
    "bareword_filehandles",
    "apostrophe_as_package_separator",
    "smartmatch",
];

fn assert_has_all(state: &PragmaState, names: &[&str]) {
    for name in names {
        assert!(state.has_feature(name), "expected feature {name:?} to be enabled");
    }
}

fn assert_has_none(state: &PragmaState, names: &[&str]) {
    for name in names {
        assert!(!state.has_feature(name), "expected feature {name:?} to be disabled");
    }
}

// ===========================================================================
// Baseline (`:default`) bundle
// ===========================================================================

#[test]
fn default_state_enables_the_default_bundle() {
    let state = PragmaState::default();
    assert_has_all(&state, DEFAULT_BUNDLE);
    // No version-bundle features leak into the baseline.
    assert_has_none(&state, &["say", "state", "signatures", "isa", "module_true", "try"]);
}

#[test]
fn default_state_leaves_non_feature_flags_cleared() {
    let state = PragmaState::default();
    assert!(!state.strict_vars);
    assert!(!state.strict_subs);
    assert!(!state.strict_refs);
    assert!(!state.warnings);
    assert!(!state.utf8);
    assert!(state.encoding.is_none());
    assert!(!state.unicode_strings);
    assert!(!state.locale);
    assert!(state.builtin_imports.is_empty());
    assert!(state.disabled_warning_categories.is_empty());
}

#[test]
fn plain_file_with_no_pragmas_reports_default_bundle() {
    // A program with content but no `use`/`no` produces an empty transition
    // map; queries fall back to the baseline, which is the `:default` bundle.
    let ast = program(vec![function_call("print", 0, 12)]);
    let map = PragmaTracker::build(&ast);

    let state = PragmaTracker::state_for_offset(&map, 6);
    assert_has_all(&state, DEFAULT_BUNDLE);
    assert!(!state.has_feature("say"));
}

#[test]
fn all_strict_keeps_the_default_bundle() {
    let state = PragmaState::all_strict();
    assert!(state.strict_vars && state.strict_subs && state.strict_refs);
    assert!(!state.warnings, "all_strict must not enable warnings");
    assert_has_all(&state, DEFAULT_BUNDLE);
}

#[test]
fn use_strict_final_state_matches_all_strict() {
    // `use strict` only toggles the strict categories; the `:default` feature
    // bundle is untouched, so the effective state equals `all_strict()`.
    let state = final_state(vec![use_node("strict", &[], 0, 11)]);
    assert_eq!(state, PragmaState::all_strict());
    assert_has_all(&state, DEFAULT_BUNDLE);
}

// ===========================================================================
// `use vX.Y` bundles disable default-on features
// ===========================================================================

#[test]
fn use_v5_36_disables_indirect_and_multidimensional() {
    let state = final_state(vec![use_node("v5.36", &[], 0, 9)]);
    // Disabled by the 5.36 bundle:
    assert_has_none(&state, &["indirect", "multidimensional", "switch"]);
    // Still on in the 5.36 bundle:
    assert_has_all(
        &state,
        &["bareword_filehandles", "apostrophe_as_package_separator", "smartmatch"],
    );
    // Added by the 5.36 bundle:
    assert_has_all(&state, &["say", "signatures", "isa", "state"]);
    // v5.36 implies strict + warnings.
    assert!(state.strict_vars && state.strict_subs && state.strict_refs);
    assert!(state.warnings);
}

#[test]
fn use_v5_38_additionally_disables_bareword_filehandles() {
    let state = final_state(vec![use_node("v5.38", &[], 0, 9)]);
    assert_has_none(&state, &["indirect", "multidimensional", "bareword_filehandles"]);
    assert_has_all(&state, &["apostrophe_as_package_separator", "smartmatch", "module_true"]);
}

#[test]
fn use_v5_42_disables_apostrophe_separator_and_smartmatch() {
    let state = final_state(vec![use_node("v5.42", &[], 0, 9)]);
    assert_has_none(
        &state,
        &[
            "indirect",
            "multidimensional",
            "bareword_filehandles",
            "apostrophe_as_package_separator",
            "smartmatch",
        ],
    );
    assert_has_all(&state, &["say", "signatures", "try", "module_true"]);
}

// ===========================================================================
// `no feature` against the baseline
// ===========================================================================

#[test]
fn no_feature_disables_a_single_default_on_feature() {
    // `no feature 'multidimensional';` lexically turns off one baseline feature
    // while leaving the rest of the `:default` bundle intact.
    let state = final_state(vec![no_node("feature", &["multidimensional"], 0, 30)]);
    assert!(!state.has_feature("multidimensional"));
    assert_has_all(
        &state,
        &["indirect", "bareword_filehandles", "apostrophe_as_package_separator", "smartmatch"],
    );
}

#[test]
fn bare_no_feature_resets_to_default_bundle() {
    // `use feature 'say'; no feature;` should drop the explicit `say` and leave
    // the `:default` bundle restored.
    let state =
        final_state(vec![use_node("feature", &["say"], 0, 18), no_node("feature", &[], 18, 28)]);
    assert!(!state.has_feature("say"), "bare `no feature` resets the explicit 'say'");
    assert_has_all(&state, DEFAULT_BUNDLE);
}

#[test]
fn use_feature_say_adds_to_the_default_bundle() {
    // Explicitly enabling a feature augments — not replaces — the baseline.
    let state = final_state(vec![use_node("feature", &["say"], 0, 18)]);
    assert!(state.has_feature("say"));
    assert_has_all(&state, DEFAULT_BUNDLE);
}
