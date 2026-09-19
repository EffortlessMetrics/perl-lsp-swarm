#![expect(
    clippy::print_stderr,
    reason = "Scenario 08 reports a local non-execution reason when the required perllsp binary is unavailable."
)]

//! Scenario 08 — Shebang detection / non-standard extensions.
//!
//! Files with `#!/usr/bin/env perl` shebang but no `.pl`/`.pm` extension.
//!
//! Acceptance criteria:
//! - Server MUST accept `didOpen` with any URI when languageId is "perl".
//! - Hover, completion MUST NOT crash.
//! - Null results are acceptable.

use perl_lsp_ux_tests::binary_available;
use perl_lsp_ux_tests::{ScenarioConfig, UxHarness};

#[test]
fn scenario_08_shebang_file_without_pl_extension() -> Result<(), String> {
    if !binary_available() {
        eprintln!("SKIP scenario_08: perl-lsp binary not found");
        return Ok(());
    }

    let source = "#!/usr/bin/env perl\nuse strict;\nuse warnings;\n\n\
                  my $answer = 42;\nprint \"Answer: $answer\\n\";\n";
    let harness = UxHarness::new(ScenarioConfig::default())
        .map_err(|error| format!("Failed to create UX harness: {error}"))?;

    harness
        .open_file("deploy_script", source)
        .map_err(|error| format!("didOpen should succeed for shebang file: {error}"))?;

    harness
        .hover("deploy_script", 4, 3)
        .map_err(|error| format!("hover crashed on non-.pl file — UX regression: {error}"))?;

    harness.assert_no_crash();
    Ok(())
}

#[test]
fn scenario_08_no_extension_file_completion_does_not_crash() -> Result<(), String> {
    if !binary_available() {
        eprintln!("SKIP scenario_08: perl-lsp binary not found");
        return Ok(());
    }

    let source = "#!/usr/bin/perl\nmy $va\n";
    let harness = UxHarness::new(ScenarioConfig::default())
        .map_err(|error| format!("Failed to create UX harness: {error}"))?;

    harness
        .open_file("run_tests", source)
        .map_err(|error| format!("didOpen should succeed: {error}"))?;

    harness
        .completion("run_tests", 1, 7)
        .map_err(|error| format!("completion crashed on non-.pl file — UX regression: {error}"))?;

    harness.assert_no_crash();
    Ok(())
}

#[test]
fn scenario_08_test_file_t_extension() -> Result<(), String> {
    if !binary_available() {
        eprintln!("SKIP scenario_08: perl-lsp binary not found");
        return Ok(());
    }

    let source = "use Test::More;\nuse strict;\n\nok(1, 'basic');\ndone_testing();\n";
    let harness = UxHarness::new(ScenarioConfig::default())
        .map_err(|error| format!("Failed to create UX harness: {error}"))?;

    harness
        .open_file("basic.t", source)
        .map_err(|error| format!("didOpen should succeed for .t extension: {error}"))?;

    harness
        .hover("basic.t", 3, 1)
        .map_err(|error| format!("hover crashed on .t test file — UX regression: {error}"))?;

    harness.assert_no_crash();
    Ok(())
}
