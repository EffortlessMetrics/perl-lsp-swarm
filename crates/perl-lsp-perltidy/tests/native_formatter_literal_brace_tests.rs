#![deny(clippy::map_err_ignore)]

use perl_lsp_perltidy::native::{
    FormatConfig, FormatContext, FormatDisposition, FormatReasonCode, NativeFormatter,
    PerlFormatter, TextPosition, TextRange,
};

fn check_document_string_opener(quote: char) {
    let formatter = NativeFormatter::new();
    let configurations = [
        (FormatConfig::default(), "    "),
        (FormatConfig { indent_width: 2, ..FormatConfig::default() }, "  "),
        (FormatConfig { use_tabs: true, ..FormatConfig::default() }, "\t"),
    ];

    for (config, indent) in configurations {
        for ending in ["\n", "\r\n"] {
            let source = format!(
                "sub f {{{ending}my $s = {quote}héllo {{  {ending}\tworld μ{quote};{ending}return 1;{ending}}}{ending}my$x=2;{ending}"
            );
            let expected = format!(
                "sub f {{{ending}{indent}my $s = {quote}héllo {{  {ending}\tworld μ{quote};{ending}{indent}return 1;{ending}}}{ending}my $x = 2;{ending}"
            );

            let result = formatter.format_document(&source, &config);

            assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
            assert!(result.changed);
            assert!(
                result.formatted.contains(&format!("{quote}héllo {{  {ending}\tworld μ{quote}")),
                "literal payload changed: {:?}",
                result.formatted
            );
            // The statement prefix moves, while the complete string payload
            // and its line terminators remain byte-exact. The literal brace
            // cannot indent the return, closing brace, or top-level statement.
            assert_eq!(result.formatted, expected, "quote={quote}, config={config:?}");

            let second = formatter.format_document(&result.formatted, &config);
            assert_eq!(second.formatted, expected);
            assert!(!second.changed);
            assert!(second.edits.is_empty());
            assert!(second.diagnostics.is_empty());
        }
    }
}

#[test]
fn double_quoted_string_opening_brace_does_not_open_a_block() {
    check_document_string_opener('"');
}

#[test]
fn single_quoted_string_opening_brace_does_not_open_a_block() {
    check_document_string_opener('\'');
}

#[test]
fn range_after_multiline_string_uses_the_enclosing_block_indent() {
    let formatter = NativeFormatter::new();
    let config = FormatConfig::default();
    let range = TextRange::new(TextPosition::new(3, 0), TextPosition::new(4, 0));

    for ending in ["\n", "\r\n"] {
        let source = format!(
            "sub f {{{ending}    my $s = \"héllo {{{ending}world\";{ending}return 1;{ending}}}{ending}my$x=2;{ending}"
        );
        let expected = format!(
            "sub f {{{ending}    my $s = \"héllo {{{ending}world\";{ending}    return 1;{ending}}}{ending}my$x=2;{ending}"
        );

        let result = formatter.format_range(&source, range, &config);

        assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
        assert!(result.changed);
        assert_eq!(result.formatted, expected);
        assert_eq!(result.edits.len(), 1);
        assert_eq!(
            result.edits[0].range,
            TextRange::new(TextPosition::new(3, 0), TextPosition::new(3, 9))
        );
        assert_eq!(result.edits[0].new_text, "    return 1;");
    }
}

#[test]
fn typed_range_after_multiline_string_is_already_formatted() {
    let formatter = NativeFormatter::new();
    let range = TextRange::new(TextPosition::new(3, 0), TextPosition::new(4, 0));

    for ending in ["\n", "\r\n"] {
        let source = format!(
            "sub f {{{ending}    my $s = \"héllo {{{ending}world\";{ending}    return 1;{ending}}}{ending}my$x=2;{ending}"
        );
        let typed = formatter.format_range_typed(
            &source,
            range,
            &FormatConfig::default(),
            &FormatContext::default(),
        );

        // The unchanged-result classifier has its own context walk. Fixing
        // only the formatter would still leave this range UnsupportedSyntax.
        assert_eq!(typed.outcome.disposition, FormatDisposition::NoChange);
        assert_eq!(typed.outcome.reason, FormatReasonCode::AlreadyFormatted);
        assert_eq!(typed.result.formatted, source);
        assert!(!typed.result.changed);
        assert!(typed.result.edits.is_empty());
        assert!(typed.result.diagnostics.is_empty());
    }
}

#[test]
fn real_fallback_openers_before_preserved_literals_still_indent_their_bodies() {
    let formatter = NativeFormatter::new();
    // These openers reach the fallback predicate, unlike a rendered `sub f {`
    // or `if (...) {` header. The following q{} line is preserved in full,
    // rejecting a fix that suppresses all openers before preserved lines.
    let openers = [
        "my $f = sub { # real opener before a comment {",
        "my $value = eval {",
        "my $value = do {",
        "my $value = \"é\" . do { # byte offsets, not character offsets",
    ];

    for opener in openers {
        for ending in ["\n", "\r\n"] {
            let source = format!(
                "# café{ending}sub outer {{{ending}{opener}{ending}        my $s = q{{opaque}};{ending}return 1;{ending}}};{ending}return 2;{ending}}}{ending}"
            );
            let expected = format!(
                "# café{ending}sub outer {{{ending}    {opener}{ending}        my $s = q{{opaque}};{ending}        return 1;{ending}    }};{ending}    return 2;{ending}}}{ending}"
            );

            let result = formatter.format_document(&source, &FormatConfig::default());

            assert!(result.diagnostics.is_empty(), "{opener}: {:?}", result.diagnostics);
            assert!(result.changed);
            assert_eq!(result.formatted, expected, "{opener}");
            let second = formatter.format_document(&result.formatted, &FormatConfig::default());
            assert_eq!(second.formatted, expected, "{opener}");
            assert!(!second.changed);
        }
    }
}
