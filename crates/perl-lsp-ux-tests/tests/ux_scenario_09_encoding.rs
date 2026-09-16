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
