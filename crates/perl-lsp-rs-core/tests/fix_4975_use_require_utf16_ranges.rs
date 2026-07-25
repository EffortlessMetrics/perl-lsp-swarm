//! Regression tests for #4975: use/require document-link ranges use UTF-16 columns.

use perl_lsp_rs_core::providers::document_links::compute_links;
use perl_parser_core::position::utf16_line_col_to_offset;
use serde_json::Value;

const URI: &str = "file:///workspace/test.pl";

fn utf16_range_substring(line: &str, line_number: u32, start: u32, end: u32) -> Option<String> {
    let start_byte = utf16_line_col_to_offset(line, line_number, start);
    let end_byte = utf16_line_col_to_offset(line, line_number, end);
    line.get(start_byte..end_byte).map(str::to_owned)
}

fn assert_link_span(
    text: &str,
    line_number: u32,
    expected_token: &str,
    link_type: Option<&str>,
) -> Result<(), String> {
    let links = compute_links(URI, text, &[]);
    let link = links.first().ok_or_else(|| format!("expected one link, got {links:?}"))?;

    if let Some(expected_type) = link_type {
        let actual_type = link.pointer("/data/type").and_then(Value::as_str);
        if actual_type != Some(expected_type) {
            return Err(format!("expected type {expected_type}, got {actual_type:?}"));
        }
    }

    let start_line =
        link.pointer("/range/start/line").and_then(Value::as_u64).ok_or("missing start line")?
            as u32;
    let start_char = link
        .pointer("/range/start/character")
        .and_then(Value::as_u64)
        .ok_or("missing start character")? as u32;
    let end_char = link
        .pointer("/range/end/character")
        .and_then(Value::as_u64)
        .ok_or("missing end character")? as u32;

    let line = text
        .lines()
        .nth(line_number as usize)
        .ok_or_else(|| format!("no line {line_number} in text"))?;

    if start_line != line_number {
        return Err(format!("link on line {start_line}, expected {line_number}"));
    }

    let span = utf16_range_substring(line, line_number, start_char, end_char)
        .ok_or_else(|| format!("invalid UTF-16 range {start_char}..{end_char}"))?;

    if span != expected_token {
        return Err(format!("range '{span}' != expected '{expected_token}'"));
    }
    Ok(())
}

#[test]
fn use_ascii_baseline_round_trips_token_span() -> Result<(), String> {
    assert_link_span("use Foo::Bar;\n", 0, "Foo::Bar", Some("module"))
}

#[test]
fn require_ascii_module_round_trips_token_span() -> Result<(), String> {
    assert_link_span("require Foo::Bar;\n", 0, "Foo::Bar", Some("module"))
}

#[test]
fn require_quoted_pm_round_trips_token_span() -> Result<(), String> {
    assert_link_span(r#"require "lib/helper.pm";"#, 0, "lib/helper.pm", Some("module"))
}

#[test]
fn require_quoted_pl_file_link_round_trips_token_span() -> Result<(), String> {
    assert_link_span(r#"require "helper.pl";"#, 0, "helper.pl", Some("file"))
}

#[test]
fn require_cafe_path_round_trips_token_span() -> Result<(), String> {
    assert_link_span(r#"require "café.pl";"#, 0, "café.pl", Some("file"))
}

#[test]
fn require_cjk_path_round_trips_token_span() -> Result<(), String> {
    assert_link_span(r#"require "日本語.pl";"#, 0, "日本語.pl", Some("file"))
}

#[test]
fn require_emoji_path_round_trips_token_span() -> Result<(), String> {
    assert_link_span(r#"require "plugin-😀.pl";"#, 0, "plugin-😀.pl", Some("file"))
}

#[test]
fn use_after_leading_unicode_whitespace_round_trips_token_span() -> Result<(), String> {
    assert_link_span("\u{3000}use Foo::Bar;\n", 0, "Foo::Bar", Some("module"))
}
