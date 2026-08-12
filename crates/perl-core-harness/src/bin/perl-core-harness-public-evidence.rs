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
    let normalized = bundle_dir.join("normalized");
    collect_json_files(&normalized, &mut files)?;
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
                findings,
            );
        }
        if nonempty == 0 {
            bail!("public JSONL evidence is empty: {}", path.display());
        }
    } else {
        let value: Value = serde_json::from_str(&raw)
            .with_context(|| format!("decoding public JSON evidence {}", path.display()))?;
        scan_json_value(&value, &logical_file, "", findings);
    }
    Ok(())
}

fn scan_json_value(value: &Value, logical_file: &str, pointer: &str, findings: &mut Vec<Finding>) {
    match value {
        Value::String(text) => {
            if let Some(kind) = host_path_kind(text) {
                findings.push(Finding {
                    logical_file: logical_file.to_string(),
                    pointer: if pointer.is_empty() { "/".to_string() } else { pointer.to_string() },
                    kind,
                });
            }
        }
        Value::Array(values) => {
            for (index, value) in values.iter().enumerate() {
                scan_json_value(value, logical_file, &format!("{pointer}/{index}"), findings);
            }
        }
        Value::Object(values) => {
            for (key, value) in values {
                let escaped = key.replace('~', "~0").replace('/', "~1");
                scan_json_value(value, logical_file, &format!("{pointer}/{escaped}"), findings);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

fn host_path_kind(text: &str) -> Option<HostPathKind> {
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
    const HOST_ROOTS: &[&str] = &[
        "/home/",
        "/tmp/",
        "/private/",
        "/users/",
        "/var/folders/",
        "/mnt/",
        "/workspace/",
        "/github/",
        "/__w/",
        "/opt/hostedtoolcache/",
        "/runner/",
    ];

    let lower = text.to_ascii_lowercase();
    if HOST_ROOTS.iter().any(|prefix| contains_at_path_boundary(&lower, prefix)) {
        return true;
    }

    let bytes = text.as_bytes();
    for index in 0..bytes.len() {
        if bytes[index] != b'/' || (index > 0 && !is_path_boundary(bytes[index - 1])) {
            continue;
        }
        if index + 1 < bytes.len() && bytes[index + 1] == b'/' {
            continue;
        }
        if is_regex_brace_prefix(bytes, index) {
            continue;
        }
        let end = bytes[index..]
            .iter()
            .position(|byte| is_path_terminator(*byte))
            .map_or(bytes.len(), |offset| index + offset);
        let candidate = &text[index..end];
        if candidate.ends_with('/') {
            continue;
        }
        let slash_count = candidate.bytes().filter(|byte| *byte == b'/').count();
        let has_path_signal = candidate.contains('.') || candidate.contains('_') || candidate.contains('-');
        if slash_count >= 3 || (slash_count >= 2 && has_path_signal) {
            return true;
        }
    }
    false
}

fn contains_at_path_boundary(text: &str, needle: &str) -> bool {
    let mut search_from = 0usize;
    while let Some(relative) = text[search_from..].find(needle) {
        let index = search_from + relative;
        if index == 0 || text.as_bytes().get(index.wrapping_sub(1)).is_some_and(|byte| is_path_boundary(*byte)) {
            return true;
        }
        search_from = index.saturating_add(1);
    }
    false
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

fn is_regex_brace_prefix(bytes: &[u8], slash_index: usize) -> bool {
    if slash_index == 0 || bytes[slash_index - 1] != b'{' {
        return false;
    }
    let prefix = &bytes[..slash_index - 1];
    prefix.ends_with(b"m") || prefix.ends_with(b"qr") || prefix.ends_with(b"s") || prefix.ends_with(b"tr")
}

#[cfg(test)]
mod tests {
    use super::*;

    type TestResult = Result<()>;

    #[test]
    fn detects_attached_unix_windows_unc_and_uri_paths() {
        assert_eq!(host_path_kind("--manifest-path=/home/runner/work/repo/Cargo.toml"), Some(HostPathKind::UnixAbsolute));
        assert_eq!(host_path_kind(r"cwd:C:\Users\runner\repo"), Some(HostPathKind::WindowsDrive));
        assert_eq!(host_path_kind(r"source=\\server\share\repo\file.rs"), Some(HostPathKind::WindowsUnc));
        assert_eq!(host_path_kind("file:///tmp/repo/report.json"), Some(HostPathKind::FileUri));
    }

    #[test]
    fn detects_nested_escaped_and_percent_encoded_paths() {
        assert_eq!(
            host_path_kind(r#"{"argument":"--root=/private/var/folders/build"}"#),
            Some(HostPathKind::UnixAbsolute)
        );
        assert_eq!(
            host_path_kind("--root=%2Fhome%2Frunner%2Frepo"),
            Some(HostPathKind::UnixAbsolute)
        );
        assert_eq!(
            host_path_kind(r#"{"cwd":"C:\\Users\\runner\\repo"}"#),
            Some(HostPathKind::WindowsDrive)
        );
    }

    #[test]
    fn accepts_urls_logical_paths_regexes_and_ordinary_text() {
        for value in [
            "https://example.com/a/b/c",
            "crates/perl-parser/src/lib.rs",
            "m{/foo/bar}",
            "qr{/foo/bar}",
            "/foo/bar/",
            "key=value:ordinary",
            "perl_core_harness.report.v1",
        ] {
            assert_eq!(host_path_kind(value), None, "unexpected host-path finding for {value}");
        }
    }

    #[test]
    fn scan_reports_json_pointer_without_echoing_private_value() -> TestResult {
        let value = serde_json::json!({
            "command": ["cargo", "--manifest-path=/home/runner/private/Cargo.toml"],
            "safe": "crates/perl-parser/src/lib.rs"
        });
        let mut findings = Vec::new();
        scan_json_value(&value, "normalized/reproduction.json", "", &mut findings);
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
        fs::write(normalized.join("report.json"), "{\"path\":\"crates/perl-parser/src/lib.rs\"}\n")?;
        fs::write(normalized.join("records.jsonl"), "{\"argument\":\"--root=/tmp/private/build\"}\n")?;

        let Err(error) = validate_public_bundle(&bundle) else {
            bail!("embedded JSONL host path must fail validation");
        };
        let message = error.to_string();
        assert!(message.contains("records.jsonl#/line/1/argument"));
        assert!(!message.contains("/tmp/private/build"));
        Ok(())
    }
}
