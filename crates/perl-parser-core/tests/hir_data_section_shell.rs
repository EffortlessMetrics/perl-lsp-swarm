//! Discriminating proof for the `__DATA__`/`__END__` first-slice HIR shell.
//!
//! Covers marker identity, exact marker/payload source ranges, the "no
//! trailing payload" edge case, the parser-stop boundary for the opaque
//! payload region, and byte-exact preservation of CRLF / non-ASCII payload
//! content.
//!
//! Refs: issue #14274 (`DataSectionDecl` HIR shell).

use perl_parser_core::hir::{
    DataSectionDecl, DataSectionMarker, HirFile, HirItem, HirKind, lower_ast,
};
use perl_parser_core::{Parser, SourceLocation};

fn lower(source: &str) -> HirFile {
    let mut parser = Parser::new(source);
    let output = parser.parse_with_recovery();
    lower_ast(&output.ast)
}

/// Find the single `DataSectionDecl` HIR item, failing loudly if there are
/// zero or more than one — a wrong implementation that emits duplicates or
/// silently drops the item must not pass unnoticed.
fn data_section_item(file: &HirFile) -> &HirItem {
    let mut found: Option<&HirItem> = None;
    for item in &file.items {
        if matches!(&item.kind, HirKind::DataSectionDecl(_)) {
            assert!(found.is_none(), "expected exactly one DataSectionDecl item, found a second");
            found = Some(item);
        }
    }
    found.expect("expected a DataSectionDecl HIR item")
}

fn data_section_decl(file: &HirFile) -> &DataSectionDecl {
    match &data_section_item(file).kind {
        HirKind::DataSectionDecl(decl) => decl,
        other => panic!("expected DataSectionDecl, got {other:?}"),
    }
}

#[test]
fn data_marker_lowers_to_data_variant() {
    let file = lower("1;\n__DATA__\nfoo\n");
    let decl = data_section_decl(&file);
    assert_eq!(decl.marker, DataSectionMarker::Data);
}

#[test]
fn end_marker_lowers_to_end_variant_and_is_not_interchangeable_with_data() {
    let data_file = lower("1;\n__DATA__\nfoo\n");
    let end_file = lower("1;\n__END__\nfoo\n");
    let data_decl = data_section_decl(&data_file);
    let end_decl = data_section_decl(&end_file);
    assert_eq!(data_decl.marker, DataSectionMarker::Data);
    assert_eq!(end_decl.marker, DataSectionMarker::End);
    assert_ne!(
        data_decl.marker, end_decl.marker,
        "__DATA__ and __END__ must lower to distinct DataSectionMarker values"
    );
}

#[test]
fn marker_range_is_exactly_the_marker_bytes_and_smaller_than_the_whole_node() {
    let source = "1;\n__DATA__\nfoo\n";
    let file = lower(source);
    let item = data_section_item(&file);
    let decl = match &item.kind {
        HirKind::DataSectionDecl(decl) => decl,
        other => panic!("expected DataSectionDecl, got {other:?}"),
    };

    let expected_start = source.find("__DATA__").expect("marker text present in source");
    let expected_end = expected_start + "__DATA__".len();
    assert_eq!(
        decl.marker_range,
        SourceLocation { start: expected_start, end: expected_end },
        "marker_range must be exactly the 8-byte marker token span, not the whole node span"
    );
    assert_eq!(&source[decl.marker_range.start..decl.marker_range.end], "__DATA__");

    let marker_len = decl.marker_range.end - decl.marker_range.start;
    let whole_node_len = item.range.end - item.range.start;
    assert!(
        marker_len < whole_node_len,
        "marker_range ({marker_len} bytes) must be strictly smaller than the whole node range \
         ({whole_node_len} bytes) once a payload follows the marker"
    );
}

/// The lexer's `DataMarker` token consumes the rest of the marker line,
/// including trailing spaces and the newline, before switching modes.  The
/// marker range must still cover only the marker word itself, so that trailing
/// layout never leaks into the range the HIR shell publishes.
#[test]
fn marker_range_excludes_trailing_whitespace_and_the_line_terminator() {
    for source in ["1;\n__DATA__   \npayload\n", "1;\n__DATA__\t\npayload\n", "1;\n__END__  \n"] {
        let file = lower(source);
        let decl = data_section_decl(&file);
        let expected_marker = if source.contains("__DATA__") { "__DATA__" } else { "__END__" };
        let expected_start = source.find(expected_marker).expect("marker text present in source");
        assert_eq!(
            decl.marker_range,
            SourceLocation { start: expected_start, end: expected_start + expected_marker.len() },
            "marker_range must cover only {expected_marker:?} in {source:?}, \
             not the trailing whitespace or newline the marker token consumes"
        );
        assert_eq!(
            &source[decl.marker_range.start..decl.marker_range.end],
            expected_marker,
            "marker_range must slice back to exactly the marker word"
        );
    }
}

#[test]
fn payload_range_covers_exactly_the_payload_and_excludes_the_marker() {
    let source = "1;\n__DATA__\nfoo\n";
    let file = lower(source);
    let decl = data_section_decl(&file);

    let payload_start = source.find("foo\n").expect("payload text present in source");
    let payload_end = payload_start + "foo\n".len();
    let payload_range = decl.payload_range.expect("payload_range must be present");
    assert_eq!(payload_range, SourceLocation { start: payload_start, end: payload_end });
    assert_eq!(&source[payload_range.start..payload_range.end], "foo\n");
    assert!(
        decl.marker_range.end <= payload_range.start,
        "payload_range must not overlap marker_range"
    );
}

#[test]
fn marker_without_trailing_payload_yields_none_not_an_empty_range() {
    let source = "1;\n__DATA__";
    let file = lower(source);
    let decl = data_section_decl(&file);
    assert_eq!(decl.marker, DataSectionMarker::Data);
    assert_eq!(
        decl.payload_range, None,
        "a file that ends at the marker must not fabricate an empty Some(range) payload"
    );
}

#[test]
fn perl_looking_payload_text_is_never_lowered_as_perl() {
    // The payload looks like real Perl (a sub declaration and a call), but it
    // must be treated as an opaque source region: the lexer consumes it as a
    // single DataBody token, so none of it is ever parsed or lowered.
    let source = "1;\n__DATA__\nsub foo { 1 }\nmy $x = bar();\n";
    let file = lower(source);

    assert!(
        !file.items.iter().any(|item| matches!(&item.kind, HirKind::SubDecl(_))),
        "payload text must never be lowered as a SubDecl"
    );
    assert!(
        !file.items.iter().any(|item| matches!(&item.kind, HirKind::CallExpr(_))),
        "payload text must never be lowered as a CallExpr"
    );
    assert!(
        !file.items.iter().any(|item| matches!(&item.kind, HirKind::VariableDecl(_))),
        "payload text must never be lowered as a VariableDecl"
    );

    // The payload is still captured whole, byte-for-byte, as the opaque
    // DataSectionDecl payload range.
    let decl = data_section_decl(&file);
    let payload_start = source.find("sub foo").expect("payload text present in source");
    let payload_range = decl.payload_range.expect("payload_range must be present");
    assert_eq!(payload_range.start, payload_start);
    assert_eq!(payload_range.end, source.len());
}

#[test]
fn crlf_and_non_ascii_payload_bytes_are_preserved_and_ranges_are_byte_exact() {
    let prefix = "1;\r\n__DATA__\r\n";
    let payload = "caf\u{e9} line\r\nsnowman \u{2603} end\r\n";
    let source = format!("{prefix}{payload}");
    let file = lower(&source);
    let decl = data_section_decl(&file);

    assert_eq!(decl.marker, DataSectionMarker::Data);

    let payload_range = decl.payload_range.expect("payload_range must be present");
    assert_eq!(payload_range, SourceLocation { start: prefix.len(), end: source.len() });
    assert_eq!(
        &source[payload_range.start..payload_range.end],
        payload,
        "payload bytes (CRLF line endings and multi-byte UTF-8 characters) must be preserved \
         exactly, byte for byte"
    );
}
