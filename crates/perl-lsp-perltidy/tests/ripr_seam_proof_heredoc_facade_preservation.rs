//! Seam proof for the heredoc-span transplant facade.
//!
//! RIPR reports declaration-anchored `field_construction` / `no_static_path`
//! probes on `LITERAL_PRESERVE_CODE` and the `SanitizedSource`
//! `{ text, sentinel, substitutions }` fields in
//! `crates/perl-lsp-perltidy/src/native/facade.rs`. A declaration has no
//! static construction site for an oracle to reach, so this file pins the
//! observable behavior instead of echoing declarations:
//!
//! - the heredoc refusal names literal preservation on both the compat path
//!   (diagnostic code) and the typed path (reason code) for the same request;
//! - multi-marker sanitization under sentinel collision selects a non-default
//!   sentinel, records every substitution, and restores all markers across
//!   every public entry point without leaking the sentinel.

#![deny(clippy::map_err_ignore)]

use perl_lsp_perltidy::native::{FormatContext, FormatDisposition, FormatReasonCode};
use perl_lsp_perltidy::{FormatConfig, NativeFormatter, PerlFormatter, TextPosition, TextRange};

#[test]
fn compat_and_typed_heredoc_refusals_name_literal_preservation() {
    let formatter = NativeFormatter::new();
    let source = "print <<'EOF';\nmy$x=1;\nEOF\nmy$y=2;\n";
    let body = TextRange::new(TextPosition::new(1, 0), TextPosition::new(2, 0));

    let compat = formatter.format_range(source, body, &FormatConfig::default());
    assert!(!compat.changed);
    assert_eq!(compat.formatted, source);
    assert!(compat.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "native.format.literal_preserve_region"
            && diagnostic.message.contains("heredoc")
    }));

    let typed = formatter.format_range_typed(
        source,
        body,
        &FormatConfig::default(),
        &FormatContext::default(),
    );
    assert_eq!(typed.outcome.disposition, FormatDisposition::Refused);
    assert_eq!(typed.outcome.reason, FormatReasonCode::LiteralPreservationUnsupported);
    assert!(!typed.result.changed);
    assert_eq!(typed.result.formatted, source);
}

#[test]
fn multiple_markers_under_sentinel_collision_round_trip_every_entry_point() {
    let formatter = NativeFormatter::new();
    // "AA AB" occupies the first usable sentinel candidates ("AA" is skipped
    // as an equal-letter pair and "AB" is taken), so the facade must select
    // "AC" and record two substitutions; both markers must round-trip.
    let source = "my$used=\"AA AB\";\nmy$a=\"<<ONE\";# note <<TWO\n";
    let expected = "my $used = \"AA AB\";\nmy $a = \"<<ONE\"; # note <<TWO\n";
    let config = FormatConfig::default();
    let range = TextRange::new(TextPosition::new(0, 0), TextPosition::new(2, 0));

    let document = formatter.format_document(source, &config);
    assert_eq!(document.formatted, expected);
    assert!(document.diagnostics.is_empty());

    let typed_document =
        formatter.format_document_typed(source, &config, &FormatContext::default());
    assert_eq!(typed_document.outcome.disposition, FormatDisposition::Applied);
    assert_eq!(typed_document.result.formatted, expected);
    assert!(typed_document.result.diagnostics.is_empty());

    let ranged = formatter.format_range(source, range, &config);
    assert_eq!(ranged.formatted, expected);
    assert!(ranged.diagnostics.is_empty());

    let typed_range =
        formatter.format_range_typed(source, range, &config, &FormatContext::default());
    assert_eq!(typed_range.outcome.disposition, FormatDisposition::Applied);
    assert_eq!(typed_range.result.formatted, expected);
    assert!(typed_range.result.diagnostics.is_empty());

    for output in [
        document.formatted.as_str(),
        typed_document.result.formatted.as_str(),
        ranged.formatted.as_str(),
        typed_range.result.formatted.as_str(),
    ] {
        assert!(output.contains("<<ONE"), "first marker must round-trip");
        assert!(output.contains("<<TWO"), "second marker must round-trip");
        assert!(output.contains("AA AB"), "pre-existing pairs must not be rewritten");
        assert!(!output.contains("AC"), "selected sentinel must not leak");
    }
    for edits in [
        document.edits.as_slice(),
        typed_document.result.edits.as_slice(),
        ranged.edits.as_slice(),
        typed_range.result.edits.as_slice(),
    ] {
        assert!(edits.iter().all(|edit| !edit.new_text.contains("AC")));
    }
}
