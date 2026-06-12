//! Typed URI parsing helpers for LSP components.
//!
//! Previously the standalone `perl-lsp-uri` crate; absorbed into
//! `perl-lsp-rs-core::uri` in Wave G3 (#4535).

use lsp_types::Uri;
use std::path::Path;
use url::Url;

fn fallback_uri() -> Uri {
    for candidate in ["file:///unknown", "file:///", "about:blank", "urn:perl-lsp:unknown"] {
        if let Ok(uri) = candidate.parse::<Uri>() {
            return uri;
        }
    }

    // Last-resort fallback that avoids panicking if URI parser behavior changes unexpectedly.
    let mut suffix = 0usize;
    loop {
        let candidate = format!("http://localhost/{suffix}");
        if let Ok(uri) = candidate.parse::<Uri>() {
            return uri;
        }
        suffix = suffix.saturating_add(1);
    }
}

/// Convert a Unix/POSIX absolute file path to a `file://` URI.
///
/// Returns `None` if the path is not recognized as an absolute path by
/// [`Url::from_file_path`].
fn file_path_uri(s: &str) -> Option<Uri> {
    let url = Url::from_file_path(Path::new(s)).ok()?;
    url.as_str().parse::<Uri>().ok()
}

/// Convert a Windows absolute file path (e.g. `C:\foo\bar.pm`) to a `file://` URI.
///
/// Accepts both backslash and forward-slash separators. Returns `None` if the
/// input does not look like a Windows drive-letter path.
fn windows_file_path_uri(s: &str) -> Option<Uri> {
    let mut chars = s.chars();
    let drive = chars.next()?;
    if !drive.is_ascii_alphabetic() || chars.next()? != ':' {
        return None;
    }
    let separator = chars.next()?;
    if separator != '\\' && separator != '/' {
        return None;
    }

    let normalized = s.replace('\\', "/");
    let url = Url::parse(&format!("file:///{normalized}")).ok()?;
    url.as_str().parse::<Uri>().ok()
}

/// Parse a URI string into [`lsp_types::Uri`].
///
/// Accepts valid URI strings and absolute local file paths (both Unix and
/// Windows styles). Falls back to a guaranteed-valid URI if parsing fails.
#[must_use]
pub fn parse_uri(s: &str) -> Uri {
    let sanitized = s.trim_start_matches('\u{feff}').trim();

    if sanitized.is_empty() {
        return fallback_uri();
    }

    if let Some(uri) = file_path_uri(sanitized).or_else(|| windows_file_path_uri(sanitized)) {
        return uri;
    }

    match sanitized.parse::<Uri>() {
        Ok(uri) => uri,
        Err(_) => Url::parse(sanitized)
            .ok()
            .and_then(|url| url.as_str().parse::<Uri>().ok())
            .unwrap_or_else(fallback_uri),
    }
}

#[cfg(test)]
mod uri_path_helpers_tests {
    use super::{file_path_uri, windows_file_path_uri};

    /// `windows_file_path_uri` rejects drive-relative paths (drive + colon but no
    /// separator), e.g. `C:relative` — these are NOT absolute Windows paths.
    /// Covers the `separator != '\' && separator != '/'` early-return branch.
    #[test]
    fn windows_file_path_uri_rejects_drive_relative_path() {
        // `C:relative` has the drive+colon prefix but the third character is not
        // a separator, so the function must return None.
        assert!(
            windows_file_path_uri("C:relative").is_none(),
            "drive-relative path must not be accepted as an absolute Windows URI"
        );
    }

    /// `windows_file_path_uri` rejects strings whose first character is not
    /// ASCII-alphabetic (e.g. a digit, punctuation, or a Unix-path `/`).
    /// Covers the `!drive.is_ascii_alphabetic()` branch of the early-return guard.
    #[test]
    fn windows_file_path_uri_rejects_non_alpha_first_char() {
        assert!(
            windows_file_path_uri("1:/foo").is_none(),
            "digit-prefixed string must not be accepted as a Windows URI"
        );
        assert!(
            windows_file_path_uri("/etc/passwd").is_none(),
            "Unix-path starting with '/' must not be accepted as a Windows URI"
        );
    }

    /// `file_path_uri` returns None for paths that are not recognized as absolute
    /// by `Url::from_file_path` (e.g. relative paths and Windows drive-relative
    /// paths on Unix).
    #[test]
    fn file_path_uri_returns_none_for_non_absolute_path() {
        assert!(
            file_path_uri("relative/path.pm").is_none(),
            "relative path must not produce a file:// URI"
        );
    }
}
