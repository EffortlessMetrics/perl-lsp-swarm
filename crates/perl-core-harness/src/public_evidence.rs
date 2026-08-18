//! One structural classifier for host-path material embedded in public evidence.
//!
//! Public harness receipts are uploaded as workflow artifacts, so every string
//! they carry is published. The earlier check only recognized a host path when a
//! whitespace-delimited token *began* with a path prefix, which misses the
//! attached forms that dominate real receipts (`--manifest-path=/home/...`,
//! `cwd:C:\Users\...`, nested JSON, percent- and backslash-escaped separators).
//!
//! This module owns the single detection contract. Both the canonical receipt
//! validator and the standalone publication gate consume it, so the two surfaces
//! cannot drift into contradictory semantics.

use serde_json::Value;

/// The kind of host-path material recognized in a public string.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum HostPathKind {
    /// A `file:` URI such as `file:///tmp/repo/report.json`.
    FileUri,
    /// A Unix absolute path such as `/home/runner/work`, including the
    /// POSIX-equivalent repeated-leading-separator forms (`///home/runner`).
    UnixAbsolute,
    /// A Windows drive path such as `C:\Users\runner` or `C:/Users/runner`.
    WindowsDrive,
    /// A Windows UNC share path such as `\\server\share`.
    WindowsUnc,
    /// A Windows device or extended-length namespace path such as `\\?\C:\...`,
    /// `\\.\C:\...`, or `\??\C:\...`.
    WindowsNamespace,
}

impl HostPathKind {
    /// The stable diagnostic label for this classification.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::FileUri => "file_uri",
            Self::UnixAbsolute => "unix_absolute",
            Self::WindowsDrive => "windows_drive",
            Self::WindowsUnc => "windows_unc",
            Self::WindowsNamespace => "windows_namespace",
        }
    }
}

/// How a public string should be interpreted when classifying host paths.
///
/// Exemptions are field-local by construction: a class is derived from the
/// immediately owning key and never propagates into nested maps, so a
/// `message` nested under `source_text` is still scanned as ordinary evidence.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum PublicStringClass {
    /// Ordinary published evidence. Fully scanned.
    #[default]
    Ordinary,
    /// Perl source text captured verbatim from a corpus file.
    SourceText,
    /// A Perl regular expression captured verbatim.
    Regex,
}

/// One host-path finding, located by logical file and JSON pointer.
///
/// A finding deliberately carries no private material: the offending value is
/// never echoed, and a path-bearing object key is replaced with a redacted
/// pointer segment.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct Finding {
    /// The bundle-relative logical file the finding was found in.
    pub logical_file: String,
    /// The JSON pointer to the offending value.
    pub pointer: String,
    /// What kind of host path was recognized.
    pub kind: HostPathKind,
}

impl Finding {
    /// Render the finding without echoing the private value.
    #[must_use]
    pub fn render(&self) -> String {
        format!("{}#{}: {} host path", self.logical_file, self.pointer, self.kind.as_str())
    }
}

/// Classify one public string, honoring its field-local exemption class.
///
/// Returns `None` when the string carries no host-path material, or when an
/// explicitly typed source/regex field owns the value.
#[must_use]
pub fn classify_public_string(text: &str, class: PublicStringClass) -> Option<HostPathKind> {
    if matches!(class, PublicStringClass::SourceText | PublicStringClass::Regex) {
        return None;
    }

    let decoded = decode_separator_escapes(text);
    let bytes = decoded.as_bytes();

    // Ordered most specific first: a `file:` URI and a Windows namespace both
    // contain separator runs that the coarser detectors would otherwise
    // mislabel.
    if let Some(kind) = find_file_uri(bytes)
        .or_else(|| find_windows_namespace(bytes))
        .or_else(|| find_windows_drive(bytes))
        .or_else(|| find_windows_unc(bytes))
        .or_else(|| find_unix_absolute(bytes))
    {
        return Some(kind);
    }
    None
}

/// Derive the field-local string class for values owned by `key`.
///
/// Only names the repository contract proves to carry verbatim Perl material
/// are exempted. Ambiguous names such as `snippet` or bare `pattern` fail
/// closed and remain ordinary evidence.
#[must_use]
pub fn classify_field(key: &str) -> PublicStringClass {
    match key.to_ascii_lowercase().as_str() {
        "source_text" | "source_code" | "source_snippet" | "perl_source" | "fixture_source" => {
            PublicStringClass::SourceText
        }
        "regex" | "regular_expression" => PublicStringClass::Regex,
        _ => PublicStringClass::Ordinary,
    }
}

/// Structurally scan one public JSON value, appending every finding.
///
/// Object keys are scanned as evidence in their own right, because public maps
/// are commonly keyed by source path. A path-bearing key is reported against
/// its containing pointer and never echoed.
pub fn scan_public_value(
    value: &Value,
    logical_file: &str,
    pointer: &str,
    class: PublicStringClass,
    findings: &mut Vec<Finding>,
) {
    match value {
        Value::String(text) => {
            if let Some(kind) = classify_public_string(text, class) {
                findings.push(Finding {
                    logical_file: logical_file.to_string(),
                    pointer: pointer_or_root(pointer),
                    kind,
                });
            }
        }
        Value::Array(values) => {
            // A typed exemption covers a homogeneous scalar array owned by the
            // exact field, so the class flows through array indices.
            for (index, element) in values.iter().enumerate() {
                scan_public_value(
                    element,
                    logical_file,
                    &format!("{pointer}/{index}"),
                    class,
                    findings,
                );
            }
        }
        Value::Object(entries) => {
            for (key, child) in entries {
                let key_kind = classify_public_string(key, PublicStringClass::Ordinary);
                if let Some(kind) = key_kind {
                    findings.push(Finding {
                        logical_file: logical_file.to_string(),
                        pointer: pointer_or_root(pointer),
                        kind,
                    });
                }

                let segment = if key_kind.is_some() {
                    "<redacted-key>".to_string()
                } else {
                    escape_json_pointer(key)
                };
                // Nested maps return to ordinary scanning: an exemption
                // identifies one field's own value, not an entire subtree.
                scan_public_value(
                    child,
                    logical_file,
                    &format!("{pointer}/{segment}"),
                    classify_field(key),
                    findings,
                );
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

fn pointer_or_root(pointer: &str) -> String {
    if pointer.is_empty() { "/".to_string() } else { pointer.to_string() }
}

fn escape_json_pointer(value: &str) -> String {
    value.replace('~', "~0").replace('/', "~1")
}

/// Normalize the separator encodings a producer can use to hide a host path.
///
/// Percent escapes (`%2F`, `%5C`, `%3A`) and the JSON escaped solidus (`\/`)
/// both survive into published strings when a diagnostic embeds serialized
/// JSON. Backslash runs are deliberately left intact so the UNC and namespace
/// detectors can still see them.
fn decode_separator_escapes(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut output = String::with_capacity(text.len());
    let mut index = 0usize;
    while index < bytes.len() {
        if bytes[index] == b'%'
            && index + 2 < bytes.len()
            && let (Some(high), Some(low)) =
                (hex_value(bytes[index + 1]), hex_value(bytes[index + 2]))
        {
            let decoded = (high << 4) | low;
            if matches!(decoded, b'/' | b'\\' | b':') {
                output.push(char::from(decoded));
                index += 3;
                continue;
            }
        }
        // A JSON encoder may emit `\/` for a solidus; the escape is not part of
        // the value and must not hide the separator from classification.
        if bytes[index] == b'\\' && index + 1 < bytes.len() && bytes[index + 1] == b'/' {
            output.push('/');
            index += 2;
            continue;
        }
        output.push(char::from(bytes[index]));
        index += 1;
    }
    output
}

const fn hex_value(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

const fn is_separator(byte: u8) -> bool {
    matches!(byte, b'/' | b'\\')
}

/// Punctuation that can precede an attached host path.
///
/// `/` and `\` are excluded so a separator run is treated as one token rather
/// than as a boundary before its own tail.
const fn is_path_boundary(byte: u8) -> bool {
    matches!(
        byte,
        b' ' | b'\t'
            | b'\n'
            | b'\r'
            | b'='
            | b':'
            | b'"'
            | b'\''
            | b'('
            | b'['
            | b'{'
            | b','
            | b';'
            | b'|'
            | b'<'
            | b'>'
    )
}

const fn at_boundary(bytes: &[u8], index: usize) -> bool {
    index == 0 || is_path_boundary(bytes[index - 1])
}

/// Length of the separator run beginning at `index`.
fn separator_run(bytes: &[u8], index: usize) -> usize {
    let mut length = 0usize;
    while index + length < bytes.len() && is_separator(bytes[index + length]) {
        length += 1;
    }
    length
}

/// Recognize a boundary-delimited `file:` URI.
///
/// Boundary awareness is what keeps this from firing on `file_path` keys or on
/// a `file:/` fragment embedded inside another URL's path.
fn find_file_uri(bytes: &[u8]) -> Option<HostPathKind> {
    const SCHEME: &[u8] = b"file:";
    (0..bytes.len()).find_map(|index| {
        if !at_boundary(bytes, index) {
            return None;
        }
        let tail = bytes.get(index..index + SCHEME.len())?;
        if !tail.eq_ignore_ascii_case(SCHEME) {
            return None;
        }
        let after = index + SCHEME.len();
        // Require a path-shaped tail so a bare `file:` mention stays ordinary.
        (separator_run(bytes, after) >= 1).then_some(HostPathKind::FileUri)
    })
}

/// Recognize Windows device and extended-length namespace prefixes.
///
/// Covers `\\?\`, `//?/`, `\\.\`, and `\??\`. The `.` form requires backslashes
/// so an ordinary `//../` Unix segment is not mislabeled as a device path.
fn find_windows_namespace(bytes: &[u8]) -> Option<HostPathKind> {
    (0..bytes.len()).find_map(|index| {
        if !at_boundary(bytes, index) {
            return None;
        }
        let run = separator_run(bytes, index);
        if run == 0 {
            return None;
        }
        let after = index + run;
        let all_backslash = bytes[index..after].iter().all(|byte| *byte == b'\\');

        // `\\?\` / `//?/`
        if run >= 2
            && bytes.get(after) == Some(&b'?')
            && bytes.get(after + 1).copied().is_some_and(is_separator)
        {
            return Some(HostPathKind::WindowsNamespace);
        }
        // `\\.\`
        if run >= 2
            && all_backslash
            && bytes.get(after) == Some(&b'.')
            && bytes.get(after + 1).copied().is_some_and(is_separator)
        {
            return Some(HostPathKind::WindowsNamespace);
        }
        // `\??\`
        if bytes.get(after) == Some(&b'?')
            && bytes.get(after + 1) == Some(&b'?')
            && bytes.get(after + 2).copied().is_some_and(is_separator)
        {
            return Some(HostPathKind::WindowsNamespace);
        }
        None
    })
}

fn find_windows_drive(bytes: &[u8]) -> Option<HostPathKind> {
    (0..bytes.len().saturating_sub(2)).find_map(|index| {
        (bytes[index].is_ascii_alphabetic()
            && bytes[index + 1] == b':'
            && is_separator(bytes[index + 2])
            && at_boundary(bytes, index))
        .then_some(HostPathKind::WindowsDrive)
    })
}

/// Recognize a UNC share path.
///
/// UNC is a backslash-only form. A bare `//` is a POSIX path with an empty
/// leading segment and belongs to the Unix detector, so classifying it here
/// would both misreport the platform and reject ordinary evidence.
fn find_windows_unc(bytes: &[u8]) -> Option<HostPathKind> {
    (0..bytes.len()).find_map(|index| {
        if !at_boundary(bytes, index) {
            return None;
        }
        let run = separator_run(bytes, index);
        if run < 2 || !bytes[index..index + run].iter().all(|byte| *byte == b'\\') {
            return None;
        }
        // A share name must follow; a trailing separator run alone is not UNC.
        bytes.get(index + run).is_some().then_some(HostPathKind::WindowsUnc)
    })
}

/// Recognize a Unix absolute path, including repeated leading separators.
///
/// A separator run that follows a URI scheme (`https://`) is skipped so
/// ordinary URLs remain publishable.
fn find_unix_absolute(bytes: &[u8]) -> Option<HostPathKind> {
    let mut index = 0usize;
    while index < bytes.len() {
        if bytes[index] != b'/' || !at_boundary(bytes, index) {
            index += 1;
            continue;
        }
        let run = separator_run(bytes, index);
        let after = index + run;
        if after >= bytes.len() {
            return None;
        }
        if has_uri_scheme_before(bytes, index) {
            index = after;
            continue;
        }
        return Some(HostPathKind::UnixAbsolute);
    }
    None
}

fn has_uri_scheme_before(bytes: &[u8], slash_index: usize) -> bool {
    if slash_index == 0 || bytes[slash_index - 1] != b':' {
        return false;
    }
    let mut start = slash_index - 1;
    while start > 0 && is_uri_scheme_byte(bytes[start - 1]) {
        start -= 1;
    }
    let scheme = &bytes[start..slash_index - 1];
    scheme.first().is_some_and(u8::is_ascii_alphabetic)
        && scheme.iter().copied().all(is_uri_scheme_byte)
}

const fn is_uri_scheme_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'-' | b'.')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ordinary(text: &str) -> Option<HostPathKind> {
        classify_public_string(text, PublicStringClass::Ordinary)
    }

    #[test]
    fn detects_attached_and_nested_host_path_forms() {
        assert_eq!(
            ordinary("--manifest-path=/home/runner/work/repo/Cargo.toml"),
            Some(HostPathKind::UnixAbsolute)
        );
        assert_eq!(ordinary(r"cwd:C:\Users\runner\repo"), Some(HostPathKind::WindowsDrive));
        assert_eq!(ordinary(r"source=\\server\share\repo\file.rs"), Some(HostPathKind::WindowsUnc));
        assert_eq!(ordinary("file:///tmp/repo/report.json"), Some(HostPathKind::FileUri));
        assert_eq!(
            ordinary(r#"{"argument":"--root=/private/var/folders/build"}"#),
            Some(HostPathKind::UnixAbsolute)
        );
        assert_eq!(
            ordinary(r#"{"cwd":"C:\\Users\\runner\\repo"}"#),
            Some(HostPathKind::WindowsDrive)
        );
    }

    #[test]
    fn rejects_shallow_repeated_and_trailing_slash_unix_paths() {
        for value in [
            "/etc/passwd",
            "/root/build",
            "/srv/cache",
            "arg=/opt/tool",
            "/home/runner/work/",
            "cwd=/srv/cache/",
            "///etc/passwd",
            "arg=////srv/cache",
        ] {
            assert_eq!(
                ordinary(value),
                Some(HostPathKind::UnixAbsolute),
                "absolute path escaped detection: {value}"
            );
        }
    }

    /// Regression for the review finding that `//` was reported as Windows UNC.
    /// A bare `//` is a POSIX path with an empty leading segment; reporting it
    /// as UNC both misnames the platform and, before this repair, rejected
    /// ordinary text. It must still be rejected, but as a Unix path.
    #[test]
    fn forward_slash_pairs_are_unix_not_unc() {
        assert_eq!(ordinary("//etc/hostname"), Some(HostPathKind::UnixAbsolute));
        assert_eq!(ordinary("arg=//host/share/file"), Some(HostPathKind::UnixAbsolute));
        assert_eq!(ordinary(r"\\host\share"), Some(HostPathKind::WindowsUnc));
    }

    #[test]
    fn rejects_windows_namespace_paths_under_their_own_kind() {
        for value in [
            r"\\?\C:\Users\runner\private",
            r"\\?\UNC\server\share\private",
            r"\\.\C:\Users\runner\private",
            r"\??\C:\Users\runner\private",
        ] {
            assert_eq!(
                ordinary(value),
                Some(HostPathKind::WindowsNamespace),
                "Windows namespace path escaped detection: {value}"
            );
        }
    }

    /// Regression for the review finding that `lower.contains("file:/")` fired
    /// on any narrative mention. Recognition is now boundary-anchored.
    #[test]
    fn file_uri_recognition_is_boundary_aware() {
        assert_eq!(ordinary("file:/notes/report"), Some(HostPathKind::FileUri));
        assert_eq!(ordinary("FILE:///tmp/x"), Some(HostPathKind::FileUri));
        assert_eq!(ordinary("file%3A%2F%2Ftmp%2Fx"), Some(HostPathKind::FileUri));
        // `file:` inside another URL's path is not its own scheme.
        assert_eq!(ordinary("https://docs.example.com/file:/reference"), None);
        assert_eq!(ordinary("profile_name"), None);
        assert_eq!(ordinary("file_path"), None);
        // A bare `file:` mention with no path tail stays ordinary.
        assert_eq!(ordinary("see file: the attachment"), None);
    }

    /// Regression for the review finding that a JSON-escaped solidus hid an
    /// absolute path from every detector.
    #[test]
    fn escaped_solidi_are_decoded_before_classification() {
        assert_eq!(ordinary(r#"{"cwd":"\/home\/runner\/work"}"#), Some(HostPathKind::UnixAbsolute));
        assert_eq!(ordinary(r"--root=%2Fhome%2Frunner%2Frepo"), Some(HostPathKind::UnixAbsolute));
    }

    #[test]
    fn accepts_urls_logical_paths_and_ordinary_text() {
        for value in [
            "https://example.com/a/b/c",
            "https:///example.com/a/b/c",
            "crates/perl-parser/src/lib.rs",
            "target/perl-core/smoke/base/perl5",
            "key=value:ordinary",
            "perl_core_harness.report.v1",
            "../relative/sibling",
            "2026-08-15T00:00:00Z",
        ] {
            assert_eq!(ordinary(value), None, "unexpected host-path finding for {value}");
        }
    }

    #[test]
    fn typed_source_and_regex_fields_are_exempt() {
        assert_eq!(
            classify_public_string("open '/etc/passwd';", PublicStringClass::SourceText),
            None
        );
        for regex in ["m{/foo/bar}", "qr{/foo/bar}", "/foo/bar/"] {
            assert_eq!(classify_public_string(regex, PublicStringClass::Regex), None);
        }
    }

    #[test]
    fn object_keys_are_scanned_without_being_echoed() {
        let value = serde_json::json!({
            "/home/runner/private.pl": {"status": "fail"},
            "C:\\Users\\runner\\private.pl": {"status": "fail"}
        });
        let mut findings = Vec::new();
        scan_public_value(
            &value,
            "smoke/base/smoke.json",
            "",
            PublicStringClass::Ordinary,
            &mut findings,
        );
        findings.sort();
        assert_eq!(findings.len(), 2);
        assert!(findings.iter().all(|finding| finding.pointer == "/"));
        let rendered = findings.iter().map(Finding::render).collect::<Vec<_>>().join("\n");
        assert!(!rendered.contains("private.pl"), "finding echoed a private key: {rendered}");
    }

    #[test]
    fn exemptions_are_field_local_and_do_not_cover_nested_maps() {
        let value = serde_json::json!({
            "source_text": {"message": "/etc/passwd"},
            "source_code": ["open '/etc/passwd';", "my $x = 1;"],
            "regex": "/foo/bar/",
            "message": "failed while reading /etc/passwd"
        });
        let mut findings = Vec::new();
        scan_public_value(
            &value,
            "smoke/base/smoke.json",
            "",
            PublicStringClass::Ordinary,
            &mut findings,
        );
        findings.sort();
        assert_eq!(findings.len(), 2, "unexpected findings: {findings:?}");
        assert_eq!(findings[0].pointer, "/message");
        assert_eq!(findings[1].pointer, "/source_text/message");
    }

    #[test]
    fn ambiguous_field_names_fail_closed() {
        assert_eq!(classify_field("snippet"), PublicStringClass::Ordinary);
        assert_eq!(classify_field("pattern"), PublicStringClass::Ordinary);
        assert_eq!(classify_field("source_text"), PublicStringClass::SourceText);
        assert_eq!(classify_field("REGEX"), PublicStringClass::Regex);
    }

    #[test]
    fn findings_locate_values_without_echoing_them() {
        let value = serde_json::json!({
            "command": ["cargo", "--manifest-path=/home/runner/private/Cargo.toml"],
            "safe": "crates/perl-parser/src/lib.rs"
        });
        let mut findings = Vec::new();
        scan_public_value(
            &value,
            "smoke/base/reproduction.json",
            "",
            PublicStringClass::Ordinary,
            &mut findings,
        );
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].pointer, "/command/1");
        assert!(!findings[0].render().contains("/home/runner/private"));
    }
}
