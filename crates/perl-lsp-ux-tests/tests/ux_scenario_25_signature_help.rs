//! Scenario 25: builtin signature-help UX coverage.
//!
//! This scenario locks the end-to-end `textDocument/signatureHelp` path that
//! the current server answers reliably: Perl builtins. User-defined call-site
//! coverage belongs in a separate runtime follow-up because that path currently
//! times out under the real stdio harness.

use std::time::Duration;

use anyhow::Result;
use perl_lsp_ux_tests::binary_available;
use perl_lsp_ux_tests::{ScenarioConfig, UxHarness};
use serde_json::{Value, json};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

const BUILTIN_FIXTURE: &str = r#"use strict;
use warnings;

my @arr = (3, 1, 2);
push(@arr, 4);
my $str = join(", ", @arr);
"#;

fn builtin_harness() -> Result<UxHarness> {
    let harness =
        UxHarness::new(ScenarioConfig::default().with_file("builtins.pl", BUILTIN_FIXTURE))?;
    harness.open_file("builtins.pl", BUILTIN_FIXTURE)?;
    Ok(harness)
}

fn request_signature_help(harness: &UxHarness, line: u32, character: u32) -> Result<Value> {
    let uri = harness.workspace.uri("builtins.pl");
    harness.client.request(
        "textDocument/signatureHelp",
        json!({
            "textDocument": { "uri": uri },
            "position": { "line": line, "character": character }
        }),
        REQUEST_TIMEOUT,
    )
}

#[test]
fn scenario_25_builtin_push_does_not_error() -> Result<()> {
    if !binary_available() {
        eprintln!("SKIP scenario_25: perl-lsp binary not found");
        return Ok(());
    }

    let harness = builtin_harness()?;
    let response = request_signature_help(&harness, 4, 8)?;

    assert!(
        response.get("error").is_none(),
        "signatureHelp on builtin push MUST NOT return a JSON-RPC error: {response:?}"
    );

    harness.assert_no_crash();
    Ok(())
}

#[test]
fn scenario_25_builtin_join_does_not_error() -> Result<()> {
    if !binary_available() {
        eprintln!("SKIP scenario_25: perl-lsp binary not found");
        return Ok(());
    }

    let harness = builtin_harness()?;
    let response = request_signature_help(&harness, 5, 15)?;

    assert!(
        response.get("error").is_none(),
        "signatureHelp on builtin join MUST NOT return a JSON-RPC error: {response:?}"
    );

    harness.assert_no_crash();
    Ok(())
}

#[test]
fn scenario_25_builtin_result_is_well_formed_when_present() -> Result<()> {
    if !binary_available() {
        eprintln!("SKIP scenario_25: perl-lsp binary not found");
        return Ok(());
    }

    let harness = builtin_harness()?;
    let response = request_signature_help(&harness, 5, 15)?;

    assert!(
        response.get("error").is_none(),
        "signatureHelp on builtin join returned an error: {response:?}"
    );

    if let Some(result) = response.get("result")
        && !result.is_null()
    {
        assert_signature_help_structure(result)?;
    }

    harness.assert_no_crash();
    Ok(())
}

#[test]
fn scenario_25_builtin_requests_are_idempotent() -> Result<()> {
    if !binary_available() {
        eprintln!("SKIP scenario_25: perl-lsp binary not found");
        return Ok(());
    }

    let harness = builtin_harness()?;
    for round in 1..=2 {
        let response = request_signature_help(&harness, 5, 15)?;
        assert!(
            response.get("error").is_none(),
            "signatureHelp round {round} MUST NOT return a JSON-RPC error: {response:?}"
        );
    }

    harness.assert_no_crash();
    Ok(())
}

fn assert_signature_help_structure(result: &Value) -> Result<()> {
    let Some(signatures) = result.get("signatures") else {
        anyhow::bail!("SignatureHelp result must have a signatures field, got: {result:?}");
    };
    assert!(signatures.is_array(), "SignatureHelp.signatures must be an array");

    let sig_array = signatures
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("signatures is not an array: {signatures:?}"))?;
    for (i, sig) in sig_array.iter().enumerate() {
        assert!(
            sig.get("label").and_then(Value::as_str).is_some(),
            "SignatureInformation[{i}] must have a string label, got: {sig:?}"
        );

        if let Some(params) = sig.get("parameters").and_then(Value::as_array) {
            for (j, param) in params.iter().enumerate() {
                assert!(
                    param.get("label").is_some(),
                    "ParameterInformation[{i}][{j}] must have a label, got: {param:?}"
                );
            }
        }
    }
    Ok(())
}

fn has_required_builtin_signature(response: &Value, expected: &str) -> bool {
    response.get("error").is_none()
        && response.pointer("/result/signatures").and_then(Value::as_array).is_some_and(
            |signatures| {
                signatures.iter().any(|signature| {
                    signature.get("label").and_then(Value::as_str) == Some(expected)
                })
            },
        )
}

#[test]
fn builtin_signature_control_rejects_null_empty_and_other_builtin() {
    for response in [
        json!({"result":null}),
        json!({"result":{"signatures":[]}}),
        json!({"result":{"signatures":[{"label":"join EXPR, LIST"}]}}),
        json!({"error":{"code":-32602},"result":{"signatures":[{"label":"push ARRAY, LIST"}]}}),
    ] {
        assert!(!has_required_builtin_signature(&response, "push ARRAY, LIST"));
    }
    assert!(has_required_builtin_signature(
        &json!({"result":{"signatures":[{"label":"push ARRAY, LIST"}]}}),
        "push ARRAY, LIST"
    ));
}

#[test]
fn scenario_25_static_builtin_signatures_have_exact_labels() {
    use perl_lsp_ux_tests::{UxCiTier, UxComponent, missing_binary_skip, run_ux_scenario};
    run_ux_scenario(
        "signature_help_core",
        "ux_scenario_25_signature_help.rs",
        "scenario_25_static_builtin_signatures_have_exact_labels",
        UxCiTier::Pr,
        Some(UxComponent::SignatureHelp),
        |recorder| {
            if !binary_available() {
                return Err(missing_binary_skip().into());
            }
            let harness = builtin_harness()?;
            let uri = harness.workspace.uri("builtins.pl");
            perl_lsp_ux_tests::wait_with_subject(
                "builtin signature active-document readiness",
                harness.wait_for_active_document_ready(&uri, Duration::from_secs(20)),
            )?;
            for (line, character, label) in [(4, 8, "push ARRAY, LIST"), (5, 15, "join EXPR, LIST")]
            {
                recorder.mark_request_start(label);
                let response = request_signature_help(&harness, line, character)?;
                recorder.check(
                    "static builtin signature is non-null and has its exact label",
                    has_required_builtin_signature(&response, label),
                )?;
                recorder.mark_first_useful_result(label);
            }
            harness.assert_no_crash();
            Ok(())
        },
    );
}
