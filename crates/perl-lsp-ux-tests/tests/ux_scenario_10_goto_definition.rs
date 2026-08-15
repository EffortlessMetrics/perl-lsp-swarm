//! Scenario 10 — Go-to-definition UX workflow coverage.
//!
//! Focus area: `textDocument/definition` behavior for first-editing-session UX.
//! This suite uses a compact BDD-style helper so each test describes intent in
//! Given/When/Then language and avoids duplicated harness boilerplate.

use anyhow::{Result, anyhow};
use perl_lsp_ux_tests::binary_available;
use perl_lsp_ux_tests::{ScenarioConfig, UxHarness};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::time::Duration;
use url::Url;

const SAME_FILE_FIXTURE: &str = r#"use strict;
use warnings;

sub greet {
    my ($name) = @_;
    return "Hello, $name!";
}

greet('World');
"#;

const CROSS_FILE_MODULE: &str = r#"package Counter;
use strict;
use warnings;

sub increment {
    my ($class, $n) = @_;
    return $n + 1;
}

1;
"#;

const CROSS_FILE_SCRIPT: &str = r#"use strict;
use warnings;
use lib 'lib';
use Counter;

my $value = Counter->increment(3);
print "$value\n";
"#;

struct DefinitionScenario {
    harness: UxHarness,
}

impl DefinitionScenario {
    fn single_file() -> Result<Self> {
        let harness = UxHarness::new(
            ScenarioConfig::default()
                .with_file("greet.pl", SAME_FILE_FIXTURE)
                .with_file("lib/Counter.pm", CROSS_FILE_MODULE)
                .with_file("main.pl", CROSS_FILE_SCRIPT),
        )?;
        Ok(Self { harness })
    }

    fn given_file_is_open(&self, path: &str, content: &str) -> Result<()> {
        self.harness.open_file(path, content)
    }

    fn when_requesting_definition_with_retry(
        &self,
        path: &str,
        line: u32,
        character: u32,
    ) -> Result<Vec<Value>> {
        self.harness.definition_with_retry(path, line, character, 5, Duration::from_millis(200))
    }

    fn then_no_crash_signals_exist(&self) {
        self.harness.assert_no_crash();
    }

    /// Canonical URI and path for a file this harness created.
    fn fixture(&self, relative_path: &str) -> (String, PathBuf) {
        (self.harness.workspace.uri(relative_path), self.harness.workspace.path(relative_path))
    }

    /// Validate every result and assert each target is one of `allowed`.
    ///
    /// `allowed` holds relative fixture paths; each is resolved to the URI and
    /// path this harness actually created, so a same-basename file from outside
    /// the fixture cannot satisfy the assertion.
    fn then_targets_stay_within(&self, definitions: &[Value], allowed: &[&str]) -> Result<()> {
        let expected: Vec<(String, PathBuf)> =
            allowed.iter().map(|path| self.fixture(path)).collect();

        for entry in definitions {
            let uri = validated_target_uri(entry)?;
            let within = expected
                .iter()
                .any(|(expected_uri, expected_path)| uri_matches(uri, expected_uri, expected_path));
            assert!(
                within,
                "definition target escaped the fixture: got {uri}, allowed {allowed:?}"
            );
        }
        Ok(())
    }

    /// True when at least one validated target is the named fixture file.
    fn any_target_is(&self, definitions: &[Value], relative_path: &str) -> bool {
        let (expected_uri, expected_path) = self.fixture(relative_path);
        definitions.iter().any(|entry| {
            validated_target_uri(entry)
                .is_ok_and(|uri| uri_matches(uri, &expected_uri, &expected_path))
        })
    }
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

/// Validate one definition result completely and return its target URI.
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
        .ok_or_else(|| anyhow!("definition result has no string `{uri_key}`: {entry:?}"))?;

    let range = entry
        .get(range_key)
        .ok_or_else(|| anyhow!("definition result has no `{range_key}`: {entry:?}"))?;
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

/// True when `uri` designates exactly the expected fixture file.
///
/// Compares canonical filesystem paths rather than the raw string so a server
/// that normalizes symlinks or percent-encoding still matches, while a
/// same-basename file in any other directory is rejected.
fn uri_matches(uri: &str, expected_uri: &str, expected_path: &Path) -> bool {
    if uri == expected_uri {
        return true;
    }
    let Some(actual) = Url::parse(uri).ok().and_then(|url| url.to_file_path().ok()) else {
        return false;
    };
    canonical(&actual) == canonical(expected_path)
}

#[test]
fn scenario_10_definition_same_file_call_site_resolves() -> Result<()> {
    if !binary_available() {
        eprintln!("SKIP scenario_10: perl-lsp binary not found");
        return Ok(());
    }

    let scenario = DefinitionScenario::single_file()?;

    // Given a script with an in-file sub definition and call site.
    scenario.given_file_is_open("greet.pl", SAME_FILE_FIXTURE)?;

    // When go-to-definition is requested on `greet('World')`.
    let definitions = scenario.when_requesting_definition_with_retry("greet.pl", 8, 0)?;

    // Then the server must return at least one result — the `sub greet`
    // declaration is literally six lines above the call site. An empty list
    // after retry indicates goto-definition is broken, not degraded UX.
    assert!(
        !definitions.is_empty(),
        "expected at least one definition location for same-file `greet()` call site \
         (sub defined on line 3) but got empty list after retries"
    );
    // And every target must be shape-valid and stay inside this fixture, with
    // at least one pointing back to greet.pl — otherwise the server resolved
    // the call to some unrelated file (real regression).
    scenario.then_targets_stay_within(&definitions, &["greet.pl"])?;
    assert!(
        scenario.any_target_is(&definitions, "greet.pl"),
        "expected at least one definition result to point back to greet.pl, got: {definitions:?}"
    );

    scenario.then_no_crash_signals_exist();
    Ok(())
}

#[test]
fn scenario_10_definition_cross_file_module_symbol_points_to_module() -> Result<()> {
    if !binary_available() {
        eprintln!("SKIP scenario_10: perl-lsp binary not found");
        return Ok(());
    }

    let scenario = DefinitionScenario::single_file()?;

    // Given a static workspace module and script using it through `use lib`.
    scenario.given_file_is_open("lib/Counter.pm", CROSS_FILE_MODULE)?;
    scenario.given_file_is_open("main.pl", CROSS_FILE_SCRIPT)?;

    // When go-to-definition is requested on `increment` in `Counter->increment`.
    let definitions = scenario.when_requesting_definition_with_retry("main.pl", 5, 23)?;

    // Then bounded retry must produce a useful cross-file location. This fixture
    // is static and checked in; a persistent empty response is a broken first-use
    // navigation path rather than an accepted dynamic boundary.
    assert!(
        !definitions.is_empty(),
        "expected a cross-file definition for static Counter->increment at \
         main.pl:5:23, but the server returned an empty list after retries"
    );
    // The call-site file may appear in a LocationLink origin, but no returned
    // target may escape the two-file fixture, and every target must be
    // shape-valid.
    scenario.then_targets_stay_within(&definitions, &["lib/Counter.pm", "main.pl"])?;

    assert!(
        scenario.any_target_is(&definitions, "lib/Counter.pm"),
        "cross-file definition results must include Counter.pm: {definitions:?}"
    );

    scenario.then_no_crash_signals_exist();
    Ok(())
}

#[test]
fn scenario_10_definition_unknown_position_is_stable() -> Result<()> {
    if !binary_available() {
        eprintln!("SKIP scenario_10: perl-lsp binary not found");
        return Ok(());
    }

    let scenario = DefinitionScenario::single_file()?;

    // Given a tiny script with no resolvable symbol at the cursor position.
    let unknown_fixture = "use strict;\nmy $x = 1;\n";
    scenario.given_file_is_open("unknown.pl", unknown_fixture)?;

    // When go-to-definition is requested over `strict`.
    let definitions = scenario.when_requesting_definition_with_retry("unknown.pl", 0, 5)?;

    // Then the response is either empty or contains shape-valid locations that
    // stay inside the fixture.
    scenario.then_targets_stay_within(&definitions, &["unknown.pl"])?;

    scenario.then_no_crash_signals_exist();
    Ok(())
}

/// Negative controls for the validators themselves.
///
/// These run without the server binary, so the discriminating power of the
/// scenario assertions is proven even on a runner that skips the live suite.
mod validator_falsifiers {
    use super::{uri_matches, validated_target_uri};
    use serde_json::json;
    use std::path::Path;

    #[test]
    fn well_formed_location_and_location_link_are_accepted() {
        let location = json!({
            "uri": "file:///tmp/fixture/lib/Counter.pm",
            "range": { "start": { "line": 5, "character": 0 },
                       "end":   { "line": 5, "character": 9 } }
        });
        assert_eq!(
            validated_target_uri(&location).ok(),
            Some("file:///tmp/fixture/lib/Counter.pm")
        );

        let link = json!({
            "targetUri": "file:///tmp/fixture/lib/Counter.pm",
            "targetRange": { "start": { "line": 5, "character": 0 },
                             "end":   { "line": 8, "character": 1 } },
            "targetSelectionRange": { "start": { "line": 5, "character": 4 },
                                      "end":   { "line": 5, "character": 13 } }
        });
        assert_eq!(validated_target_uri(&link).ok(), Some("file:///tmp/fixture/lib/Counter.pm"));
    }

    #[test]
    fn null_range_is_rejected() {
        let entry = json!({ "uri": "file:///tmp/fixture/lib/Counter.pm", "range": null });
        assert!(
            validated_target_uri(&entry).is_err(),
            "a null range must not satisfy the shape check"
        );
    }

    #[test]
    fn non_numeric_range_position_is_rejected() {
        let entry = json!({
            "targetUri": "file:///tmp/fixture/lib/Counter.pm",
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
            "targetUri": "file:///tmp/fixture/lib/Counter.pm",
            "targetRange": { "start": { "line": 5, "character": 0 },
                             "end":   { "line": 8, "character": 1 } },
            "targetSelectionRange": { "start": { "line": 5 },
                                      "end":   { "line": 5, "character": 13 } }
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
        let expected_uri = "file:///tmp/ux-fixture-a/lib/Counter.pm";
        let expected_path = Path::new("/tmp/ux-fixture-a/lib/Counter.pm");

        assert!(
            uri_matches(expected_uri, expected_uri, expected_path),
            "the exact fixture URI must match"
        );
        assert!(
            !uri_matches("file:///tmp/ux-fixture-b/lib/Counter.pm", expected_uri, expected_path),
            "a same-basename file in another workspace must not match"
        );
        assert!(
            !uri_matches("file:///usr/share/perl5/Counter.pm", expected_uri, expected_path),
            "a same-basename dependency path must not match"
        );
        assert!(
            !uri_matches("not-a-uri", expected_uri, expected_path),
            "an unparseable URI must not match"
        );
    }
}
