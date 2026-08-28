use crate::*;

// Tests for PerlLanguage descriptor


#[test]
fn test_language_returns_descriptor_with_nonzero_kind_count() {
    let lang = language();
    assert!(lang.node_kind_count() > 0, "language should report at least one node kind");
}

#[test]
fn test_language_constant_has_nonzero_kind_count() {
    assert!(LANGUAGE.node_kind_count() > 0, "LANGUAGE should have at least one node kind");
}

#[test]
fn test_language_reports_program_as_named_kind() {
    let lang = language();
    assert!(lang.node_kind_is_named("Program"), "'Program' should be a named kind");
}

#[test]
fn test_language_rejects_unknown_kind() {
    let lang = language();
    assert!(
        !lang.node_kind_is_named("__nonexistent_kind__"),
        "unknown kind should not be named"
    );
}

#[test]
fn test_language_kind_names_contains_program() {
    let lang = language();
    let names = lang.node_kind_names();
    assert!(names.contains(&"Program"), "kind names should include 'Program'");
}

#[test]
fn test_language_default_returns_same_as_language() {
    // PartialEq compares the backing slice elements, not just the pointer.
    // Both language() and PerlLanguage::default() return LANGUAGE so this
    // also verifies the Default impl wires up the correct constant.
    assert_eq!(language(), PerlLanguage::default());
}

#[test]
fn test_language_kind_names_declaration_order_and_no_duplicates() {
    // ALL_KIND_NAMES is now in declaration order (not alphabetical) via strum::VariantNames
    // (changed in PR #1491). Verify there are no duplicates and 'Program' is first.
    let lang = language();
    let names = lang.node_kind_names();
    assert!(!names.is_empty(), "node_kind_names must not be empty");
    assert_eq!(
        names.first(),
        Some(&"Program"),
        "First kind name should be 'Program' (declaration order)"
    );
    let unique: std::collections::BTreeSet<&str> = names.iter().copied().collect();
    assert_eq!(
        names.len(),
        unique.len(),
        "node_kind_names must not contain duplicates: {} entries, {} unique",
        names.len(),
        unique.len()
    );
}

#[test]
fn test_language_is_named_with_empty_string_returns_false() {
    // Empty string is not a valid kind name and must not be found.
    assert!(!language().node_kind_is_named(""), "empty kind name must return false");
}
