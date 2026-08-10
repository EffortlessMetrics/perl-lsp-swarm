//! Pinning tests for issue #4949: byte_to_utf16_col deduplication.
//!
//! Before the fix, document_links/mod.rs contained a local copy of
//! `byte_to_utf16_col` that used `.get(..byte_offset).unwrap_or(byte_offset as u32)`.
//! The `unwrap_or` fallback returns the raw byte offset when the slice boundary
//! is not on a char boundary, which is wrong for any line containing multi-byte
//! characters.
//!
//! After the fix both copies delegate to `perl_position_tracking::offset_to_utf16_line_col`,
//! which correctly handles surrogate pairs and clamps to char boundaries.

use perl_lsp_rs_core::providers::document_links::compute_links;
use perl_tdd_support::must_some;
use serde_json::Value;

/// Helper: extract the character range from a link Value.
fn link_range(link: &Value) -> (u32, u32) {
    let start = must_some(link["range"]["start"]["character"].as_u64()) as u32;
    let end = must_some(link["range"]["end"]["character"].as_u64()) as u32;
    (start, end)
}

/// Helper: find the first link whose data.module or tooltip mentions a module.
fn find_module_link<'a>(links: &'a [Value], module: &str) -> Option<&'a Value> {
    links.iter().find(|l| {
        l["data"]["module"].as_str().is_some_and(|m| m == module)
            || l["tooltip"].as_str().is_some_and(|t| t.contains(module))
    })
}

// ─────────────────────────────────────────────────────────────────────────────
// Group 1 — POD L<> links: emoji on same line shifts UTF-16 columns
// ─────────────────────────────────────────────────────────────────────────────

/// Basic POD module link without emoji — baseline for the emoji tests below.
#[test]
fn pod_link_ascii_only_baseline() {
    // "ABCD L<Foo::Bar> text"
    //  0         1
    //  0123456789012345678...
    // col_start = 7 (byte of 'F', after "ABCD L<")
    // col_end   = 15 (byte of '>', after "ABCD L<Foo::Bar")
    let src = "=pod\n\nABCD L<Foo::Bar> text\n\n=cut\n";
    let links = compute_links("file:///t.pl", src, &[]);
    let link = must_some(find_module_link(&links, "Foo::Bar"));
    assert_eq!(link["range"]["start"]["line"].as_u64(), Some(2), "link on line 2");
    let (start, end) = link_range(link);
    assert_eq!(start, 7, "col_start: 'ABCD L<' = 7 ASCII chars = 7 UTF-16 units");
    assert_eq!(end, 15, "col_end: offset of '>' = 7 + len('Foo::Bar') = 15 UTF-16 units");
}

/// When a non-BMP emoji precedes the L<> link, the emoji contributes 2 UTF-16
/// code units, so the reported columns shift by 1 compared to the ASCII-only case.
///
/// Line: "😀 L<Foo::Bar> text"
///   '😀' = 4 UTF-8 bytes = 2 UTF-16 units
///   ' '(1) + 'L'(1) + '<'(1) → col_start = 5
///   "Foo::Bar"(8) before '>'  → col_end   = 13
#[test]
fn pod_link_emoji_prefix_utf16_column_is_correct() {
    let src = "=pod\n\n\u{1F600} L<Foo::Bar> text\n\n=cut\n";
    let links = compute_links("file:///t.pl", src, &[]);
    let link = must_some(find_module_link(&links, "Foo::Bar"));
    assert_eq!(link["range"]["start"]["line"].as_u64(), Some(2), "link on line 2");
    let (start, end) = link_range(link);
    // emoji(2) + space(1) + 'L'(1) + '<'(1) = 5 units before 'F'
    assert_eq!(start, 5, "col_start must account for the emoji's 2 UTF-16 code units");
    // emoji(2) + space(1) + 'L'(1) + '<'(1) + 'Foo::Bar'(8) = 13 units before '>'
    assert_eq!(end, 13, "col_end must account for the emoji's 2 UTF-16 code units");
}

/// Two non-BMP emoji before the link: each contributes 2 UTF-16 units.
///
/// Line: "😀😀 L<Foo::Bar> text"
///   2 emoji = 8 UTF-8 bytes = 4 UTF-16 units
///   ' '(1) + 'L'(1) + '<'(1) → col_start = 7
///   "Foo::Bar"(8) before '>'  → col_end   = 15
#[test]
fn pod_link_double_emoji_prefix_utf16_column_is_correct() {
    let src = "=pod\n\n\u{1F600}\u{1F601} L<Foo::Bar> text\n\n=cut\n";
    let links = compute_links("file:///t.pl", src, &[]);
    let link = must_some(find_module_link(&links, "Foo::Bar"));
    let (start, end) = link_range(link);
    assert_eq!(start, 7, "two emoji = 4 UTF-16 units + ' L<' = 7");
    assert_eq!(end, 15, "two emoji = 4 UTF-16 units + ' L<Foo::Bar>' = 15");
}

/// BMP multi-byte characters (2–3 UTF-8 bytes, 1 UTF-16 unit each) must not
/// inflate the column count — they are single UTF-16 code units.
///
/// 'é' (U+00E9) = 2 UTF-8 bytes = 1 UTF-16 unit
/// '中' (U+4E2D) = 3 UTF-8 bytes = 1 UTF-16 unit
/// Line: "é中 L<Foo::Bar> text"
///   é(1) + 中(1) + ' '(1) + 'L'(1) + '<'(1) = 5 UTF-16 units before 'F'
#[test]
fn pod_link_bmp_multibyte_prefix_counts_as_one_utf16_unit_each() {
    let src = "=pod\n\n\u{00E9}\u{4E2D} L<Foo::Bar> text\n\n=cut\n";
    let links = compute_links("file:///t.pl", src, &[]);
    let link = must_some(find_module_link(&links, "Foo::Bar"));
    let (start, end) = link_range(link);
    assert_eq!(start, 5, "BMP chars count as 1 UTF-16 unit each, so é+中+ L< = 5");
    assert_eq!(end, 13, "é+中+ L<Foo::Bar = 13 UTF-16 units before '>'");
}

// ─────────────────────────────────────────────────────────────────────────────
// Group 2 — No link is emitted for the same package or empty input
// ─────────────────────────────────────────────────────────────────────────────

/// A POD link targeting the current package must not produce a document link.
#[test]
fn pod_link_to_current_package_is_suppressed() {
    let src = "package Foo::Bar;\n\n=pod\n\nSee L<Foo::Bar>.\n\n=cut\n";
    let links = compute_links("file:///t.pl", src, &[]);
    assert!(
        find_module_link(&links, "Foo::Bar").is_none(),
        "self-referential POD link must not produce a document link"
    );
}

/// Empty input produces no links.
#[test]
fn empty_source_produces_no_links() {
    let links = compute_links("file:///t.pl", "", &[]);
    assert!(links.is_empty());
}
