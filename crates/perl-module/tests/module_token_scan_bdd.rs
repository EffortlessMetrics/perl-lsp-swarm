use perl_module::{ModuleTokenSpan, is_lookup_safe_module_name, parse_module_token};

#[test]
fn handles_simple_canonical_module_tokens() {
    let line = "use Foo::Bar;";
    assert_eq!(parse_module_token(line, 4), Some(ModuleTokenSpan { start: 4, end: 12 }));
}

#[test]
fn handles_legacy_quote_separator_tokens() {
    let line = "use Foo'Bar;";
    assert_eq!(parse_module_token(line, 4), Some(ModuleTokenSpan { start: 4, end: 11 }));
}

#[test]
fn rejects_non_identifier_starts() {
    assert!(parse_module_token("  42Foo", 0).is_none());
    assert!(parse_module_token("use Foo::", 4).is_none());
    assert!(parse_module_token("use Foo'", 4).is_none());
}

#[test]
fn handles_multiple_segments() {
    let line = "require App::Config::Loader;";
    assert_eq!(parse_module_token(line, 8), Some(ModuleTokenSpan { start: 8, end: 27 }));
}

#[test]
fn rejects_xid_characters_outside_perl_word_class() {
    // U+2118 is XID_Start but not Perl's Unicode \w class.
    assert!(parse_module_token("use ℘Module;", 4).is_none());
    // U+00B7 is XID_Continue but not Perl's Unicode \w class.
    assert!(!is_lookup_safe_module_name("Foo·Bar"));
    assert!(!is_lookup_safe_module_name("℘Module"));
    assert!(parse_module_token("use Foo·Bar;", 4).is_some_and(|span| span.end == 7));
}
