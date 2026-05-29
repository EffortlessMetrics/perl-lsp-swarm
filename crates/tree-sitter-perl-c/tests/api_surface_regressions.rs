//! Public API coverage for the C-backed tree-sitter Perl binding.

use std::{
    error::Error,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use tree_sitter::Parser;
use tree_sitter_perl_c::{
    ParsePerlError, ParsePerlFileError, PerlParser, get_scanner_config, parse_perl_bytes,
    parse_perl_bytes_with_parser, parse_perl_file, try_create_parser,
    try_parse_perl_bytes_with_parser,
};

fn unique_missing_file(name: &str) -> PathBuf {
    let nanos =
        SystemTime::now().duration_since(UNIX_EPOCH).map_or(0_u128, |duration| duration.as_nanos());

    std::env::temp_dir().join(format!("tree_sitter_perl_c_api_missing_{name}_{nanos}.pl"))
}

#[test]
fn public_scanner_config_reports_c_backend() {
    assert_eq!(get_scanner_config(), "c-scanner");
}

#[test]
fn boxed_parse_perl_file_error_preserves_path_context() -> Result<(), Box<dyn Error>> {
    let missing = unique_missing_file("boxed_file_error");

    let error = match parse_perl_file(&missing) {
        Ok(_) => return Err("expected parse_perl_file to fail for a missing file".into()),
        Err(error) => error,
    };

    let file_error = error
        .downcast_ref::<ParsePerlFileError>()
        .ok_or("expected boxed error to downcast to ParsePerlFileError")?;

    assert_eq!(file_error.path(), missing.as_path());

    let display = file_error.to_string();
    assert!(display.contains(&missing.to_string_lossy().to_string()));
    assert!(display.contains("failed to read Perl source file"));
    assert!(file_error.source().is_some());

    Ok(())
}

#[test]
fn typed_and_boxed_reused_parser_apis_report_parse_none_for_unconfigured_parser() {
    let mut typed_parser = Parser::new();
    let typed_result = try_parse_perl_bytes_with_parser(&mut typed_parser, b"my $value = 1;\n");
    assert!(matches!(typed_result, Err(ParsePerlError::ParseReturnedNone)));

    let mut boxed_parser = Parser::new();
    let boxed_result = parse_perl_bytes_with_parser(&mut boxed_parser, b"my $value = 1;\n");
    assert!(boxed_result.is_err());
}

#[test]
fn reusable_parser_accepts_non_utf8_bytes_and_recovers_for_next_source()
-> Result<(), Box<dyn Error>> {
    let mut parser = PerlParser::new()?;

    let byte_tree = parser.parse_bytes(b"my $value = \"latin1: \xE9\";\n")?;
    assert_eq!(byte_tree.root_node().kind(), "source_file");
    assert!(!byte_tree.root_node().has_error());

    let bad_tree = parser.parse_code("my $broken = ;\n")?;
    assert!(bad_tree.root_node().has_error());

    let good_tree = parser.parse_code("my $ok = 42;\n")?;
    assert_eq!(good_tree.root_node().kind(), "source_file");
    assert!(!good_tree.root_node().has_error());

    Ok(())
}

#[test]
fn configured_parser_can_parse_multiple_byte_inputs_through_typed_api() -> Result<(), Box<dyn Error>>
{
    let mut parser = try_create_parser()?;

    let first = try_parse_perl_bytes_with_parser(&mut parser, b"my $first = 1;\n")?;
    assert_eq!(first.root_node().kind(), "source_file");
    assert!(!first.root_node().has_error());

    let second = try_parse_perl_bytes_with_parser(&mut parser, b"my $second = 2;\n")?;
    assert_eq!(second.root_node().kind(), "source_file");
    assert!(!second.root_node().has_error());

    Ok(())
}

#[test]
fn parse_perl_bytes_accepts_data_section_like_binary_tail() -> Result<(), Box<dyn Error>> {
    let tree = parse_perl_bytes(b"my $value = 1;\n__DATA__\n\x00\xFF\x80\n")?;

    assert_eq!(tree.root_node().kind(), "source_file");
    assert!(!tree.root_node().has_error());

    Ok(())
}
