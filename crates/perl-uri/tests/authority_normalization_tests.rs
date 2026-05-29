//! Authority and edge-case coverage for `perl-uri` public APIs.

#[cfg(not(target_arch = "wasm32"))]
mod filesystem_authorities {
    use perl_tdd_support::{must, must_some};
    use perl_uri::{fs_path_to_uri, normalize_uri, source_path_from_uri_or_path, uri_to_fs_path};

    #[test]
    fn uri_to_fs_path_accepts_localhost_with_percent_encoded_components() {
        let path =
            must_some(uri_to_fs_path("file://localhost/tmp/path%20with%20spaces/Mod%20Name.pm"));
        let text = path.to_string_lossy();
        assert!(text.contains("path with spaces"), "directory not decoded: {text}");
        assert!(text.ends_with("Mod Name.pm"), "filename not decoded: {text}");
    }

    #[test]
    fn uri_to_fs_path_accepts_ipv4_loopback_with_unicode_path() {
        let path = must_some(uri_to_fs_path("file://127.0.0.1/tmp/caf%C3%A9.pm"));
        let text = path.to_string_lossy();
        assert!(text.ends_with("café.pm"), "unicode path not decoded: {text}");
    }

    #[test]
    fn uri_to_fs_path_maps_empty_local_authority_to_root() {
        let localhost = must_some(uri_to_fs_path("file://localhost"));
        let loopback = must_some(uri_to_fs_path("file://127.0.0.1"));
        assert!(
            localhost.is_absolute(),
            "localhost root was not absolute: {}",
            localhost.display()
        );
        assert!(loopback.is_absolute(), "loopback root was not absolute: {}", loopback.display());
    }

    #[test]
    fn normalize_uri_canonicalizes_loopback_authority_with_spaces() {
        let normalized = normalize_uri("file://127.0.0.1/tmp/path%20with%20spaces/script.pl");
        assert_eq!(normalized, "file:///tmp/path%20with%20spaces/script.pl");
    }

    #[test]
    fn source_path_from_uri_or_path_rejects_blank_input() {
        assert!(source_path_from_uri_or_path("   \n\t  ").is_none());
    }

    #[test]
    fn fs_path_to_uri_encodes_reserved_uri_delimiters_in_path_segments() {
        let dir = tempfile::tempdir().map_err(|e| e.to_string()).and_then(|dir| {
            let path = dir.path().join("name#fragment?query.pm");
            let uri = fs_path_to_uri(&path)?;
            Ok((dir, uri))
        });
        let (_dir, uri) = must(dir);
        assert!(
            uri.contains("name%23fragment%3Fquery.pm"),
            "reserved characters not encoded: {uri}"
        );
    }

    #[test]
    fn fs_path_to_uri_and_back_preserves_unicode_and_spaces() {
        let dir = must(tempfile::tempdir().map_err(|e| e.to_string()));
        let original = dir.path().join("space dir").join("café module.pm");
        let uri = must(fs_path_to_uri(&original));
        let recovered = must_some(uri_to_fs_path(&uri));
        assert_eq!(recovered, original);
    }
}

mod classification_authorities {
    use perl_uri::{is_file_uri, is_special_scheme, uri_extension, uri_key};

    #[test]
    fn uri_key_canonicalizes_loopback_authorities_and_preserves_suffixes() {
        assert_eq!(
            uri_key("file://127.0.0.1/tmp/module.pm?rev=1#L42"),
            "file:///tmp/module.pm?rev=1#L42"
        );
        assert_eq!(
            uri_key("file://[::1]/tmp/module.pm?rev=1#L42"),
            "file:///tmp/module.pm?rev=1#L42"
        );
    }

    #[test]
    fn uri_key_trims_file_uri_before_keying() {
        assert_eq!(uri_key("\n\tfile:///tmp/module.pm  "), "file:///tmp/module.pm");
    }

    #[test]
    fn is_file_uri_is_case_insensitive_but_requires_double_slash() {
        assert!(is_file_uri("FiLe://localhost/tmp/module.pm"));
        assert!(!is_file_uri("FiLe:/tmp/module.pm"));
    }

    #[test]
    fn special_scheme_fallbacks_are_case_insensitive_for_invalid_urls() {
        assert!(is_special_scheme("VSCODE-VFS:contains spaces that avoid authority parsing"));
        assert!(is_special_scheme("VSCODE-NOTEBOOK:contains spaces that avoid authority parsing"));
        assert!(is_special_scheme(
            "VSCODE-NOTEBOOK-CELL:contains spaces that avoid authority parsing"
        ));
    }

    #[test]
    fn uri_extension_ignores_query_before_fragment_when_both_are_present() {
        assert_eq!(uri_extension("file:///tmp/script.pl?download=.txt#frag.pm"), Some("pl"));
        assert_eq!(uri_extension("file:///tmp/script.pm#frag.pl?download=.txt"), Some("pm"));
    }
}
