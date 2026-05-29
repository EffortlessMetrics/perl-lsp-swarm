//! Authority and encoding regression receipts for `perl-uri`.
//!
//! These tests cover local-authority canonicalization and literal path-byte
//! handling that sit at the seam between `uri_key`, `normalize_uri`, and
//! filesystem conversion.

use perl_uri::{normalize_uri, uri_key};

#[cfg(not(target_arch = "wasm32"))]
use perl_uri::{fs_path_to_uri, source_path_from_uri_or_path, uri_to_fs_path};

#[test]
fn uri_key_strips_localhost_authority_without_dropping_query_or_fragment() {
    let key = uri_key("file://localhost/tmp/lib/Foo.pm?version=1#L12");
    assert_eq!(key, "file:///tmp/lib/Foo.pm?version=1#L12");
}

#[test]
fn uri_key_strips_ipv4_loopback_authority_without_dropping_query_or_fragment() {
    let key = uri_key("file://127.0.0.1/tmp/lib/Foo.pm?version=1#L12");
    assert_eq!(key, "file:///tmp/lib/Foo.pm?version=1#L12");
}

#[test]
fn uri_key_strips_ipv6_loopback_authority_without_dropping_query_or_fragment() {
    let key = uri_key("file://[::1]/tmp/lib/Foo.pm?version=1#L12");
    assert_eq!(key, "file:///tmp/lib/Foo.pm?version=1#L12");
}

#[test]
fn uri_key_preserves_non_local_authority_query_and_fragment() {
    let input = "file://example.com/share/Foo.pm?version=1#L12";
    assert_eq!(uri_key(input), input);
}

#[test]
fn normalize_uri_canonicalizes_loopback_authority_with_encoded_path() {
    let normalized = normalize_uri("file://127.0.0.1/tmp/path%20with%20spaces/Foo.pm");
    assert_eq!(normalized, "file:///tmp/path%20with%20spaces/Foo.pm");
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn uri_to_fs_path_ignores_lsp_query_and_fragment_components() -> Result<(), String> {
    let path = uri_to_fs_path("file:///tmp/lib/Foo.pm?version=1#L12")
        .ok_or("expected file URI to resolve")?;
    if !path.ends_with("Foo.pm") {
        return Err(format!(
            "expected filename without query or fragment, got: {}",
            path.display()
        ));
    }
    if path.to_string_lossy().contains("version=1") || path.to_string_lossy().contains("#L12") {
        return Err(format!("query or fragment leaked into path: {}", path.display()));
    }
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn fs_path_to_uri_percent_encodes_uri_delimiters_in_path_segments() -> Result<(), String> {
    let uri = fs_path_to_uri("/tmp/perl uri/name#with?delimiters%.pl")?;
    for expected in ["perl%20uri", "name%23with%3Fdelimiters%25.pl"] {
        if !uri.contains(expected) {
            return Err(format!("expected {expected} in encoded URI, got: {uri}"));
        }
    }
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn source_path_from_uri_or_path_rejects_authority_that_is_not_localhost() {
    assert!(source_path_from_uri_or_path("file://example.com/tmp/Foo.pm").is_none());
}
