use super::{
    ByteRange, LEGACY_SUMMARY_LIMITATIONS, LEGACY_SUMMARY_SCHEMA, LEGACY_SUMMARY_SUBJECT,
    OutputFormat, help_text, legacy_parse_summary, read_source_bytes, render_output,
};
use perl_parser::{Node, NodeKind, SourceLocation};

#[test]
fn legacy_summary_uses_canonical_kind_name_for_struct_variant()
-> Result<(), Box<dyn std::error::Error>> {
    let location = SourceLocation { start: 0, end: 2 };
    let child = Node::new(NodeKind::Number { value: "42".to_string() }, location);
    let root = Node::new(NodeKind::Program { statements: vec![child] }, location);

    let summary = legacy_parse_summary(&root);
    assert_eq!(summary.schema, LEGACY_SUMMARY_SCHEMA);
    assert_eq!(summary.subject, LEGACY_SUMMARY_SUBJECT);
    assert_eq!(summary.native_root_kind, "Program");
    assert_eq!(summary.root_byte_range, ByteRange { start: 0, end: 2 });
    assert_eq!(summary.limitations, LEGACY_SUMMARY_LIMITATIONS);

    let encoded = serde_json::to_string(&summary)?;
    let value: serde_json::Value = serde_json::from_str(&encoded)?;
    assert_eq!(value["native_root_kind"], "Program");
    assert_eq!(value["native_root_kind"].as_str(), Some("Program"));
    Ok(())
}

#[test]
fn legacy_summary_uses_canonical_kind_name_for_unit_variant()
-> Result<(), Box<dyn std::error::Error>> {
    let root = Node::new(NodeKind::Diamond, SourceLocation { start: 4, end: 6 });
    let summary = legacy_parse_summary(&root);

    assert_eq!(summary.native_root_kind, "Diamond");
    assert_eq!(summary.root_byte_range, ByteRange { start: 4, end: 6 });
    Ok(())
}

#[test]
fn legacy_summary_serializes_as_valid_compact_and_pretty_json()
-> Result<(), Box<dyn std::error::Error>> {
    let root = Node::new(
        NodeKind::Program { statements: Vec::new() },
        SourceLocation { start: 0, end: 0 },
    );

    let compact = render_output(&root, OutputFormat::LegacyJson, false)?;
    let pretty = render_output(&root, OutputFormat::LegacyJson, true)?;
    let compact_value: serde_json::Value = serde_json::from_str(&compact)?;
    let pretty_value: serde_json::Value = serde_json::from_str(&pretty)?;

    assert_eq!(compact_value, pretty_value);
    assert_eq!(compact_value["schema"], LEGACY_SUMMARY_SCHEMA);
    assert_eq!(compact_value["subject"], LEGACY_SUMMARY_SUBJECT);
    assert_eq!(compact_value["native_root_kind"], "Program");
    assert_eq!(compact_value["limitations"][0], "root_summary_only");
    Ok(())
}

#[test]
fn legacy_sexp_bytes_are_preserved() -> Result<(), Box<dyn std::error::Error>> {
    let location = SourceLocation { start: 0, end: 1 };
    let root = Node::new(
        NodeKind::Program {
            statements: vec![Node::new(NodeKind::Number { value: "7".to_string() }, location)],
        },
        location,
    );

    assert_eq!(render_output(&root, OutputFormat::LegacySexp, false)?, root.to_sexp());
    Ok(())
}

#[test]
fn help_identifies_legacy_and_unstable_surfaces() {
    let help = help_text();
    assert!(help.contains("Legacy native-AST S-expression"));
    assert!(help.contains("not canonical Tree-sitter output"));
    assert!(help.contains("not NativeParseArtifact"));
    assert!(help.contains("Unstable human-only Rust Debug output"));
}

#[test]
fn read_source_bytes_preserves_utf8() -> Result<(), Box<dyn std::error::Error>> {
    let decoded = read_source_bytes(b"use strict;\n".to_vec())?;
    assert_eq!(decoded, "use strict;\n");
    Ok(())
}

#[test]
fn read_source_bytes_decodes_latin1_losslessly() -> Result<(), Box<dyn std::error::Error>> {
    // "Sår" in ISO-8859-1 bytes
    let decoded = read_source_bytes(vec![0x53, 0xE5, 0x72, 0x0A])?;
    assert_eq!(decoded, "Sår\n");
    Ok(())
}

#[test]
fn read_source_bytes_decodes_windows_1252_punctuation() -> Result<(), Box<dyn std::error::Error>> {
    // “quote” in Windows-1252 bytes
    let decoded = read_source_bytes(vec![0x93, b'q', b'u', b'o', b't', b'e', 0x94, b'\n'])?;
    assert_eq!(decoded, "“quote”\n");
    Ok(())
}

#[test]
fn read_source_bytes_repairs_utf8_mojibake() -> Result<(), Box<dyn std::error::Error>> {
    // `cafÃ©` is mojibake for `café` after a UTF-8 -> Latin-1 decode/encode cycle.
    let decoded = read_source_bytes("cafÃ©\n".as_bytes().to_vec())?;
    assert_eq!(decoded, "café\n");
    Ok(())
}

#[test]
fn read_source_bytes_decodes_utf16_le_bom() -> Result<(), Box<dyn std::error::Error>> {
    let bytes = vec![
        0xFF, 0xFE, // UTF-16LE BOM
        b'u', 0x00, b's', 0x00, b'e', 0x00, b' ', 0x00, b'8', 0x00, b';', 0x00, b'\n', 0x00,
    ];
    let decoded = read_source_bytes(bytes)?;
    assert_eq!(decoded, "use 8;\n");
    Ok(())
}

#[test]
fn read_source_bytes_decodes_utf16_be_bom() -> Result<(), Box<dyn std::error::Error>> {
    // UTF-16BE BOM followed by "use 8;\n" in big-endian encoding.
    let bytes = vec![
        0xFE, 0xFF, // UTF-16BE BOM
        0x00, b'u', 0x00, b's', 0x00, b'e', 0x00, b' ', 0x00, b'8', 0x00, b';', 0x00, b'\n',
    ];
    let decoded = read_source_bytes(bytes)?;
    assert_eq!(decoded, "use 8;\n");
    Ok(())
}

#[test]
fn read_source_bytes_decodes_utf16_surrogate_pair() -> Result<(), Box<dyn std::error::Error>> {
    // UTF-16LE BOM + U+1F600 (grinning face), encoded as surrogate pair
    // high=0xD83D, low=0xDE00 → LE bytes: 3D D8 00 DE.
    let bytes = vec![
        0xFF, 0xFE, // UTF-16LE BOM
        0x3D, 0xD8, 0x00, 0xDE, // surrogate pair for U+1F600
    ];
    let decoded = read_source_bytes(bytes)?;
    assert_eq!(decoded, "\u{1F600}");
    Ok(())
}

#[test]
fn read_source_bytes_handles_unpaired_high_surrogate() -> Result<(), Box<dyn std::error::Error>> {
    // UTF-16LE BOM + lone high surrogate (0xD83D) followed by a valid BMP char 'A' (0x0041).
    // from_utf16_lossy replaces the unpaired surrogate with U+FFFD.
    let bytes = vec![
        0xFF, 0xFE, // UTF-16LE BOM
        0x3D, 0xD8, // unpaired high surrogate (no low surrogate follows)
        0x41, 0x00, // 'A'
    ];
    let decoded = read_source_bytes(bytes)?;
    assert_eq!(decoded, "\u{FFFD}A");
    Ok(())
}

#[test]
fn read_source_bytes_handles_unpaired_low_surrogate() -> Result<(), Box<dyn std::error::Error>> {
    // UTF-16LE BOM + lone low surrogate (0xDE00) without a preceding high surrogate.
    let bytes = vec![
        0xFF, 0xFE, // UTF-16LE BOM
        0x00, 0xDE, // unpaired low surrogate
    ];
    let decoded = read_source_bytes(bytes)?;
    assert_eq!(decoded, "\u{FFFD}");
    Ok(())
}

#[test]
fn read_source_bytes_handles_utf16_odd_byte_length() -> Result<(), Box<dyn std::error::Error>> {
    // UTF-16LE BOM + 'A' (0x41 0x00) + trailing lone byte 0x42.
    // The loop condition `index + 1 < bytes.len()` drops the trailing byte.
    let bytes = vec![
        0xFF, 0xFE, // UTF-16LE BOM
        0x41, 0x00, // 'A'
        0x42, // orphan trailing byte — must not panic
    ];
    let decoded = read_source_bytes(bytes)?;
    assert_eq!(decoded, "A");
    Ok(())
}

#[test]
fn read_source_bytes_handles_utf16_bom_only() -> Result<(), Box<dyn std::error::Error>> {
    // Just the BOM with no payload — empty string expected, no panic.
    let decoded = read_source_bytes(vec![0xFF, 0xFE])?;
    assert_eq!(decoded, "");
    Ok(())
}

#[test]
fn read_source_bytes_handles_empty_input() -> Result<(), Box<dyn std::error::Error>> {
    let decoded = read_source_bytes(Vec::new())?;
    assert_eq!(decoded, "");
    Ok(())
}

#[test]
fn read_source_bytes_handles_truncated_utf8_multibyte() -> Result<(), Box<dyn std::error::Error>> {
    // Valid UTF-8 "ab" followed by a truncated 2-byte sequence (0xC3 without continuation).
    // from_utf8 fails → Windows-1252 fallback kicks in. 0xC3 is undefined in the mapping
    // table so it falls through to char::from(byte) = U+00C3 ('Ã').
    let bytes = vec![b'a', b'b', 0xC3];
    let decoded = read_source_bytes(bytes)?;
    assert_eq!(decoded, "ab\u{00C3}");
    Ok(())
}

#[test]
fn read_source_bytes_handles_lone_utf8_continuation_byte() -> Result<(), Box<dyn std::error::Error>>
{
    // 0x80 is a UTF-8 continuation byte with no leader — invalid UTF-8.
    // Falls through to Windows-1252 which maps 0x80 → U+20AC ('€').
    let bytes = vec![b'x', 0x80, b'y'];
    let decoded = read_source_bytes(bytes)?;
    assert_eq!(decoded, "x\u{20AC}y");
    Ok(())
}

#[test]
fn read_source_bytes_preserves_null_bytes_in_utf8() -> Result<(), Box<dyn std::error::Error>> {
    // NUL (0x00) is valid UTF-8 and valid in Rust strings.
    let bytes = vec![b'a', 0x00, b'b'];
    let decoded = read_source_bytes(bytes)?;
    assert_eq!(decoded, "a\u{0000}b");
    Ok(())
}

#[test]
fn read_source_bytes_maps_undefined_windows_1252_bytes_as_latin1()
-> Result<(), Box<dyn std::error::Error>> {
    // Windows-1252 has five undefined slots: 0x81, 0x8D, 0x8F, 0x90, 0x9D.
    // The fallback's `_` arm maps them via `char::from(byte)` which is Latin-1 (U+00xx).
    // Combined with a truncated UTF-8 prefix byte to force the fallback path.
    let bytes = vec![0xC3, 0x81, 0x8D, 0x8F, 0x90, 0x9D];
    let decoded = read_source_bytes(bytes)?;
    assert_eq!(decoded, "\u{00C3}\u{0081}\u{008D}\u{008F}\u{0090}\u{009D}");
    Ok(())
}

#[test]
fn read_source_bytes_handles_utf16_with_embedded_null_code_unit()
-> Result<(), Box<dyn std::error::Error>> {
    // UTF-16LE BOM + 'A' + U+0000 (NUL, as a 16-bit code unit) + 'B'.
    let bytes = vec![
        0xFF, 0xFE, // UTF-16LE BOM
        0x41, 0x00, // 'A'
        0x00, 0x00, // NUL
        0x42, 0x00, // 'B'
    ];
    let decoded = read_source_bytes(bytes)?;
    assert_eq!(decoded, "A\u{0000}B");
    Ok(())
}

#[test]
fn read_source_bytes_rejects_partial_bom_as_not_utf16() -> Result<(), Box<dyn std::error::Error>> {
    // A single 0xFF byte is neither a full BOM nor valid UTF-8; Windows-1252 fallback
    // maps 0xFF through the `_` arm to U+00FF ('ÿ').
    let decoded = read_source_bytes(vec![0xFF])?;
    assert_eq!(decoded, "\u{00FF}");
    Ok(())
}

#[test]
fn read_source_bytes_keeps_valid_non_mojibake_text() -> Result<(), Box<dyn std::error::Error>> {
    let decoded = read_source_bytes("Ångström\n".as_bytes().to_vec())?;
    assert_eq!(decoded, "Ångström\n");
    Ok(())
}
