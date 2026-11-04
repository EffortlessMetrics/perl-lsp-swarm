#![cfg(feature = "integration-test")]

//! Real-stdio proof of package ownership for call sites in a closed disk file.
//!
//! This bounded slice does not claim a complete provider reference set: the
//! legacy text fallback's extra declaration locations are tracked under #811.

use perl_lsp_ux_tests::{ScenarioConfig, UxHarness};
use serde_json::{Value, json};
use std::path::Path;
use std::time::Duration;

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn qualify_disk_call_sites(caller: &str, expected: &[(&str, &[u32])]) -> TestResult {
    // Refuse a silent skip or a different executable selected from PATH.
    let binary = std::env::var("PERL_LSP_BIN")?;
    assert!(Path::new(&binary).is_absolute() && Path::new(&binary).is_file());
    let timeout = Duration::from_secs(20);
    let harness = UxHarness::new(
        ScenarioConfig { timeout, ..ScenarioConfig::default() }
            .with_file("Outer.pm", "package Outer;\nsub mark {}\n")
            .with_file("Inner.pm", "package Inner;\nsub mark {}\n")
            .with_file("Third.pm", "package Third;\nsub mark {}\n")
            .with_file("caller.pl", caller),
    )?;
    harness.wait_for_index_ready(timeout).map_err(|error| format!("index readiness: {error:?}"))?;
    let caller_uri = harness.workspace.uri("caller.pl");

    for (package, lines) in expected {
        let name = format!("{package}.pm");
        let declaration = format!("package {package};\nsub mark {{}}\n");
        harness.open_file(&name, &declaration)?;
        let uri = harness.workspace.uri(&name);
        harness
            .wait_for_active_document_ready(&uri, timeout)
            .map_err(|error| format!("document readiness for {package}: {error:?}"))?;
        for include_declaration in [false, true] {
            let response = harness.client.request(
                "textDocument/references",
                json!({
                    "textDocument": {"uri": uri},
                    "position": {"line": 1, "character": 5},
                    "context": {"includeDeclaration": include_declaration}
                }),
                timeout,
            )?;
            assert!(response.get("error").is_none(), "references RPC error: {response}");
            let locations = response
                .get("result")
                .and_then(Value::as_array)
                .ok_or_else(|| format!("missing/malformed references array: {response}"))?;
            // caller.pl is never opened, so the open-document text fallback
            // cannot supply these disk call sites or hide a dropped index entry.
            let mut actual: Vec<_> = locations
                .iter()
                .filter(|location| location.get("uri").and_then(Value::as_str) == Some(&caller_uri))
                .map(Value::to_string)
                .collect();
            let mut expected: Vec<_> = lines
                .iter()
                .map(|line| {
                    json!({
                        "uri": caller_uri,
                        "range": {
                            "start": {"line": line, "character": 0},
                            "end": {"line": line, "character": 6}
                        }
                    })
                    .to_string()
                })
                .collect();
            actual.sort();
            expected.sort();
            assert_eq!(
                actual, expected,
                "{package} call sites, includeDeclaration={include_declaration}; full response: {response}"
            );
        }
    }
    harness.client.shutdown_and_exit(timeout)?;
    Ok(())
}

#[test]
fn nested_package_disk_call_sites_through_real_stdio() -> TestResult {
    qualify_disk_call_sites(
        "package Outer;\nmark();\npackage Inner {\nmark();\npackage Third {\nmark();\n}\nmark();\n}\nmark();\npackage Third;\nmark();\n",
        &[("Outer", &[1, 9]), ("Inner", &[3, 7]), ("Third", &[5, 11])],
    )
}

#[test]
fn statement_package_disk_call_sites_through_real_stdio() -> TestResult {
    qualify_disk_call_sites(
        "package Outer;\nmark();\n{\npackage Inner;\nmark();\n}\nmark();\npackage Third;\nmark();\n",
        &[("Outer", &[1, 6]), ("Inner", &[4]), ("Third", &[8])],
    )
}
