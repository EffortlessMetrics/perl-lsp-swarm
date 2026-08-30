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
//! - Close/reopen MUST observe a new generation-sensitive readiness event and
//!   document symbols MUST come from the reopened editor buffer, not the disk
//!   snapshot or either prior open-document generation.
//! - No request may return a JSON-RPC error or crash the server.

use anyhow::{Result, bail};
use perl_lsp_ux_tests::{
    LspEvent, ScenarioConfig, UxHarness, binary_available, document_symbol_names,
};
use serde_json::{Value, json};
use std::time::{Duration, Instant};

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

const LIFECYCLE_FILE: &str = "lifecycle.pl";
const READY_METHOD: &str = "perl-lsp/active-document-ready";
const DISK_SYMBOL: &str = "disk_symbol";
const INITIAL_SYMBOL: &str = "initial_symbol";
const PRE_CLOSE_SYMBOL: &str = "pre_close_symbol";
const REOPENED_SYMBOL: &str = "reopened_symbol";
const READY_TIMEOUT: Duration = Duration::from_secs(10);
const READY_POLL_INTERVAL: Duration = Duration::from_millis(20);

const DISK_SOURCE: &str = r#"use strict;
use warnings;

sub disk_symbol {
    return "disk";
}

disk_symbol();
"#;

const INITIAL_SOURCE: &str = r#"use strict;
use warnings;

sub initial_symbol {
    return "initial";
}

initial_symbol();
"#;

const PRE_CLOSE_SOURCE: &str = r#"use strict;
use warnings;

sub pre_close_symbol {
    return "pre-close";
}

pre_close_symbol();
"#;

const REOPENED_SOURCE: &str = r#"use strict;
use warnings;

sub reopened_symbol {
    return "reopened";
}

reopened_symbol();
"#;

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

fn ready_generation(event: &LspEvent, uri: &str) -> Option<u64> {
    let LspEvent::Other { method, params } = event else {
        return None;
    };
    if method != READY_METHOD || params.get("uri").and_then(Value::as_str) != Some(uri) {
        return None;
    }
    params.get("generation").and_then(Value::as_u64)
}

fn ready_generations(events: &[LspEvent], uri: &str) -> Vec<u64> {
    events.iter().filter_map(|event| ready_generation(event, uri)).collect()
}

fn has_generation_after(
    generations: &[u64],
    already_seen: usize,
    expected_generation: u64,
) -> bool {
    generations
        .get(already_seen..)
        .is_some_and(|new_generations| new_generations.contains(&expected_generation))
}

fn wait_for_ready_generation_after(
    harness: &UxHarness,
    uri: &str,
    expected_generation: u64,
    already_seen: usize,
    timeout: Duration,
) -> Result<Vec<u64>> {
    let deadline = Instant::now() + timeout;
    loop {
        let generations = ready_generations(&harness.peek_notifications(), uri);
        if has_generation_after(&generations, already_seen, expected_generation) {
            return Ok(generations);
        }
        if Instant::now() >= deadline {
            bail!(
                "timed out after {}ms waiting for a new {READY_METHOD} event for {uri} with \
                 generation {expected_generation} after {already_seen} prior matching events; \
                 observed matching generations: {generations:?}",
                timeout.as_millis()
            );
        }
        std::thread::sleep(READY_POLL_INTERVAL);
    }
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

#[test]
fn scenario_13_close_reopen_requires_new_generation_and_open_buffer_authority() -> Result<()> {
    if !binary_available() {
        eprintln!("SKIP scenario_13 close/reopen: perl-lsp binary not found");
        return Ok(());
    }

    let harness = UxHarness::new(
        ScenarioConfig { timeout: Duration::from_secs(15), ..Default::default() }
            .env("PERL_LSP_WORKSPACE", "1")
            .env("PERL_LSP_E2E", "1")
            .with_file(LIFECYCLE_FILE, DISK_SOURCE),
    )?;
    let uri = harness.workspace.uri(LIFECYCLE_FILE);

    harness.client.did_open(&uri, INITIAL_SOURCE)?;
    let initial_generations = wait_for_ready_generation_after(&harness, &uri, 1, 0, READY_TIMEOUT)?;
    let initial_ready_count = initial_generations.len();

    harness.client.did_change_full(&uri, 2, PRE_CLOSE_SOURCE)?;
    let pre_close_generations =
        wait_for_ready_generation_after(&harness, &uri, 2, initial_ready_count, READY_TIMEOUT)?;
    let pre_close_ready_count = pre_close_generations.len();

    harness.client.notify(
        "textDocument/didClose",
        json!({
            "textDocument": {
                "uri": uri.clone()
            }
        }),
    )?;
    harness.client.did_open(&uri, REOPENED_SOURCE)?;

    let reopened_generations =
        wait_for_ready_generation_after(&harness, &uri, 1, pre_close_ready_count, READY_TIMEOUT)?;
    assert!(
        reopened_generations.len() > pre_close_ready_count,
        "reopen barrier must be backed by post-snapshot readiness evidence: \
         snapshot={pre_close_ready_count}, observed={reopened_generations:?}"
    );
    let post_snapshot_generations = &reopened_generations[pre_close_ready_count..];
    assert!(
        post_snapshot_generations.contains(&1),
        "reopen barrier must observe generation 1 after close/reopen; \
         post-snapshot generations: {post_snapshot_generations:?}"
    );

    let disk_source = std::fs::read_to_string(harness.workspace.path(LIFECYCLE_FILE))?;
    assert_eq!(
        disk_source, DISK_SOURCE,
        "test setup must keep the backing file distinct from all open-buffer generations"
    );

    let symbols = harness.document_symbols(LIFECYCLE_FILE)?;
    let names = document_symbol_names(&symbols);
    assert!(
        names.iter().any(|name| *name == REOPENED_SYMBOL),
        "document symbols after the reopen barrier must come from the reopened buffer; got {names:?}"
    );
    for stale_symbol in [DISK_SYMBOL, INITIAL_SYMBOL, PRE_CLOSE_SYMBOL] {
        assert!(
            !names.iter().any(|name| *name == stale_symbol),
            "document symbols after reopen must not expose stale/backing `{stale_symbol}`; got {names:?}"
        );
    }

    harness.assert_no_crash();
    Ok(())
}

#[cfg(test)]
mod shape_unit_tests {
    use super::{READY_METHOD, assert_symbol_shapes, has_generation_after, ready_generations};
    use perl_lsp_ux_tests::LspEvent;
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

    #[test]
    fn late_pre_close_generation_does_not_release_reopen_barrier() {
        let mut generations = vec![1, 2];
        let snapshot = generations.len();

        assert!(!has_generation_after(&generations, snapshot, 1));

        generations.push(2);
        assert!(
            !has_generation_after(&generations, snapshot, 1),
            "a delayed pre-close generation must not release the generation-1 reopen barrier"
        );

        generations.push(1);
        assert!(
            has_generation_after(&generations, snapshot, 1),
            "a new post-snapshot generation 1 must release the reopen barrier"
        );
    }

    #[test]
    fn readiness_filter_requires_matching_uri_method_and_numeric_generation() {
        let wanted_uri = "file:///workspace/lifecycle.pl";
        let events = vec![
            LspEvent::Other {
                method: READY_METHOD.to_string(),
                params: json!({"uri": wanted_uri, "generation": 1}),
            },
            LspEvent::Other {
                method: READY_METHOD.to_string(),
                params: json!({"uri": "file:///workspace/other.pl", "generation": 2}),
            },
            LspEvent::Other {
                method: "perl-lsp/other".to_string(),
                params: json!({"uri": wanted_uri, "generation": 3}),
            },
            LspEvent::Other {
                method: READY_METHOD.to_string(),
                params: json!({"uri": wanted_uri, "generation": "4"}),
            },
        ];

        assert_eq!(ready_generations(&events, wanted_uri), vec![1]);
    }
}
