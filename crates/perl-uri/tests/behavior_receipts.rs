//! Behavioral receipts for public `perl-uri` conversion seams.
//!
//! These tests focus on cross-function behavior that LSP callers depend on:
//! stable lookup keys for local authorities, path/URI round-trips, and graceful
//! rejection of inputs that cannot name a local source file.

use perl_uri::uri_key;
#[cfg(not(target_arch = "wasm32"))]
use perl_uri::{fs_path_to_uri, normalize_uri, source_path_from_uri_or_path, uri_to_fs_path};

#[test]
fn uri_key_loopback_authorities_preserve_query_and_fragment() -> Result<(), String> {
    assert_eq!(
        uri_key("file://localhost/tmp/lib/My/Module.pm?rev=2#L10"),
        "file:///tmp/lib/My/Module.pm?rev=2#L10"
    );
    assert_eq!(
        uri_key("file://127.0.0.1/tmp/script.pl?debug=1#main"),
        "file:///tmp/script.pl?debug=1#main"
    );
    assert_eq!(
        uri_key("file://[::1]/tmp/t/uri.t?case=ipv6#assertion"),
        "file:///tmp/t/uri.t?case=ipv6#assertion"
    );
    Ok(())
}

#[test]
fn uri_key_canonicalizes_legacy_drive_pipe_in_standard_file_uri() -> Result<(), String> {
    assert_eq!(
        uri_key("file:///E|/workspace/My App/lib/Widget.pm"),
        "file:///e:/workspace/My%20App/lib/Widget.pm"
    );
    Ok(())
}

#[test]
fn uri_key_preserves_non_local_file_authority_with_query_fragment() -> Result<(), String> {
    assert_eq!(
        uri_key("file://remote-host/share/Module.pm?rev=2#symbol"),
        "file://remote-host/share/Module.pm?rev=2#symbol"
    );
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn fs_path_to_uri_relative_path_round_trips_through_current_dir() -> Result<(), String> {
    let relative = std::path::Path::new("crates/perl-uri/tests/behavior_receipts.rs");
    let uri = fs_path_to_uri(relative)?;
    let recovered = uri_to_fs_path(&uri).ok_or("relative URI did not convert back to a path")?;
    let expected = std::env::current_dir()
        .map_err(|e| format!("failed to read current directory: {e}"))?
        .join(relative);

    if recovered != expected {
        return Err(format!(
            "round-trip mismatch: recovered={}, expected={}",
            recovered.display(),
            expected.display()
        ));
    }

    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn uri_to_fs_path_accepts_legacy_windows_file_uri() -> Result<(), String> {
    let path = uri_to_fs_path(r"file://C:\Users\dev\script.pl")
        .ok_or("legacy Windows file URI did not convert to a path")?;
    let path_text = path.to_string_lossy();

    if !path_text.contains("Users") || !path.ends_with("script.pl") {
        return Err(format!("unexpected legacy Windows path: {}", path.display()));
    }

    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn uri_to_fs_path_rejects_malformed_non_legacy_uri() -> Result<(), String> {
    assert!(uri_to_fs_path("://not a uri").is_none());
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn source_path_from_uri_or_path_rejects_blank_and_special_inputs() -> Result<(), String> {
    assert!(source_path_from_uri_or_path("   \n\t  ").is_none());
    assert!(source_path_from_uri_or_path("untitled:Untitled-1").is_none());
    assert!(source_path_from_uri_or_path("vscode-vfs://github/owner/repo/lib.pm").is_none());
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn normalize_uri_handles_legacy_unc_share_roots() -> Result<(), String> {
    assert_eq!(normalize_uri(r"\\server\share"), "file://server/share");
    assert_eq!(normalize_uri(r"file://\\server\share"), "file://server/share");
    Ok(())
}
