#![deny(clippy::map_err_ignore)]
// Cohort C1 activation (#12598): all production rows exact-excepted; new findings move the crate back to non-C1.
//! Targeted coverage tests for `perl-uri` gaps surfaced by `cargo llvm-cov`.
//!
//! Each test references the specific code path it exercises so that
//! future refactors can update both the implementation and its receipt
//! together.

use perl_uri::uri_key;

// ── classify::normalize_unc_path_to_key (UNC bare server/share) ──────
//
// `classify.rs:101-115` returns `Some("file://{server}/{share}")` when a
// UNC payload has no path segments after the share. The existing tests
// in `classify_boundary_cases.rs` exercise UNC paths with a trailing
// file segment but never a bare `\\server\share` form.

#[test]
fn uri_key_bare_unc_path_without_trailing_path() {
    // Pure UNC with no path past the share name.
    assert_eq!(uri_key(r"\\server\share"), "file://server/share");
}

#[test]
fn uri_key_bare_unc_path_via_legacy_two_slash_form() {
    assert_eq!(uri_key(r"file://\\server\share"), "file://server/share");
}

#[test]
fn uri_key_bare_unc_path_with_trailing_separator() {
    // Trailing separator collapses to an empty segment and is filtered out,
    // so this still lands in the "no rest" branch.
    assert_eq!(uri_key(r"\\server\share\"), "file://server/share");
}

// ── classify::normalize_windows_path_to_key (length guard) ──────────
//
// `classify.rs:120-127` early-returns `None` for path strings shorter
// than three bytes after slash stripping. Without an explicit test the
// guard registers as an unhit branch, and a future "drop the length
// check" refactor would not break any existing assertions.

#[test]
fn uri_key_drive_only_short_form_is_returned_as_is() {
    // A drive-relative compatibility input retains its exact spelling.
    let key = uri_key("C:");
    assert_eq!(key, "C:");
}

#[test]
fn uri_key_single_letter_input_falls_through() {
    // Too short for Windows path or UNC pre-pass; not a valid URL either.
    assert_eq!(uri_key("X"), "X");
}

#[test]
fn uri_key_empty_after_file_double_slash_falls_through() {
    // `file://` strips to empty, which is shorter than 3 chars — both
    // helpers refuse it, then Url::parse handles it cleanly.
    assert_eq!(uri_key("file://"), "file:///");
}

// Drive-relative inputs cannot acquire an absolute file identity.

#[test]
fn uri_key_windows_path_missing_separator_after_colon() {
    assert_eq!(uri_key("C:foo"), "C:foo");
}

#[test]
fn uri_key_windows_path_missing_separator_with_pipe_drive() {
    assert_eq!(uri_key("C|foo"), "C|foo");
}

#[test]
fn uri_key_windows_path_missing_separator_via_two_slash_form() {
    assert_eq!(uri_key("file://C:foo"), "file://C:foo");
}

#[test]
fn uri_key_windows_drive_roots_keep_directory_separator() {
    // Drive-root inputs force the `path.is_empty()` branch after the drive
    // prefix is stripped. The canonical key must keep the trailing slash so it
    // remains distinct from the invalid/relative `C:` form covered above.
    assert_eq!(uri_key(r"C:\"), "file:///c:/");
    assert_eq!(uri_key("D:/"), "file:///d:/");
    assert_eq!(uri_key(r"file://E:\"), "file:///e:/");
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn normalize_uri_windows_drive_roots_keep_directory_separator() {
    // `normalize_uri` takes the same legacy pre-pass as `uri_key`; lock the
    // root-drive edge case at the public normalization entry point too.
    assert_eq!(perl_uri::normalize_uri(r"C:\"), "file:///c:/");
    assert_eq!(perl_uri::normalize_uri("D:/"), "file:///d:/");
    assert_eq!(perl_uri::normalize_uri(r"file://E:\"), "file:///e:/");
}

#[test]
fn uri_key_loopback_authority_without_path_canonicalizes_to_local_root() {
    // Local file authorities with no explicit path parse as `/`; after the
    // authority is stripped they should share the canonical local-root key.
    assert_eq!(uri_key("file://localhost"), "file:///");
    assert_eq!(uri_key("file://127.0.0.1"), "file:///");
    assert_eq!(uri_key("file://[::1]"), "file:///");
}

// ── lib::windows_rooted_file_uri_to_path (non-Windows stub) ─────────
//
// On non-Windows targets the helper at `lib.rs:198-201` is a stub that
// always returns `None`. It is reachable from `uri_to_fs_path` only
// when `url::Url::to_file_path` itself fails, e.g. for file URIs with a
// non-localhost host (which the URL crate strips for `localhost`).

#[cfg(all(not(target_arch = "wasm32"), not(windows)))]
mod non_windows_stub {
    use perl_uri::uri_to_fs_path;

    #[test]
    fn non_localhost_file_host_returns_none() {
        // `Url::to_file_path()` returns `Err(())` for non-localhost hosts
        // on Unix; the fallback then exercises the non-Windows stub.
        assert!(uri_to_fs_path("file://example.com/foo.pl").is_none());
    }

    #[test]
    fn ipv4_file_host_maps_to_local_path() -> Result<(), String> {
        let path = uri_to_fs_path("file://127.0.0.1/foo.pl").ok_or("expected Some")?;
        if !path.ends_with("foo.pl") {
            return Err(format!("unexpected path: {}", path.display()));
        }
        Ok(())
    }

    #[test]
    fn explicit_server_share_file_uri_returns_none() {
        assert!(uri_to_fs_path("file://server/share/file.pl").is_none());
    }
}

// Literal path data remains unchanged after standards-based URI decoding.
#[cfg(not(target_arch = "wasm32"))]
mod literal_path_bytes {
    use perl_uri::uri_to_fs_path;

    #[cfg(unix)]
    #[test]
    fn non_utf8_path_bytes_pass_through_unchanged() -> Result<(), String> {
        use std::os::unix::ffi::OsStrExt;
        let path = uri_to_fs_path("file:///tmp/%FF.pl").ok_or("expected Some")?;
        assert_eq!(path.as_os_str().as_bytes(), b"/tmp/\xff.pl");
        Ok(())
    }

    #[test]
    fn literal_marker_and_control_codepoints_are_not_repaired() -> Result<(), String> {
        for (uri, name) in [
            ("file:///%C3%83%E4%B8%AD.pl", "Ã中.pl"),
            ("file:///%C3%83a.pl", "Ãa.pl"),
            ("file:///%C3%83%C2%83.pl", "Ã\u{0083}.pl"),
        ] {
            let expected =
                std::path::PathBuf::from(std::path::MAIN_SEPARATOR.to_string()).join(name);
            let path = uri_to_fs_path(uri).ok_or("expected Some")?;
            assert_eq!(path, expected);
        }
        Ok(())
    }
}

// ── lib::normalize_uri (non-URL relative path branch) ───────────────
//
// `lib.rs:309-313` handles the "input is not a valid URL but we can
// still treat it as a filesystem path" branch. The existing relative
// path test in `additional_unit_tests.rs` exercises this, but we add a
// deterministic assertion here so the branch's contract is locked in.

#[cfg(not(target_arch = "wasm32"))]
mod normalize_uri_relative_path {
    use perl_uri::normalize_uri;

    #[test]
    fn relative_path_segment_is_resolved_against_cwd() {
        // `Url::parse` rejects relative URLs without a base, so we fall
        // through to `fs_path_to_uri`, which joins with the current
        // working directory and emits a `file://` URI.
        let result = normalize_uri("subdir/module.pm");
        assert!(
            result.starts_with("file:///"),
            "expected file:// URI for relative path, got: {result}"
        );
        assert!(result.ends_with("subdir/module.pm"), "tail missing: {result}");
    }

    #[test]
    fn bare_filename_resolves_against_cwd() {
        let result = normalize_uri("README.md");
        assert!(result.starts_with("file:///"), "got: {result}");
        assert!(result.ends_with("README.md"), "tail missing: {result}");
    }

    #[test]
    fn non_url_unrecognised_scheme_string_stays_url_serialised() {
        // `Url::parse` accepts `foo:bar` (opaque) — so we exit through
        // the URL branch and the input is returned canonicalised.
        let result = normalize_uri("foo:bar");
        // The `url` crate normalises by re-serialising; either prefix is
        // acceptable so long as the opaque payload survives.
        assert!(result.contains("bar"), "lost payload: {result}");
    }
}
