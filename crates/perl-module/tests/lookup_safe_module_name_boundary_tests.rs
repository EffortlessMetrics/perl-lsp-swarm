//! Boundary discriminators for `is_lookup_safe_module_name`.
//!
//! Each case pins one predicate so a single-line mutation of the guard fails
//! exactly that assertion (RIPR new-gap exposure for #5009).

use perl_module::is_lookup_safe_module_name;

#[test]
fn accepts_canonical_and_underscore_identifiers() {
    assert_eq!(is_lookup_safe_module_name("Foo"), true);
    assert_eq!(is_lookup_safe_module_name("Foo::Bar"), true);
    assert_eq!(is_lookup_safe_module_name("_Private::Util"), true);
    assert_eq!(is_lookup_safe_module_name("Foo::Bar_2"), true);
}

#[test]
fn rejects_empty_and_whitespace() {
    assert_eq!(is_lookup_safe_module_name(""), false);
    assert_eq!(is_lookup_safe_module_name("   "), false);
    assert_eq!(is_lookup_safe_module_name("Foo Bar"), false);
    assert_eq!(is_lookup_safe_module_name(" Foo"), false);
    assert_eq!(is_lookup_safe_module_name("Foo "), false);
}

#[test]
fn rejects_path_shaped_and_sigil_input() {
    assert_eq!(is_lookup_safe_module_name("Foo/Bar"), false);
    assert_eq!(is_lookup_safe_module_name("Foo\\Bar"), false);
    assert_eq!(is_lookup_safe_module_name("$Foo"), false);
    assert_eq!(is_lookup_safe_module_name("@Foo"), false);
    assert_eq!(is_lookup_safe_module_name("%Foo"), false);
}

#[test]
fn rejects_traversal_dotdot_segment() {
    // Discriminates `part != ".."` in the segment predicate.
    assert_eq!(is_lookup_safe_module_name(".."), false);
    assert_eq!(is_lookup_safe_module_name("Foo::..::Bar"), false);
    assert_eq!(is_lookup_safe_module_name("../../etc/passwd"), false);
}

#[test]
fn rejects_empty_segment_and_digit_start() {
    // Discriminates non-empty segment + alphabetic/_ start rules.
    assert_eq!(is_lookup_safe_module_name("Foo::"), false);
    assert_eq!(is_lookup_safe_module_name("::Foo"), false);
    assert_eq!(is_lookup_safe_module_name("Foo::1Bar"), false);
    assert_eq!(is_lookup_safe_module_name("1Foo"), false);
}

#[test]
fn rejects_non_alnum_characters_in_segment() {
    // Discriminates `part.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')`.
    assert_eq!(is_lookup_safe_module_name("Foo::Bar-Baz"), false);
    assert_eq!(is_lookup_safe_module_name("Foo::Bar.Baz"), false);
}

#[test]
fn accepts_legacy_quote_separator_when_normalized() {
    assert_eq!(is_lookup_safe_module_name("Foo'Bar"), true);
}
