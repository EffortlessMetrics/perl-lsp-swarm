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
    assert!(!has_required_static_symbol(&[], "deploy_task"));
    assert!(!has_required_static_symbol(&[json!({"name":"unrelated"})], "deploy_task"));
    assert!(!has_required_static_symbol(&[json!({"name":null})], "deploy_task"));
    assert!(has_required_static_symbol(&[json!({"name":"deploy_task"})], "deploy_task"));
    assert!(has_required_static_symbol(
        &[json!({"name":"package","children":[{"name":"deploy_task"}]})],
        "deploy_task"
    ));
}

#[test]
fn scenario_08_static_symbol_survives_encoding_and_extension_boundary() {
    use perl_lsp_ux_tests::{UxCiTier, UxComponent, missing_binary_skip, run_ux_scenario};
    run_ux_scenario(
        "shebang_detection",
        "ux_scenario_08_shebang_detection.rs",
        "scenario_08_static_symbol_survives_encoding_and_extension_boundary",
        UxCiTier::Pr,
        Some(UxComponent::Infra),
        |recorder| {
            if !binary_available() {
                return Err(missing_binary_skip().into());
            }
            let source = "#!/usr/bin/env perl\nuse strict;\nuse warnings;\nsub deploy_task { return 42; }\ndeploy_task();\n";
            let harness =
                UxHarness::new(ScenarioConfig::default().with_file("deploy_script", source))?;
            harness.open_file("deploy_script", &source)?;
            let uri = harness.workspace.uri("deploy_script");
            perl_lsp_ux_tests::wait_with_subject(
                "static symbol active-document readiness",
                harness.wait_for_active_document_ready(&uri, std::time::Duration::from_secs(20)),
            )?;
            recorder.mark_request_start("static_document_symbol");
            let symbols = harness.document_symbols("deploy_script")?;
            recorder.check(
                "static document exposes deploy_task symbol",
                has_required_static_symbol(&symbols, "deploy_task"),
            )?;
            recorder.mark_first_useful_result("static_document_symbol");
            harness.assert_no_crash();
            Ok(())
        },
    );
}
