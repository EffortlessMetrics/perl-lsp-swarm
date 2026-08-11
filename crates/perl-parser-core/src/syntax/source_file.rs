//! Shared Perl source-file classification helpers.
//!
//! These helpers provide one canonical definition for what constitutes a Perl
//! source file across workspace discovery and runtime file operations.

use std::borrow::Cow;
use std::path::Path;

/// Number of bytes to inspect for binary content detection.
///
/// 4 KB is enough to catch all common binary formats (ELF, PE, ZIP, PNG, …)
/// while being cheap to scan.
const BINARY_PROBE_BYTES: usize = 4096;

/// Minimum ratio of NUL bytes (within the probe window) required to classify
/// content as binary.
///
/// Real binary files are dominated by NUL bytes (often >10%), while a
/// legitimate Perl file that carries a small amount of binary data before a
/// `__DATA__`/`__END__` token has a handful at most. Requiring >5% NUL
/// density eliminates single-NUL false positives while still catching compact
/// binary signatures like ZIP (`PK\x03\x04…`, ~14% NUL) and ELF (~56% NUL)
/// within the probe window (#5209).
const BINARY_NUL_RATIO: f64 = 0.05;

/// Returns `true` if `text` appears to contain binary (non-text) content.
///
/// The heuristic scans the first [`BINARY_PROBE_BYTES`] bytes (or the region
/// before a `__DATA__`/`__END__` token, whichever is smaller) for null bytes
/// (`\0`). A NUL-byte ratio above [`BINARY_NUL_RATIO`] classifies the content
/// as binary.
///
/// # Why null bytes?
///
/// - Fast: a scan of at most 4 KB.
/// - Low false-positive rate: Perl source virtually never contains `\0`. The
///   only legitimate case is binary data in `__DATA__`/`__END__` sections,
///   which is excluded from the scan window.
/// - High true-positive rate: every common compiled binary contains many `\0`.
///
/// # `__DATA__`/`__END__` handling
///
/// Perl files may carry binary blobs after a `__DATA__` or `__END__` token
/// (packed records, images, serialized data). Such files are legitimate Perl
/// source for everything before the token, so the scan stops at the first
/// occurrence of either token (#5209).
#[must_use]
pub fn is_binary_content(text: &str) -> bool {
    // Cap the probe at BINARY_PROBE_BYTES, rounding down to a char boundary so
    // that a multibyte character split at the boundary doesn't cause
    // str::get to return None (which would silently uncap the probe).
    let cap = BINARY_PROBE_BYTES.min(text.len());
    let cap = text.floor_char_boundary(cap);
    let probe_bytes = &text[..cap];

    // Find the probe window: everything before __DATA__/__END__. If neither
    // token is present, use the full probe.
    let window = data_section_start(probe_bytes).unwrap_or(probe_bytes.len());

    let scan = &probe_bytes[..window];
    if scan.is_empty() {
        return false;
    }
    let nul_count = scan.bytes().filter(|&b| b == 0).count();
    let ratio = nul_count as f64 / scan.len() as f64;
    ratio > BINARY_NUL_RATIO
}

/// Returns the byte offset of the first `__DATA__` or `__END__` token in
/// `text` that appears at the start of a line, or `None` if neither is present.
///
/// Both tokens must appear on their own line to be recognized by Perl, so we
/// only match line-start occurrences.
///
/// The offset is computed from the raw byte slice (not `str::lines()`) so that
/// CRLF line endings are accounted for correctly: `\r\n` is 2 bytes, not 1.
fn data_section_start(text: &str) -> Option<usize> {
    // Iterate over byte offsets, tracking line starts manually to handle both
    // LF and CRLF endings correctly (str::lines() collapses both to a single
    // delimiter, losing the byte-count accuracy we need for the returned offset).
    let bytes = text.as_bytes();
    let mut line_start = 0usize;
    while line_start < bytes.len() {
        // Find the end of this line (next \n or end of text).
        let line_end = bytes[line_start..]
            .iter()
            .position(|&b| b == b'\n')
            .map_or(bytes.len(), |pos| line_start + pos);

        // The line content excludes a trailing \r (for CRLF files).
        let content_end = if line_end > line_start && bytes[line_end - 1] == b'\r' {
            line_end - 1
        } else {
            line_end
        };

        // Skip leading whitespace to find the first token.
        let line = &text[line_start..content_end];
        let trimmed = line.trim_start();
        let leading = line.len() - trimmed.len();
        let token_start = line_start + leading;

        let first_word = trimmed.split_ascii_whitespace().next().unwrap_or("");
        // Perl recognizes __DATA__ and __END__ as line-start tokens. The token
        // may be followed by punctuation or other text on the same line (Perl's
        // lexer matches the token and treats the rest of the line as data), so
        // we check that the first word starts with the marker AND is followed
        // by a non-identifier character (or end of string). This avoids matching
        // look-alikes like __DATA__FOO while accepting __DATA__; and __END__.
        if is_data_marker_token(first_word) {
            return Some(token_start);
        }

        // Advance past this line and its line ending (\n or \r\n).
        line_start = if line_end < bytes.len() { line_end + 1 } else { bytes.len() };
    }
    None
}

/// Returns `true` if `word` is a Perl data-section marker (`__DATA__` or
/// `__END__`), optionally followed by a non-identifier character (e.g. a
/// semicolon) or end of string. This mirrors Perl's lexer, which matches the
/// token at line start and treats the rest of the line as data.
fn is_data_marker_token(word: &str) -> bool {
    for marker in &["__DATA__", "__END__"] {
        if word == *marker {
            return true;
        }
        // Check if the word starts with the marker followed by a non-identifier
        // character (e.g. "__DATA__;junk" → marker + ";junk").
        if let Some(rest) = word.strip_prefix(marker) {
            if let Some(next_char) = rest.chars().next() {
                if !next_char.is_ascii_alphanumeric() && next_char != '_' {
                    return true;
                }
            }
        }
    }
    false
}

/// Canonical Perl source file extensions.
///
/// Includes core Perl script and module extensions as well as common embedded
/// Perl template formats: `.ep` (Mojolicious), `.tt`/`.tt2` (Template Toolkit),
/// and `.mason` (Mason/HTML::Mason).
pub const PERL_SOURCE_EXTENSIONS: [&str; 10] =
    ["pl", "pm", "t", "psgi", "cgi", "fcgi", "ep", "tt", "tt2", "mason"];

/// Returns `true` if `extension` is a recognized Perl source extension.
///
/// Accepts values with or without a leading dot and matches
/// case-insensitively.
#[must_use]
pub fn is_perl_source_extension(extension: &str) -> bool {
    let ext = extension.strip_prefix('.').unwrap_or(extension);
    PERL_SOURCE_EXTENSIONS.iter().any(|candidate| candidate.eq_ignore_ascii_case(ext))
}

/// Returns `true` if `path` points to a recognized Perl source file.
#[must_use]
pub fn is_perl_source_path(path: &Path) -> bool {
    path.extension().and_then(|ext| ext.to_str()).is_some_and(is_perl_source_extension)
}

/// Returns `true` if `uri` or path-like string points to a Perl source file.
///
/// Supports:
/// - Plain filesystem paths
/// - `file://` URIs
/// - Percent-encoded URI path segments
/// - Optional query/fragment suffixes
#[must_use]
pub fn is_perl_source_uri(uri: &str) -> bool {
    let path_part = uri.split_once(['?', '#']).map_or(uri, |(path_prefix, _)| path_prefix);
    let decoded_path = percent_decode_uri_path(path_part);
    is_perl_source_path(Path::new(decoded_path.as_ref()))
}

fn percent_decode_uri_path(path: &str) -> Cow<'_, str> {
    if !path.as_bytes().contains(&b'%') {
        return Cow::Borrowed(path);
    }

    let bytes = path.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    let mut changed = false;

    while index < bytes.len() {
        if bytes[index] == b'%'
            && let (Some(high), Some(low)) = (bytes.get(index + 1), bytes.get(index + 2))
            && let (Some(high), Some(low)) = (hex_value(*high), hex_value(*low))
        {
            decoded.push((high << 4) | low);
            index += 3;
            changed = true;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }

    if !changed {
        return Cow::Borrowed(path);
    }

    String::from_utf8(decoded).map_or(Cow::Borrowed(path), Cow::Owned)
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        BINARY_PROBE_BYTES, PERL_SOURCE_EXTENSIONS, is_binary_content, is_perl_source_extension,
        is_perl_source_path, is_perl_source_uri,
    };
    use std::path::Path;

    #[test]
    fn exposes_expected_extension_set() {
        assert_eq!(
            PERL_SOURCE_EXTENSIONS,
            ["pl", "pm", "t", "psgi", "cgi", "fcgi", "ep", "tt", "tt2", "mason"]
        );
    }

    #[test]
    fn classifies_extensions_case_insensitively() {
        assert!(is_perl_source_extension("pl"));
        assert!(is_perl_source_extension(".pm"));
        assert!(is_perl_source_extension("T"));
        assert!(is_perl_source_extension("PsGi"));
        assert!(is_perl_source_extension("cgi"));
        assert!(is_perl_source_extension(".CGI"));
        assert!(!is_perl_source_extension("txt"));
    }

    #[test]
    fn classifies_filesystem_paths() {
        assert!(is_perl_source_path(Path::new("/workspace/script.pl")));
        assert!(is_perl_source_path(Path::new("/workspace/lib/Foo/Bar.PM")));
        assert!(is_perl_source_path(Path::new("/workspace/app.psgi")));
        assert!(is_perl_source_path(Path::new("/var/www/cgi-bin/form.cgi")));
        assert!(is_perl_source_path(Path::new("/var/www/cgi-bin/fast.cgi.fcgi")));
        assert!(is_perl_source_path(Path::new("/var/www/cgi-bin/upload.CGI")));
        assert!(!is_perl_source_path(Path::new("/workspace/README.md")));
        assert!(!is_perl_source_path(Path::new("/workspace/no_extension")));
    }

    #[test]
    fn classifies_uri_like_inputs() {
        assert!(is_perl_source_uri("file:///workspace/script.pl"));
        assert!(is_perl_source_uri("file:///workspace/lib/Foo/Bar.pm"));
        assert!(is_perl_source_uri("file:///workspace/app.psgi"));
        assert!(is_perl_source_uri("file:///workspace/app.psgi?version=1#section"));
        assert!(is_perl_source_uri("file:///var/www/cgi-bin/form.cgi"));
        assert!(is_perl_source_uri("file:///var/www/cgi-bin/search.cgi?q=perl#results"));
        assert!(!is_perl_source_uri("file:///workspace/README.md"));
    }

    #[test]
    fn classifies_percent_encoded_uri_path_extensions() {
        assert!(is_perl_source_uri("file:///workspace/script%2Epl"));
        assert!(is_perl_source_uri("file:///workspace/lib/Foo%2FBar.%70%6D"));
        assert!(is_perl_source_uri("file:///workspace/templates/index%2Ehtml%2Eep?rev=1#L4"));
        assert!(!is_perl_source_uri("file:///workspace/README%2Emd"));
    }

    #[test]
    fn invalid_percent_escapes_remain_literal() {
        assert!(is_perl_source_uri("file:///workspace/script%ZZ.pl"));
        assert!(!is_perl_source_uri("file:///workspace/script.%ZZ"));
    }

    #[test]
    fn cgi_and_psgi_are_recognized() {
        // CGI scripts (.cgi) — web projects, Apache/Nginx CGI handlers
        assert!(is_perl_source_extension("cgi"));
        assert!(is_perl_source_extension("CGI"));
        assert!(is_perl_source_extension("fcgi"));
        assert!(is_perl_source_extension("FCGI"));
        assert!(is_perl_source_path(Path::new("/var/www/cgi-bin/form.cgi")));
        assert!(is_perl_source_uri("file:///var/www/cgi-bin/form.cgi"));

        // PSGI apps (.psgi) — Plack/PSGI applications
        assert!(is_perl_source_extension("psgi"));
        assert!(is_perl_source_extension("PSGI"));
        assert!(is_perl_source_path(Path::new("/workspace/app.psgi")));
        assert!(is_perl_source_uri("file:///workspace/app.psgi"));

        // Non-Perl extensions remain unrecognized
        assert!(!is_perl_source_extension("sh"));
        assert!(!is_perl_source_extension("py"));
    }

    #[test]
    fn template_extensions_are_recognized() {
        // .ep — Mojolicious embedded Perl templates
        assert!(is_perl_source_extension("ep"));
        assert!(is_perl_source_extension("EP"));
        assert!(is_perl_source_path(Path::new("/app/templates/index.html.ep")));
        assert!(is_perl_source_uri("file:///app/templates/index.html.ep"));

        // .tt — Template Toolkit templates (version 2 default)
        assert!(is_perl_source_extension("tt"));
        assert!(is_perl_source_extension("TT"));
        assert!(is_perl_source_path(Path::new("/app/templates/page.tt")));
        assert!(is_perl_source_uri("file:///app/templates/page.tt"));

        // .tt2 — Template Toolkit 2 explicit extension
        assert!(is_perl_source_extension("tt2"));
        assert!(is_perl_source_extension("TT2"));
        assert!(is_perl_source_path(Path::new("/app/templates/layout.tt2")));
        assert!(is_perl_source_uri("file:///app/templates/layout.tt2"));

        // .mason — HTML::Mason / Mason2 templates
        assert!(is_perl_source_extension("mason"));
        assert!(is_perl_source_extension("MASON"));
        assert!(is_perl_source_path(Path::new("/app/comp/header.mason")));
        assert!(is_perl_source_uri("file:///app/comp/header.mason"));

        // Non-template extensions remain unrecognized
        assert!(!is_perl_source_extension("html"));
        assert!(!is_perl_source_extension("tmpl"));
    }

    #[test]
    fn supports_windows_style_paths() {
        assert!(is_perl_source_uri(r"C:\workspace\script.pl"));
        assert!(is_perl_source_uri(r"file:///C:/workspace/lib/Foo.pm"));
        assert!(!is_perl_source_uri(r"C:\workspace\README.txt"));
    }

    // ── is_binary_content ─────────────────────────────────────────────────

    #[test]
    fn binary_content_null_byte_is_detected() {
        // Simulate a binary file arriving as a string with embedded null bytes.
        // Many NULs well above the ratio threshold.
        let binary = "PK\x00\x03some binary content\x00\x00\x00";
        assert!(is_binary_content(binary), "null-byte-heavy content must trigger binary guard");
    }

    #[test]
    fn binary_content_single_null_byte_does_not_trigger_guard() {
        // A single stray NUL byte in otherwise-clean Perl source must NOT be
        // classified as binary — the ratio heuristic requires >5% NUL density
        // (#5209).
        let text = "use strict;\x00\nuse warnings;\n";
        assert!(
            !is_binary_content(text),
            "single null byte in clean source must not trigger binary guard"
        );
    }

    #[test]
    fn binary_content_clean_perl_is_not_binary() {
        let perl = "#!/usr/bin/perl\nuse strict;\nuse warnings;\n\nprint \"Hello, World!\\n\";\n";
        assert!(!is_binary_content(perl), "clean Perl source must not be classified as binary");
    }

    #[test]
    fn binary_content_empty_string_is_not_binary() {
        assert!(!is_binary_content(""), "empty string must not be classified as binary");
    }

    #[test]
    fn binary_content_unicode_text_is_not_binary() {
        // High-byte UTF-8 sequences must not trigger the guard
        let utf8 = "# Perl with Unicode: \u{00e9}t\u{00e9}\nprint \"caf\u{00e9}\\n\";\n";
        assert!(!is_binary_content(utf8), "UTF-8 text without null bytes must not be binary");
    }

    #[test]
    fn binary_content_only_scans_first_probe_window() {
        // A null byte beyond the probe window must NOT trigger the guard —
        // we only scan the first BINARY_PROBE_BYTES bytes.
        let safe_prefix = "a".repeat(BINARY_PROBE_BYTES);
        let text_with_late_null = format!("{safe_prefix}\x00trailing");
        assert!(
            !is_binary_content(&text_with_late_null),
            "null byte beyond probe window must not trigger the guard"
        );
    }

    #[test]
    fn binary_content_high_nul_ratio_at_probe_boundary_is_detected() {
        // Many NUL bytes near the end of the probe window, dense enough to
        // exceed the ratio threshold, must still be detected.
        let prefix = "a".repeat(BINARY_PROBE_BYTES - 250);
        let nuls = "\x00".repeat(250);
        let text = format!("{prefix}{nuls}rest");
        assert!(
            is_binary_content(&text),
            "high-density NUL region within probe window must trigger binary guard"
        );
    }

    #[test]
    fn binary_content_elf_header_is_detected() {
        // ELF magic: \x7fELF followed by binary data
        let elf_like = "\x7fELF\x02\x01\x01\x00\x00\x00\x00\x00\x00\x00\x00\x00";
        assert!(is_binary_content(elf_like), "ELF-like header with null bytes must be binary");
    }

    #[test]
    fn binary_content_zip_pk_header_is_detected() {
        // ZIP files start with PK\x03\x04
        let zip_like = "PK\x03\x04\x14\x00\x00\x00\x08\x00";
        assert!(is_binary_content(zip_like), "ZIP-like header with null bytes must be binary");
    }

    #[test]
    fn binary_content_data_section_with_binary_is_not_binary() {
        // A Perl file with binary data after __DATA__ must not be flagged —
        // the scan stops at the __DATA__ token (#5209).
        let perl = "package Foo;\nuse strict;\n\n1;\n__DATA__\n\x00\x00\x00binary blob\x00";
        assert!(
            !is_binary_content(perl),
            "binary data after __DATA__ must not trigger binary guard"
        );
    }

    #[test]
    fn binary_content_end_section_with_binary_is_not_binary() {
        // Same for __END__.
        let perl = "package Bar;\nuse strict;\n\n1;\n__END__\n\x00\x00\x00\x00\x00\x00blob";
        assert!(
            !is_binary_content(perl),
            "binary data after __END__ must not trigger binary guard"
        );
    }

    #[test]
    fn binary_content_nul_before_data_section_still_ratio_gated() {
        // A few NULs before __DATA__ must not trigger unless they exceed the
        // ratio threshold.
        let perl = "package Foo;\nuse strict;\x00\n1;\n__DATA__\nblob";
        assert!(
            !is_binary_content(perl),
            "single NUL before __DATA__ must not trigger binary guard"
        );
    }

    #[test]
    fn binary_content_data_section_with_crlf_line_endings() {
        // CRLF line endings must not corrupt the __DATA__ offset computation
        // (chatgpt-codex P1 review on PR #5716): \r\n is 2 bytes, not 1.
        let perl = "package Foo;\r\nuse strict;\r\n1;\r\n__DATA__\r\n\x00\x00\x00binary blob\x00";
        assert!(
            !is_binary_content(perl),
            "binary data after __DATA__ with CRLF endings must not trigger binary guard"
        );
    }

    #[test]
    fn binary_content_punctuated_data_marker() {
        // Perl recognizes __DATA__ and __END__ even when followed by
        // punctuation like a semicolon (chatgpt-codex P2 review on PR #5716).
        let perl = "package Foo;\nuse strict;\n1;\n__DATA__;junk\n\x00\x00\x00binary blob\x00";
        assert!(
            !is_binary_content(perl),
            "binary data after __DATA__; must not trigger binary guard"
        );
    }
}
