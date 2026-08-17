//! Scenario 18 — Go-to-declaration UX workflow coverage.
//!
//! Verifies that `textDocument/declaration` is wired up end-to-end for the LSP
//! server process used in UX regression testing.
//!
//! Contract:
//! - The static same-file `inc($value)` call MUST resolve to `sub inc` after a
//!   bounded readiness-settlement retry.
//! - Each returned result MUST be a structurally valid `LocationLink`
//!   (`targetUri` + `targetRange`) or `Location` (`uri` + `range`), with every
//!   present range carrying numeric `start`/`end` line and character values.
//! - Each returned result MUST designate the fixture file this harness created,
//!   identified by canonical path rather than by basename.
//! - A position with no established declaration may return an empty result.
//! - No request may return a JSON-RPC error or crash the server.

use anyhow::{Result, anyhow};
use perl_lsp_ux_tests::binary_available;
use perl_lsp_ux_tests::{ScenarioConfig, UxHarness};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::time::Duration;
use url::Url;

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

const FIXTURE_NAME: &str = "declaration.pl";
const CALL_LINE: u32 = 10;
const CALL_CHARACTER: u32 = 14;
const DECLARATION_LINE: u64 = 5;
const DECLARATION_ATTEMPTS: usize = 5;
const DECLARATION_RETRY_DELAY: Duration = Duration::from_millis(200);

fn declaration_with_retry(harness: &UxHarness, line: u32, character: u32) -> Result<Vec<Value>> {
    for attempt in 1..=DECLARATION_ATTEMPTS {
        let declarations = harness.declaration(FIXTURE_NAME, line, character)?;
        if !declarations.is_empty() {
            return Ok(declarations);
        }

        if attempt < DECLARATION_ATTEMPTS {
            std::thread::sleep(DECLARATION_RETRY_DELAY);
        }
    }

    Ok(Vec::new())
}

/// Reject a range whose `start`/`end` positions are absent or non-numeric.
///
/// Key presence alone is not enough: a `null` range, or one whose `line` is a
/// string, is a malformed protocol result and must fail the scenario rather
/// than silently satisfy a shape check.
fn validate_range(range: &Value, key: &str) -> Result<()> {
    for bound in ["start", "end"] {
        let position =
            range.get(bound).ok_or_else(|| anyhow!("`{key}` is missing `{bound}`: {range:?}"))?;
        for field in ["line", "character"] {
            if position.get(field).and_then(Value::as_u64).is_none() {
                return Err(anyhow!(
                    "`{key}.{bound}.{field}` must be a non-negative integer: {range:?}"
                ));
            }
        }
    }
    Ok(())
}

/// Validate one declaration result completely and return its target URI.
///
/// Accepts a `LocationLink` (`targetUri` + `targetRange`, with an optional
/// `targetSelectionRange`) or a `Location` (`uri` + `range`). Every range that
/// is present is validated, so no entry can pass on key presence alone.
fn validated_target_uri(entry: &Value) -> Result<&str> {
    let (uri_key, range_key) = if entry.get("targetUri").is_some() {
        ("targetUri", "targetRange")
    } else {
        ("uri", "range")
    };

    let uri = entry
        .get(uri_key)
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("declaration result has no string `{uri_key}`: {entry:?}"))?;

    let range = entry
        .get(range_key)
        .ok_or_else(|| anyhow!("declaration result has no `{range_key}`: {entry:?}"))?;
    validate_range(range, range_key)?;

    if let Some(selection) = entry.get("targetSelectionRange") {
        validate_range(selection, "targetSelectionRange")?;
    }

    Ok(uri)
}

/// Canonicalize when the path exists, otherwise keep it as written.
fn canonical(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

/// True when `uri` designates exactly the fixture file this harness created.
///
/// Compares canonical filesystem paths rather than the raw string so a server
/// that normalizes symlinks or percent-encoding still matches, while a
/// same-basename file in any other directory is rejected.
fn uri_matches_fixture(uri: &str, expected_uri: &str, expected_path: &Path) -> bool {
    if uri == expected_uri {
        return true;
    }
    let Some(actual) = Url::parse(uri).ok().and_then(|url| url.to_file_path().ok()) else {
        return false;
    };
    canonical(&actual) == canonical(expected_path)
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

/// Assert every result is structurally valid and stays inside the fixture.
fn assert_results_are_fixture_bound(declarations: &[Value], harness: &UxHarness) -> Result<()> {
    let expected_uri = harness.workspace.uri(FIXTURE_NAME);
    let expected_path = harness.workspace.path(FIXTURE_NAME);

    for entry in declarations {
        let uri = validated_target_uri(entry)?;
        assert!(
            uri_matches_fixture(uri, &expected_uri, &expected_path),
            "declaration result escaped the static fixture: got {uri}, expected {expected_uri}"
        );
    }
    Ok(())
}

#[test]
fn scenario_18_static_subroutine_call_resolves_to_declaration() -> Result<()> {
    if !binary_available() {
        eprintln!("SKIP scenario_18: perl-lsp binary not found");
        return Ok(());
    }

    let harness =
        UxHarness::new(ScenarioConfig::default().with_file(FIXTURE_NAME, DECLARATION_FIXTURE))?;

    harness.open_file(FIXTURE_NAME, DECLARATION_FIXTURE)?;
    let declarations = declaration_with_retry(&harness, CALL_LINE, CALL_CHARACTER)?;

    assert!(
        !declarations.is_empty(),
        "expected declaration for the static `inc(...)` call at \
         {FIXTURE_NAME}:{CALL_LINE}:{CALL_CHARACTER}, but the server returned an \
         empty list after {DECLARATION_ATTEMPTS} attempts"
    );

    assert_results_are_fixture_bound(&declarations, &harness)?;

    let points_to_inc =
        declarations.iter().any(|entry| entry_start_line(entry) == Some(DECLARATION_LINE));
    assert!(
        points_to_inc,
        "expected at least one declaration result to target `sub inc` on line \
         {DECLARATION_LINE}: {declarations:?}"
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
        UxHarness::new(ScenarioConfig::default().with_file(FIXTURE_NAME, DECLARATION_FIXTURE))?;

    harness.open_file(FIXTURE_NAME, DECLARATION_FIXTURE)?;
    let declarations = harness.declaration(FIXTURE_NAME, 2, 0)?;

    assert_results_are_fixture_bound(&declarations, &harness)?;

    harness.assert_no_crash();
    Ok(())
}

/// Negative controls for the validators themselves.
///
/// These run without the server binary, so the discriminating power of the
/// scenario assertions is proven even on a runner that skips the live suite.
mod validator_falsifiers {
    use super::{uri_matches_fixture, validated_target_uri};
    use serde_json::json;
    use std::path::Path;

    #[test]
    fn well_formed_location_and_location_link_are_accepted() {
        let location = json!({
            "uri": "file:///tmp/fixture/declaration.pl",
            "range": { "start": { "line": 5, "character": 0 },
                       "end":   { "line": 5, "character": 9 } }
        });
        assert_eq!(
            validated_target_uri(&location).ok(),
            Some("file:///tmp/fixture/declaration.pl")
        );

        let link = json!({
            "targetUri": "file:///tmp/fixture/declaration.pl",
            "targetRange": { "start": { "line": 5, "character": 0 },
                             "end":   { "line": 8, "character": 1 } },
            "targetSelectionRange": { "start": { "line": 5, "character": 4 },
                                      "end":   { "line": 5, "character": 7 } }
        });
        assert_eq!(validated_target_uri(&link).ok(), Some("file:///tmp/fixture/declaration.pl"));
    }

    #[test]
    fn null_range_is_rejected() {
        let entry = json!({ "uri": "file:///tmp/fixture/declaration.pl", "range": null });
        assert!(
            validated_target_uri(&entry).is_err(),
            "a null range must not satisfy the shape check"
        );
    }

    #[test]
    fn non_numeric_range_position_is_rejected() {
        let entry = json!({
            "targetUri": "file:///tmp/fixture/declaration.pl",
            "targetRange": { "start": { "line": "5", "character": 0 },
                             "end":   { "line": 5, "character": 9 } }
        });
        assert!(
            validated_target_uri(&entry).is_err(),
            "a string `line` must not satisfy the shape check"
        );
    }

    #[test]
    fn malformed_target_selection_range_is_rejected() {
        let entry = json!({
            "targetUri": "file:///tmp/fixture/declaration.pl",
            "targetRange": { "start": { "line": 5, "character": 0 },
                             "end":   { "line": 8, "character": 1 } },
            "targetSelectionRange": { "start": { "line": 5 },
                                      "end":   { "line": 5, "character": 7 } }
        });
        assert!(
            validated_target_uri(&entry).is_err(),
            "a `targetSelectionRange` missing `character` must be rejected"
        );
    }

    #[test]
    fn missing_uri_is_rejected() {
        let entry = json!({
            "range": { "start": { "line": 5, "character": 0 },
                       "end":   { "line": 5, "character": 9 } }
        });
        assert!(validated_target_uri(&entry).is_err(), "a result with no URI must be rejected");
    }

    #[test]
    fn same_basename_outside_the_fixture_is_rejected() {
        let expected_uri = "file:///tmp/ux-fixture-a/declaration.pl";
        let expected_path = Path::new("/tmp/ux-fixture-a/declaration.pl");

        assert!(
            uri_matches_fixture(expected_uri, expected_uri, expected_path),
            "the exact fixture URI must match"
        );
        assert!(
            !uri_matches_fixture(
                "file:///tmp/ux-fixture-b/declaration.pl",
                expected_uri,
                expected_path
            ),
            "a same-basename file in another directory must not match"
        );
        assert!(
            !uri_matches_fixture(
                "file:///usr/share/perl5/declaration.pl",
                expected_uri,
                expected_path
            ),
            "a same-basename dependency path must not match"
        );
        assert!(
            !uri_matches_fixture("not-a-uri", expected_uri, expected_path),
            "an unparseable URI must not match"
        );
    }
}
