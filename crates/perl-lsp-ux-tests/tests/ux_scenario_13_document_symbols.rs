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
//! - Every returned symbol MUST have a name and either range or location shape.
//! - Symbol kinds, when present, MUST use the LSP SymbolKind range.
//! - A file with no symbols MUST return an empty list.
//! - No request may return a JSON-RPC error or crash the server.

use anyhow::Result;
use perl_lsp_ux_tests::binary_available;
use perl_lsp_ux_tests::{document_symbol_names, ScenarioConfig, UxHarness};
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

fn document_symbols_with_retry(harness: &UxHarness, path: &str) -> Result<Vec<Value>> {
    for attempt in 1..=SYMBOL_ATTEMPTS {
        let symbols = harness.document_symbols(path)?;
        if !symbols.is_empty() {
            return Ok(symbols);
        }

        if attempt < SYMBOL_ATTEMPTS {
            std::thread::sleep(SYMBOL_RETRY_DELAY);
        }
    }

    Ok(Vec::new())
}

fn assert_symbol_shapes(symbols: &[Value]) {
    for symbol in symbols {
        let name = symbol.get("name").and_then(Value::as_str).unwrap_or_default();
        assert!(!name.trim().is_empty(), "Each symbol must have a non-empty name: {symbol:?}");

        if let Some(kind) = symbol.get("kind") {
            let kind = kind.as_u64().unwrap_or_default();
            assert!((1..=26).contains(&kind), "Symbol kind must be 1-26: {symbol:?}");
        }

        let has_range = symbol.get("range").is_some();
        let has_location = symbol.get("location").is_some();
        assert!(
            has_range || has_location,
            "Symbol must have either range or location shape: {symbol:?}"
        );

        if let Some(children) = symbol.get("children").and_then(Value::as_array) {
            assert_symbol_shapes(children);
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
        !symbols.is_empty(),
        "expected package and subroutine symbols for static Greeter.pm, but documentSymbol \
         returned an empty list after {SYMBOL_ATTEMPTS} attempts"
    );
    assert_symbol_shapes(&symbols);

    let names = document_symbol_names(&symbols);
    for expected in EXPECTED_SYMBOLS {
        assert!(
            names.contains(&expected),
            "expected document symbol `{expected}` in Greeter.pm, got: {names:?}"
        );
    }

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
