//! UX-focused coverage for the LSP test harness.
//!
//! These tests validate that the harness convenience methods map to realistic
//! editor workflows (initialize/open/edit/save/close + workspace symbol waits).

#[path = "support/mod.rs"]
mod support;

use serde_json::json;
use std::time::{Duration, Instant};
use support::lsp_harness::{LspHarness, WaitForSymbolMode, workspace_symbol_response_contains};

#[test]
fn harness_supports_edit_save_diagnostics_workflow() -> Result<(), String> {
    let mut harness = LspHarness::new();
    harness.initialize_ready("file:///workspace", None)?;

    let uri = "file:///workspace/workflow.pl";
    harness.open_document(uri, "my $value = ;\n")?;

    let first =
        harness.wait_for_notification("textDocument/publishDiagnostics", Duration::from_secs(2))?;
    let first_uri = first
        .get("uri")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "publishDiagnostics missing params.uri".to_string())?;
    if first_uri != uri {
        return Err(format!("Expected diagnostics for {uri}, got {first_uri}"));
    }

    let first_count =
        first.get("diagnostics").and_then(|v| v.as_array()).map_or(0, std::vec::Vec::len);
    if first_count == 0 {
        return Err("Expected at least one diagnostic for broken document".to_string());
    }

    harness.change_full(uri, 2, "my $value = 42;\n")?;
    harness.did_save(uri)?;

    let mut saw_followup_diagnostics = false;
    for _ in 0..8 {
        let batch = harness.drain_notifications(Some("textDocument/publishDiagnostics"), 250);
        for notif in batch {
            let notif_uri = notif.pointer("/params/uri").and_then(|v| v.as_str());
            if notif_uri == Some(uri) {
                saw_followup_diagnostics = true;
                break;
            }
        }
        if saw_followup_diagnostics {
            break;
        }
    }

    if !saw_followup_diagnostics {
        return Err("Expected follow-up diagnostics notification after change + save".to_string());
    }

    harness.close(uri)?;
    Ok(())
}

#[test]
fn workspace_symbol_match_requires_name_and_uri() {
    let target_uri = "file:///workspace/lib/Target.pm";

    let same_file_decoy = json!([{
        "name": "same_file_decoy",
        "location": {
            "uri": target_uri,
            "range": {
                "start": { "line": 0, "character": 0 },
                "end": { "line": 0, "character": 16 }
            }
        }
    }]);
    assert!(
        !workspace_symbol_response_contains(&same_file_decoy, "target", Some(target_uri)),
        "an unrelated symbol from the target file must not satisfy readiness"
    );

    let wrong_uri = json!([{
        "name": "target",
        "location": {
            "uri": "file:///workspace/lib/Other.pm",
            "range": {
                "start": { "line": 0, "character": 0 },
                "end": { "line": 0, "character": 6 }
            }
        }
    }]);
    assert!(
        !workspace_symbol_response_contains(&wrong_uri, "target", Some(target_uri)),
        "the requested name from another file must not satisfy a URI-bound wait"
    );

    let exact_package_name = json!([{
        "name": "MyApp::Target",
        "location": {
            "uri": target_uri,
            "range": {
                "start": { "line": 1, "character": 0 },
                "end": { "line": 1, "character": 13 }
            }
        }
    }]);
    assert!(
        workspace_symbol_response_contains(&exact_package_name, "MyApp::Target", Some(target_uri)),
        "an exact qualified package name should satisfy readiness"
    );

    let qualified_name = json!([{
        "name": "MyApp::target",
        "location": {
            "uri": target_uri,
            "range": {
                "start": { "line": 2, "character": 4 },
                "end": { "line": 2, "character": 10 }
            }
        }
    }]);
    assert!(
        workspace_symbol_response_contains(&qualified_name, "target", Some(target_uri)),
        "a qualified symbol whose leaf name matches should satisfy readiness"
    );

    let sigiled_name = json!([{
        "name": "$target",
        "location": {
            "uri": target_uri,
            "range": {
                "start": { "line": 3, "character": 3 },
                "end": { "line": 3, "character": 10 }
            }
        }
    }]);
    assert!(
        workspace_symbol_response_contains(&sigiled_name, "target", Some(target_uri)),
        "Perl variable sigils should not prevent a matching symbol from satisfying readiness"
    );
}

#[test]
fn wait_for_symbol_rejects_same_file_decoy() -> Result<(), String> {
    let (mut harness, workspace) = LspHarness::with_workspace(&[(
        "lib/Target.pm",
        "package Decoy;\nsub target_helper { 1 }\n1;\n",
    )])?;
    let target_uri = workspace.uri("lib/Target.pm");
    harness.open(&target_uri, "package Decoy;\nsub target_helper { 1 }\n1;\n")?;
    harness.wait_for_idle(Duration::from_millis(200));

    let response = harness.request("workspace/symbol", json!({ "query": "target" }))?;
    let saw_decoy = response.as_array().is_some_and(|symbols| {
        symbols.iter().any(|symbol| {
            symbol.get("name").and_then(|name| name.as_str()) == Some("target_helper")
                && symbol
                    .pointer("/location/uri")
                    .and_then(|uri| uri.as_str())
                    .is_some_and(|uri| uri == target_uri.as_str())
        })
    });
    if !saw_decoy {
        return Err(format!("workspace/symbol did not return the decoy response: {response}"));
    }

    for mode in [WaitForSymbolMode::Default, WaitForSymbolMode::Fast] {
        let result = harness.wait_for_symbol_with_mode(
            "target",
            Some(&target_uri),
            Duration::from_secs(2),
            mode,
        );
        if result.is_ok() {
            return Err(format!("same-file decoy must not satisfy wait_for_symbol in {mode:?}"));
        }
    }
    Ok(())
}

#[test]
fn wait_for_symbol_rejects_matching_name_from_wrong_uri() -> Result<(), String> {
    let (mut harness, workspace) = LspHarness::with_workspace(&[
        ("lib/Expected.pm", "package Expected;\nsub target { 1 }\n1;\n"),
        ("lib/Other.pm", "package Other;\n1;\n"),
    ])?;
    let expected_uri = workspace.uri("lib/Expected.pm");
    let other_uri = workspace.uri("lib/Other.pm");
    harness.open(&expected_uri, "package Expected;\nsub target { 1 }\n1;\n")?;
    harness.open(&other_uri, "package Other;\n1;\n")?;
    harness.wait_for_idle(Duration::from_millis(200));

    let response = harness.request("workspace/symbol", json!({ "query": "target" }))?;
    let saw_expected = response.as_array().is_some_and(|symbols| {
        symbols.iter().any(|symbol| {
            symbol.get("name").and_then(|name| name.as_str()) == Some("target")
                && symbol
                    .pointer("/location/uri")
                    .and_then(|uri| uri.as_str())
                    .is_some_and(|uri| uri == expected_uri.as_str())
        })
    });
    if !saw_expected {
        return Err(format!("workspace/symbol did not return the expected response: {response}"));
    }

    for mode in [WaitForSymbolMode::Default, WaitForSymbolMode::Fast] {
        let budget = Duration::from_secs(2);
        let started = Instant::now();
        let result = harness.wait_for_symbol_with_mode("target", Some(&other_uri), budget, mode);
        if result.is_ok() {
            return Err(format!(
                "matching name from another URI must not satisfy wait_for_symbol in {mode:?}"
            ));
        }
        let elapsed = started.elapsed();
        if elapsed < budget.saturating_sub(Duration::from_millis(250)) {
            return Err(format!(
                "matching name from another URI returned too early in {mode:?}: elapsed {elapsed:?}"
            ));
        }
    }
    Ok(())
}

#[test]
fn harness_with_workspace_and_wait_for_symbol_matches_file_uri() -> Result<(), String> {
    let (mut harness, workspace) = LspHarness::with_workspace(&[
        (
            "lib/MyApp/Greeting.pm",
            "package MyApp::Greeting;\nsub greet {\n    return 'hi';\n}\n1;\n",
        ),
        ("main.pl", "use lib 'lib';\nuse MyApp::Greeting;\nprint MyApp::Greeting::greet();\n"),
    ])?;

    let target_uri = workspace.uri("lib/MyApp/Greeting.pm");
    harness.wait_for_symbol("greet", Some(&target_uri), Duration::from_secs(4))?;

    let symbols = harness.request(
        "workspace/symbol",
        json!({
            "query": "greet"
        }),
    )?;

    if !workspace_symbol_response_contains(&symbols, "greet", Some(&target_uri)) {
        return Err(format!(
            "Expected workspace/symbol to include greet from {target_uri}; got {symbols}"
        ));
    }

    Ok(())
}

#[test]
fn harness_timed_request_reports_duration_and_result() -> Result<(), String> {
    let mut harness = LspHarness::new();
    harness.initialize_ready("file:///workspace", None)?;

    let (result, elapsed) = harness.timed_request("workspace/symbol", json!({ "query": "" }))?;

    if !result.is_array() {
        return Err("workspace/symbol should return an array result".to_string());
    }
    if elapsed == Duration::ZERO {
        return Err("timed_request should report a non-zero elapsed duration".to_string());
    }

    Ok(())
}
