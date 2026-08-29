#![deny(clippy::map_err_ignore)]
// Cohort C1 activation (#12598): all production rows exact-excepted; new findings move the crate back to non-C1.
//! Framework semantic extraction tests for Dancer2/Mojolicious route definitions.
//!
//! These tests verify that route handler symbols are synthesized when a web
//! framework `use` statement is detected, enabling goto-definition and hover
//! on route method names.

use perl_semantic_analyzer::{
    Parser,
    declaration::{current_package_at, symbol_at_cursor},
    symbol::{SymbolExtractor, SymbolKind, SymbolTable},
};
use perl_tdd_support::{must, must_some};

fn extract_symbols(code: &str) -> SymbolTable {
    let mut parser = Parser::new(code);
    let ast = must(parser.parse());
    SymbolExtractor::new_with_source(code).extract(&ast)
}

fn has_symbol(table: &SymbolTable, name: &str, kind: SymbolKind) -> bool {
    table.symbols.get(name).is_some_and(|symbols| symbols.iter().any(|symbol| symbol.kind == kind))
}

fn has_reference(table: &SymbolTable, name: &str, kind: SymbolKind) -> bool {
    table
        .references
        .get(name)
        .is_some_and(|references| references.iter().any(|reference| reference.kind == kind))
}

fn symbol_doc(table: &SymbolTable, name: &str, kind: SymbolKind) -> Option<String> {
    table
        .symbols
        .get(name)
        .and_then(|symbols| symbols.iter().find(|s| s.kind == kind))
        .and_then(|s| s.documentation.clone())
}

fn symbol_attrs(table: &SymbolTable, name: &str, kind: SymbolKind) -> Vec<String> {
    table
        .symbols
        .get(name)
        .and_then(|symbols| symbols.iter().find(|s| s.kind == kind))
        .map(|s| s.attributes.clone())
        .unwrap_or_default()
}

// === Dancer2 route detection (retired to canonical facts, #8928) ===
//
// The admitted Dancer2 route forms are owned by the canonical route facts
// (#8918/#8921/#8924) minted under exact #8914 activation. The legacy
// route-path `Subroutine` synthesis is retired for exactly those forms:
// these tests assert the retirement boundary and the containment
// properties. Parity evidence for the retirement lives in
// `dancer2_provider_cutover_parity.rs`; the labeled canonical symbol shape
// is served by the provider slice in `perl-lsp-rs-core`.

#[test]
fn dancer2_admitted_route_forms_retire_legacy_synthesis() {
    // Every admitted verb form retires the legacy route-path symbol.
    for code in [
        "use Dancer2;
get '/hello' => sub { 'Hello World' };",
        "use Dancer2;
post '/api/users' => sub { 1 };",
        "use Dancer2;
put '/api/users/:id' => sub { 'updated' };",
        "use Dancer2;
del '/api/users/:id' => sub { 'deleted' };",
        "use Dancer2;
patch '/api/users/:id' => sub { 'patched' };",
        "use Dancer2;
any '/multi' => sub { 'multi' };",
    ] {
        let table = extract_symbols(code);
        assert!(
            table.symbols.keys().all(|name| !name.starts_with('/')),
            "admitted Dancer2 form must not synthesize a legacy route-path symbol: {code}"
        );
    }
}

#[test]
fn dancer2_multiple_admitted_routes_all_retired() {
    let code = r#"
use Dancer2;

get '/foo' => sub { my $foo_local = 'foo'; };
post '/bar' => sub { my $bar_local = 'bar'; };
get '/baz' => sub { 'baz' };
"#;
    let table = extract_symbols(code);
    assert!(
        table.symbols.keys().all(|name| !name.starts_with('/')),
        "every admitted route retires the legacy synthesis"
    );
    // Positive control: the table is not empty and handler-local lexical
    // symbols stay indexed, so the negative assertion above cannot pass
    // vacuously (an extractor that dropped everything would fail here).
    assert!(table.symbols.contains_key("foo_local"), "handler-local symbols stay indexed");
    assert!(table.symbols.contains_key("bar_local"), "handler-local symbols stay indexed");
}

#[test]
fn dancer2_excluded_keyword_keeps_legacy_boundary() {
    // `!get` at the activating import: the canonical path owns nothing
    // (the keyword was never imported), so the legacy path keeps this
    // unadmitted form — the recorded retirement boundary.
    let code = r#"
use Dancer2 '!get';

get '/hello' => sub {
    return 'Hello World';
};
"#;
    let table = extract_symbols(code);
    assert!(
        has_symbol(&table, "/hello", SymbolKind::Subroutine),
        "excluded-keyword form stays on the legacy path (retirement boundary)"
    );
}

#[test]
fn dancer2_custom_dsl_keeps_legacy_boundary() {
    // A custom DSL owns its keyword vocabulary; the canonical path is a
    // dynamic boundary and admits nothing, so the legacy path keeps the
    // form — the recorded retirement boundary.
    let code = r#"
use Dancer2 dsl => 'My::DSL';

get '/hello' => sub {
    return 'Hello World';
};
"#;
    let table = extract_symbols(code);
    assert!(
        has_symbol(&table, "/hello", SymbolKind::Subroutine),
        "custom-DSL form stays on the legacy path (retirement boundary)"
    );
}

#[test]
fn dancer2_route_without_use_is_not_synthesized() {
    // Without `use Dancer2`, a bare `get` call should NOT produce a route symbol
    let code = r#"
get '/hello' => sub {
    return 'Hello World';
};
"#;
    let table = extract_symbols(code);
    assert!(
        !has_symbol(&table, "/hello", SymbolKind::Subroutine),
        "bare `get` without `use Dancer2` should NOT produce a route symbol"
    );
}

// === Mojolicious::Lite route detection ===

#[test]
fn mojolicious_lite_get_route_emits_subroutine_symbol() {
    let code = r#"
use Mojolicious::Lite;

get '/hello' => sub {
    my $c = shift;
    $c->render(text => 'Hello World');
};
"#;
    let table = extract_symbols(code);
    assert!(
        has_symbol(&table, "/hello", SymbolKind::Subroutine),
        "expected route symbol `/hello` for Mojolicious::Lite `get '/hello' => sub`"
    );
}

#[test]
fn mojolicious_lite_post_route_emits_subroutine_symbol() {
    let code = r#"
use Mojolicious::Lite;

post '/api/submit' => sub {
    my $c = shift;
    $c->render(json => { ok => 1 });
};
"#;
    let table = extract_symbols(code);
    assert!(
        has_symbol(&table, "/api/submit", SymbolKind::Subroutine),
        "expected route symbol `/api/submit` for Mojolicious::Lite POST route"
    );
}

#[test]
fn mojolicious_lite_route_symbol_has_http_method_attribute() {
    let code = r#"
use Mojolicious::Lite;

post '/submit' => sub { my $c = shift };
"#;
    let table = extract_symbols(code);
    let attrs = symbol_attrs(&table, "/submit", SymbolKind::Subroutine);
    assert!(
        attrs.iter().any(|a| a == "http_method=POST"),
        "expected `http_method=POST` attribute on Mojo route symbol, got: {attrs:?}"
    );
}

// === any route (Dancer2) ===

// Dancer v1 keeps string-target references: upstream Dancer v1 allows an action to
// be the name of a subroutine (#8910 containment keeps the families separate).
#[test]
fn dancer_route_target_string_adds_subroutine_reference() {
    let code = r#"
use Dancer;

get '/about' => 'show_about';

sub show_about {
    return 'About';
}
"#;
    let table = extract_symbols(code);
    assert!(
        has_reference(&table, "show_about", SymbolKind::Subroutine),
        "expected route target string `show_about` to be recorded as a Subroutine reference"
    );
}

// Dancer2 route construction requires a CodeRef handler; a string target must not
// become an exact subroutine reference or definition (#8910).
#[test]
fn dancer2_route_target_string_does_not_add_subroutine_reference() {
    let code = r#"
use Dancer2;

get '/status' => 'show_status';

sub show_status {
    return 'ok';
}
"#;
    let table = extract_symbols(code);
    assert!(
        !has_reference(&table, "show_status", SymbolKind::Subroutine),
        "Dancer2 string target `show_status` must NOT be recorded as a Subroutine reference"
    );
}

// `Dancer2::Core` is not the DSL module and must not activate Dancer2 semantics (#8910).
#[test]
fn dancer2_core_use_does_not_activate_route_semantics() {
    let code = r#"
use Dancer2::Core;

get '/x' => sub { 1 };
"#;
    let table = extract_symbols(code);
    assert!(
        !has_symbol(&table, "/x", SymbolKind::Subroutine),
        "`use Dancer2::Core` must not activate Dancer2 route synthesis"
    );
}

// A locally defined same-named sub keeps `get` ordinary Perl without activation.
#[test]
fn same_named_local_get_without_activation_is_ordinary_perl() {
    let code = r#"
sub get { return 'local' }

get '/x' => sub { 1 };
"#;
    let table = extract_symbols(code);
    assert!(
        !has_symbol(&table, "/x", SymbolKind::Subroutine),
        "same-named `get` without framework activation must stay ordinary Perl"
    );
    assert!(
        has_symbol(&table, "get", SymbolKind::Subroutine),
        "the local `sub get` itself should still be indexed"
    );
}

// Activation is per-package: another package in the same file does not inherit it.
// `App` is exactly activated, so its admitted route retires legacy synthesis;
// `Other` has no activation and must not synthesize either.
#[test]
fn dancer2_activation_does_not_leak_across_packages() {
    let code = r#"
package App;
use Dancer2;
get '/activated' => sub { 1 };

package Other;
get '/not_activated' => sub { 1 };
"#;
    let table = extract_symbols(code);
    assert!(
        !has_symbol(&table, "/activated", SymbolKind::Subroutine),
        "exact `use Dancer2` in `App` retires the legacy synthesis (canonical owns it)"
    );
    assert!(
        !has_symbol(&table, "/not_activated", SymbolKind::Subroutine),
        "package `Other` must not inherit Dancer2 activation"
    );
}

// === Plack::Builder middleware chain detection ===

#[test]
fn plack_builder_enable_emits_middleware_symbol() {
    let code = r#"
use Plack::Builder;

builder {
    enable 'Static';
    enable 'Plack::Middleware::Session';
};
"#;
    let table = extract_symbols(code);

    assert!(
        has_symbol(&table, "Plack::Middleware::Static", SymbolKind::Package),
        "expected quoted middleware `Static` to normalize to `Plack::Middleware::Static`"
    );
    assert!(
        has_symbol(&table, "Plack::Middleware::Session", SymbolKind::Package),
        "expected quoted middleware `Plack::Middleware::Session` to be preserved"
    );

    let attrs = symbol_attrs(&table, "Plack::Middleware::Static", SymbolKind::Package);
    assert!(
        attrs.iter().any(|a| a == "framework=Plack::Builder"),
        "expected Plack framework attribute on middleware symbol, got: {attrs:?}"
    );

    let doc = symbol_doc(&table, "Plack::Middleware::Static", SymbolKind::Package);
    assert!(
        doc.is_some_and(|d| d.contains("middleware") && d.contains("Static")),
        "expected middleware documentation to mention the normalized module name"
    );
}

#[test]
fn plack_builder_enable_symbol_at_cursor_resolves_middleware_package() {
    let code = r#"
use Plack::Builder;

builder {
    enable 'Static';
    enable 'Plack::Middleware::Session';
};
"#;
    let mut parser = Parser::new(code);
    let ast = must(parser.parse());

    let static_offset = must_some(code.find("Static"));
    let current_pkg = current_package_at(&ast, static_offset);
    let symbol = must_some(symbol_at_cursor(&ast, static_offset, current_pkg));

    assert_eq!(
        symbol.pkg.as_ref(),
        "Plack::Middleware::Static",
        "short-name Plack middleware should normalize to the full package name"
    );
    assert_eq!(
        symbol.name.as_ref(),
        "Plack::Middleware::Static",
        "short-name Plack middleware should normalize to the full package name"
    );
}

#[test]
fn plack_builder_mount_emits_mount_symbol() {
    let code = r#"
use Plack::Builder;

builder {
    mount '/api' => $api_app;
    mount '/' => $app;
};
"#;
    let table = extract_symbols(code);

    assert!(has_symbol(&table, "/api", SymbolKind::Subroutine), "expected `/api` mount symbol");
    assert!(has_symbol(&table, "/", SymbolKind::Subroutine), "expected `/` mount symbol");

    let attrs = symbol_attrs(&table, "/api", SymbolKind::Subroutine);
    assert!(
        attrs.iter().any(|a| a == "mount_path=/api"),
        "expected mount path attribute on `/api`, got: {attrs:?}"
    );
    assert!(
        attrs.iter().any(|a| a == "mount_target=$api_app"),
        "expected mount target attribute on `/api`, got: {attrs:?}"
    );
}

#[test]
fn plack_builder_without_use_is_not_synthesized() {
    let code = r#"
builder {
    enable Static;
    mount '/api' => $api_app;
};
"#;
    let table = extract_symbols(code);

    assert!(
        !has_symbol(&table, "Plack::Middleware::Static", SymbolKind::Package),
        "bare `builder` should not synthesize Plack middleware symbols"
    );
    assert!(
        !has_symbol(&table, "/api", SymbolKind::Subroutine),
        "bare `builder` should not synthesize mount symbols"
    );
}
