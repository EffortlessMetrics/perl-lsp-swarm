use perl_uri::{is_special_scheme, uri_extension, uri_key};

#[test]
fn localhost_file_authority_preserves_query_and_fragment() -> Result<(), Box<dyn std::error::Error>>
{
    assert_eq!(
        uri_key("file://localhost/tmp/module.pm?rev=42#L10"),
        "file:///tmp/module.pm?rev=42#L10"
    );
    Ok(())
}

#[test]
fn loopback_file_authorities_preserve_query_and_fragment() -> Result<(), Box<dyn std::error::Error>>
{
    assert_eq!(
        uri_key("file://127.0.0.1/tmp/script.pl?debug=1#main"),
        "file:///tmp/script.pl?debug=1#main"
    );
    assert_eq!(
        uri_key("file://[::1]/tmp/script.pl?debug=1#main"),
        "file:///tmp/script.pl?debug=1#main"
    );
    Ok(())
}

#[test]
fn canonical_drive_uri_with_pipe_separator_preserves_suffix()
-> Result<(), Box<dyn std::error::Error>> {
    assert_eq!(
        uri_key("file:///Z|/Work/Module.pm?rev=42#L10"),
        "file:///z:/Work/Module.pm?rev=42#L10"
    );
    Ok(())
}

#[test]
fn invalid_special_scheme_prefixes_must_match_full_prefix() -> Result<(), Box<dyn std::error::Error>>
{
    assert!(!is_special_scheme("untitle invalid uri"));
    assert!(!is_special_scheme("vscode-notebook-cel invalid uri"));
    assert!(!is_special_scheme("vscode-vf invalid uri"));
    Ok(())
}

#[test]
fn extension_stops_at_earliest_query_or_fragment_separator()
-> Result<(), Box<dyn std::error::Error>> {
    assert_eq!(uri_extension("file:///tmp/module.pm#L10?ignored=.pl"), Some("pm"));
    assert_eq!(uri_extension("file:///tmp/module.pm?rev=42#ignored=.pl"), Some("pm"));
    Ok(())
}
