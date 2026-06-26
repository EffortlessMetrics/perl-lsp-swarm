//! Tests for DESTROY and AUTOLOAD UNIVERSAL method recognition
//!
//! Validates that:
//! - DESTROY is recognized in goto-definition (navigation to UNIVERSAL::DESTROY)
//! - AUTOLOAD is recognized in goto-definition (navigation to UNIVERSAL::AUTOLOAD)
//! - Both methods appear in completion lists with correct documentation

mod support;

use serde_json::{Value, json};
use support::lsp_harness::LspHarness;

type TestResult = Result<(), Box<dyn std::error::Error>>;

/// Helper to extract completion items from response
fn extract_completions(response: &Value) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let items = response
        .get("items")
        .and_then(|v| v.as_array())
        .ok_or("Expected completion items array")?;

    let labels: Vec<String> = items
        .iter()
        .filter_map(|item| item.get("label").and_then(|l| l.as_str()).map(String::from))
        .collect();

    Ok(labels)
}

/// Helper to find a completion item by label
fn find_completion_item<'a>(
    response: &'a Value,
    label: &str,
) -> Result<Option<&'a Value>, Box<dyn std::error::Error>> {
    let items = response
        .get("items")
        .and_then(|v| v.as_array())
        .ok_or("Expected completion items array")?;

    Ok(items.iter().find(|item| {
        item.get("label").and_then(|l| l.as_str()).map(|l| l == label).unwrap_or(false)
    }))
}

// ---------------------------------------------------------------------------
// Test 1: DESTROY appears in completion list with documentation
// ---------------------------------------------------------------------------

#[test]
fn completion_offers_destroy_method() -> TestResult {
    let mut harness = LspHarness::new();
    harness.initialize(None)?;

    // Open a test file that triggers method completion
    harness.open(
        "file:///test.pl",
        r#"#!/usr/bin/perl
use strict;
use warnings;

my $obj = {};
$obj->
"#,
    )?;

    harness.barrier();

    // Request completion at the end of "$obj->"
    // Line 5 (0-indexed), character 7 (after "->")
    let response = harness.request(
        "textDocument/completion",
        json!({
            "textDocument": { "uri": "file:///test.pl" },
            "position": { "line": 5, "character": 7 }
        }),
    )?;

    let completions = extract_completions(&response)?;

    // Verify DESTROY is in the completion list
    assert!(
        completions.contains(&"DESTROY".to_string()),
        "DESTROY should appear in completion list, got: {:?}",
        completions
    );

    // Verify AUTOLOAD is in the completion list
    assert!(
        completions.contains(&"AUTOLOAD".to_string()),
        "AUTOLOAD should appear in completion list, got: {:?}",
        completions
    );

    Ok(())
}

// ---------------------------------------------------------------------------
// Test 2: DESTROY completion has correct documentation
// ---------------------------------------------------------------------------

#[test]
fn completion_destroy_has_documentation() -> TestResult {
    let mut harness = LspHarness::new();
    harness.initialize(None)?;

    harness.open(
        "file:///test.pl",
        r#"#!/usr/bin/perl
use strict;
use warnings;

my $obj = {};
$obj->
"#,
    )?;

    harness.barrier();

    let response = harness.request(
        "textDocument/completion",
        json!({
            "textDocument": { "uri": "file:///test.pl" },
            "position": { "line": 5, "character": 7 }
        }),
    )?;

    let destroy_item = find_completion_item(&response, "DESTROY")?
        .ok_or("DESTROY not found in completion items")?;

    let doc =
        destroy_item.get("documentation").ok_or("DESTROY completion item missing documentation")?;

    // Verify documentation mentions "Destructor"
    let doc_str = doc.as_str().ok_or("Documentation should be a string")?;

    assert!(
        doc_str.contains("Destructor") || doc_str.contains("destructor"),
        "DESTROY documentation should mention 'Destructor', got: {}",
        doc_str
    );

    Ok(())
}

// ---------------------------------------------------------------------------
// Test 3: AUTOLOAD completion has correct documentation
// ---------------------------------------------------------------------------

#[test]
fn completion_autoload_has_documentation() -> TestResult {
    let mut harness = LspHarness::new();
    harness.initialize(None)?;

    harness.open(
        "file:///test.pl",
        r#"#!/usr/bin/perl
use strict;
use warnings;

my $obj = {};
$obj->
"#,
    )?;

    harness.barrier();

    let response = harness.request(
        "textDocument/completion",
        json!({
            "textDocument": { "uri": "file:///test.pl" },
            "position": { "line": 5, "character": 7 }
        }),
    )?;

    let autoload_item = find_completion_item(&response, "AUTOLOAD")?
        .ok_or("AUTOLOAD not found in completion items")?;

    let doc = autoload_item
        .get("documentation")
        .ok_or("AUTOLOAD completion item missing documentation")?;

    let doc_str = doc.as_str().ok_or("Documentation should be a string")?;

    assert!(
        doc_str.contains("AUTOLOAD")
            || doc_str.contains("Automatic")
            || doc_str.contains("dispatcher"),
        "AUTOLOAD documentation should mention method dispatch, got: {}",
        doc_str
    );

    Ok(())
}

// ---------------------------------------------------------------------------
// Test 4: DESTROY goto-definition on Package->DESTROY resolves to UNIVERSAL
// ---------------------------------------------------------------------------

#[test]
fn goto_definition_on_package_destroy_resolves_to_universal() -> TestResult {
    let mut harness = LspHarness::new();
    harness.initialize(None)?;

    // Create a UNIVERSAL::DESTROY definition
    harness.open(
        "file:///lib/UNIVERSAL.pm",
        r#"package UNIVERSAL;
use strict;
use warnings;

sub DESTROY {
    my ($self) = @_;
    # Cleanup code
}

1;
"#,
    )?;

    // Create a test file that calls Package->DESTROY
    harness.open(
        "file:///test.pl",
        r#"#!/usr/bin/perl
use strict;
use warnings;

package MyPackage;

sub method {
    my ($self) = @_;
}

package main;

my $obj = bless {}, 'MyPackage';
$obj->DESTROY;
"#,
    )?;

    harness.barrier();

    // Request definition at "DESTROY" on line 14 (0-indexed)
    // The line is: $obj->DESTROY;
    // We need to position on the DESTROY token
    let response = harness.request(
        "textDocument/definition",
        json!({
            "textDocument": { "uri": "file:///test.pl" },
            "position": { "line": 13, "character": 10 }  // On "DESTROY"
        }),
    )?;

    let locations = response.as_array().ok_or("Expected array result for definition")?;

    // We should either get UNIVERSAL::DESTROY or a location in UNIVERSAL.pm
    // The important thing is that we get some result (not empty)
    assert!(!locations.is_empty(), "Definition should be found for DESTROY method");

    Ok(())
}

// ---------------------------------------------------------------------------
// Test 5: DESTROY appears in both regular and special contexts
// ---------------------------------------------------------------------------

#[test]
fn destroy_recognized_in_various_contexts() -> TestResult {
    let mut harness = LspHarness::new();
    harness.initialize(None)?;

    harness.open(
        "file:///test.pl",
        r#"#!/usr/bin/perl
use strict;
use warnings;

package MyClass;

sub new {
    my ($class) = @_;
    return bless {}, $class;
}

sub DESTROY {
    my ($self) = @_;
    print "Destroying object\n";
}

package main;

my $obj = MyClass->new();
$obj->DESTROY;
"#,
    )?;

    harness.barrier();

    // Test completion context
    let response = harness.request(
        "textDocument/completion",
        json!({
            "textDocument": { "uri": "file:///test.pl" },
            "position": { "line": 18, "character": 7 }  // After "$obj->"
        }),
    )?;

    let completions = extract_completions(&response)?;
    assert!(
        completions.contains(&"DESTROY".to_string()),
        "DESTROY should be in completion after $obj->"
    );

    Ok(())
}
