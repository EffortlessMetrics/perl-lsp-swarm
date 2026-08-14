// Test infrastructure — allow test-friendly patterns.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! Scenario 13 — Document symbols feature grid coverage.
//!
//! Verifies that `textDocument/documentSymbol` is wired up end-to-end for the
//! LSP feature advertised in `features.toml`.
//!
//! Acceptance criteria:
//! - The rich static module MUST return its package and all named subs after a
//!   bounded readiness-settlement retry.
//! - Every returned symbol MUST be exactly one recognized LSP result form:
//!   DocumentSymbol (`kind` + object `range`) or SymbolInformation (`kind` +
//!   object `location` with `uri` and object `range`).
//! - A file with no symbols MUST return an empty list.
//! - No request may return a JSON-RPC error or crash the server.

use anyhow::Result;
use perl_lsp_ux_tests::binary_available;
use perl_lsp_ux_tests::{ScenarioConfig, UxHarness, document_symbol_names};
use serde_json::Value;
use std::time::Duration;

/// Source with three named subs and a package declaration — rich symbol table.
const SYMBOLS_SOURCE: &str = "\
package Greeter;\n\
use strict;\n\
use warnings;\n\
\n\
sub new {\n\
    my ($class, %opts) = @_;\n\
    return bless { name => $opts{name} // 'World' }, $class;\n\
}\n\
\n\
sub greet {\n\
    my ($self) = @_;\n\
    return \"Hello, \" . $self->{name} . \"!\";\n\
}\n\
\n\
sub farewell {\n\
    my ($self) = @_;\n\
    return \"Goodbye, \" . $self->{name} . \"!\";\n\
}\n\
\n\
1;\n\
";

const SYMBOL_ATTEMPTS: usize = 5;
const SYMBOL_RETRY_DELAY: Duration = Duration::from_millis(200);
const EXPECTED_SYMBOLS: [&str; 4] = ["Greeter", "new", "greet", "farewell"];

fn expected_symbol_set_present(symbols: &[Value]) -> bool {
    let names = document_symbol_names(symbols);
    EXPECTED_SYMBOLS.iter().all(|expected| names.iter().any(|name| name == expected))
}

fn document_symbols_with_retry(harness: &UxHarness, path: &str) -> Result<Vec<Value>> {
    let mut last = Vec::new();
    for attempt in 1..=SYMBOL_ATTEMPTS {
        let symbols = harness.document_symbols(path)?;
        if expected_symbol_set_present(&symbols) {
            return Ok(symbols);
        }
        last = symbols;
        if attempt < SYMBOL_ATTEMPTS {
            std::thread::sleep(SYMBOL_RETRY_DELAY);
        }
    }
    Ok(last)
}

fn require_lsp_range(value: &Value, context: &str) -> Result<(), String> {
    let range = value.as_object().ok_or_else(|| format!("{context} must be an object"))?;
    for field in ["start", "end"] {
        let pos = range
            .get(field)
            .and_then(Value::as_object)
            .ok_or_else(|| format!("{context}.{field} must be an object"))?;
        for coord in ["line", "character"] {
            if pos.get(coord).and_then(Value::as_u64).is_none() {
                return Err(format!("{context}.{field}.{coord} must be a u64"));
            }
        }
    }
    Ok(())
}

fn assert_symbol_shapes(symbols: &[Value]) {
    for symbol in symbols {
        let name = symbol
            .get("name")
            .and_then(Value::as_str)
            .filter(|name| !name.trim().is_empty())
            .unwrap_or_else(|| panic!("Each symbol must have a non-empty name: {symbol:?}"));

        let kind = symbol
            .get("kind")
            .and_then(Value::as_u64)
            .unwrap_or_else(|| panic!("Symbol `{name}` must include LSP SymbolKind: {symbol:?}"));
        assert!((1..=26).contains(&kind), "Symbol `{name}` kind must be 1-26: {symbol:?}");

        let has_range = symbol.get("range").is_some();
        let has_location = symbol.get("location").is_some();
        assert!(
            has_range ^ has_location,
            "Symbol `{name}` must be exactly one of DocumentSymbol or SymbolInformation: {symbol:?}"
        );

        if has_range {
            require_lsp_range(
                symbol.get("range").expect("range present"),
                &format!("DocumentSymbol `{name}` range"),
            )
            .unwrap_or_else(|err| panic!("{err}: {symbol:?}"));
            if let Some(children) = symbol.get("children") {
                let children = children.as_array().unwrap_or_else(|| {
                    panic!("DocumentSymbol `{name}` children must be an array: {symbol:?}")
                });
                assert_symbol_shapes(children);
            }
        } else {
            let location = symbol.get("location").and_then(Value::as_object).unwrap_or_else(|| {
                panic!("SymbolInformation `{name}` location must be an object: {symbol:?}")
            });
            let uri = location.get("uri").and_then(Value::as_str).unwrap_or_default();
            assert!(
                !uri.trim().is_empty(),
                "SymbolInformation `{name}` location.uri must be non-empty: {symbol:?}"
            );
            require_lsp_range(
                location.get("range").unwrap_or(&Value::Null),
                &format!("SymbolInformation `{name}` location.range"),
            )
            .unwrap_or_else(|err| panic!("{err}: {symbol:?}"));
            assert!(
                symbol.get("children").is_none(),
                "SymbolInformation `{name}` must not carry DocumentSymbol children: {symbol:?}"
            );
        }
    }
}

#[test]
fn scenario_13_rich_file_returns_all_known_symbols() -> Result<()> {
    if !binary_available() {
        eprintln!("SKIP scenario_13: perl-lsp binary not found");
        return Ok(());
    }

    let harness = UxHarness::new(
        ScenarioConfig { timeout: Duration::from_secs(15), ..Default::default() }
            .with_file("Greeter.pm", SYMBOLS_SOURCE),
    )?;

    harness.open_file("Greeter.pm", SYMBOLS_SOURCE)?;
    let symbols = document_symbols_with_retry(&harness, "Greeter.pm")?;

    assert!(
        expected_symbol_set_present(&symbols),
        "expected package and all named subroutine symbols for static Greeter.pm after \
         {SYMBOL_ATTEMPTS} settlement attempts, got: {:?}",
        document_symbol_names(&symbols)
    );
    assert_symbol_shapes(&symbols);

    harness.assert_no_crash();
    Ok(())
}

#[test]
fn scenario_13_empty_file_returns_empty() -> Result<()> {
    if !binary_available() {
        eprintln!("SKIP scenario_13: perl-lsp binary not found");
        return Ok(());
    }

    let source = "# empty file\n";
    let harness = UxHarness::new(ScenarioConfig::default().with_file("empty.pl", source))?;

    harness.open_file("empty.pl", source)?;
    let symbols = harness.document_symbols("empty.pl")?;

    assert!(symbols.is_empty(), "file with no symbols must return an empty list: {symbols:?}");
    harness.assert_no_crash();
    Ok(())
}

#[cfg(test)]
mod shape_unit_tests {
    use super::assert_symbol_shapes;
    use serde_json::json;

    #[test]
    fn rejects_null_range_and_missing_kind() {
        let symbols = vec![json!({
            "name": "Greeter",
            "range": null,
            "location": {}
        })];
        let result = std::panic::catch_unwind(|| assert_symbol_shapes(&symbols));
        assert!(result.is_err(), "malformed null range / empty location must fail");
    }

    #[test]
    fn accepts_document_symbol_shape() {
        let symbols = vec![json!({
            "name": "Greeter",
            "kind": 4,
            "range": {
                "start": {"line": 0, "character": 0},
                "end": {"line": 0, "character": 7}
            },
            "selectionRange": {
                "start": {"line": 0, "character": 0},
                "end": {"line": 0, "character": 7}
            },
            "children": [{
                "name": "greet",
                "kind": 12,
                "range": {
                    "start": {"line": 1, "character": 0},
                    "end": {"line": 3, "character": 1}
                }
            }]
        })];
        assert_symbol_shapes(&symbols);
    }

    #[test]
    fn accepts_symbol_information_shape() {
        let symbols = vec![json!({
            "name": "greet",
            "kind": 12,
            "location": {
                "uri": "file:///tmp/Greeter.pm",
                "range": {
                    "start": {"line": 1, "character": 0},
                    "end": {"line": 3, "character": 1}
                }
            }
        })];
        assert_symbol_shapes(&symbols);
    }
}
