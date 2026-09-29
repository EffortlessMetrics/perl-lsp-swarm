#![deny(clippy::map_err_ignore)]
// Cohort C1 activation (#12598): all production rows exact-excepted; new findings move the crate back to non-C1.
//! RIPR seam proofs for `classify_framework_module` (#8119).
//!
//! `classify_framework_module` is the single authority mapping a `use`d module
//! spelling to a [`FrameworkKind`]. It was extracted from `SymbolExtractor` so
//! that package-graph activation gating and symbol extraction agree on what
//! counts as activating an object framework, but nothing pinned the mapping
//! itself: every caller reached it through a parse, so a wrong arm surfaced
//! only as a changed edge somewhere downstream.
//!
//! Each arm is asserted with both operands spelled out literally — the module
//! string and the `FrameworkKind` variant — so a single-arm mutation fails the
//! assertion that names that arm rather than a distant package-graph control.

use perl_semantic_analyzer::analysis::symbol::{FrameworkKind, classify_framework_module};

/// `Moo` and `Mouse` share the `FrameworkKind::Moo` arm.
#[test]
fn seam_moo_and_mouse_classify_as_moo() {
    assert_eq!(classify_framework_module("Moo"), Some(FrameworkKind::Moo));
    assert_eq!(classify_framework_module("Mouse"), Some(FrameworkKind::Moo));
}

/// `Moo::Role` and `Mouse::Role` share the `FrameworkKind::MooRole` arm.
#[test]
fn seam_moo_role_and_mouse_role_classify_as_moo_role() {
    assert_eq!(classify_framework_module("Moo::Role"), Some(FrameworkKind::MooRole));
    assert_eq!(classify_framework_module("Mouse::Role"), Some(FrameworkKind::MooRole));
}

/// `Moose` is a class activation, distinct from `Moose::Role`.
#[test]
fn seam_moose_classifies_as_moose_and_not_moose_role() {
    assert_eq!(classify_framework_module("Moose"), Some(FrameworkKind::Moose));
    assert_ne!(classify_framework_module("Moose"), Some(FrameworkKind::MooseRole));
}

/// `Moose::Role` grants `with` but not `extends`, so it must not be `Moose`.
#[test]
fn seam_moose_role_classifies_as_moose_role_and_not_moose() {
    assert_eq!(classify_framework_module("Moose::Role"), Some(FrameworkKind::MooseRole));
    assert_ne!(classify_framework_module("Moose::Role"), Some(FrameworkKind::Moose));
}

/// `Role::Tiny` marks the package as a role.
#[test]
fn seam_role_tiny_classifies_as_role_tiny() {
    assert_eq!(classify_framework_module("Role::Tiny"), Some(FrameworkKind::RoleTiny));
    assert_ne!(classify_framework_module("Role::Tiny"), Some(FrameworkKind::RoleTinyWith));
}

/// `Role::Tiny::With` marks the package as a role consumer.
///
/// It differs from `Role::Tiny` only by suffix, so a prefix match would
/// collapse the two.
#[test]
fn seam_role_tiny_with_classifies_as_role_tiny_with() {
    assert_eq!(classify_framework_module("Role::Tiny::With"), Some(FrameworkKind::RoleTinyWith));
    assert_ne!(classify_framework_module("Role::Tiny::With"), Some(FrameworkKind::RoleTiny));
}

/// `Class::Tiny` is deliberately not a DSL activation.
///
/// It is tracked separately because it does not import the Moo/Moose DSL
/// keywords, so classifying it would license `extends`/`with` emission for a
/// package that never imported them.
#[test]
fn seam_class_tiny_is_not_a_dsl_activation() {
    assert_eq!(classify_framework_module("Class::Tiny"), None);
    assert_eq!(classify_framework_module("Class::Tiny::RW"), None);
    assert_eq!(classify_framework_module("Class::Accessor"), None);
    assert_ne!(classify_framework_module("Class::Tiny"), Some(FrameworkKind::ClassTiny));
}

/// Unrelated modules fall through to `None`.
#[test]
fn seam_unrelated_modules_classify_as_none() {
    assert_eq!(classify_framework_module("strict"), None);
    assert_eq!(classify_framework_module("warnings"), None);
    assert_eq!(classify_framework_module("parent"), None);
    assert_eq!(classify_framework_module("base"), None);
    assert_eq!(classify_framework_module("List::Util"), None);
    assert_eq!(classify_framework_module(""), None);
}

/// The mapping is an exact match, not a prefix or substring match.
///
/// `Moose::Util` contains `Moose`; `MooseX::Types` starts with `Moose`. Either
/// matching loosely would activate the DSL for a package that only loaded a
/// helper module.
#[test]
fn seam_near_miss_spellings_classify_as_none() {
    assert_eq!(classify_framework_module("Moose::Util"), None);
    assert_eq!(classify_framework_module("MooseX::Types"), None);
    assert_eq!(classify_framework_module("Moose::Role::Extra"), None);
    assert_eq!(classify_framework_module("Moo::Roles"), None);
    assert_eq!(classify_framework_module("Role::Tiny::Withal"), None);
    assert_eq!(classify_framework_module("My::Moose"), None);
}

/// Classification is case-sensitive.
#[test]
fn seam_classification_is_case_sensitive() {
    assert_eq!(classify_framework_module("moo"), None);
    assert_eq!(classify_framework_module("MOOSE"), None);
    assert_eq!(classify_framework_module("moose::role"), None);
    assert_eq!(classify_framework_module("role::tiny"), None);
}
