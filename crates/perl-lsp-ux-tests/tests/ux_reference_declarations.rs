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

// AmperCall does not supply the cursor resolver's symbol key, so these
// reference cursors exercise the qualified-name index fallback directly.
fn qualify_fqn_reference_cursor(
    caller: &str,
    character: u32,
    first_start: u32,
    first_end: u32,
    definitions: &[(&str, &str)],
) -> TestResult {
    let binary = std::env::var("PERL_LSP_BIN")?;
    assert!(Path::new(&binary).is_absolute() && Path::new(&binary).is_file());
    let timeout = Duration::from_secs(20);
    let mut config =
        ScenarioConfig { timeout, ..ScenarioConfig::default() }.with_file("caller.pl", caller);
    for &(filename, text) in definitions {
        config = config.with_file(filename, text);
    }
    let harness = UxHarness::new(config)?;
    harness.wait_for_index_ready(timeout).map_err(|e| format!("index readiness: {e:?}"))?;
    for &(filename, text) in definitions.iter().chain(std::iter::once(&("caller.pl", caller))) {
        harness.open_file(filename, text)?;
        harness
            .wait_for_active_document_ready(&harness.workspace.uri(filename), timeout)
            .map_err(|e| format!("document readiness for {filename}: {e:?}"))?;
    }
    let caller_uri = harness.workspace.uri("caller.pl");
    let mut mismatches = Vec::new();
    for include_declaration in [false, true] {
        let response = harness.client.request(
            "textDocument/references",
            json!({
                "textDocument": {"uri": caller_uri},
                "position": {"line": 0, "character": character},
                "context": {"includeDeclaration": include_declaration}
            }),
            timeout,
        )?;
        assert!(response.get("error").is_none(), "RPC error: {response}");
        let locations = response
            .get("result")
            .and_then(Value::as_array)
            .ok_or_else(|| format!("missing/malformed references array: {response}"))?;
        let mut expected = vec![
            json!({"uri": caller_uri, "range": {
                "start": {"line": 0, "character": first_start},
                "end": {"line": 0, "character": first_end}
            }})
            .to_string(),
            json!({"uri": caller_uri, "range": {
                "start": {"line": 1, "character": 0},
                "end": {"line": 1, "character": 13}
            }})
            .to_string(),
        ];
        // Exclusion must return no Outer.pm location; inclusion adds exactly
        // the name anchor, never the compatibility query's full declaration.
        if include_declaration {
            for &(filename, _) in definitions {
                expected.push(
                    json!({"uri": harness.workspace.uri(filename), "range": {
                        "start": {"line": 1, "character": 4},
                        "end": {"line": 1, "character": 8}
                    }})
                    .to_string(),
                );
            }
        }
        let mut actual: Vec<_> = locations.iter().map(Value::to_string).collect();
        actual.sort();
        expected.sort();
        if actual != expected {
            mismatches.push(format!(
                "includeDeclaration={include_declaration}: expected {expected:?}, actual {actual:?}"
            ));
        }
    }
    harness.client.shutdown_and_exit(timeout)?;
    assert!(
        mismatches.is_empty(),
        "complete FQN reference set mismatches:\n{}",
        mismatches.join("\n")
    );
    Ok(())
}

#[test]
fn qualified_ampersand_call_cursor_respects_declaration_inclusion() -> TestResult {
    qualify_fqn_reference_cursor(
        "&Outer::mark();\nOuter::mark();\n",
        10,
        0,
        14,
        &[("Outer.pm", "package Outer;\nsub mark {}\n")],
    )
}

#[test]
fn qualified_code_reference_cursor_respects_declaration_inclusion() -> TestResult {
    qualify_fqn_reference_cursor(
        "\\&Outer::mark;\nOuter::mark();\n",
        11,
        1,
        13,
        &[("Outer.pm", "package Outer;\nsub mark {}\n")],
    )
}

#[test]
fn qualified_ampersand_cursor_includes_all_definition_sites() -> TestResult {
    qualify_fqn_reference_cursor(
        "&Outer::mark();\nOuter::mark();\n",
        10,
        0,
        14,
        &[
            ("Outer.pm", "package Outer;\nsub mark {}\n"),
            ("Second.pm", "package Outer;\nsub mark {}\n"),
        ],
    )
}

#[test]
fn qualified_code_reference_cursor_includes_all_definition_sites() -> TestResult {
    qualify_fqn_reference_cursor(
        "\\&Outer::mark;\nOuter::mark();\n",
        11,
        1,
        13,
        &[
            ("Outer.pm", "package Outer;\nsub mark {}\n"),
            ("Second.pm", "package Outer;\nsub mark {}\n"),
        ],
    )
}

#[test]
fn explicitly_qualified_declaration_obeys_inclusion() -> TestResult {
    let binary = std::env::var("PERL_LSP_BIN")?;
    assert!(Path::new(&binary).is_absolute() && Path::new(&binary).is_file());
    let timeout = Duration::from_secs(20);
    let declarations = "sub Wanted::mark {}\n";
    let caller = "# Wanted::mark\n";
    let harness = UxHarness::new(
        ScenarioConfig { timeout, ..ScenarioConfig::default() }
            .with_file("Wanted.pm", declarations)
            .with_file("caller.pl", caller),
    )?;
    harness.wait_for_index_ready(timeout).map_err(|e| format!("index readiness: {e:?}"))?;
    for (filename, text) in [("Wanted.pm", declarations), ("caller.pl", caller)] {
        harness.open_file(filename, text)?;
        harness
            .wait_for_active_document_ready(&harness.workspace.uri(filename), timeout)
            .map_err(|e| format!("document readiness for {filename}: {e:?}"))?;
    }
    let mut mismatches = Vec::new();
    for include_declaration in [false, true] {
        let response = harness.client.request(
            "textDocument/references",
            json!({
                "textDocument": {"uri": harness.workspace.uri("caller.pl")},
                "position": {"line": 0, "character": 11},
                "context": {"includeDeclaration": include_declaration}
            }),
            timeout,
        )?;
        assert!(response.get("error").is_none(), "RPC error: {response}");
        let locations = response
            .get("result")
            .and_then(Value::as_array)
            .ok_or_else(|| format!("missing/malformed references array: {response}"))?;
        let expected = if include_declaration {
            vec![json!({"uri": harness.workspace.uri("Wanted.pm"), "range": {
                "start": {"line": 0, "character": 12},
                "end": {"line": 0, "character": 16}
            }})]
        } else {
            Vec::new()
        };
        if locations != &expected {
            mismatches.push(format!("includeDeclaration={include_declaration}: expected {expected:?}, actual {locations:?}"));
        }
    }
    harness.client.shutdown_and_exit(timeout)?;
    assert!(
        mismatches.is_empty(),
        "qualified declaration reference mismatches:\n{}",
        mismatches.join("\n")
    );
    Ok(())
}
