//! Typed URI parsing helpers for LSP components.
//!
//! Previously the standalone `perl-lsp-uri` crate; absorbed into
//! `perl-lsp-rs-core::uri` in Wave G3 (#4535).

use gen_lsp_types::Uri;
use std::path::Path;
use url::Url;

fn fallback_uri() -> Uri {
    // The selected substrate's `Uri` is String-backed (no construction-time
    // parse); validity is enforced here through `url::Url` before wrapping.
    for candidate in ["file:///unknown", "file:///", "about:blank", "urn:perl-lsp:unknown"] {
        if Url::parse(candidate).is_ok() {
            return Uri(candidate.to_string());
        }
    }

    // Last-resort fallback that avoids panicking if URI parser behavior changes unexpectedly.
    let mut suffix = 0usize;
    loop {
        let candidate = format!("http://localhost/{suffix}");
        if Url::parse(&candidate).is_ok() {
            return Uri(candidate);
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
    Some(Uri(url.as_str().to_string()))
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
    Some(Uri(url.as_str().to_string()))
}

/// Parse a URI string into [`gen_lsp_types::Uri`].
///
/// Accepts valid URI strings and absolute local file paths (both Unix and
/// Windows styles). Falls back to a guaranteed-valid URI if parsing fails.
///
/// The selected substrate's `Uri` is String-backed, so validity is enforced
/// through an explicit `url::Url` validation gate (DELTA-URI-DEFAULT,
/// protocol-type-substrate-matrix §5) while the caller's spelling is
/// preserved byte-for-byte. Returning `Url::parse`'s canonical form instead
/// would silently rewrite dot-segments and percent-escapes (e.g.
/// `file:///a/../b.pl` → `file:///b.pl`) and change document identity on the
/// wire; the incumbent lsp-types adapter preserved client spelling. Final
/// URI ruling stays with LT03 (#8156/#8484).
#[must_use]
pub fn parse_uri(s: &str) -> Uri {
    let sanitized = s.trim_start_matches('\u{feff}').trim();

    if sanitized.is_empty() {
        return fallback_uri();
    }

    if let Some(uri) = file_path_uri(sanitized).or_else(|| windows_file_path_uri(sanitized)) {
        return uri;
    }

    match Url::parse(sanitized) {
        Ok(_) => Uri(sanitized.to_string()),
        Err(_) => fallback_uri(),
    }
}

#[cfg(test)]
mod uri_path_helpers_tests {
    use super::{Url, file_path_uri, parse_uri, windows_file_path_uri};

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
            uri.as_ref(),
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
            uri.as_ref(),
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
            uri.as_ref(),
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
            uri.as_ref(),
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
            uri.as_ref(),
            "file:///workspace/lib/Mod.pm",
            "parse_uri must convert a Unix absolute path to a file:// URI via path-detection"
        );
    }

    /// Regression guard (#11803 repair): a validated URI keeps its dot-segments
    /// exactly as received. The `Url::parse` gate is validation-only; letting it
    /// canonicalize would silently rewrite `file:///workspace/a/../lib/Mod.pm`
    /// into `file:///workspace/lib/Mod.pm` and change document identity on the
    /// wire (incumbent lsp-types behavior preserved spelling).
    #[test]
    fn parse_uri_preserves_dot_segments() {
        let uri = parse_uri("file:///workspace/a/../lib/Mod.pm");
        assert_eq!(
            uri.as_ref(),
            "file:///workspace/a/../lib/Mod.pm",
            "validated URI spelling must be preserved instead of Url-canonicalized"
        );
    }

    /// Percent-escape spelling is preserved as well (`Url::parse` decodes
    /// unreserved escapes such as `%7e`); clients key documents on the bytes
    /// they sent, so recanonicalization would split one document into two
    /// identities across client and server.
    #[test]
    fn parse_uri_preserves_percent_escape_spelling() {
        let uri = parse_uri("file:///lib/%7emod.pm");
        assert_eq!(
            uri.as_ref(),
            "file:///lib/%7emod.pm",
            "percent-escape spelling must be preserved instead of being recanonicalized"
        );
    }

    /// The `Url::parse` gate still rejects garbage: input that fails validation
    /// falls back to a guaranteed-valid URI instead of being echoed verbatim.
    #[test]
    fn parse_uri_falls_back_on_invalid_uri() {
        let invalid = "not a uri ::";
        let uri = parse_uri(invalid);
        assert_ne!(
            uri.as_ref(),
            invalid,
            "input failing the validation gate must not be echoed verbatim"
        );
        assert!(
            Url::parse(uri.as_ref()).is_ok(),
            "fallback result must itself pass the validation gate"
        );
    }
}
