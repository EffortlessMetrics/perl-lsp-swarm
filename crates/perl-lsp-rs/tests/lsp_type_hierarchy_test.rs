mod support;

use serde_json::json;
use support::lsp_harness::LspHarness;

type TestResult = Result<(), Box<dyn std::error::Error>>;

/// Test Type Hierarchy support (LSP 3.17)
#[test]
fn test_type_hierarchy_prepare() -> TestResult {
    let mut harness = LspHarness::new();
    harness.initialize_with_root("file:///test", Some(json!({})))?;

    // Open a document with class hierarchy
    let uri = "file:///test.pl";
    let content = r#"package Base;

package Derived;
use parent 'Base';

package Another;
our @ISA = ('Base');

sub new {
    my $class = shift;
    bless {}, $class;
}
"#;

    harness.open(uri, content)?;

    let items = harness.request(
        "textDocument/prepareTypeHierarchy",
        json!({
            "textDocument": { "uri": uri },
            "position": { "line": 0, "character": 8 }
        }),
    )?;

    let items = items.as_array().ok_or("Should return type hierarchy items")?;
    assert!(!items.is_empty(), "Should have at least one item");

    let item = &items[0];
    assert_eq!(item["name"], "Base");
    assert_eq!(item["kind"], 5); // Class
    assert!(item["uri"].as_str().is_some());
    assert!(item["range"].is_object());
    assert!(item["selectionRange"].is_object());

    Ok(())
}

#[test]
fn test_type_hierarchy_supertypes() -> TestResult {
    let mut harness = LspHarness::new();
    harness.initialize_with_root("file:///test", Some(json!({})))?;

    // Open a document with inheritance
    let uri = "file:///test.pl";
    let content = r#"package Child;
use parent 'Parent1';
use parent 'Parent2';

package Parent1;
package Parent2;
"#;

    harness.open(uri, content)?;

    let items = harness.request(
        "textDocument/prepareTypeHierarchy",
        json!({
            "textDocument": { "uri": uri },
            "position": { "line": 0, "character": 8 }
        }),
    )?;

    let items = items.as_array().ok_or("Result is not an array")?;
    let child_item = &items[0];

    let supertypes = harness.request(
        "typeHierarchy/supertypes",
        json!({
            "item": {
                "name": child_item["name"],
                "kind": child_item["kind"],
                "uri": child_item["uri"],
                "range": child_item["range"],
                "selectionRange": child_item["selectionRange"],
                "data": {
                    "uri": uri,
                    "name": "Child"
                }
            }
        }),
    )?;

    let supertypes = supertypes.as_array().ok_or("Failed to get supertypes array")?;

    // Should find Parent1 and Parent2
    let names: Vec<String> =
        supertypes.iter().filter_map(|item| item["name"].as_str()).map(|s| s.to_string()).collect();

    assert!(names.contains(&"Parent1".to_string()), "Should find Parent1");
    assert!(names.contains(&"Parent2".to_string()), "Should find Parent2");

    Ok(())
}

#[test]
fn test_type_hierarchy_subtypes() -> TestResult {
    let mut harness = LspHarness::new();
    harness.initialize_with_root("file:///test", Some(json!({})))?;

    // Open a document with derived classes
    let uri = "file:///test.pl";
    let content = r#"package Base;

package Derived1;
use parent 'Base';

package Derived2;
our @ISA = ('Base');
"#;

    harness.open(uri, content)?;

    let items = harness.request(
        "textDocument/prepareTypeHierarchy",
        json!({
            "textDocument": { "uri": uri },
            "position": { "line": 0, "character": 8 }
        }),
    )?;

    let items = items.as_array().ok_or("Result is not an array")?;
    let base_item = &items[0];

    let subtypes = harness.request(
        "typeHierarchy/subtypes",
        json!({
            "item": {
                "name": base_item["name"],
                "kind": base_item["kind"],
                "uri": base_item["uri"],
                "range": base_item["range"],
                "selectionRange": base_item["selectionRange"],
                "data": {
                    "uri": uri,
                    "name": "Base"
                }
            }
        }),
    )?;

    let subtypes = subtypes.as_array().ok_or("Failed to get subtypes array")?;

    // Should find Derived1 and Derived2
    let names: Vec<String> =
        subtypes.iter().filter_map(|item| item["name"].as_str()).map(|s| s.to_string()).collect();

    assert!(names.contains(&"Derived1".to_string()), "Should find Derived1");
    assert!(names.contains(&"Derived2".to_string()), "Should find Derived2");

    Ok(())
}

#[test]
fn test_type_hierarchy_capability_advertised() -> TestResult {
    let mut harness = LspHarness::new();
    let init = harness.initialize_with_root("file:///test", Some(json!({})))?;

    let caps = init.get("capabilities").ok_or("Failed to get capabilities from init response")?;

    // Type hierarchy should be advertised in non-lock mode
    if !cfg!(feature = "lsp-ga-lock") {
        let provider = &caps["typeHierarchyProvider"];
        let advertised = provider.as_bool() == Some(true) || provider.is_object();
        if !advertised {
            return Err(
                format!("typeHierarchyProvider should be advertised, got: {provider:?}").into()
            );
        }
    }

    Ok(())
}

#[test]
fn test_type_hierarchy_with_namespace_packages() -> TestResult {
    let mut harness = LspHarness::new();
    harness.initialize_with_root("file:///test", Some(json!({})))?;

    // Test with Foo::Bar -> Foo hierarchy
    let uri = "file:///test.pl";
    let content = r#"package Foo;

package Foo::Bar;

package Foo::Bar::Baz;
"#;

    harness.open(uri, content)?;

    let items = harness.request(
        "textDocument/prepareTypeHierarchy",
        json!({
            "textDocument": { "uri": uri },
            "position": { "line": 2, "character": 12 }
        }),
    )?;

    let items = items.as_array().ok_or("Failed to get items array")?;
    assert!(!items.is_empty());

    let item = &items[0];
    assert_eq!(item["name"], "Foo::Bar");

    Ok(())
}
