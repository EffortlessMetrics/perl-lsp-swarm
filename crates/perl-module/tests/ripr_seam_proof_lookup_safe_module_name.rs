//! RIPR seam proof for `is_lookup_safe_module_name` segment predicates (`path/mod.rs`, #5009).
//!
//! Each test pins ONE decision boundary in the `normalized.split("::").all(|part| { ... })`
//! closure so a single-line mutation of the guarding sub-condition causes exactly that
//! test to fail via `assert_eq!(is_lookup_safe_module_name(...), ...)`.

use perl_module::is_lookup_safe_module_name;

#[test]
fn seam_dotdot_segment_equality_boundary() {
    assert_eq!(is_lookup_safe_module_name("Foo::Bar"), true);
    assert_eq!(is_lookup_safe_module_name(".."), false);
    assert_eq!(is_lookup_safe_module_name("Foo::..::Bar"), false);
    assert_eq!(is_lookup_safe_module_name("../../etc/passwd"), false);
}

#[test]
fn seam_segment_start_equality_boundary() {
    assert_eq!(is_lookup_safe_module_name("Foo"), true);
    assert_eq!(is_lookup_safe_module_name("_Private::Util"), true);
    assert_eq!(is_lookup_safe_module_name("1Foo"), false);
    assert_eq!(is_lookup_safe_module_name("Foo::1Bar"), false);
}

#[test]
fn seam_segment_charset_equality_boundary() {
    assert_eq!(is_lookup_safe_module_name("Foo::Bar_2"), true);
    assert_eq!(is_lookup_safe_module_name("Foo::Bar-Baz"), false);
    assert_eq!(is_lookup_safe_module_name("Foo::Bar.Baz"), false);
}

#[test]
fn seam_rejects_empty_whitespace_and_path_shaped_input() {
    assert_eq!(is_lookup_safe_module_name(""), false);
    assert_eq!(is_lookup_safe_module_name("   "), false);
    assert_eq!(is_lookup_safe_module_name("Foo Bar"), false);
    assert_eq!(is_lookup_safe_module_name("Foo/Bar"), false);
    assert_eq!(is_lookup_safe_module_name("Foo\\Bar"), false);
    assert_eq!(is_lookup_safe_module_name("$Foo"), false);
    assert_eq!(is_lookup_safe_module_name("@Foo"), false);
    assert_eq!(is_lookup_safe_module_name("%Foo"), false);
}

#[test]
fn seam_rejects_empty_package_segments() {
    assert_eq!(is_lookup_safe_module_name("Foo::"), false);
    assert_eq!(is_lookup_safe_module_name("::Foo"), false);
}

#[test]
fn seam_accepts_legacy_quote_separator_when_normalized() {
    assert_eq!(is_lookup_safe_module_name("Foo'Bar"), true);
}
