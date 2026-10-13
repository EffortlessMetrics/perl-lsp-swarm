#![cfg(feature = "integration-test")]

//! Complete public-protocol reference sets for same-named package declarations.
use perl_lsp_ux_tests::{ScenarioConfig, UxHarness};
use serde_json::{Value, json};
use std::path::Path;
use std::time::Duration;

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[test]
fn references_respect_declaration_role_and_package_identity() -> TestResult {
    let binary = std::env::var("PERL_LSP_BIN")?;
    assert!(Path::new(&binary).is_absolute() && Path::new(&binary).is_file());
    let timeout = Duration::from_secs(20);
    let modules = [
        ("Outer", "package Outer;\nsub mark {}\nsub unused {}\nsub control {}\n"),
        ("Inner", "package Inner;\nsub mark {}\n"),
        ("Third", "package Third;\nsub mark {}\n"),
    ];
    let mut config = ScenarioConfig { timeout, ..ScenarioConfig::default() }.with_file(
        "caller.pl",
        "Outer::mark();\nInner::mark();\nThird::mark();\nOuter::control();\n",
    );
    for (package, text) in modules {
        config = config.with_file(format!("{package}.pm"), text);
    }
    let harness = UxHarness::new(config)?;
    harness.wait_for_index_ready(timeout).map_err(|e| format!("index readiness: {e:?}"))?;
    // Open all declarations before any query. Disk call sites remain closed.
    for (package, text) in modules {
        let filename = format!("{package}.pm");
        harness.open_file(&filename, text)?;
        harness
            .wait_for_active_document_ready(&harness.workspace.uri(&filename), timeout)
            .map_err(|e| format!("document readiness for {package}: {e:?}"))?;
    }
    let mut mismatches = Vec::new();
    for (package, declaration_line, name, call_line) in [
        ("Outer", 1, "mark", Some(0)),
        ("Inner", 1, "mark", Some(1)),
        ("Third", 1, "mark", Some(2)),
        ("Outer", 2, "unused", None),
        ("Outer", 3, "control", Some(3)),
    ] {
        let uri = harness.workspace.uri(&format!("{package}.pm"));
        for include_declaration in [false, true] {
            let response = harness.client.request("textDocument/references", json!({
                "textDocument": {"uri": uri}, "position": {"line": declaration_line, "character": 5},
                "context": {"includeDeclaration": include_declaration}
            }), timeout)?;
            assert!(response.get("error").is_none(), "RPC error: {response}");
            let locations = response
                .get("result")
                .and_then(Value::as_array)
                .ok_or_else(|| format!("missing/malformed references array: {response}"))?;
            let mut expected = Vec::new();
            if let Some(line) = call_line {
                expected.push(
                    json!({"uri": harness.workspace.uri("caller.pl"), "range": {
                        "start": {"line": line, "character": 0},
                        "end": {"line": line, "character": package.len() + 2 + name.len() + 2}
                    }})
                    .to_string(),
                );
            }
            if include_declaration {
                expected.push(
                    json!({"uri": uri, "range": {
                        "start": {"line": declaration_line, "character": 4},
                        "end": {"line": declaration_line, "character": 4 + name.len()}
                    }})
                    .to_string(),
                );
            }
            let mut actual: Vec<_> = locations.iter().map(Value::to_string).collect();
            actual.sort();
            expected.sort();
            if actual != expected {
                mismatches.push(format!("{package}::{name}, includeDeclaration={include_declaration}: expected {expected:?}, actual {actual:?}"));
            }
        }
    }
    harness.client.shutdown_and_exit(timeout)?;
    assert!(mismatches.is_empty(), "complete reference set mismatches:\n{}", mismatches.join("\n"));
    Ok(())
}
