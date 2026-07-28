//! RIPR seam proof for `is_lookup_safe_module_name` segment predicates (`path/mod.rs`, #5009).
//!
//! Each test pins ONE decision boundary in the `normalized.split("::").all(|part| { ... })`
//! closure so a single-line mutation of the guarding sub-condition causes exactly that
//! test to fail via `assert_eq!(is_lookup_safe_module_name(...), ...)`.
//!
//! Seam — lookup-safe module name guard (`path/mod.rs:26-41`)
//!   Issue: crafted module strings (path traversal, slash-shaped names) must be rejected
//!   before `module_name_to_path` feeds filesystem existence probes.
//!   Fix: `is_lookup_safe_module_name` validates segment shape before any @INC join.
//!
//! Key predicates under test:
//!   (a) `part != ".."` at line 38 — rejects traversal segments.
//!   (b) `part.starts_with(|c| c.is_ascii_alphabetic() || c == '_')` at line 39.
//!   (c) `part.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')` at line 40.

use perl_module::is_lookup_safe_module_name;

// ── BOUNDARY A: `part != ".."` (line 38) ─────────────────────────────────────

/// Discriminates `part != ".."` — bare `..` must reject.
#[test]
fn seam_rejects_bare_dotdot_segment() {
    assert_eq!(is_lookup_safe_module_name(".."), false);
}

/// Discriminates `part != ".."` — embedded `..` segment must reject.
#[test]
fn seam_rejects_embedded_dotdot_segment() {
    assert_eq!(is_lookup_safe_module_name("Foo::..::Bar"), false);
    assert_eq!(is_lookup_safe_module_name("../../etc/passwd"), false);
}

// ── BOUNDARY B: segment start rule (line 39) ─────────────────────────────────

/// Discriminates alphabetic/`_` segment start — digit-leading segments reject.
#[test]
fn seam_rejects_digit_leading_segment() {
    assert_eq!(is_lookup_safe_module_name("1Foo"), false);
    assert_eq!(is_lookup_safe_module_name("Foo::1Bar"), false);
}

/// Control: canonical identifiers with `_` prefix/start remain accepted.
#[test]
fn seam_accepts_underscore_prefixed_segments() {
    assert_eq!(is_lookup_safe_module_name("_Private::Util"), true);
    assert_eq!(is_lookup_safe_module_name("Foo::Bar_2"), true);
}

// ── BOUNDARY C: segment charset rule (line 40) ───────────────────────────────

/// Discriminates `part.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')`.
#[test]
fn seam_rejects_non_alnum_segment_characters() {
    assert_eq!(is_lookup_safe_module_name("Foo::Bar-Baz"), false);
    assert_eq!(is_lookup_safe_module_name("Foo::Bar.Baz"), false);
}

// ── BOUNDARY D: outer trim / path-shaped rejection (lines 27-33) ─────────────

#[test]
fn seam_accepts_canonical_double_colon_names() {
    assert_eq!(is_lookup_safe_module_name("Foo"), true);
    assert_eq!(is_lookup_safe_module_name("Foo::Bar"), true);
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
