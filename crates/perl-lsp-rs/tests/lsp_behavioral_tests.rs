// Integration tests print diagnostic output for CI troubleshooting; this is
// not the LSP server's stdio transport, so print_stdout/print_stderr don't
// apply the way they do to production code.
#![allow(clippy::print_stdout, clippy::print_stderr)]

/// Behavioral tests for LSP functionality
/// These tests verify actual functionality, not just response shapes
/// They ensure the wired infrastructure produces real results
use serde_json::json;
use std::path::Path;
use std::process::{Command, Output};
use std::time::Duration;
use url::Url;

// Import the proper test harness
mod support;
use support::lsp_harness::{LspHarness, TempWorkspace};

type TestResult = Result<(), Box<dyn std::error::Error>>;
type ChildResult<T> = Result<T, Box<dyn std::error::Error>>;

const FALLBACK_CHILD_MODE: &str = "PERL_LSP_BEHAVIORAL_FALLBACK_CHILD";
const FALLBACK_CHILD_MARKER: &str = "PERL_LSP_BEHAVIORAL_FALLBACK_CHILD_RAN";

fn in_fallback_child(selector: &str) -> bool {
    std::env::var_os(FALLBACK_CHILD_MODE).is_some_and(|value| value.to_string_lossy() == selector)
}

fn run_fallback_child(selector: &str) -> ChildResult<Output> {
    let mut command = Command::new(std::env::current_exe()?);
    command.args(["--exact", selector, "--nocapture"]);
    command.env(FALLBACK_CHILD_MODE, selector);
    command.env("LSP_TEST_FALLBACKS", "1");
    command.env("LSP_TEST_TIMEOUT_MS", "15000");
    command.env("LSP_TEST_SHORT_MS", "5000");
    Ok(command.output()?)
}

fn assert_fallback_child_ran(output: &Output, selector: &str) -> TestResult {
    let expected = format!("{FALLBACK_CHILD_MARKER}={selector}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let count = stdout.lines().filter(|line| *line == expected).count();
    if count != 1 {
        return Err(format!(
            "expected one fallback child marker {expected:?}, found {count}; stdout={stdout} stderr={}",
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    Ok(())
}

/// Convert a path to a file:// URI string, cross-platform safe
fn path_to_uri(path: &Path) -> Result<String, Box<dyn std::error::Error>> {
    Ok(Url::from_file_path(path)
        .map_err(|_| format!("file path to URI failed: {}", path.display()))?
        .to_string())
}

fn uri_matches(expected: &str, actual: &str) -> bool {
    if expected == actual {
        return true;
    }

    if cfg!(windows) {
        return expected.eq_ignore_ascii_case(actual);
    }

    false
}

mod test_fixtures {
    pub const MAIN_FILE: &str = r#"#!/usr/bin/env perl
use strict;
use warnings;

use My::Module;

my $obj = My::Module->new(name => 'test');
$obj->process();

sub calculate {
    my ($x, $y) = @_;
    return $x + $y;
}

my $result = calculate(5, 10);
print "Result: $result\n";

# PENDING: implement caching
my $config = {
    host => 'localhost',
    port => 3000,
};
"#;

    pub const MODULE_FILE: &str = r#"package My::Module;
use strict;
use warnings;

sub new {
    my ($class, %args) = @_;
    return bless \%args, $class;
}

sub process {
    my $self = shift;
    print "Processing: $self->{name}\n";
    return 1;
}

1;
"#;
}

/// Create and initialize a test server with the fixture files
fn create_test_server() -> Result<(LspHarness, TempWorkspace), Box<dyn std::error::Error>> {
    // Create harness with real temp workspace
    let (mut harness, workspace) = LspHarness::with_workspace(&[
        ("script.pl", test_fixtures::MAIN_FILE),
        ("lib/My/Module.pm", test_fixtures::MODULE_FILE),
    ])?;

    // Open documents with real file URIs from the temp workspace
    harness.open_document(&workspace.uri("script.pl"), test_fixtures::MAIN_FILE)?;

    harness.open_document(&workspace.uri("lib/My/Module.pm"), test_fixtures::MODULE_FILE)?;

    // Send didSave notifications to trigger any incremental indexing
    harness.did_save(&workspace.uri("script.pl")).ok();
    harness.did_save(&workspace.uri("lib/My/Module.pm")).ok();

    // Wait for the server to process files and become idle (optimized for performance)
    harness.wait_for_idle(Duration::from_millis(200));

    Ok((harness, workspace))
}

#[test]
fn test_cross_file_definition() -> TestResult {
    let (mut harness, workspace) = create_test_server()?;

    // Wait until the module is discoverable (increased timeout for CI stability)
    harness.wait_for_symbol(
        "My::Module",
        Some(workspace.uri("lib/My/Module.pm").as_str()),
        Duration::from_millis(500),
    )?;

    // Request go-to-definition for My::Module usage
    let result = harness.request(
        "textDocument/definition",
        json!({
            "textDocument": {"uri": workspace.uri("script.pl")},
            "position": {"line": 4, "character": 6} // On "My::Module"
        }),
    )?;

    {
        let locations = result.as_array().ok_or("Should return location array")?;
        // Previously this early-returned Ok(()) in fallback mode, hiding a
        // broken go-to-definition behind a green check (issue #4642). The test
        // must assert that the module definition is actually resolved.
        assert!(!locations.is_empty(), "Should find module definition");

        // Verify it points to the module file
        let first_location = &locations[0];
        assert!(
            first_location["uri"].as_str().is_some_and(|actual| uri_matches(
                workspace.uri("lib/My/Module.pm").as_str(),
                actual
            )),
            "Should navigate to module file"
        );
    }
    Ok(())
}

#[test]
fn test_cross_file_references() -> TestResult {
    let (mut harness, workspace) = create_test_server()?;

    // Wait until the module is indexed (increased timeout for CI stability)
    harness.wait_for_symbol(
        "process",
        Some(workspace.uri("lib/My/Module.pm").as_str()),
        Duration::from_millis(500),
    )?;

    // Request references for the 'process' method, which has both a declaration
    // in the module and a cross-file method-call usage in script.pl.
    let result = harness.request(
        "textDocument/references",
        json!({
            "textDocument": {"uri": workspace.uri("lib/My/Module.pm")},
            "position": {"line": 9, "character": 5}, // On "process" method
            "context": {"includeDeclaration": true}
        }),
    )?;

    {
        let references = result.as_array().ok_or("Should return reference array")?;
        // Previously this early-returned Ok(()) in fallback mode when fewer
        // than two references were found, hiding a broken references provider
        // behind a green check (issue #4642). The test must assert that both
        // the declaration and the cross-file usage are actually resolved.
        assert!(references.len() >= 2, "Should find declaration and usage");

        // Check for reference in script.pl
        let has_script_ref = references.iter().any(|r| {
            r["uri"]
                .as_str()
                .is_some_and(|actual| uri_matches(workspace.uri("script.pl").as_str(), actual))
        });
        assert!(has_script_ref, "Should find reference in script.pl");
    }
    Ok(())
}

#[test]
fn test_workspace_symbol_search() -> TestResult {
    const SELECTOR: &str = "test_workspace_symbol_search";
    if in_fallback_child(SELECTOR) {
        let (mut harness, workspace) = create_test_server()?;
        let result = harness.request_with_timeout(
            "workspace/symbol",
            json!({"query": "process"}),
            Duration::from_secs(10),
        )?;
        let symbols = result.as_array().ok_or("Should return symbol array")?;
        assert!(!symbols.is_empty(), "Should find 'process' method");
        let process_symbol = symbols
            .iter()
            .find(|symbol| symbol["name"].as_str() == Some("process"))
            .ok_or("Should find process method")?;
        assert!(
            process_symbol["location"]["uri"].as_str().is_some_and(|actual| uri_matches(
                workspace.uri("lib/My/Module.pm").as_str(),
                actual
            )),
            "Process method should be in Module.pm"
        );
        println!("{FALLBACK_CHILD_MARKER}={SELECTOR}");
        return Ok(());
    }
    let output = run_fallback_child(SELECTOR)?;
    assert!(output.status.success(), "fallback child failed: {output:?}");
    assert_fallback_child_ran(&output, SELECTOR)
}

#[test]
fn test_extract_variable_returns_edits() -> TestResult {
    const SELECTOR: &str = "test_extract_variable_returns_edits";
    if in_fallback_child(SELECTOR) {
        let (mut harness, workspace) = create_test_server()?;
        let result = harness.request_with_timeout(
            "textDocument/codeAction",
            json!({
                "textDocument": {"uri": workspace.uri("script.pl")},
                "range": {
                    "start": {"line": 11, "character": 11},
                    "end": {"line": 11, "character": 18}
                },
                "context": {"diagnostics": []}
            }),
            Duration::from_secs(10),
        )?;
        let actions = result.as_array().ok_or("Should return action array")?;
        let extract_action = actions
            .iter()
            .find(|action| action["title"].as_str().is_some_and(|title| title.contains("Extract")))
            .ok_or("Should find extract variable action")?;
        let changes = extract_action
            .get("edit")
            .and_then(|edit| edit.get("changes"))
            .ok_or("Should have workspace edit changes")?;
        let edits = changes[workspace.uri("script.pl").as_str()]
            .as_array()
            .ok_or("Should have edits array")?;
        assert!(!edits.is_empty(), "Should have actual text edits");
        println!("{FALLBACK_CHILD_MARKER}={SELECTOR}");
        return Ok(());
    }
    let output = run_fallback_child(SELECTOR)?;
    assert!(output.status.success(), "fallback child failed: {output:?}");
    assert_fallback_child_ran(&output, SELECTOR)
}

#[test]
// AC2:runCritic - perl.runCritic command integration with diagnostic workflow
fn test_critic_violations_emit_diagnostics() -> TestResult {
    let (mut harness, workspace) = create_test_server()?;

    // Create a test file without strict or warnings
    let test_file = r#"#!/usr/bin/perl
# This file should trigger Perl::Critic violations

my $variable = 42;
print "Value: $variable\n";

sub calculate {
    my ($a, $b) = @_;
    $a + $b;  # Missing explicit return
}
"#;

    // Open the document
    let file_path = workspace.dir.path().join("critic_test.pl");
    std::fs::write(&file_path, test_file)?;
    harness.open_document(&path_to_uri(&file_path)?, test_file)?;

    // Execute perl.runCritic command (with extended timeout for potential external tool)
    let result = harness.request_with_timeout(
        "workspace/executeCommand",
        json!({
            "command": "perl.runCritic",
            "arguments": [path_to_uri(&file_path)?]
        }),
        Duration::from_secs(5),
    )?;

    // Check that we got violations
    {
        assert!(result.get("status").is_some(), "Should have status field");
        assert_eq!(result["status"].as_str(), Some("success"), "Command should succeed");

        let violation_count = result["violationCount"].as_u64().unwrap_or(0);
        assert!(
            violation_count >= 2,
            "Should detect at least 2 violations (missing strict and warnings)"
        );

        // Check for specific violations. Since #3299 the default (Native)
        // `perl.runCritic` reports native rule IDs (`native.testing.require_use_*`)
        // rather than the legacy BuiltInAnalyzer PascalCase policy names.
        if let Some(violations) = result["violations"].as_array() {
            let has_strict_violation = violations.iter().any(|v| {
                v["policy"]
                    .as_str()
                    .is_some_and(|p| p.contains("native.testing.require_use_strict"))
            });
            let has_warnings_violation = violations.iter().any(|v| {
                v["policy"]
                    .as_str()
                    .is_some_and(|p| p.contains("native.testing.require_use_warnings"))
            });

            assert!(has_strict_violation, "Should detect missing 'use strict'");
            assert!(has_warnings_violation, "Should detect missing 'use warnings'");
        }
    }

    // Now request code actions to fix the violations
    let actions_result = harness.request(
        "textDocument/codeAction",
        json!({
            "textDocument": {"uri": path_to_uri(&file_path)?},
            "range": {
                "start": {"line": 0, "character": 0},
                "end": {"line": 1, "character": 0}
            },
            "context": {"diagnostics": [], "only": ["quickfix"]}
        }),
    )?;

    // Verify we have quickfixes for Perl::Critic violations
    {
        let actions = actions_result.as_array().ok_or("Should return action array")?;
        assert!(!actions.is_empty(), "Should have code actions");

        // Look for strict/warnings quickfixes
        let has_strict_fix =
            actions.iter().any(|a| a["title"].as_str().is_some_and(|t| t.contains("strict")));

        assert!(has_strict_fix, "Should have quickfix for adding strict/warnings");
    }
    Ok(())
}

#[cfg(feature = "lsp-extras")]
#[test]
fn test_test_generation_actions_present() -> TestResult {
    let (mut harness, workspace) = create_test_server()?;

    // Request code actions for the calculate subroutine
    let result = harness.request(
        "textDocument/codeAction",
        json!({
            "textDocument": {"uri": workspace.uri("script.pl")},
            "range": {
                "start": {"line": 9, "character": 0},
                "end": {"line": 12, "character": 1} // Cover "calculate" subroutine
            },
            "context": {"diagnostics": []}
        }),
    )?;

    {
        let actions = result.as_array().ok_or("Should return action array")?;

        // Find test generation action
        let test_action = actions
            .iter()
            .find(|a| a["title"].as_str().is_some_and(|t| t.contains("Generate test")));

        assert!(test_action.is_some(), "Should have test generation action");

        // Verify it has the right command
        let action = test_action.ok_or("Should have test generation action")?;
        assert_eq!(
            action["command"]["command"].as_str(),
            Some("perl.generateTest"),
            "Should use perl.generateTest command"
        );

        // Verify arguments include test code
        let args = &action["command"]["arguments"];
        let args_array = args.as_array().ok_or("Should have arguments")?;
        assert!(!args_array.is_empty(), "Should have test generation arguments");

        let first_arg = &args_array[0];
        assert!(first_arg["name"].is_string(), "Should include subroutine name");
        assert!(first_arg["test"].is_string(), "Should include generated test code");
    }
    Ok(())
}

#[test]
fn test_completion_detail_formatting() -> TestResult {
    const SELECTOR: &str = "test_completion_detail_formatting";
    if in_fallback_child(SELECTOR) {
        let (mut harness, workspace) = create_test_server()?;
        let result = harness.request_with_timeout(
            "textDocument/completion",
            json!({
                "textDocument": {"uri": workspace.uri("script.pl")},
                "position": {"line": 7, "character": 6}
            }),
            Duration::from_secs(10),
        )?;
        let items = if result.is_array() {
            result.as_array().ok_or("Expected array")?
        } else {
            result["items"].as_array().ok_or("Expected completion items array")?
        };
        assert!(!items.is_empty(), "Should have completion items");
        let typed_items = items
            .iter()
            .filter(|item| {
                item["detail"]
                    .as_str()
                    .is_some_and(|detail| detail.len() < 50 && !detail.contains("InferredType"))
            })
            .count();
        assert!(typed_items > 0, "Should have type information in completion details");
        println!("{FALLBACK_CHILD_MARKER}={SELECTOR}");
        return Ok(());
    }
    let output = run_fallback_child(SELECTOR)?;
    assert!(output.status.success(), "fallback child failed: {output:?}");
    assert_fallback_child_ran(&output, SELECTOR)
}

#[test]
fn test_hover_enriched_information() -> TestResult {
    let (mut harness, workspace) = create_test_server()?;

    // Request hover for My::Module
    let result = harness.request(
        "textDocument/hover",
        json!({
            "textDocument": {"uri": workspace.uri("script.pl")},
            "position": {"line": 4, "character": 10} // On "My::Module"
        }),
    )?;

    {
        // Previously this early-returned Ok(()) when hover was null or empty in
        // fallback mode, hiding a broken hover provider behind a green check
        // (issue #4642). The test must assert that real hover content is
        // returned for a `use My::Module` reference.
        assert!(!result.is_null(), "Should return hover information");

        let contents = &result["contents"];
        let hover_text = if let Some(value) = contents["value"].as_str() {
            value.to_string()
        } else if let Some(markup) = contents.as_array() {
            markup.iter().filter_map(|m| m["value"].as_str()).collect::<Vec<_>>().join("\n")
        } else {
            String::new()
        };

        assert!(!hover_text.is_empty(), "Should have hover content");

        // Check for enriched information
        assert!(
            hover_text.contains("Module")
                || hover_text.contains("package")
                || hover_text.contains("use"),
            "Should show package/module information"
        );
    }
    Ok(())
}

#[test]
fn test_folding_ranges_work() -> TestResult {
    let (mut harness, workspace) = create_test_server()?;

    // Request folding ranges with timeout
    let result = harness.request_with_timeout(
        "textDocument/foldingRange",
        json!({
            "textDocument": {"uri": workspace.uri("lib/My/Module.pm")}
        }),
        Duration::from_millis(500),
    )?;

    {
        let ranges = result.as_array().ok_or("Should return folding ranges")?;
        // Previously this early-returned Ok(()) in fallback mode when no ranges
        // were returned, hiding a broken foldingRange provider behind a green
        // check (issue #4642). The fixture module has a multi-line `sub new`
        // body, so at least one folding range must be produced.
        assert!(!ranges.is_empty(), "Should have folding ranges");

        // Check for at least one multiline folding range.
        let has_multiline_fold = ranges.iter().any(|r| {
            let start = r["startLine"].as_u64();
            let end = r["endLine"].as_u64();
            matches!((start, end), (Some(s), Some(e)) if e > s)
        });
        assert!(has_multiline_fold, "Should have at least one multiline folding range");
    }
    Ok(())
}

#[test]
fn test_utf16_definition_with_non_ascii_on_same_line() -> TestResult {
    let (mut harness, workspace) = create_test_server()?;

    // Module with a trivial body
    let module = r#"package My::Module;
use strict;
sub new { bless {}, shift }
1;
"#;

    // Same line contains 2 emojis (each 2 UTF-16 units) and an umlaut (1 unit)
    // The caret will sit on 'M' in `My::Module` after those non-ASCII chars.
    let line = r#"my $obj = "😀😀 zö " . My::Module->new();"#;

    let script = format!(
        r#"#!/usr/bin/env perl
use utf8;
use strict;
use lib "lib";
use My::Module;
{}
"#,
        line
    );

    // Create the module file
    let module_path = workspace.dir.path().join("lib/My/Module.pm");
    std::fs::create_dir_all(module_path.parent().ok_or("No parent directory")?)?;
    std::fs::write(&module_path, module)?;
    harness.open_document(&path_to_uri(&module_path)?, module)?;
    harness.did_save(&path_to_uri(&module_path)?).ok();

    // Create and open the script
    let script_path = workspace.dir.path().join("script.pl");
    std::fs::write(&script_path, &script)?;
    harness.open_document(&path_to_uri(&script_path)?, &script)?;
    harness.did_save(&path_to_uri(&script_path)?).ok();

    // Wait until the symbol appears so we don't race the indexer
    let module_uri = path_to_uri(&module_path)?;
    harness.wait_for_symbol("My::Module", Some(&module_uri), Duration::from_millis(500))?;
    harness.wait_for_idle(Duration::from_millis(200));

    // Compute the UTF-16 column for the 'M' in "My::Module" on that exact line.
    let line_idx =
        script.lines().position(|l| l == line).ok_or("line with non-ASCII is present")?;
    let m_byte = line.find("My::Module").ok_or("line contains My::Module")?;
    let char_col_utf16 = utf16_units(&line[..m_byte]);

    // Ask for definition using UTF-16 character units
    let result = harness.request_with_timeout(
        "textDocument/definition",
        json!({
            "textDocument": { "uri": path_to_uri(&script_path)? },
            "position": { "line": line_idx, "character": char_col_utf16 }
        }),
        Duration::from_millis(500),
    )?;

    // Should resolve to the module file
    let locations = result.as_array().ok_or("definition returns array")?;
    // Previously this early-returned Ok(()) in fallback mode when no locations
    // were returned, hiding a UTF-16 position-handling regression behind a
    // green check (issue #4642). The test must assert the definition resolves
    // to the module file even when the prefix contains non-ASCII characters.
    assert!(!locations.is_empty(), "should return at least one location");
    assert!(
        locations[0]["uri"].as_str().is_some_and(|actual| uri_matches(module_uri.as_str(), actual)),
        "definition should jump to module file"
    );
    Ok(())
}

// Helper to count UTF-16 code units
fn utf16_units(s: &str) -> usize {
    // Count UTF-16 code units in the prefix (surrogate pairs count as 2)
    s.encode_utf16().count()
}

#[test]
fn test_word_boundary_references() -> TestResult {
    let (mut harness, workspace) = create_test_server()?;

    // Create a file with similar variable names to test boundary detection
    let file_path = workspace.dir.path().join("boundary_test.pl");
    let content = r#"#!/usr/bin/perl
my $process = 1;
my $process_data = 2;
my $preprocessor = 3;
print $process;        # Should match
print $process_data;   # Should NOT match
print $preprocessor;   # Should NOT match
"#;

    std::fs::write(&file_path, content)?;
    harness.open_document(&path_to_uri(&file_path)?, content)?;
    harness.did_save(&path_to_uri(&file_path)?).ok();
    harness.wait_for_idle(Duration::from_millis(200));

    // Find references to $process (not $process_data or $preprocessor)
    let result = harness.request_with_timeout(
        "textDocument/references",
        json!({
            "textDocument": { "uri": path_to_uri(&file_path)? },
            "position": { "line": 1, "character": 4 },  // Position within $process
            "context": { "includeDeclaration": true }
        }),
        Duration::from_millis(500),
    )?;

    {
        let refs = result.as_array().ok_or("Should return references")?;
        // Previously this early-returned Ok(()) in fallback mode when no
        // matches were found, hiding a word-boundary regression (e.g. matching
        // `$process_data` or `$preprocessor` for `$process`) behind a green
        // check (issue #4642). The test now asserts the word-boundary property
        // it claims to verify.
        //
        // NOTE: removing the escape hatch also surfaced two separate
        // references-provider defects that are out of scope for this
        // word-boundary test and tracked separately:
        //   (a) URI case inconsistency (`file:///F:/...` vs `file:///f:/...`)
        //       causes the same in-file reference to be emitted twice.
        //   (b) The `$process` scalar is conflated with the `process` sub
        //       across files (matches in lib/My/Module.pm and script.pl).
        // Because of (a), an exact-count assertion would be brittle against a
        // duplicate bug rather than the word-boundary behavior under test, so
        // we assert presence/absence of the relevant lines instead.

        let boundary_uri = path_to_uri(&file_path)?;
        let boundary_lines: Vec<u64> = refs
            .iter()
            .filter(|r| {
                r["uri"].as_str().is_some_and(|actual| uri_matches(boundary_uri.as_str(), actual))
            })
            .filter_map(|r| r["range"]["start"]["line"].as_u64())
            .collect();

        // The declaration (line 1) and the exact `print $process` usage
        // (line 4) must be among the in-file references.
        assert!(
            boundary_lines.contains(&1),
            "Should find $process declaration on line 1, got lines: {boundary_lines:?}"
        );
        assert!(
            boundary_lines.contains(&4),
            "Should find $process usage on line 4, got lines: {boundary_lines:?}"
        );

        // Word-boundary: the similar-prefixed variables must NOT match.
        assert!(
            !boundary_lines.contains(&5),
            "Should NOT find $process_data on line 5, got lines: {boundary_lines:?}"
        );
        assert!(
            !boundary_lines.contains(&6),
            "Should NOT find $preprocessor on line 6, got lines: {boundary_lines:?}"
        );
    }
    Ok(())
}
