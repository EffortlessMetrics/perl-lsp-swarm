//! Exact-source corrections owned by #8156. These are focused regressions,
//! not the wider URI/path fixture registry planned in #8470.

use perl_uri::{normalize_uri, uri_key};

#[test]
fn drive_relative_compatibility_inputs_keep_their_original_identity() {
    for input in [
        "C:",
        "C:lib/Foo.pm",
        r"c:lib\Foo.pm",
        "D|lib/Foo.pm",
        "D|",
        "file://C:",
        "file://C:lib/Foo.pm",
        "file://localhost/C:lib/Foo.pm",
        r"FILE://LOCALHOST\C:lib\Foo.pm",
    ] {
        assert_eq!(uri_key(input), input, "key promoted {input:?}");
        assert_eq!(normalize_uri(input), input, "normalizer promoted {input:?}");
    }
}

#[test]
fn drive_absolute_and_non_drive_controls_retain_their_meaning() {
    for input in ["C:/lib/Foo.pm", r"C:\lib\Foo.pm", "file://C:/lib/Foo.pm"] {
        assert_eq!(uri_key(input), "file:///c:/lib/Foo.pm");
        assert_eq!(normalize_uri(input), "file:///c:/lib/Foo.pm");
    }
    assert_eq!(uri_key("file:///C:/lib/Foo.pm"), "file:///c:/lib/Foo.pm");
    for input in [
        "untitled:buffer-1",
        "custom:opaque",
        "x:opaque",
        "https://example.com/a:b",
        "./C:relative-name.pm",
        "/tmp/C:literal-name.pm",
        "relative:name.pm",
    ] {
        assert_eq!(uri_key(input), input, "non-drive identity changed for {input:?}");
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod native {
    #[cfg(unix)]
    use std::path::Path;

    use perl_uri::{fs_path_to_uri, source_path_from_uri_or_path, uri_to_fs_path};

    #[test]
    fn decoded_literal_names_are_preserved_exactly() -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        for name in [
            "cafÃ©.pl",
            "café.pl",
            "Â£.pl",
            "â.pl",
            "ð.pl",
            "�.pl",
            "Ã中.pl",
            "plain-module.pl",
            "space % # ?.pl",
        ] {
            let original = directory.path().join(name);
            let uri = url::Url::from_file_path(&original).map_err(|()| "fixture URI")?;
            assert_eq!(uri_to_fs_path(uri.as_str()).as_deref(), Some(original.as_path()));
            assert_eq!(
                source_path_from_uri_or_path(uri.as_str()).as_deref(),
                Some(original.as_path())
            );
        }
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn coexisting_marker_and_unicode_names_remain_separately_addressable()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let marker = directory.path().join("cafÃ©.pl");
        let unicode = directory.path().join("café.pl");
        std::fs::write(&marker, b"literal marker subject")?;
        std::fs::write(&unicode, b"unicode subject")?;

        for (original, expected) in [
            (&marker, b"literal marker subject".as_slice()),
            (&unicode, b"unicode subject".as_slice()),
        ] {
            let uri = url::Url::from_file_path(original).map_err(|()| "fixture URI")?;
            let decoded = uri_to_fs_path(uri.as_str()).ok_or("missing decoded path")?;
            assert_eq!(&decoded, original);
            assert_eq!(std::fs::read(decoded)?, expected);
        }
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn native_non_utf8_path_bytes_round_trip_without_loss() -> Result<(), Box<dyn std::error::Error>>
    {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;

        let directory = tempfile::tempdir()?;
        let original = directory.path().join(OsString::from_vec(b"literal-\xff-\xc3.pl".to_vec()));
        let uri = fs_path_to_uri(&original)?;
        assert_eq!(uri_to_fs_path(&uri).as_deref(), Some(original.as_path()));
        let drive_relative = OsString::from_vec(b"C:literal-\xff.pl".to_vec());
        assert!(fs_path_to_uri(Path::new(&drive_relative)).is_err());
        Ok(())
    }

    #[test]
    fn drive_relative_inputs_cannot_reach_a_physical_path() {
        for input in [
            "C:",
            "C:lib/Foo.pm",
            r"c:lib\Foo.pm",
            "D|lib/Foo.pm",
            "file://C:",
            "file://C:lib/Foo.pm",
            "file://localhost/C:lib/Foo.pm",
        ] {
            assert!(uri_to_fs_path(input).is_none(), "URI projection accepted {input:?}");
            assert!(
                source_path_from_uri_or_path(input).is_none(),
                "source projection accepted {input:?}"
            );
        }
        for input in ["C:", "C:lib/Foo.pm", r"c:lib\Foo.pm", "D|lib/Foo.pm"] {
            assert!(fs_path_to_uri(input).is_err(), "path conversion accepted {input:?}");
        }
    }

    #[test]
    fn supported_absolute_projection_controls_are_unchanged()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let original = directory.path().join("plain module.pm");
        let uri = fs_path_to_uri(&original)?;
        assert_eq!(uri_to_fs_path(&uri).as_deref(), Some(original.as_path()));
        assert_eq!(source_path_from_uri_or_path(&uri).as_deref(), Some(original.as_path()));
        assert!(uri_to_fs_path("file:///C:/lib/Foo.pm").is_some());
        assert!(uri_to_fs_path("https://example.com/a:b").is_none());
        #[cfg(unix)]
        {
            let colon = Path::new("/tmp/C:literal-name.pm");
            let uri = fs_path_to_uri(colon)?;
            assert_eq!(uri_to_fs_path(&uri).as_deref(), Some(colon));
        }
        Ok(())
    }
}
