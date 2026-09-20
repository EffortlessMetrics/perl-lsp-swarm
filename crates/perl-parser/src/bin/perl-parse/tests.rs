use std::io::{self, Write};
use std::time::Duration;

use perl_parser::{Node, NodeKind, ParseError, SourceLocation};

use super::{
    ByteRange, CliRequest, LEGACY_SUMMARY_LIMITATIONS, LEGACY_SUMMARY_SCHEMA,
    LEGACY_SUMMARY_SUBJECT, OutputFormat, ProcessStatus, TotalStats, execute, help_text,
    legacy_parse_summary, parse_args, read_source_bytes, render_output, write_error, write_help,
    write_usage_error, write_version,
};

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

fn check(condition: bool, message: impl Into<String>) -> Result<(), Box<dyn std::error::Error>> {
    if condition { Ok(()) } else { Err(message.into().into()) }
}

fn check_equal<L, R>(actual: &L, expected: &R) -> Result<(), Box<dyn std::error::Error>>
where
    L: std::fmt::Debug + PartialEq<R> + ?Sized,
    R: std::fmt::Debug + ?Sized,
{
    if actual == expected {
        Ok(())
    } else {
        Err(format!("expected {expected:?}, got {actual:?}").into())
    }
}

fn utf8(bytes: &[u8]) -> Result<String, Box<dyn std::error::Error>> {
    Ok(String::from_utf8(bytes.to_vec())?)
}

struct ImmediateFail;

impl Write for ImmediateFail {
    fn write(&mut self, _buf: &[u8]) -> io::Result<usize> {
        Err(io::Error::new(io::ErrorKind::BrokenPipe, "injected immediate write failure"))
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

struct FailAfter {
    allowed: usize,
    written: usize,
}

impl FailAfter {
    fn new(allowed: usize) -> Self {
        Self { allowed, written: 0 }
    }
}

impl Write for FailAfter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if self.written >= self.allowed {
            return Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "injected write failure after partial success",
            ));
        }
        let remaining = self.allowed - self.written;
        let n = buf.len().min(remaining);
        self.written += n;
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

struct WriteZeroSink;

impl Write for WriteZeroSink {
    fn write(&mut self, _buf: &[u8]) -> io::Result<usize> {
        Ok(0)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

struct DiscardingWriter {
    inner: Vec<u8>,
}

impl Write for DiscardingWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.inner.write(buf)?;
        Err(io::Error::new(io::ErrorKind::BrokenPipe, "injected failure after accepting bytes"))
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn temp_perl(contents: &str) -> Result<tempfile::NamedTempFile, Box<dyn std::error::Error>> {
    let file = tempfile::NamedTempFile::new()?;
    std::fs::write(file.path(), contents)?;
    Ok(file)
}

#[test]
fn help_flag_wins_over_later_version_and_files() -> Result<(), Box<dyn std::error::Error>> {
    check(
        matches!(parse_args(["--help", "--version", "script.pl"]), Ok(CliRequest::Help)),
        stringify!(matches!(
            parse_args(["--help", "--version", "script.pl"]),
            Ok(CliRequest::Help)
        )),
    )?;
    check(
        matches!(parse_args(["-h", "-V"]), Ok(CliRequest::Help)),
        stringify!(matches!(parse_args(["-h", "-V"]), Ok(CliRequest::Help))),
    )?;
    Ok(())
}

#[test]
fn version_flag_wins_over_later_help() -> Result<(), Box<dyn std::error::Error>> {
    check(
        matches!(parse_args(["--version", "--help"]), Ok(CliRequest::Version)),
        stringify!(matches!(parse_args(["--version", "--help"]), Ok(CliRequest::Version))),
    )?;
    check(
        matches!(parse_args(["-V", "script.pl", "--help"]), Ok(CliRequest::Version)),
        stringify!(matches!(parse_args(["-V", "script.pl", "--help"]), Ok(CliRequest::Version))),
    )?;
    Ok(())
}

#[test]
fn help_after_a_file_argument_still_exits_as_help() -> Result<(), Box<dyn std::error::Error>> {
    check(
        matches!(parse_args(["script.pl", "--help"]), Ok(CliRequest::Help)),
        stringify!(matches!(parse_args(["script.pl", "--help"]), Ok(CliRequest::Help))),
    )?;
    Ok(())
}

#[test]
fn empty_argv_defaults_to_stdin_run() -> Result<(), Box<dyn std::error::Error>> {
    match parse_args(Vec::<&str>::new())? {
        CliRequest::Run(args) => {
            check_equal(&(args.inputs.len()), &1)?;
            check(
                matches!(args.inputs.first().ok_or("expected one input")?, super::Input::Stdin),
                stringify!(matches!(
                    args.inputs.first().ok_or("expected one input")?,
                    super::Input::Stdin
                )),
            )?;
        }
        other => return Err(format!("expected Run, got {other:?}").into()),
    }
    Ok(())
}

#[test]
fn dash_input_is_stdin() -> Result<(), Box<dyn std::error::Error>> {
    match parse_args(["-"])? {
        CliRequest::Run(args) => {
            check(
                matches!(args.inputs.first().ok_or("expected one input")?, super::Input::Stdin),
                stringify!(matches!(
                    args.inputs.first().ok_or("expected one input")?,
                    super::Input::Stdin
                )),
            )?;
        }
        other => return Err(format!("expected Run, got {other:?}").into()),
    }
    Ok(())
}

#[test]
fn write_help_matches_println_bytes() -> Result<(), Box<dyn std::error::Error>> {
    let mut stdout = Vec::new();
    write_help(&mut stdout)?;
    let expected = format!("{}\n", help_text());
    check_equal(&(utf8(&stdout)?), &expected)?;
    Ok(())
}

#[test]
fn write_version_matches_println_bytes() -> Result<(), Box<dyn std::error::Error>> {
    let mut stdout = Vec::new();
    write_version(&mut stdout)?;
    check_equal(&(utf8(&stdout)?), &(format!("perl-parse v{}\n", env!("CARGO_PKG_VERSION"))))?;
    Ok(())
}

#[test]
fn write_usage_error_matches_eprintln_bytes() -> Result<(), Box<dyn std::error::Error>> {
    let mut stderr = Vec::new();
    write_usage_error(&mut stderr, "Unknown option: --nope")?;
    check_equal(
        &(utf8(&stderr)?),
        &("Error: Unknown option: --nope\nTry 'perl-parse --help' for more information.\n"),
    )?;
    Ok(())
}

#[test]
fn execute_help_and_version_write_exact_bytes() -> Result<(), Box<dyn std::error::Error>> {
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let status = execute(["--help"], &mut stdout, &mut stderr)?;
    check_equal(&status, &ProcessStatus::Success)?;
    check_equal(&(utf8(&stdout)?), &(format!("{}\n", help_text())))?;
    check(stderr.is_empty(), stringify!(stderr.is_empty()))?;

    stdout.clear();
    let status = execute(["--version"], &mut stdout, &mut stderr)?;
    check_equal(&status, &ProcessStatus::Success)?;
    check_equal(&(utf8(&stdout)?), &(format!("perl-parse v{}\n", env!("CARGO_PKG_VERSION"))))?;
    check(stderr.is_empty(), stringify!(stderr.is_empty()))?;
    Ok(())
}

#[test]
fn execute_unknown_option_is_usage_failure_with_exact_bytes()
-> Result<(), Box<dyn std::error::Error>> {
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let status = execute(["--nope"], &mut stdout, &mut stderr)?;
    check_equal(&status, &ProcessStatus::Failure)?;
    check(stdout.is_empty(), stringify!(stdout.is_empty()))?;
    check_equal(
        &(utf8(&stderr)?),
        &("Error: Unknown option: --nope\nTry 'perl-parse --help' for more information.\n"),
    )?;
    Ok(())
}

#[test]
fn execute_missing_format_argument_is_usage_failure() -> Result<(), Box<dyn std::error::Error>> {
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let status = execute(["--format"], &mut stdout, &mut stderr)?;
    check_equal(&status, &ProcessStatus::Failure)?;
    check_equal(
        &(utf8(&stderr)?),
        &("Error: Missing format argument\nTry 'perl-parse --help' for more information.\n"),
    )?;
    Ok(())
}

#[test]
fn stats_omit_averages_when_no_files_were_parsed() -> Result<(), Box<dyn std::error::Error>> {
    let mut stats = TotalStats::new();
    stats.add_error("missing.pl");
    let mut out = Vec::new();
    stats.write(&mut out)?;
    let text = utf8(&out)?;
    check_equal(
        &text,
        &("\n=== Total Statistics ===\n\
         Files parsed: 0\n\
         Files failed: 1\n\
         Total size: 0 bytes (0.00 KB)\n\
         Total time: 0ns\n\
         Total nodes: 0\n"),
    )?;
    check(!text.contains("Average speed"), stringify!(!text.contains("Average speed")))?;
    check(
        !text.contains("Average nodes per file"),
        stringify!(!text.contains("Average nodes per file")),
    )?;
    check(
        !text.contains("=== File Details ==="),
        stringify!(!text.contains("=== File Details ===")),
    )?;
    Ok(())
}

#[test]
fn stats_preserve_integer_averages_and_file_details_for_two_files()
-> Result<(), Box<dyn std::error::Error>> {
    let mut stats = TotalStats::new();
    let time = Duration::from_secs(1);
    stats.add_file("a.pl", 1_000_000, time, 5);
    stats.add_file("b.pl", 1_000_000, time, 5);
    let mut out = Vec::new();
    stats.write(&mut out)?;
    check_equal(
        &(utf8(&out)?),
        &("\n=== Total Statistics ===\n\
         Files parsed: 2\n\
         Files failed: 0\n\
         Total size: 2000000 bytes (1953.12 KB)\n\
         Total time: 2s\n\
         Total nodes: 10\n\
         Average speed: 1.00 MB/s\n\
         Average nodes per file: 5\n\
         \n=== File Details ===\n\
         a.pl: 1000000 bytes, 1s, 5 nodes\n\
         b.pl: 1000000 bytes, 1s, 5 nodes\n"),
    )?;
    Ok(())
}

#[test]
fn stats_omit_file_details_for_a_single_file() -> Result<(), Box<dyn std::error::Error>> {
    let mut stats = TotalStats::new();
    stats.add_file("only.pl", 2048, Duration::from_millis(1500), 3);
    let mut out = Vec::new();
    stats.write(&mut out)?;
    let text = utf8(&out)?;
    check(
        text.contains("Average nodes per file: 3\n"),
        stringify!(text.contains("Average nodes per file: 3\n")),
    )?;
    check(
        !text.contains("=== File Details ==="),
        stringify!(!text.contains("=== File Details ===")),
    )?;
    check(!text.contains("only.pl:"), stringify!(!text.contains("only.pl:")))?;
    Ok(())
}

#[test]
fn stats_include_file_details_at_twenty_files_and_omit_at_twenty_one()
-> Result<(), Box<dyn std::error::Error>> {
    let mut twenty = TotalStats::new();
    for index in 0..20 {
        twenty.add_file(&format!("f{index}.pl"), 1, Duration::from_nanos(1), 1);
    }
    let mut out = Vec::new();
    twenty.write(&mut out)?;
    check(
        utf8(&out)?.contains("=== File Details ==="),
        stringify!(utf8(&out)?.contains("=== File Details ===")),
    )?;

    let mut twenty_one = TotalStats::new();
    for index in 0..21 {
        twenty_one.add_file(&format!("f{index}.pl"), 1, Duration::from_nanos(1), 1);
    }
    out.clear();
    twenty_one.write(&mut out)?;
    check(
        !utf8(&out)?.contains("=== File Details ==="),
        stringify!(!utf8(&out)?.contains("=== File Details ===")),
    )?;
    Ok(())
}

#[test]
fn stats_file_details_mark_failed_inputs() -> Result<(), Box<dyn std::error::Error>> {
    let mut stats = TotalStats::new();
    stats.add_file("ok.pl", 4, Duration::from_nanos(1), 1);
    stats.add_error("bad.pl");
    let mut out = Vec::new();
    stats.write(&mut out)?;
    let text = utf8(&out)?;
    check(
        text.contains("ok.pl: 4 bytes, 1ns, 1 nodes\n"),
        stringify!(text.contains("ok.pl: 4 bytes, 1ns, 1 nodes\n")),
    )?;
    check(text.contains("bad.pl: FAILED\n"), stringify!(text.contains("bad.pl: FAILED\n")))?;
    Ok(())
}

#[test]
fn write_error_unexpected_eof_and_invalid_string_are_exact()
-> Result<(), Box<dyn std::error::Error>> {
    let mut stderr = Vec::new();
    write_error(&ParseError::UnexpectedEof, "", &mut stderr)?;
    check_equal(&(utf8(&stderr)?), &("Parse error: Unexpected end of input\n"))?;

    stderr.clear();
    write_error(&ParseError::InvalidString, "unused", &mut stderr)?;
    check_equal(&(utf8(&stderr)?), &("Parse error: Invalid string literal\n"))?;
    Ok(())
}

#[test]
fn write_error_unexpected_token_includes_context_bytes() -> Result<(), Box<dyn std::error::Error>> {
    let source = "ab\ncd";
    let mut stderr = Vec::new();
    write_error(
        &ParseError::UnexpectedToken {
            expected: "expression".to_string(),
            found: "d".to_string(),
            location: 4,
        },
        source,
        &mut stderr,
    )?;
    check_equal(
        &(utf8(&stderr)?),
        &("Parse error: Unexpected token at line 2, column 2\n  Expected: expression\n  Found: d\n\n  1 | ab\n  2 | cd\n    |  ^\n"),
    )?;
    Ok(())
}

#[test]
fn execute_legacy_sexp_stdout_matches_render_output() -> Result<(), Box<dyn std::error::Error>> {
    let file = temp_perl("use strict;\nmy $value = 1;\n")?;
    let path = file.path().to_str().ok_or("temp path was not UTF-8")?;
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let status = execute([path], &mut stdout, &mut stderr)?;
    check_equal(&status, &ProcessStatus::Success)?;
    check(stderr.is_empty(), stringify!(stderr.is_empty()))?;

    let mut parser = perl_parser::Parser::new("use strict;\nmy $value = 1;\n");
    let ast = parser.parse().map_err(|error| error.to_string())?;
    let expected = format!("{}\n", render_output(&ast, OutputFormat::LegacySexp, false)?);
    check_equal(&(utf8(&stdout)?), &expected)?;
    Ok(())
}

#[test]
fn execute_quiet_suppresses_parse_output_but_still_writes_stats()
-> Result<(), Box<dyn std::error::Error>> {
    let file = temp_perl("use strict;\nmy $value = 1;\n")?;
    let path = file.path().to_str().ok_or("temp path was not UTF-8")?;
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let status = execute(["-q", "-s", path], &mut stdout, &mut stderr)?;
    check_equal(&status, &ProcessStatus::Success)?;
    check(stdout.is_empty(), stringify!(stdout.is_empty()))?;
    let stderr_text = utf8(&stderr)?;
    check(
        stderr_text.starts_with("\n=== Total Statistics ===\n"),
        stringify!(stderr_text.starts_with("\n=== Total Statistics ===\n")),
    )?;
    check(
        stderr_text.contains("Files parsed: 1\n"),
        stringify!(stderr_text.contains("Files parsed: 1\n")),
    )?;
    check(
        stderr_text.contains("Files failed: 0\n"),
        stringify!(stderr_text.contains("Files failed: 0\n")),
    )?;
    check(
        !stderr_text.contains("=== File Details ==="),
        stringify!(!stderr_text.contains("=== File Details ===")),
    )?;
    Ok(())
}

#[test]
fn execute_continue_counts_unreadable_input_and_stays_nonzero()
-> Result<(), Box<dyn std::error::Error>> {
    let before = temp_perl("use strict;\nmy $value = 1;\n")?;
    let after = temp_perl("use strict;\nmy $value = 2;\n")?;
    let scratch = tempfile::tempdir()?;
    let missing = scratch.path().join("missing-perl-parse-input.pl");
    let before_path = before.path().to_str().ok_or("temp path was not UTF-8")?;
    let after_path = after.path().to_str().ok_or("temp path was not UTF-8")?;
    let missing_path = missing.to_str().ok_or("missing path was not UTF-8")?;
    check(!missing.exists(), stringify!(!missing.exists()))?;

    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let status = execute(
        ["--continue", "--stats", "--quiet", before_path, missing_path, after_path],
        &mut stdout,
        &mut stderr,
    )?;
    check_equal(&status, &ProcessStatus::Failure)?;
    check(stdout.is_empty(), stringify!(stdout.is_empty()))?;
    let stderr_text = utf8(&stderr)?;
    check(stderr_text.contains("Files parsed: 2"), &stderr_text)?;
    check(stderr_text.contains("Files failed: 1"), &stderr_text)?;
    check(stderr_text.contains(&format!("{missing_path}: FAILED")), &stderr_text)?;
    check(stderr_text.contains(&format!("Error reading {missing_path}:")), &stderr_text)?;
    Ok(())
}

#[test]
fn execute_write_failure_is_err_even_under_continue() -> Result<(), Box<dyn std::error::Error>> {
    let first = temp_perl("use strict;\nmy $value = 1;\n")?;
    let second = temp_perl("use strict;\nmy $value = 2;\n")?;
    let first_path = first.path().to_str().ok_or("temp path was not UTF-8")?;
    let second_path = second.path().to_str().ok_or("temp path was not UTF-8")?;

    let mut stdout = ImmediateFail;
    let mut stderr = Vec::new();
    let result =
        execute(["--continue", "--stats", first_path, second_path], &mut stdout, &mut stderr);
    check(result.is_err(), "output failure must reach execute as Err, not a successful status")?;

    let stderr_text = utf8(&stderr)?;
    check_equal(&stderr_text, &format!("=== Parsing {first_path} ===\n"))?;
    check(
        !stderr_text.contains(second_path),
        "second input must not be visited after output failure",
    )?;
    check(
        !stderr_text.contains("=== Total Statistics ==="),
        "statistics must not be written after terminal output failure",
    )?;
    Ok(())
}

#[test]
fn help_write_failures_reach_the_boundary() -> Result<(), Box<dyn std::error::Error>> {
    let mut stderr = Vec::new();
    check(
        write_help(&mut ImmediateFail).is_err(),
        stringify!(write_help(&mut ImmediateFail).is_err()),
    )?;
    check(
        write_version(&mut ImmediateFail).is_err(),
        stringify!(write_version(&mut ImmediateFail).is_err()),
    )?;
    check(
        write_usage_error(&mut ImmediateFail, "boom").is_err(),
        stringify!(write_usage_error(&mut ImmediateFail, "boom").is_err()),
    )?;
    check(
        execute(["--help"], &mut ImmediateFail, &mut stderr).is_err(),
        stringify!(execute(["--help"], &mut ImmediateFail, &mut stderr).is_err()),
    )?;
    Ok(())
}

#[test]
fn stats_and_error_writes_fail_immediately() -> Result<(), Box<dyn std::error::Error>> {
    let mut stats = TotalStats::new();
    stats.add_file("a.pl", 1, Duration::from_nanos(1), 1);
    check(
        stats.write(&mut ImmediateFail).is_err(),
        stringify!(stats.write(&mut ImmediateFail).is_err()),
    )?;
    check(
        write_error(&ParseError::InvalidString, "", &mut ImmediateFail).is_err(),
        stringify!(write_error(&ParseError::InvalidString, "", &mut ImmediateFail).is_err()),
    )?;
    Ok(())
}

#[test]
fn stats_write_fails_after_a_partial_write() -> Result<(), Box<dyn std::error::Error>> {
    let mut stats = TotalStats::new();
    stats.add_file("a.pl", 1, Duration::from_nanos(1), 1);
    stats.add_file("b.pl", 1, Duration::from_nanos(1), 1);
    check(
        stats.write(&mut FailAfter::new(8)).is_err(),
        stringify!(stats.write(&mut FailAfter::new(8)).is_err()),
    )?;
    Ok(())
}

#[test]
fn help_write_fails_on_write_zero() -> Result<(), Box<dyn std::error::Error>> {
    let err = write_help(&mut WriteZeroSink).err().ok_or("Ok(0) must become a write error")?;
    check_equal(&(err.kind()), &io::ErrorKind::WriteZero)?;
    Ok(())
}

#[test]
fn execute_help_fails_on_write_zero() -> Result<(), Box<dyn std::error::Error>> {
    let mut stderr = Vec::new();
    let err = execute(["--help"], &mut WriteZeroSink, &mut stderr)
        .err()
        .ok_or("WriteZero must reach execute")?;
    check_equal(&(err.kind()), &io::ErrorKind::WriteZero)?;
    Ok(())
}

#[test]
fn discarding_a_write_error_is_detected() -> Result<(), Box<dyn std::error::Error>> {
    let mut writer = DiscardingWriter { inner: Vec::new() };
    let err = write_help(&mut writer).err().ok_or(
        "a writer that returns Err after accepting bytes must fail; swallowing with .ok() would hide this",
    )?;
    check_equal(&(err.kind()), &io::ErrorKind::BrokenPipe)?;
    check(!writer.inner.is_empty(), "control: some bytes were accepted before the error")?;
    Ok(())
}

#[test]
fn execute_maps_output_error_away_from_success() -> Result<(), Box<dyn std::error::Error>> {
    let file = temp_perl("use strict;\nmy $value = 1;\n")?;
    let path = file.path().to_str().ok_or("temp path was not UTF-8")?;
    let result = execute([path], &mut ImmediateFail, &mut Vec::new());
    check(
        result.is_err(),
        "discarding the write error with .ok() would yield Ok(Success) and fail this test",
    )?;
    Ok(())
}
