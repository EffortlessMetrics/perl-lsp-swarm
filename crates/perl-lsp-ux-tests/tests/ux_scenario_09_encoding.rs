#![expect(
    clippy::print_stderr,
    reason = "Scenario 09 reports a local non-execution reason when the required perllsp binary is unavailable."
)]

//! Scenario 09 — BOM and encoding edge cases.
//!
//! Tests UTF-8 BOM, Latin-1 characters, `use utf8` declarations, and
//! Unicode in comments.
//!
//! Acceptance criteria:
//! - Server MUST NOT crash for any of these inputs.
//! - No error-level `window/showMessage` for encoding issues.
//! - Hover and completion MUST NOT crash (empty results OK).

use perl_lsp_ux_tests::binary_available;
use perl_lsp_ux_tests::{ScenarioConfig, UxHarness};

#[test]
fn scenario_09_utf8_bom_file_does_not_crash() -> Result<(), String> {
    if !binary_available() {
        eprintln!("SKIP scenario_09: perl-lsp binary not found");
        return Ok(());
    }

    let bom = "\u{FEFF}";
    let source = format!("{bom}use strict;\nuse warnings;\nmy $x = 1;\n");
    let harness = UxHarness::new(ScenarioConfig::default())
        .map_err(|error| format!("Failed to create UX harness: {error}"))?;

    harness
        .open_file("bom.pl", &source)
        .map_err(|error| format!("didOpen should succeed with UTF-8 BOM: {error}"))?;

    harness
        .hover("bom.pl", 2, 3)
        .map_err(|error| format!("hover crashed on UTF-8 BOM file — UX regression: {error}"))?;

    harness.assert_no_crash();
    Ok(())
}

#[test]
fn scenario_09_unicode_in_strings_does_not_crash() -> Result<(), String> {
    if !binary_available() {
        eprintln!("SKIP scenario_09: perl-lsp binary not found");
        return Ok(());
    }

    // Unicode characters in string literals (valid UTF-8, common in i18n Perl code).
    let source = "use utf8;\nuse strict;\nuse warnings;\n\n\
                  my $name = \"\u{4E16}\u{754C}\";\n\
                  print \"Hello, $name\\n\";\n";
    let harness = UxHarness::new(ScenarioConfig::default())
        .map_err(|error| format!("Failed to create UX harness: {error}"))?;

    harness
        .open_file("utf8_decl.pl", source)
        .map_err(|error| format!("didOpen should succeed with use utf8: {error}"))?;

    harness
        .hover("utf8_decl.pl", 4, 3)
        .map_err(|error| format!("hover crashed on use utf8 file — UX regression: {error}"))?;

    harness.assert_no_crash();
    Ok(())
}

#[test]
fn scenario_09_high_codepoint_comments_do_not_crash() -> Result<(), String> {
    if !binary_available() {
        eprintln!("SKIP scenario_09: perl-lsp binary not found");
        return Ok(());
    }

    // Em-dashes and smart-quotes in comments — common in legacy Perl codebases.
    let source = "#!/usr/bin/perl\n\
                  # This is a comment with \u{2014} an em-dash\n\
                  # And \u{201C}smart quotes\u{201D}\n\
                  use strict;\n\
                  my $x = 1; # \u{2013} some note\n";
    let harness = UxHarness::new(ScenarioConfig::default())
        .map_err(|error| format!("Failed to create UX harness: {error}"))?;

    harness
        .open_file("smart_quotes.pl", source)
        .map_err(|error| format!("didOpen should succeed: {error}"))?;

    harness.hover("smart_quotes.pl", 4, 5).map_err(|error| {
        format!("hover crashed on smart-quote comment — UX regression: {error}")
    })?;

    harness.assert_no_crash();
    Ok(())
}

#[test]
fn scenario_09_latin1_extended_chars_in_comment() -> Result<(), String> {
    if !binary_available() {
        eprintln!("SKIP scenario_09: perl-lsp binary not found");
        return Ok(());
    }

    // Latin-1 supplemental characters (U+00E0..U+00FF) in a comment.
    let source = "use strict;\nuse warnings;\n# café résumé naïve\nmy $x = 1;\n";
    let harness = UxHarness::new(ScenarioConfig::default())
        .map_err(|error| format!("Failed to create UX harness: {error}"))?;

    harness
        .open_file("latin1.pl", source)
        .map_err(|error| format!("didOpen should succeed: {error}"))?;

    harness
        .hover("latin1.pl", 3, 3)
        .map_err(|error| format!("hover crashed on Latin-1 comment — UX regression: {error}"))?;

    harness.assert_no_crash();
    Ok(())
}

fn has_required_static_symbol(symbols: &[serde_json::Value], expected: &str) -> bool {
    symbols.iter().any(|symbol| {
        symbol.get("name").and_then(serde_json::Value::as_str) == Some(expected)
            || symbol
                .get("children")
                .and_then(serde_json::Value::as_array)
                .is_some_and(|children| has_required_static_symbol(children, expected))
    })
}

#[test]
fn static_symbol_control_rejects_empty_unrelated_and_accepts_named_symbol() {
    use serde_json::json;
    assert!(!has_required_static_symbol(&[], "decode_payload"));
    assert!(!has_required_static_symbol(&[json!({"name":"unrelated"})], "decode_payload"));
    assert!(!has_required_static_symbol(&[json!({"name":null})], "decode_payload"));
    assert!(has_required_static_symbol(&[json!({"name":"decode_payload"})], "decode_payload"));
    assert!(has_required_static_symbol(
        &[json!({"name":"package","children":[{"name":"decode_payload"}]})],
        "decode_payload"
    ));
}

#[test]
fn scenario_09_static_symbol_survives_encoding_and_extension_boundary() {
    use perl_lsp_ux_tests::{UxCiTier, UxComponent, missing_binary_skip, run_ux_scenario};
    run_ux_scenario(
        "encoding_and_bom_resilience",
        "ux_scenario_09_encoding.rs",
        "scenario_09_static_symbol_survives_encoding_and_extension_boundary",
        UxCiTier::Pr,
        Some(UxComponent::Infra),
        |recorder| {
            if !binary_available() {
                return Err(missing_binary_skip().into());
            }
            let source = concat!(
                "\u{FEFF}",
                "#!/usr/bin/env perl\nuse strict;\nuse warnings;\nsub decode_payload { return 42; }\ndecode_payload();\n"
            );
            let harness = UxHarness::new(ScenarioConfig::default().with_file("bom.pl", &source))?;
            harness.open_file("bom.pl", &source)?;
            let uri = harness.workspace.uri("bom.pl");
            perl_lsp_ux_tests::wait_with_subject(
                "static symbol active-document readiness",
                harness.wait_for_active_document_ready(&uri, std::time::Duration::from_secs(20)),
            )?;
            recorder.mark_request_start("static_document_symbol");
            let symbols = harness.document_symbols("bom.pl")?;
            recorder.check(
                "static document exposes decode_payload symbol",
                has_required_static_symbol(&symbols, "decode_payload"),
            )?;
            recorder.mark_first_useful_result("static_document_symbol");
            harness.assert_no_crash();
            Ok(())
        },
    );
}
