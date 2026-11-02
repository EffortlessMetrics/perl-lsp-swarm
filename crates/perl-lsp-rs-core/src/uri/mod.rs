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

fn parse_uri_candidate(s: &str) -> Option<Uri> {
    let sanitized = s.trim_start_matches('\u{feff}').trim();

    if sanitized.is_empty() {
        return None;
    }

    if let Some(uri) = file_path_uri(sanitized).or_else(|| windows_file_path_uri(sanitized)) {
        return Some(uri);
    }

    sanitized.parse::<Uri>().ok().or_else(|| {
        Url::parse(sanitized)
            .ok()
            .and_then(|url| url.as_str().parse::<Uri>().ok())
    })
}

/// Parse a URI string without the guaranteed-valid fallback chain.
///
/// Returns `None` when the input is empty (after BOM/whitespace trimming) or
/// cannot be interpreted as a URI or absolute local file path. Workspace
/// adapters use this when an invalid folder URI should be skipped rather than
/// substituted (see `DELTA-URI-DEFAULT` / #14292).
#[must_use]
pub fn try_parse_uri(s: &str) -> Option<Uri> {
    parse_uri_candidate(s)
}

/// Strict parse for tests and callers that must observe failure as `Err`.
///
/// Propagates the same success cases as [`try_parse_uri`]; invalid input
/// becomes `Err` instead of a fallback URI or `None`.
pub fn parse_uri_or_err(s: &str) -> Result<Uri, UriStrictParseError> {
    parse_uri_candidate(s).ok_or(UriStrictParseError)
}

/// Error returned when [`parse_uri_or_err`] cannot interpret the input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UriStrictParseError;

/// Parse a URI string into [`lsp_types::Uri`].
///
/// Accepts valid URI strings and absolute local file paths (both Unix and
/// Windows styles). Falls back to a guaranteed-valid URI if parsing fails.
#[must_use]
pub fn parse_uri(s: &str) -> Uri {
    parse_uri_candidate(s).unwrap_or_else(fallback_uri)
}

#[cfg(test)]
mod uri_path_helpers_tests {
    use super::{file_path_uri, parse_uri, windows_file_path_uri};

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

    /// `windows_file_path_uri` returns None for an empty string because
    /// `chars.next()` returns None on the first call. Covers the first `?` guard.
    #[test]
    fn windows_file_path_uri_rejects_empty_string() {
        assert!(
            windows_file_path_uri("").is_none(),
            "empty string must not be accepted as a Windows URI"
        );
    }

    /// `windows_file_path_uri` returns None for a one-character string (e.g. `"C"`)
    /// because the second `chars.next()` returns None before the colon check.
    /// Covers the `chars.next()? != ':'` None branch.
    #[test]
    fn windows_file_path_uri_rejects_single_char_string() {
        assert!(
            windows_file_path_uri("C").is_none(),
            "single-char string must not be accepted as a Windows URI"
        );
    }

    /// `windows_file_path_uri` returns None for a two-character string (e.g. `"C:"`)
    /// because the third `chars.next()` returns None before the separator check.
    /// Covers the separator `chars.next()?` None branch (line 47).
    #[test]
    fn windows_file_path_uri_rejects_two_char_string() {
        assert!(
            windows_file_path_uri("C:").is_none(),
            "two-char string (drive + colon, no separator) must not be accepted as a Windows URI"
        );
    }

    /// `windows_file_path_uri` successfully converts a Windows path with backslash
    /// separators to a `file://` URI. Works on all platforms because the function
    /// manually constructs the URI string without relying on OS path APIs.
    /// Covers the success path (lines 52–54).
    #[test]
    fn windows_file_path_uri_accepts_backslash_path() -> Result<(), Box<dyn std::error::Error>> {
        let uri = windows_file_path_uri(r"C:\Users\dev\lib\Mod.pm")
            .ok_or("Windows backslash path must produce a URI")?;
        assert_eq!(
            uri.as_str(),
            "file:///C:/Users/dev/lib/Mod.pm",
            "backslash path must be normalised to forward slashes in the URI"
        );
        Ok(())
    }

    /// `windows_file_path_uri` successfully converts a Windows path with forward-slash
    /// separators to a `file://` URI. Covers the forward-slash separator branch.
    #[test]
    fn windows_file_path_uri_accepts_forward_slash_path() -> Result<(), Box<dyn std::error::Error>>
    {
        let uri = windows_file_path_uri("C:/Users/dev/lib/Mod.pm")
            .ok_or("Windows forward-slash path must produce a URI")?;
        assert_eq!(
            uri.as_str(),
            "file:///C:/Users/dev/lib/Mod.pm",
            "forward-slash Windows path must produce the correct file:// URI"
        );
        Ok(())
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

    /// `file_path_uri` successfully converts a Unix absolute path on Unix hosts.
    /// Covers the success path through `Url::from_file_path` → `parse::<Uri>`.
    #[cfg(unix)]
    #[test]
    fn file_path_uri_accepts_unix_absolute_path() -> Result<(), Box<dyn std::error::Error>> {
        let uri = file_path_uri("/tmp/lib/Mod.pm")
            .ok_or("Unix absolute path must produce a file:// URI")?;
        assert_eq!(
            uri.as_str(),
            "file:///tmp/lib/Mod.pm",
            "Unix absolute path must round-trip to the expected file:// URI"
        );
        Ok(())
    }

    /// `parse_uri` routes Windows bare paths through the path-detection branch
    /// (lines 69–71), returning a `file://` URI without going through
    /// `sanitized.parse::<Uri>()`. Works on all platforms.
    /// Covers the `if let Some(uri) = ... { return uri; }` branch.
    #[test]
    fn parse_uri_routes_windows_path_through_file_path_detection() {
        let uri = parse_uri(r"C:\workspace\lib\Mod.pm");
        assert_eq!(
            uri.as_str(),
            "file:///C:/workspace/lib/Mod.pm",
            "parse_uri must convert a Windows path to a file:// URI via path-detection"
        );
    }

    /// `parse_uri` routes Unix absolute paths through the file_path_uri branch on Unix.
    /// Covers the same `if let Some(uri)` branch but via the Unix helper.
    #[cfg(unix)]
    #[test]
    fn parse_uri_routes_unix_path_through_file_path_detection() {
        let uri = parse_uri("/workspace/lib/Mod.pm");
        assert_eq!(
            uri.as_str(),
            "file:///workspace/lib/Mod.pm",
            "parse_uri must convert a Unix absolute path to a file:// URI via path-detection"
        );
    }
}

#[cfg(test)]
mod uri_parse_gate_tests {
    use super::{parse_uri, parse_uri_or_err, try_parse_uri, UriStrictParseError};
    use lsp_types::Uri;

    /// Each literal in the ADR-0037 fallback chain must remain parseable so
    /// `parse_uri` can always return a wire-safe URI.
    #[test]
    fn fallback_literal_candidates_are_parseable() {
        for candidate in [
            "file:///unknown",
            "file:///",
            "about:blank",
            "urn:perl-lsp:unknown",
        ] {
            assert!(
                candidate.parse::<Uri>().is_ok(),
                "fallback candidate {candidate} must parse as Uri"
            );
        }
    }

    #[test]
    fn parse_uri_invalid_input_uses_first_fallback_literal() {
        assert_eq!(
            parse_uri("not a valid uri!!!").as_str(),
            "file:///unknown",
            "invalid input must use the first successful fallback literal"
        );
    }

    #[test]
    fn fallback_localhost_last_resort_literal_is_valid() {
        assert!(
            "http://localhost/0".parse::<Uri>().is_ok(),
            "http://localhost/N loop must have a parseable last-resort candidate"
        );
    }

    #[test]
    fn try_parse_uri_returns_none_for_empty_or_invalid_workspace_string() {
        assert!(try_parse_uri("").is_none());
        assert!(try_parse_uri("   ").is_none());
        assert!(try_parse_uri("not a valid uri!!!").is_none());
    }

    #[test]
    fn parse_uri_or_err_returns_err_for_invalid_test_input() {
        assert_eq!(
            parse_uri_or_err("not a valid uri!!!"),
            Err(UriStrictParseError)
        );
        assert!(parse_uri_or_err("file:///tmp/test.pl").is_ok());
    }
}
