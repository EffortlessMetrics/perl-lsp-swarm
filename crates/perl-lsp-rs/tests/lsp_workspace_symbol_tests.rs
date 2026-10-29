//! Tests for workspace/symbol and workspaceSymbol/resolve LSP features
//!
//! Validates the workspace symbol provider functionality including:
//! - Searching symbols with a query string
//! - Empty query returning all symbols
//! - Query that matches no results
//! - Resolving a symbol to get additional detail
//! - Capability advertisement in server initialization

mod support;
use serde_json::json;
use support::lsp_harness::LspHarness;

type TestResult = Result<(), Box<dyn std::error::Error>>;

/// Test workspace symbol search with a specific query
#[test]
fn test_workspace_symbol_query() -> TestResult {
    let mut harness = LspHarness::new();
    let _init = harness.initialize(None)?;

    let doc_uri = "file:///test_ws_sym.pl";
    harness.open(
        doc_uri,
        r#"package SearchTarget;

sub find_user {
    my $id = shift;
    return { id => $id, name => "User $id" };
}

sub find_all_users {
    return [];
}

sub delete_user {
    my $id = shift;
    return 1;
}

1;
"#,
    )?;

    // Search for symbols matching "find": exactly find_user and
    // find_all_users (delete_user must not match). Null or empty fails.
    let response = harness
        .request("workspace/symbol", json!({ "query": "find" }))
        .map_err(|e| format!("workspace/symbol request failed: {e}"))?;

    let symbols = response
        .as_array()
        .ok_or_else(|| format!("workspace/symbol must return an array, got: {response:?}"))?;
    let mut names: Vec<&str> = symbols.iter().filter_map(|s| s["name"].as_str()).collect();
    names.sort_unstable();
    assert_eq!(names.len(), symbols.len(), "every symbol must carry a name: {response:?}");
    assert_eq!(names, vec!["find_all_users", "find_user"], "got: {response:?}");
    for sym in symbols {
        assert_eq!(sym["kind"].as_u64(), Some(12), "sub symbol kind must be Function(12): {sym:?}");
        assert_eq!(
            sym["location"]["uri"].as_str(),
            Some(doc_uri),
            "symbol location must name the opened document: {sym:?}"
        );
        assert_eq!(sym["containerName"].as_str(), Some("SearchTarget"), "got: {sym:?}");
    }

    Ok(())
}

/// Test workspace symbol search with an empty query
#[test]
fn test_workspace_symbol_empty_query() -> TestResult {
    let mut harness = LspHarness::new();
    let _init = harness.initialize(None)?;

    let doc_uri = "file:///test_ws_empty.pl";
    harness.open(
        doc_uri,
        r#"package MyModule;

sub alpha { return 1; }
sub beta { return 2; }
sub gamma { return 3; }

1;
"#,
    )?;

    // Empty query returns every symbol in the document: the package plus
    // the three subs. Null or empty fails.
    let response = harness
        .request("workspace/symbol", json!({ "query": "" }))
        .map_err(|e| format!("workspace/symbol request failed: {e}"))?;

    let symbols = response
        .as_array()
        .ok_or_else(|| format!("workspace/symbol must return an array, got: {response:?}"))?;
    let mut names: Vec<&str> = symbols.iter().filter_map(|s| s["name"].as_str()).collect();
    names.sort_unstable();
    assert_eq!(names.len(), symbols.len(), "every symbol must carry a name: {response:?}");
    assert_eq!(names, vec!["MyModule", "alpha", "beta", "gamma"], "got: {response:?}");
    for sym in symbols {
        let want_kind = if sym["name"].as_str() == Some("MyModule") { 2 } else { 12 };
        assert_eq!(sym["kind"].as_u64(), Some(want_kind), "got: {sym:?}");
        assert_eq!(sym["location"]["uri"].as_str(), Some(doc_uri), "got: {sym:?}");
    }

    Ok(())
}

/// Test workspace symbol search with no matching results
#[test]
fn test_workspace_symbol_no_results() -> TestResult {
    let mut harness = LspHarness::new();
    let _init = harness.initialize(None)?;

    let doc_uri = "file:///test_ws_noresult.pl";
    harness.open(
        doc_uri,
        r#"sub hello { return "world"; }
"#,
    )?;

    // Search for something that definitely does not exist: the exact
    // result is an empty array, never null.
    let response = harness
        .request("workspace/symbol", json!({ "query": "zzz_nonexistent_xyzzy_symbol_12345" }))
        .map_err(|e| format!("workspace/symbol request failed: {e}"))?;

    let symbols = response
        .as_array()
        .ok_or_else(|| format!("workspace/symbol must return an array, got: {response:?}"))?;
    assert!(symbols.is_empty(), "non-matching query must return [], got: {response:?}");

    Ok(())
}

/// Test resolving a workspace symbol to get additional detail
#[test]
fn test_workspace_symbol_resolve() -> TestResult {
    let mut harness = LspHarness::new();
    let _init = harness.initialize(None)?;

    let doc_uri = "file:///test_ws_resolve.pl";
    harness.open(
        doc_uri,
        r#"package Resolver;

sub target_function {
    my ($arg1, $arg2) = @_;
    return $arg1 + $arg2;
}

1;
"#,
    )?;

    // Build a basic symbol as would be returned by workspace/symbol
    let basic_symbol = json!({
        "name": "target_function",
        "kind": 12,
        "location": {
            "uri": doc_uri,
            "range": {
                "start": { "line": 2, "character": 0 },
                "end": { "line": 5, "character": 1 }
            }
        }
    });

    // Resolve the symbol for additional detail. The server advertises
    // resolveProvider, so the spec method must answer (not -32601) with
    // the symbol enriched from the opened document.
    let response = harness
        .request("workspaceSymbol/resolve", basic_symbol)
        .map_err(|e| format!("workspaceSymbol/resolve request failed: {e}"))?;

    if response.is_null() {
        return Err("workspaceSymbol/resolve returned null for an opened symbol".into());
    }
    assert_eq!(response["name"].as_str(), Some("target_function"), "got: {response:?}");
    assert_eq!(response["kind"].as_i64(), Some(12), "got: {response:?}");
    assert_eq!(response["detail"].as_str(), Some("sub target_function"), "got: {response:?}");
    assert_eq!(response["location"]["uri"].as_str(), Some(doc_uri), "got: {response:?}");

    Ok(())
}

/// Duplicate names in one file resolve to the requested declaration,
/// not the first name match.
#[test]
fn test_workspace_symbol_resolve_prefers_requested_location() -> TestResult {
    let mut harness = LspHarness::new();
    let _init = harness.initialize(None)?;

    let doc_uri = "file:///dup_resolve.pl";
    harness
        .open(doc_uri, "package Dup;\nsub target { return 1; }\nsub target { return 2; }\n1;\n")?;

    // Ask to resolve the second `target` (line 2); the first shares its name.
    let response = harness
        .request(
            "workspaceSymbol/resolve",
            json!({
                "name": "target",
                "kind": 12,
                "location": {
                    "uri": doc_uri,
                    "range": {
                        "start": { "line": 2, "character": 0 },
                        "end": { "line": 2, "character": 24 }
                    }
                }
            }),
        )
        .map_err(|e| format!("workspaceSymbol/resolve request failed: {e}"))?;

    assert_eq!(
        response["location"]["range"]["start"]["line"].as_u64(),
        Some(2),
        "resolve must keep the requested declaration, got: {response:?}"
    );
    assert_eq!(response["detail"].as_str(), Some("sub target"), "got: {response:?}");

    Ok(())
}

/// Test that workspaceSymbolProvider capability is advertised
#[test]
fn test_workspace_symbol_capability_advertised() -> TestResult {
    let mut harness = LspHarness::new();
    let init_response = harness.initialize(None)?;

    let capabilities = &init_response["capabilities"];

    // The server advertises resolve support; the resolve test above
    // proves the spec method actually answers.
    assert_eq!(
        capabilities["workspaceSymbolProvider"]["resolveProvider"].as_bool(),
        Some(true),
        "workspaceSymbolProvider must advertise resolveProvider:true, got: {capabilities:?}"
    );

    Ok(())
}

/// Test workspace symbol search across multiple open documents
#[test]
fn test_workspace_symbol_multiple_documents() -> TestResult {
    let mut harness = LspHarness::new();
    let _init = harness.initialize(None)?;

    // Open first document
    let doc1_uri = "file:///module_a.pl";
    harness.open(
        doc1_uri,
        r#"package ModuleA;

sub shared_helper {
    return "A";
}

1;
"#,
    )?;

    // Open second document
    let doc2_uri = "file:///module_b.pl";
    harness.open(
        doc2_uri,
        r#"package ModuleB;

sub shared_utility {
    return "B";
}

1;
"#,
    )?;

    // Search for "shared" which appears in both documents: exactly one
    // symbol per document, each naming its own URI. Null or empty fails.
    let response = harness
        .request("workspace/symbol", json!({ "query": "shared" }))
        .map_err(|e| format!("workspace/symbol request failed: {e}"))?;

    let symbols = response
        .as_array()
        .ok_or_else(|| format!("workspace/symbol must return an array, got: {response:?}"))?;
    let mut found: Vec<(&str, &str)> = symbols
        .iter()
        .filter_map(|s| Some((s["name"].as_str()?, s["location"]["uri"].as_str()?)))
        .collect();
    found.sort_unstable();
    assert_eq!(found.len(), symbols.len(), "every symbol must carry name+uri: {response:?}");
    assert_eq!(
        found,
        vec![("shared_helper", doc1_uri), ("shared_utility", doc2_uri)],
        "got: {response:?}"
    );

    Ok(())
}

/// Test that workspace/symbol finds Perl 5.38+ native class and method declarations.
///
/// Before the fix in symbol_extraction.rs, NodeKind::Class and NodeKind::Method had no
/// arms in extract_simple_symbols, so they were silently dropped from workspace search.
#[test]
fn test_workspace_symbol_finds_native_class_and_method() -> TestResult {
    let mut harness = LspHarness::new();
    let _init = harness.initialize(None)?;

    let doc_uri = "file:///native_class_ws.pl";
    harness.open(
        doc_uri,
        "class MyPoint {\n    method get_x { return 0; }\n    method get_y { return 0; }\n}\n",
    )?;

    // Search for "get_" — must match both native methods. Null fails.
    let response = harness
        .request("workspace/symbol", json!({ "query": "get_" }))
        .map_err(|e| format!("workspace/symbol request failed: {e}"))?;
    let symbols = response
        .as_array()
        .ok_or_else(|| format!("workspace/symbol must return an array, got: {response:?}"))?;
    let mut names: Vec<&str> = symbols.iter().filter_map(|s| s["name"].as_str()).collect();
    names.sort_unstable();
    assert_eq!(names.len(), symbols.len(), "every symbol must carry a name: {response:?}");
    assert_eq!(names, vec!["get_x", "get_y"], "got: {response:?}");
    for sym in symbols {
        assert_eq!(sym["kind"].as_u64(), Some(6), "native method kind must be Method(6): {sym:?}");
    }

    // Search for "MyPoint" — must find the class declaration.
    let response2 = harness
        .request("workspace/symbol", json!({ "query": "MyPoint" }))
        .map_err(|e| format!("workspace/symbol request failed: {e}"))?;
    let symbols2 = response2
        .as_array()
        .ok_or_else(|| format!("workspace/symbol must return an array, got: {response2:?}"))?;
    // The class query also surfaces its contained methods (container
    // match); the exact result is the class plus both methods.
    let mut kinds2: Vec<(&str, u64)> =
        symbols2.iter().filter_map(|s| Some((s["name"].as_str()?, s["kind"].as_u64()?))).collect();
    kinds2.sort_unstable();
    assert_eq!(kinds2.len(), symbols2.len(), "every symbol must carry name+kind: {response2:?}");
    assert_eq!(kinds2, vec![("MyPoint", 5), ("get_x", 6), ("get_y", 6)], "got: {response2:?}");

    Ok(())
}

/// Test that workspace/symbol finds `our $VERSION` and other `our` package variables.
///
/// `our` declarations are package-interface variables that should be searchable
/// workspace-wide.  The WorkspaceSymbolsProvider indexes them via SymbolExtractor,
/// which sets declaration="our" and kind=Variable(Scalar/Array/Hash).
#[test]
fn test_workspace_symbol_finds_our_variable() -> TestResult {
    let mut harness = LspHarness::new();
    let _init = harness.initialize(None)?;

    let doc_uri = "file:///our_vars_ws.pl";
    harness.open(
        doc_uri,
        r#"package Acme::Widget;

our $VERSION = '1.00';
our @EXPORT  = ('new');
our %CONFIG  = (debug => 0);

sub new { return bless {}, shift; }

1;
"#,
    )?;

    // Search for "VERSION" — must find $VERSION
    let response = harness
        .request("workspace/symbol", json!({ "query": "VERSION" }))
        .map_err(|e| format!("workspace/symbol request failed: {e}"))?;

    assert!(response.is_array(), "workspace/symbol should return an array, got: {:?}", response);

    let symbols = response.as_array().ok_or("response is not an array")?;
    let names: Vec<&str> = symbols.iter().filter_map(|s| s["name"].as_str()).collect();

    assert!(
        names.iter().any(|n| n.contains("VERSION")),
        "workspace/symbol should find '$VERSION' when querying 'VERSION'; got: {:?}",
        names
    );

    // The found symbol must have Variable kind (13)
    for sym in symbols {
        if let Some(name) = sym["name"].as_str()
            && name.contains("VERSION")
        {
            assert_eq!(
                sym["kind"].as_u64(),
                Some(13),
                "'$VERSION' should have LSP kind 13 (Variable); got: {:?}",
                sym["kind"]
            );
        }
    }

    Ok(())
}

/// Test that workspace/symbol finds Moo/Moose `has` attribute declarations.
///
/// The WorkspaceSymbolsProvider uses SymbolExtractor which synthesizes attribute
/// symbols with declaration="has" and kind=Variable(Scalar).  They must be
/// reachable by workspace symbol search.
#[test]
fn test_workspace_symbol_finds_moo_has_attribute() -> TestResult {
    let mut harness = LspHarness::new();
    let _init = harness.initialize(None)?;

    let doc_uri = "file:///moo_has_ws.pl";
    harness.open(
        doc_uri,
        r#"package Demo::User;
use Moo;
has 'username' => (is => 'ro', isa => 'Str', required => 1);

sub greet { return "hello"; }

1;
"#,
    )?;

    // Search for "username" — should find the Moo attribute
    let response = harness
        .request("workspace/symbol", json!({ "query": "username" }))
        .map_err(|e| format!("workspace/symbol request failed: {e}"))?;

    assert!(response.is_array(), "workspace/symbol should return an array, got: {:?}", response);

    let symbols = response.as_array().ok_or("response is not an array")?;
    let names: Vec<&str> = symbols.iter().filter_map(|s| s["name"].as_str()).collect();

    assert!(
        names.iter().any(|n| *n == "username" || n.contains("username")),
        "workspace/symbol should find 'username' Moo attribute; got: {:?}",
        names
    );

    Ok(())
}

/// Verify short-query filtering at the public workspace/symbol handler.
///
/// The handler combines source-backed and generated members. This test keeps
/// both sources in one response so a new source that skips the short-query
/// guard cannot hide behind source-local tests.
#[test]
fn test_workspace_symbol_short_query_filters_all_sources() -> TestResult {
    let mut harness = LspHarness::new();
    let _init = harness.initialize(None)?;

    let doc_uri = "file:///short_query_handler.pl";
    harness.open(
        doc_uri,
        r#"package ShortQuery::Handler;
use Moo;

has attr_value   => (is => 'ro');
has callback_ref => (is => 'ro');

sub alpha_sub     { 1 }
sub main_alpha_fn { 2 }
sub get_all_items { 3 }

1;
"#,
    )?;

    let short_response = harness
        .request("workspace/symbol", json!({ "query": "a" }))
        .map_err(|e| format!("short workspace/symbol request failed: {e}"))?;
    let short_symbols =
        short_response.as_array().ok_or("short workspace/symbol response was not an array")?;
    let short_names: Vec<&str> =
        short_symbols.iter().filter_map(|symbol| symbol["name"].as_str()).collect();

    assert!(
        short_names.contains(&"alpha_sub"),
        "short query should retain source prefix match: {short_names:?}"
    );
    assert!(
        short_names.iter().any(|name| name.starts_with("attr_value")),
        "short query should retain generated prefix match: {short_names:?}"
    );
    for excluded in ["main_alpha_fn", "get_all_items", "callback_ref"] {
        assert!(
            !short_names.iter().any(|name| name.starts_with(excluded)),
            "short query unexpectedly returned substring-only match {excluded:?}: {short_names:?}"
        );
    }

    let loose_response = harness
        .request("workspace/symbol", json!({ "query": "ll" }))
        .map_err(|e| format!("loose workspace/symbol request failed: {e}"))?;
    let loose_symbols =
        loose_response.as_array().ok_or("loose workspace/symbol response was not an array")?;
    let callback_ref = loose_symbols.iter().find(|symbol| {
        symbol["name"].as_str().is_some_and(|name| name.starts_with("callback_ref"))
    });
    assert!(
        callback_ref.is_some(),
        "two-character query should return generated loose match callback_ref: {loose_symbols:?}"
    );

    Ok(())
}

/// Test that workspace/symbol uses perl-lsp-workspace-symbols provider.
///
/// Verifies that a method declared inside a named package receives a
/// `containerName` field in the response — a field only populated by
/// `WorkspaceSymbolsProvider`, not by the legacy `extract_simple_symbols`
/// fallback. This confirms the crate is wired in.
#[test]
fn test_workspace_symbol_provider_wired() -> TestResult {
    let mut harness = LspHarness::new();
    let _init = harness.initialize(None)?;

    let doc_uri = "file:///provider_wired_test.pl";
    harness.open(
        doc_uri,
        r#"package Acme::Widget;

sub build {
    my ($class, %args) = @_;
    return bless {}, $class;
}

1;
"#,
    )?;

    let response = harness
        .request("workspace/symbol", json!({ "query": "build" }))
        .map_err(|e| format!("workspace/symbol request failed: {e}"))?;

    assert!(response.is_array(), "workspace/symbol should return an array, got: {:?}", response);

    let symbols = response.as_array().ok_or("response is not an array")?;
    let build_sym = symbols.iter().find(|s| s["name"].as_str() == Some("build"));

    let sym = build_sym
        .ok_or("workspace/symbol did not return 'build' — WorkspaceSymbolsProvider not wired")?;

    // containerName is populated by WorkspaceSymbolsProvider when the sub is inside a package.
    // extract_simple_symbols never sets containerName — its absence would prove the old path is active.
    assert!(
        sym.get("containerName").is_some(),
        "Symbol 'build' should have containerName set by WorkspaceSymbolsProvider, got: {:?}",
        sym
    );

    Ok(())
}
