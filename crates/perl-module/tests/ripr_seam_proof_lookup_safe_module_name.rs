//! RIPR seam proof for `is_lookup_safe_module_name` segment predicates (`path/mod.rs`, #5009).
//!
//! Each test pins ONE decision boundary in the `normalized.split("::").all(|part| { ... })`
//! closure so a single-line mutation of the guarding sub-condition causes exactly that
//! test to fail via a direct boolean assertion.

use perl_module::is_lookup_safe_module_name;

#[test]
fn seam_dotdot_segment_equality_boundary() {
    assert!(is_lookup_safe_module_name("Foo::Bar"));
    assert!(!is_lookup_safe_module_name(".."));
    assert!(!is_lookup_safe_module_name("Foo::..::Bar"));
    assert!(!is_lookup_safe_module_name("../../etc/passwd"));
}

#[test]
fn seam_segment_start_equality_boundary() {
    assert!(is_lookup_safe_module_name("Foo"));
    assert!(is_lookup_safe_module_name("_Private::Util"));
    assert!(!is_lookup_safe_module_name("1Foo"));
    assert!(!is_lookup_safe_module_name("Foo::1Bar"));
}

#[test]
fn seam_segment_charset_equality_boundary() {
    assert!(is_lookup_safe_module_name("Foo::Bar_2"));
    assert!(!is_lookup_safe_module_name("Foo::Bar-Baz"));
    assert!(!is_lookup_safe_module_name("Foo::Bar.Baz"));
}

#[test]
fn seam_rejects_empty_whitespace_and_path_shaped_input() {
    assert!(!is_lookup_safe_module_name(""));
    assert!(!is_lookup_safe_module_name("   "));
    assert!(!is_lookup_safe_module_name("Foo Bar"));
    assert!(!is_lookup_safe_module_name("Foo/Bar"));
    assert!(!is_lookup_safe_module_name("Foo\\Bar"));
    assert!(!is_lookup_safe_module_name("$Foo"));
    assert!(!is_lookup_safe_module_name("@Foo"));
    assert!(!is_lookup_safe_module_name("%Foo"));
}

#[test]
fn seam_rejects_empty_package_segments() {
    assert!(!is_lookup_safe_module_name("Foo::"));
    assert!(!is_lookup_safe_module_name("::Foo"));
}

#[test]
fn seam_accepts_legacy_quote_separator_when_normalized() {
    assert!(is_lookup_safe_module_name("Foo'Bar"));
}
