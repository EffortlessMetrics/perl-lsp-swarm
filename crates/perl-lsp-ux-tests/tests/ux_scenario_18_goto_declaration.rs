//! Scenario 18 — Go-to-declaration UX workflow coverage.
//!
//! Verifies that `textDocument/declaration` is wired up end-to-end for the LSP
//! server process used in UX regression testing.
//!
//! Contract:
//! - The static same-file `inc($value)` call MUST resolve to `sub inc` after a
//!   bounded readiness-settlement retry.
//! - Each returned result MUST include URI/range shape (`targetUri` +
//!   `targetRange` for links, or `uri` + `range` for locations).
//! - A position with no established declaration may return an empty result.
//! - No request may return a JSON-RPC error or crash the server.

use anyhow::Result;
use perl_lsp_ux_tests::binary_available;
use perl_lsp_ux_tests::{ScenarioConfig, UxHarness};
use serde_json::Value;
use std::time::Duration;

const DECLARATION_FIXTURE: &str = r#"use strict;
use warnings;

my $value = 41;

sub inc {
    my ($n) = @_;
    return $n + 1;
}

my $result = inc($value);
print "$result\n";
"#;

const CALL_LINE: u32 = 10;
const CALL_CHARACTER: u32 = 14;
const DECLARATION_LINE: u64 = 5;
const DECLARATION_ATTEMPTS: usize = 5;
const DECLARATION_RETRY_DELAY: Duration = Duration::from_millis(200);

fn declaration_with_retry(
    harness: &UxHarness,
    line: u32,
    character: u32,
) -> Result<Vec<Value>> {
    for attempt in 1..=DECLARATION_ATTEMPTS {
        let declarations = harness.declaration("declaration.pl", line, character)?;
        if !declarations.is_empty() {
            return Ok(declarations);
        }

        if attempt < DECLARATION_ATTEMPTS {
            std::thread::sleep(DECLARATION_RETRY_DELAY);
        }
    }

    Ok(Vec::new())
}

fn is_lsp_location_shape(entry: &Value) -> bool {
    let is_link = entry.get("targetUri").is_some() && entry.get("targetRange").is_some();
    let is_location = entry.get("uri").is_some() && entry.get("range").is_some();
    is_link || is_location
}

fn entry_uri(entry: &Value) -> Option<&str> {
    entry.get("targetUri").or_else(|| entry.get("uri")).and_then(Value::as_str)
}

fn entry_start_line(entry: &Value) -> Option<u64> {
    entry
        .get("targetSelectionRange")
        .or_else(|| entry.get("targetRange"))
        .or_else(|| entry.get("range"))?
        .get("start")?
        .get("line")?
        .as_u64()
}

#[test]
fn scenario_18_static_subroutine_call_resolves_to_declaration() -> Result<()> {
    if !binary_available() {
        eprintln!("SKIP scenario_18: perl-lsp binary not found");
        return Ok(());
    }

    let harness =
        UxHarness::new(ScenarioConfig::default().with_file("declaration.pl", DECLARATION_FIXTURE))?;

    harness.open_file("declaration.pl", DECLARATION_FIXTURE)?;
    let declarations = declaration_with_retry(&harness, CALL_LINE, CALL_CHARACTER)?;

    assert!(
        !declarations.is_empty(),
        "expected declaration for static `inc($value)` call at declaration.pl:{CALL_LINE}:{CALL_CHARACTER}, \
         but the server returned an empty list after {DECLARATION_ATTEMPTS} attempts"
    );

    for entry in &declarations {
        assert!(
            is_lsp_location_shape(entry),
            "declaration result must be LocationLink or Location: {entry:?}"
        );
        let uri = entry_uri(entry)
            .ok_or_else(|| anyhow::anyhow!("declaration result has no target URI: {entry:?}"))?;
        assert!(
            uri.ends_with("declaration.pl"),
            "declaration result escaped the static fixture: {entry:?}"
        );
    }

    let points_to_inc = declarations
        .iter()
        .any(|entry| entry_start_line(entry) == Some(DECLARATION_LINE));
    assert!(
        points_to_inc,
        "expected at least one declaration result to target `sub inc` on line {DECLARATION_LINE}: \
         {declarations:?}"
    );

    harness.assert_no_crash();
    Ok(())
}

#[test]
fn scenario_18_unknown_position_is_empty_or_shape_valid() -> Result<()> {
    if !binary_available() {
        eprintln!("SKIP scenario_18: perl-lsp binary not found");
        return Ok(());
    }

    let harness =
        UxHarness::new(ScenarioConfig::default().with_file("declaration.pl", DECLARATION_FIXTURE))?;

    harness.open_file("declaration.pl", DECLARATION_FIXTURE)?;
    let declarations = harness.declaration("declaration.pl", 2, 0)?;

    for entry in &declarations {
        assert!(
            is_lsp_location_shape(entry),
            "unknown-position declaration result must be LocationLink or Location: {entry:?}"
        );
    }

    harness.assert_no_crash();
    Ok(())
}
