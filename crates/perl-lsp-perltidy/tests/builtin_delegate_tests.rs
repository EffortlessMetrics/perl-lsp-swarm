//! Compatibility name, one canonical native engine.
#![allow(deprecated)]
use perl_lsp_perltidy::{BuiltInFormatter, PerlTidyConfig};

#[test]
fn builtin_compatibility_name_delegates_to_the_canonical_native_engine() {
    let config =
        PerlTidyConfig { indent_columns: Some(2), tabs: Some(false), ..Default::default() };
    let policy = perl_tidy::NativeFormatConfig {
        line_width: 80,
        indent_width: 2,
        use_tabs: false,
        ..Default::default()
    };
    let source = "sub f { my$x=1; }\n";
    let expected = perl_tidy::NativeFormatter::new().format_document_typed(
        source,
        &policy,
        &perl_tidy::FormatContext::default(),
    );
    assert_eq!(expected.outcome.disposition, perl_tidy::FormatDisposition::Applied);
    assert_eq!(BuiltInFormatter::new(config).format(source), expected.result.formatted);
}

#[test]
fn retired_scanner_cannot_rewrite_invalid_perl() {
    let source = "if ($ok) {\n)\nprint 1;\n}\n";
    assert_eq!(BuiltInFormatter::new(PerlTidyConfig::default()).format(source), source);
}

#[test]
fn external_profile_argv_and_timeout_cannot_select_external_execution() {
    let source = "my$x=1;\n";
    let expected = BuiltInFormatter::new(PerlTidyConfig::default()).format(source);
    let config = PerlTidyConfig {
        profile: Some("unreadable-and-untrusted.rc".into()),
        extra_args: vec!["--not-a-native-option".into()],
        timeout_secs: 0,
        ..Default::default()
    };
    assert_eq!(BuiltInFormatter::new(config).format(source), expected);
    assert_ne!(expected, source, "the native path must actually execute");
}
