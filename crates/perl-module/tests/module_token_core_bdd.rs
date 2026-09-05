use perl_module::{ModuleTokenSpan, has_standalone_module_token_boundaries, parse_module_token};

#[test]
fn given_canonical_form_when_token_starts_at_offset_it_is_parsed() {
    let line = "use App::Config;";
    let span = parse_module_token(line, 4);

    assert_eq!(span, Some(ModuleTokenSpan { start: 4, end: 15 }));
}

#[test]
fn given_legacy_form_when_token_starts_at_offset_it_is_parsed() {
    let line = "use App'Config;";
    let span = parse_module_token(line, 4);

    assert_eq!(span, Some(ModuleTokenSpan { start: 4, end: 14 }));
}

#[test]
fn given_unicode_segments_when_token_starts_at_offset_it_preserves_byte_span() {
    let module = "Δοκιμή::設定2";
    let line = format!("use {module};");
    let span = parse_module_token(&line, 4);

    assert_eq!(span, Some(ModuleTokenSpan { start: 4, end: 4 + module.len() }));
}

#[test]
fn given_start_inside_multibyte_character_when_parsing_then_no_span() {
    let line = "use Δοκιμή::設定;";
    let inside_delta = 5;

    assert!(!line.is_char_boundary(inside_delta));
    assert_eq!(parse_module_token(line, inside_delta), None);
}

#[test]
fn given_partial_standalone_prefix_when_matching_boundaries_then_false() {
    let line = "use App::Config::Loader;";
    let span = parse_module_token(line, 4);

    assert!(span.is_some(), "token should be parsed");
    let span = span.unwrap_or(ModuleTokenSpan { start: 0, end: 0 });
    let partial_end = span.start + "App::Config".len();

    assert!(!has_standalone_module_token_boundaries(line, span.start, partial_end));
}

#[test]
fn given_unicode_identifier_neighbor_when_matching_boundaries_then_false() {
    let line = "λApp::Config;";
    let start = "λ".len();
    let span = parse_module_token(line, start);

    assert_eq!(span, Some(ModuleTokenSpan { start, end: start + "App::Config".len() }));
    assert!(!has_standalone_module_token_boundaries(line, start, start + "App::Config".len()));
}

#[test]
fn given_invalid_utf8_spans_when_matching_boundaries_then_false_without_panicking() {
    let line = "Δοκιμή::設定";

    assert!(!has_standalone_module_token_boundaries(line, 0, 0));
    assert!(!has_standalone_module_token_boundaries(line, 2, 1));
    assert!(!has_standalone_module_token_boundaries(line, 0, line.len() + 1));
    assert!(!has_standalone_module_token_boundaries(line, 1, line.len()));
}

#[test]
fn given_plain_text_when_token_absent_then_no_span() {
    assert!(parse_module_token("use App::", 4).is_none());
    assert!(parse_module_token("  42App", 0).is_none());
}
