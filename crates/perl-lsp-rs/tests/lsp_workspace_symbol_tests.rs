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

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

/// Assert a workspace/symbol response is a non-null array and return its items.
///
/// A `null` (or JSON-RPC error surfaced by the caller) must fail the test
/// rather than skip verification (#17685).
fn array_symbols(response: &serde_json::Value) -> TestResult<Vec<serde_json::Value>> {
    assert!(!response.is_null(), "workspace/symbol must not return null, got: {response:?}");
    let symbols = response
        .as_array()
        .ok_or_else(|| format!("workspace/symbol should return an array, got: {response:?}"))?;
    Ok(symbols.clone())
}

fn symbol_names(symbols: &[serde_json::Value]) -> Vec<&str> {
    symbols.iter().filter_map(|s| s["name"].as_str()).collect()
}

fn find_symbol<'a>(
    symbols: &'a [serde_json::Value],
    name: &str,
) -> TestResult<&'a serde_json::Value> {
    symbols
        .iter()
        .find(|s| s["name"].as_str() == Some(name))
        .ok_or_else(|| format!("expected symbol '{name}' in {symbols:?}").into())
}

/// Assert one symbol's exact kind, container, URI, and UTF-16 range.
fn assert_symbol(
    sym: &serde_json::Value,
    kind: u64,
    container: &str,
    uri: &str,
    range: ((u64, u64), (u64, u64)),
) -> TestResult {
    let ((start_line, start_char), (end_line, end_char)) = range;
    assert_eq!(sym["kind"].as_u64(), Some(kind), "symbol kind for {sym:?}");
    assert_eq!(sym["containerName"].as_str(), Some(container), "symbol containerName for {sym:?}");
    assert_eq!(sym["location"]["uri"].as_str(), Some(uri), "symbol location uri for {sym:?}");
    assert_eq!(
        sym["location"]["range"],
        json!({
            "start": { "line": start_line, "character": start_char },
            "end": { "line": end_line, "character": end_char }
        }),
        "symbol location range for {sym:?}"
    );
    Ok(())
}

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
    // find_all_users (kind 12, Function), never a null/empty result.
    let response = harness
        .request("workspace/symbol", json!({ "query": "find" }))
        .map_err(|e| format!("workspace/symbol request failed: {e}"))?;
    let symbols = array_symbols(&response)?;

    let mut names = symbol_names(&symbols);
    names.sort_unstable();
    assert_eq!(
        names,
        vec!["find_all_users", "find_user"],
        "query 'find' should return exactly the two find_* subs, got: {symbols:?}"
    );

    assert_symbol(
        find_symbol(&symbols, "find_user")?,
        12,
        "SearchTarget",
        doc_uri,
        ((2, 0), (5, 1)),
    )?;
    assert_symbol(
        find_symbol(&symbols, "find_all_users")?,
        12,
        "SearchTarget",
        doc_uri,
        ((7, 0), (9, 1)),
    )?;

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

    // Empty query returns every symbol in the document: the package
    // (kind 2, Module) plus alpha/beta/gamma (kind 12, Function).
    let response = harness
        .request("workspace/symbol", json!({ "query": "" }))
        .map_err(|e| format!("workspace/symbol request failed: {e}"))?;
    let symbols = array_symbols(&response)?;

    let mut names = symbol_names(&symbols);
    names.sort_unstable();
    assert_eq!(
        names,
        vec!["MyModule", "alpha", "beta", "gamma"],
        "empty query should return exactly the package and three subs, got: {symbols:?}"
    );

    let package = find_symbol(&symbols, "MyModule")?;
    assert_eq!(package["kind"].as_u64(), Some(2), "package should have kind 2 (Module)");
    assert_eq!(package["location"]["uri"].as_str(), Some(doc_uri));
    assert_eq!(
        package["location"]["range"],
        json!({
            "start": { "line": 0, "character": 0 },
            "end": { "line": 0, "character": 16 }
        }),
        "package location range for {package:?}"
    );
    assert_symbol(find_symbol(&symbols, "alpha")?, 12, "MyModule", doc_uri, ((2, 0), (2, 23)))?;
    assert_symbol(find_symbol(&symbols, "beta")?, 12, "MyModule", doc_uri, ((3, 0), (3, 22)))?;
    assert_symbol(find_symbol(&symbols, "gamma")?, 12, "MyModule", doc_uri, ((4, 0), (4, 23)))?;

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

    // Search for something that definitely does not exist: exactly an
    // empty array (not null, which previously passed this test vacuously).
    let response = harness
        .request("workspace/symbol", json!({ "query": "zzz_nonexistent_xyzzy_symbol_12345" }))
        .map_err(|e| format!("workspace/symbol request failed: {e}"))?;

    assert_eq!(
        response,
        json!([]),
        "non-matching query should return exactly an empty array, got: {response:?}"
    );

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

    // Resolve the symbol for additional detail. The LSP 3.17 method is
    // `workspaceSymbol/resolve`; the server advertises
    // `resolveProvider: true`, so this must return the enhanced symbol —
    // not null and not a method-not-found error (#17685).
    let response = harness
        .request("workspaceSymbol/resolve", basic_symbol)
        .map_err(|e| format!("workspaceSymbol/resolve request failed: {e}"))?;

    assert!(!response.is_null(), "workspaceSymbol/resolve must not return null, got: {response:?}");
    assert_eq!(
        response["name"].as_str(),
        Some("target_function"),
        "Resolved symbol should keep its name"
    );
    assert_eq!(response["kind"].as_u64(), Some(12), "Resolved symbol should keep its kind");
    assert_eq!(
        response["detail"].as_str(),
        Some("sub target_function"),
        "Resolved symbol should gain its exact detail"
    );
    assert_eq!(
        response["containerName"].as_str(),
        Some("Resolver"),
        "Resolved symbol should gain its container"
    );
    assert_eq!(
        response["location"]["uri"].as_str(),
        Some(doc_uri),
        "Resolved symbol should keep its location uri"
    );
    assert_eq!(
        response["location"]["range"],
        json!({
            "start": { "line": 2, "character": 0 },
            "end": { "line": 5, "character": 1 }
        }),
        "Resolved symbol should keep its exact location range"
    );

    Ok(())
}

/// Test that workspaceSymbolProvider capability is advertised
#[test]
fn test_workspace_symbol_capability_advertised() -> TestResult {
    let mut harness = LspHarness::new();
    let init_response = harness.initialize(None)?;

    let capabilities = &init_response["capabilities"];

    let ws_provider = capabilities.get("workspaceSymbolProvider");
    assert!(
        ws_provider.is_some(),
        "Server should advertise workspaceSymbolProvider capability. Capabilities: {:?}",
        capabilities
    );

    // The advertised object is exactly { "resolveProvider": true }, which is
    // the contract `workspaceSymbol/resolve` routing must honor (#17685).
    assert_eq!(
        ws_provider,
        Some(&json!({ "resolveProvider": true })),
        "workspaceSymbolProvider should advertise resolve support exactly"
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
    // symbol per document, with exact names, kinds, URIs, and ranges.
    let response = harness
        .request("workspace/symbol", json!({ "query": "shared" }))
        .map_err(|e| format!("workspace/symbol request failed: {e}"))?;
    let symbols = array_symbols(&response)?;

    let mut names = symbol_names(&symbols);
    names.sort_unstable();
    assert_eq!(
        names,
        vec!["shared_helper", "shared_utility"],
        "query 'shared' should return exactly one symbol per document, got: {symbols:?}"
    );
    assert_symbol(
        find_symbol(&symbols, "shared_helper")?,
        12,
        "ModuleA",
        doc1_uri,
        ((2, 0), (4, 1)),
    )?;
    assert_symbol(
        find_symbol(&symbols, "shared_utility")?,
        12,
        "ModuleB",
        doc2_uri,
        ((2, 0), (4, 1)),
    )?;

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

    // Search for "get_" — exactly both native methods (kind 6, Method).
    let response = harness
        .request("workspace/symbol", json!({ "query": "get_" }))
        .map_err(|e| format!("workspace/symbol request failed: {e}"))?;
    let symbols = array_symbols(&response)?;

    let mut names = symbol_names(&symbols);
    names.sort_unstable();
    assert_eq!(
        names,
        vec!["get_x", "get_y"],
        "workspace/symbol should find exactly the native methods, got: {symbols:?}"
    );
    assert_symbol(find_symbol(&symbols, "get_x")?, 6, "MyPoint", doc_uri, ((1, 4), (1, 30)))?;
    assert_symbol(find_symbol(&symbols, "get_y")?, 6, "MyPoint", doc_uri, ((2, 4), (2, 30)))?;

    // Search for "MyPoint" — the class (kind 5) plus its two methods.
    let response2 = harness
        .request("workspace/symbol", json!({ "query": "MyPoint" }))
        .map_err(|e| format!("workspace/symbol request failed: {e}"))?;
    let symbols2 = array_symbols(&response2)?;

    let mut names2 = symbol_names(&symbols2);
    names2.sort_unstable();
    assert_eq!(
        names2,
        vec!["MyPoint", "get_x", "get_y"],
        "workspace/symbol should find the class and its methods, got: {symbols2:?}"
    );
    assert_symbol(find_symbol(&symbols2, "MyPoint")?, 5, "main", doc_uri, ((0, 0), (3, 1)))?;

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
