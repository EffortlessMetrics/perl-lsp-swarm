#![warn(missing_docs)]
#![cfg_attr(clippy, allow(missing_docs))]

//! Fail-closed host-path validation for normalized public compiler evidence.

use color_eyre::eyre::{Context, Result, bail};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

fn main() -> Result<()> {
    color_eyre::install()?;
    let mut args = std::env::args().skip(1);
    let flag = args.next().ok_or_else(|| {
        color_eyre::eyre::eyre!(
            "usage: perl-core-harness-public-evidence --bundle-dir <bundle-directory>"
        )
    })?;
    if flag != "--bundle-dir" {
        bail!("expected --bundle-dir, found {flag}");
    }
    let bundle_dir = PathBuf::from(
        args.next()
            .ok_or_else(|| color_eyre::eyre::eyre!("missing value for --bundle-dir"))?,
    );
    if let Some(extra) = args.next() {
        bail!("unexpected argument: {extra}");
    }

    validate_public_bundle(&bundle_dir)
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum HostPathKind {
    FileUri,
    UnixAbsolute,
    WindowsDrive,
    WindowsUnc,
}

impl HostPathKind {
    const fn as_str(self) -> &'static str {
        match self {
            Self::FileUri => "file_uri",
            Self::UnixAbsolute => "unix_absolute",
            Self::WindowsDrive => "windows_drive",
            Self::WindowsUnc => "windows_unc",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PublicStringClass {
    Ordinary,
    SourceText,
    Regex,
}

#[derive(Debug, Eq, Ord, PartialEq, PartialOrd)]
struct Finding {
    logical_file: String,
    pointer: String,
    kind: HostPathKind,
}

fn validate_public_bundle(bundle_dir: &Path) -> Result<()> {
    let bundle_dir = fs::canonicalize(bundle_dir)
        .with_context(|| format!("canonicalizing bundle directory {}", bundle_dir.display()))?;
    if !bundle_dir.is_dir() {
        bail!("bundle directory is not a directory: {}", bundle_dir.display());
    }

    let mut files = Vec::new();
    let index = bundle_dir.join("index.json");
    if index.is_file() {
        files.push(index);
    }
    collect_json_files(&bundle_dir.join("normalized"), &mut files)?;
    files.sort();
    files.dedup();
    if files.is_empty() {
        bail!("bundle contains no public JSON or JSONL evidence");
    }

    let mut findings = Vec::new();
    for path in files {
        validate_public_file(&bundle_dir, &path, &mut findings)?;
    }
    findings.sort();
    findings.dedup();

    if !findings.is_empty() {
        let rendered = findings
            .iter()
            .map(|finding| {
                format!(
                    "{}#{}: {} host path",
                    finding.logical_file,
                    finding.pointer,
                    finding.kind.as_str()
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        bail!(
            "public compiler evidence contains {} embedded host-path finding(s); private values were not echoed:\n{}",
            findings.len(),
            rendered
        );
    }

    println!(
        "public compiler evidence host-path validation passed for {}",
        bundle_dir.display()
    );
    Ok(())
}

fn collect_json_files(directory: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
    if !directory.exists() {
        return Ok(());
    }
    let mut entries = fs::read_dir(directory)
        .with_context(|| format!("reading evidence directory {}", directory.display()))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let path = entry.path();
        let file_type = entry
            .file_type()
            .with_context(|| format!("reading file type for {}", path.display()))?;
        if file_type.is_dir() {
            collect_json_files(&path, files)?;
        } else if file_type.is_file()
            && matches!(path.extension().and_then(|value| value.to_str()), Some("json" | "jsonl"))
        {
            files.push(path);
        }
    }
    Ok(())
}

fn validate_public_file(bundle_dir: &Path, path: &Path, findings: &mut Vec<Finding>) -> Result<()> {
    let logical_file = path
        .strip_prefix(bundle_dir)
        .with_context(|| format!("normalizing evidence path {}", path.display()))?
        .to_string_lossy()
        .replace('\\', "/");
    let raw = fs::read_to_string(path)
        .with_context(|| format!("reading public evidence {}", path.display()))?;

    if path.extension().and_then(|value| value.to_str()) == Some("jsonl") {
        let mut nonempty = 0usize;
        for (line_index, line) in raw.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            nonempty = nonempty.saturating_add(1);
            let value: Value = serde_json::from_str(line).with_context(|| {
                format!("decoding JSONL line {} in {}", line_index + 1, path.display())
            })?;
            scan_json_value(
                &value,
                &logical_file,
                &format!("/line/{}", line_index + 1),
                PublicStringClass::Ordinary,
                findings,
            );
        }
        if nonempty == 0 {
            bail!("public JSONL evidence is empty: {}", path.display());
        }
    } else {
        let value: Value = serde_json::from_str(&raw)
            .with_context(|| format!("decoding public JSON evidence {}", path.display()))?;
        scan_json_value(
            &value,
            &logical_file,
            "",
            PublicStringClass::Ordinary,
            findings,
        );
    }
    Ok(())
}

fn scan_json_value(
    value: &Value,
    logical_file: &str,
    pointer: &str,
    string_class: PublicStringClass,
    findings: &mut Vec<Finding>,
) {
    match value {
        Value::String(text) => {
            if let Some(kind) = host_path_kind(text, string_class) {
                findings.push(Finding {
                    logical_file: logical_file.to_string(),
                    pointer: pointer_or_root(pointer),
                    kind,
                });
            }
        }
        Value::Array(values) => {
            for (index, value) in values.iter().enumerate() {
                scan_json_value(
                    value,
                    logical_file,
                    &format!("{pointer}/{index}"),
                    string_class,
                    findings,
                );
            }
        }
        Value::Object(values) => {
            for (key, value) in values {
                let key_finding = host_path_kind(key, PublicStringClass::Ordinary);
                if let Some(kind) = key_finding {
                    findings.push(Finding {
                        logical_file: logical_file.to_string(),
                        pointer: pointer_or_root(pointer),
                        kind,
                    });
                }

                let segment = if key_finding.is_some() {
                    "<redacted-key>".to_string()
                } else {
                    escape_json_pointer(key)
                };
                let child_class = classify_field(key).unwrap_or(string_class);
                scan_json_value(
                    value,
                    logical_file,
                    &format!("{pointer}/{segment}"),
                    child_class,
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

fn classify_field(key: &str) -> Option<PublicStringClass> {
    match key.to_ascii_lowercase().as_str() {
        "source_text" | "source_code" | "source_snippet" | "perl_source" | "fixture_source"
        | "snippet" => Some(PublicStringClass::SourceText),
        "regex" | "pattern" | "regular_expression" => Some(PublicStringClass::Regex),
        _ => None,
    }
}

fn host_path_kind(text: &str, string_class: PublicStringClass) -> Option<HostPathKind> {
    if matches!(string_class, PublicStringClass::SourceText | PublicStringClass::Regex) {
        return None;
    }

    let decoded = decode_path_escapes(text);
    let lower = decoded.to_ascii_lowercase();
    if lower.contains("file:/") {
        return Some(HostPathKind::FileUri);
    }
    if contains_windows_drive_path(&decoded) {
        return Some(HostPathKind::WindowsDrive);
    }
    if contains_unc_path(&decoded) {
        return Some(HostPathKind::WindowsUnc);
    }
    if looks_like_perl_regex_literal(decoded.trim()) {
        return None;
    }
    contains_unix_absolute_path(&decoded).then_some(HostPathKind::UnixAbsolute)
}

fn decode_path_escapes(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut output = String::with_capacity(text.len());
    let mut index = 0usize;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len()
            && let (Some(high), Some(low)) = (hex_value(bytes[index + 1]), hex_value(bytes[index + 2]))
        {
            let decoded = (high << 4) | low;
            if matches!(decoded, b'/' | b'\\' | b':') {
                output.push(char::from(decoded));
                index += 3;
                continue;
            }
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

fn contains_windows_drive_path(text: &str) -> bool {
    let bytes = text.as_bytes();
    (0..bytes.len().saturating_sub(2)).any(|index| {
        bytes[index].is_ascii_alphabetic()
            && bytes[index + 1] == b':'
            && matches!(bytes[index + 2], b'/' | b'\\')
            && (index == 0 || is_path_boundary(bytes[index - 1]))
    })
}

fn contains_unc_path(text: &str) -> bool {
    let bytes = text.as_bytes();
    (0..bytes.len().saturating_sub(3)).any(|index| {
        let pair = &bytes[index..index + 2];
        let unc = pair == b"\\\\" || pair == b"//";
        if !unc || (index > 0 && bytes[index - 1] == b':') {
            return false;
        }
        let boundary = index == 0 || is_path_boundary(bytes[index - 1]);
        boundary
            && bytes[index + 2] != b'/'
            && bytes[index + 2] != b'\\'
            && bytes[index + 3] != b'/'
            && bytes[index + 3] != b'\\'
    })
}

fn contains_unix_absolute_path(text: &str) -> bool {
    let bytes = text.as_bytes();
    for index in 0..bytes.len() {
        if bytes[index] != b'/' || (index > 0 && !is_path_boundary(bytes[index - 1])) {
            continue;
        }
        if index + 1 >= bytes.len() || bytes[index + 1] == b'/' {
            continue;
        }
        let end = bytes[index..]
            .iter()
            .position(|byte| is_path_terminator(*byte))
            .map_or(bytes.len(), |offset| index + offset);
        if end > index + 1 {
            return true;
        }
    }
    false
}

fn looks_like_perl_regex_literal(value: &str) -> bool {
    if value.len() > 2 && value.starts_with('/') && value.ends_with('/') {
        return true;
    }
    ["m{", "qr{", "s{", "tr{"].iter().any(|prefix| value.starts_with(prefix))
        && value.ends_with('}')
}

const fn is_path_boundary(byte: u8) -> bool {
    matches!(
        byte,
        b' ' | b'\t' | b'\n' | b'\r' | b'=' | b':' | b'"' | b'\'' | b'(' | b'[' | b'{' | b','
    )
}

const fn is_path_terminator(byte: u8) -> bool {
    matches!(
        byte,
        b' ' | b'\t' | b'\n' | b'\r' | b'"' | b'\'' | b')' | b']' | b'}' | b',' | b';'
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    type TestResult = Result<()>;

    #[test]
    fn detects_attached_unix_windows_unc_and_uri_paths() {
        assert_eq!(
            host_path_kind(
                "--manifest-path=/home/runner/work/repo/Cargo.toml",
                PublicStringClass::Ordinary
            ),
            Some(HostPathKind::UnixAbsolute)
        );
        assert_eq!(
            host_path_kind(r"cwd:C:\Users\runner\repo", PublicStringClass::Ordinary),
            Some(HostPathKind::WindowsDrive)
        );
        assert_eq!(
            host_path_kind(
                r"source=\\server\share\repo\file.rs",
                PublicStringClass::Ordinary
            ),
            Some(HostPathKind::WindowsUnc)
        );
        assert_eq!(
            host_path_kind("file:///tmp/repo/report.json", PublicStringClass::Ordinary),
            Some(HostPathKind::FileUri)
        );
    }

    #[test]
    fn rejects_shallow_unknown_unix_roots() {
        for value in ["/etc/passwd", "/root/build", "/srv/cache", "arg=/opt/tool"] {
            assert_eq!(
                host_path_kind(value, PublicStringClass::Ordinary),
                Some(HostPathKind::UnixAbsolute),
                "absolute path escaped detection: {value}"
            );
        }
    }

    #[test]
    fn detects_nested_escaped_and_percent_encoded_paths() {
        assert_eq!(
            host_path_kind(
                r#"{"argument":"--root=/private/var/folders/build"}"#,
                PublicStringClass::Ordinary
            ),
            Some(HostPathKind::UnixAbsolute)
        );
        assert_eq!(
            host_path_kind("--root=%2Fhome%2Frunner%2Frepo", PublicStringClass::Ordinary),
            Some(HostPathKind::UnixAbsolute)
        );
        assert_eq!(
            host_path_kind(
                r#"{"cwd":"C:\\Users\\runner\\repo"}"#,
                PublicStringClass::Ordinary
            ),
            Some(HostPathKind::WindowsDrive)
        );
    }

    #[test]
    fn accepts_urls_logical_paths_and_reviewed_source_classes() {
        for value in [
            "https://example.com/a/b/c",
            "crates/perl-parser/src/lib.rs",
            "m{/foo/bar}",
            "qr{/foo/bar}",
            "/foo/bar/",
            "key=value:ordinary",
            "perl_core_harness.report.v1",
        ] {
            assert_eq!(
                host_path_kind(value, PublicStringClass::Ordinary),
                None,
                "unexpected host-path finding for {value}"
            );
        }
        assert_eq!(
            host_path_kind("open '/etc/passwd';", PublicStringClass::SourceText),
            None
        );
        assert_eq!(host_path_kind("/foo/bar", PublicStringClass::Regex), None);
    }

    #[test]
    fn scans_object_keys_without_echoing_them() -> TestResult {
        let value = serde_json::json!({
            "/home/runner/private.pl": {"status": "fail"},
            "C:\\Users\\runner\\private.pl": {"status": "fail"}
        });
        let mut findings = Vec::new();
        scan_json_value(
            &value,
            "normalized/report.json",
            "",
            PublicStringClass::Ordinary,
            &mut findings,
        );
        findings.sort();
        assert_eq!(findings.len(), 2);
        assert!(findings.iter().all(|finding| finding.pointer == "/"));
        let rendered = format!("{findings:?}");
        assert!(!rendered.contains("private.pl"));
        Ok(())
    }

    #[test]
    fn field_classes_do_not_exempt_failure_messages() {
        let value = serde_json::json!({
            "source_text": "open '/etc/passwd';",
            "regex": "/foo/bar/",
            "message": "failed while reading /etc/passwd"
        });
        let mut findings = Vec::new();
        scan_json_value(
            &value,
            "normalized/report.json",
            "",
            PublicStringClass::Ordinary,
            &mut findings,
        );
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].pointer, "/message");
    }

    #[test]
    fn scan_reports_json_pointer_without_echoing_private_value() -> TestResult {
        let value = serde_json::json!({
            "command": ["cargo", "--manifest-path=/home/runner/private/Cargo.toml"],
            "safe": "crates/perl-parser/src/lib.rs"
        });
        let mut findings = Vec::new();
        scan_json_value(
            &value,
            "normalized/reproduction.json",
            "",
            PublicStringClass::Ordinary,
            &mut findings,
        );
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].pointer, "/command/1");
        let rendered = format!(
            "{}#{}: {} host path",
            findings[0].logical_file,
            findings[0].pointer,
            findings[0].kind.as_str()
        );
        if rendered.contains("/home/runner/private") {
            bail!("public finding must not echo the private path");
        }
        Ok(())
    }

    #[test]
    fn bundle_validation_covers_json_and_jsonl() -> TestResult {
        let temp = tempfile::tempdir()?;
        let bundle = temp.path().join("bundle");
        let normalized = bundle.join("normalized");
        fs::create_dir_all(&normalized)?;
        fs::write(bundle.join("index.json"), "{\"logical_path\":\"normalized/report.json\"}\n")?;
        fs::write(
            normalized.join("report.json"),
            "{\"path\":\"crates/perl-parser/src/lib.rs\"}\n",
        )?;
        fs::write(
            normalized.join("records.jsonl"),
            "{\"argument\":\"--root=/tmp/private/build\"}\n",
        )?;

        let Err(error) = validate_public_bundle(&bundle) else {
            bail!("embedded JSONL host path must fail validation");
        };
        let message = error.to_string();
        assert!(message.contains("records.jsonl#/line/1/argument"));
        assert!(!message.contains("/tmp/private/build"));
        Ok(())
    }
}
