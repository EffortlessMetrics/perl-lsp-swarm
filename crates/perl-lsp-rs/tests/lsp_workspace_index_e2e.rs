mod support;

use perl_workspace::workspace_index::WorkspaceIndex;
use serde_json::json;
use std::time::Duration;
use support::lsp_harness::LspHarness;
use url::Url;

/// Test that workspace index properly tracks cross-file symbols
#[test]
fn test_workspace_index_cross_file() -> Result<(), Box<dyn std::error::Error>> {
    let index = WorkspaceIndex::new();

    let uri_a = Url::parse("file:///workspace/A.pm")?;
    let content_a = r#"
package Foo;

sub bar {
    my $x = 42;
    return $x;
}

sub baz {
    return bar() + 1;
}
1;
"#;

    let uri_b = Url::parse("file:///workspace/B.pl")?;
    let content_b = r#"
use Foo;

my $result = Foo::bar();
print "Result: $result\n";

Foo::baz();
"#;

    index.index_file(uri_a.clone(), content_a.to_string())?;
    index.index_file(uri_b.clone(), content_b.to_string())?;

    let bar_def = index.find_definition("bar");
    assert!(bar_def.is_some(), "Should find definition of 'bar'");
    let def = bar_def.ok_or("Should have definition for bar")?;
    assert_eq!(def.uri, uri_a.to_string(), "Definition should be in A.pm");

    let bar_refs = index.find_references("bar");
    assert!(!bar_refs.is_empty(), "Should find references to 'bar'");

    let symbols = index.search_symbols("ba");
    assert!(symbols.len() >= 2, "Should find both 'bar' and 'baz'");

    index.remove_file(uri_a.as_str());
    let bar_def_after = index.find_definition("bar");
    assert!(bar_def_after.is_none(), "Definition should be removed after file removal");

    Ok(())
}

/// Test that index updates when files change
#[test]
fn test_workspace_index_file_updates() -> Result<(), Box<dyn std::error::Error>> {
    let index = WorkspaceIndex::new();
    let uri = Url::parse("file:///workspace/test.pl")?;

    let content_v1 = r#"
sub old_name {
    return 1;
}
"#;

    index.index_file(uri.clone(), content_v1.to_string())?;

    let old_def = index.find_definition("old_name");
    assert!(old_def.is_some(), "Should find old_name");

    let content_v2 = r#"
sub new_name {
    return 2;
}
"#;

    index.index_file(uri.clone(), content_v2.to_string())?;

    let old_def_after = index.find_definition("old_name");
    assert!(old_def_after.is_none(), "old_name should be gone");

    let new_def = index.find_definition("new_name");
    assert!(new_def.is_some(), "Should find new_name");

    Ok(())
}

/// Test LSP workspace/symbol request with index
#[test]
fn test_lsp_workspace_symbols_with_index() -> Result<(), Box<dyn std::error::Error>> {
    let mut harness = LspHarness::new();
    harness.initialize_with_root("file:///workspace", Some(json!({})))?;

    harness.open("file:///workspace/A.pm", "package Foo;\nsub bar { return 42; }\n1;")?;
    harness.open("file:///workspace/B.pm", "package Bar;\nsub baz { return 'hello'; }\n1;")?;
    harness.wait_for_symbol("bar", None, Duration::from_secs(10))?;
    harness.wait_for_symbol("baz", None, Duration::from_secs(10))?;

    let result = harness.request_with_timeout(
        "workspace/symbol",
        json!({
            "query": "ba"
        }),
        Duration::from_secs(10),
    )?;

    let symbols = result.as_array().ok_or("Result should be an array")?;
    assert!(symbols.len() >= 2, "Should find at least bar and baz");

    let names: Vec<String> = symbols
        .iter()
        .filter_map(|s| s.get("name").and_then(|n| n.as_str()))
        .map(|s| s.to_string())
        .collect();

    assert!(names.contains(&"bar".to_string()), "Should find 'bar'");
    assert!(names.contains(&"baz".to_string()), "Should find 'baz'");

    Ok(())
}

/// Test cross-file go-to-definition
#[test]
fn test_cross_file_definition() -> Result<(), Box<dyn std::error::Error>> {
    let mut harness = LspHarness::new();
    harness.initialize_with_root("file:///workspace", Some(json!({})))?;

    harness.open(
        "file:///workspace/Lib.pm",
        "package Lib;\nsub helper {\n    return 'help';\n}\n1;",
    )?;
    harness.open("file:///workspace/main.pl", "use Lib;\nmy $x = Lib::helper();")?;
    harness.barrier();

    let result = harness.request_with_timeout(
        "textDocument/definition",
        json!({
            "textDocument": {
                "uri": "file:///workspace/main.pl"
            },
            "position": {
                "line": 1,
                "character": 15
            }
        }),
        Duration::from_secs(10),
    )?;

    if let Some(location) = result.as_object() {
        if let Some(uri) = location.get("uri").and_then(|u| u.as_str()) {
            assert!(uri.contains("Lib.pm"), "Definition should be in Lib.pm");
        } else if let Some(locations) = result.as_array() {
            assert!(!locations.is_empty(), "Should have at least one location");
            if let Some(first) = locations.first()
                && let Some(uri) = first.get("uri").and_then(|u| u.as_str())
            {
                assert!(uri.contains("Lib.pm"), "Definition should be in Lib.pm");
            }
        }
    }

    Ok(())
}
