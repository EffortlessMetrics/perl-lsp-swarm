//! Tests for Test2 subtest discovery.

use super::*;
use perl_test_must::{must_some_with, must_with};

fn discover(source: &str) -> Vec<DiscoveredSubtest> {
    let mut parser = perl_parser::Parser::new(source);
    let ast = must_with(parser.parse(), "source parses");
    discover_subtests(&ast, source)
}

#[test]
fn discovers_top_level_named_subtests() {
    let source = "use Test2::V0;\n\
        subtest 'user lookup' => sub {\n\
            ok(1, 'found');\n\
        };\n\
        subtest 'email' => sub {\n\
            is(1, 1, 'matches');\n\
        };\n\
        done_testing;\n";
    let subtests = discover(source);
    assert_eq!(subtests.len(), 2);
    assert_eq!(subtests[0].name, SubtestName::Named("user lookup".to_string()));
    assert_eq!(subtests[1].name, SubtestName::Named("email".to_string()));
    assert!(subtests[0].children.is_empty());
}

#[test]
fn discovers_nested_subtests_as_a_tree() {
    let source = "subtest 'outer' => sub {\n\
            ok(1);\n\
            subtest 'inner' => sub {\n\
                subtest 'deepest' => sub { ok(1); };\n\
            };\n\
        };\n";
    let subtests = discover(source);
    assert_eq!(subtests.len(), 1, "only the outer subtest is top-level");
    let outer = &subtests[0];
    assert_eq!(outer.name, SubtestName::Named("outer".to_string()));
    assert_eq!(outer.children.len(), 1);
    let inner = &outer.children[0];
    assert_eq!(inner.name, SubtestName::Named("inner".to_string()));
    assert_eq!(inner.children.len(), 1);
    assert_eq!(inner.children[0].name, SubtestName::Named("deepest".to_string()));
}

#[test]
fn double_quoted_names_are_unquoted() {
    let source = "subtest \"double quoted\" => sub { ok(1); };\n";
    let subtests = discover(source);
    assert_eq!(subtests.len(), 1);
    assert_eq!(subtests[0].name, SubtestName::Named("double quoted".to_string()));
}

#[test]
fn dynamic_name_is_not_guessed() {
    let source = "my $name = 'x';\nsubtest $name => sub { ok(1); };\n";
    let subtests = discover(source);
    assert_eq!(subtests.len(), 1);
    assert_eq!(subtests[0].name, SubtestName::Dynamic);
    assert_eq!(subtests[0].name.label(), "subtest (dynamic)");
    assert!(subtests[0].name.as_static().is_none());
}

#[test]
fn interpolated_name_with_variable_is_dynamic() {
    let source = "my $i = 1;\nsubtest \"case $i\" => sub { ok(1); };\n";
    let subtests = discover(source);
    assert_eq!(subtests.len(), 1);
    assert_eq!(subtests[0].name, SubtestName::Dynamic);
}

#[test]
fn no_subtests_yields_empty() {
    let source = "use Test2::V0;\nok(1);\nis(2, 2);\ndone_testing;\n";
    assert!(discover(source).is_empty());
}

#[test]
fn subtest_inside_conditional_is_found() {
    let source = "if ($ENV{EXTRA}) {\n\
            subtest 'conditional' => sub { ok(1); };\n\
        }\n";
    let subtests = discover(source);
    assert_eq!(subtests.len(), 1);
    assert_eq!(subtests[0].name, SubtestName::Named("conditional".to_string()));
}

#[test]
fn document_symbols_mirror_the_tree() {
    let source = "subtest 'outer' => sub {\n\
            subtest 'inner' => sub { ok(1); };\n\
        };\n";
    let subtests = discover(source);
    let symbols = subtest_document_symbols(&subtests);
    assert_eq!(symbols.len(), 1);
    assert_eq!(symbols[0].name, "outer");
    assert_eq!(symbols[0].detail, "subtest");
    assert_eq!(symbols[0].children.len(), 1);
    assert_eq!(symbols[0].children[0].name, "inner");
    // The selection range (name arg) is inside the full range.
    assert!(
        symbols[0].selection_range.start.to_byte_offset(source)
            >= symbols[0].range.start.to_byte_offset(source)
    );
}

#[test]
fn nearest_subtest_resolves_innermost_at_cursor() {
    // Lines (0-based):
    // 0: subtest 'outer' => sub {
    // 1:     ok(1);
    // 2:     subtest 'inner' => sub {
    // 3:         ok(2);
    // 4:     };
    // 5: };
    let source = "subtest 'outer' => sub {\n    ok(1);\n    subtest 'inner' => sub {\n        ok(2);\n    };\n};\n";
    let subtests = discover(source);

    // Cursor on line 3 (inside inner) resolves to the inner subtest.
    let inner = must_some_with(nearest_subtest_at_line(&subtests, 3), "cursor is inside a subtest");
    assert_eq!(inner.name, SubtestName::Named("inner".to_string()));

    // Cursor on line 1 (inside outer, before inner) resolves to outer.
    let outer = must_some_with(nearest_subtest_at_line(&subtests, 1), "cursor is inside a subtest");
    assert_eq!(outer.name, SubtestName::Named("outer".to_string()));
}

#[test]
fn nearest_subtest_returns_none_outside_any_subtest() {
    let source = "use Test2::V0;\nok(1);\nsubtest 'x' => sub { ok(1); };\ndone_testing;\n";
    let subtests = discover(source);
    // Line 1 is the bare `ok(1);` — not inside any subtest.
    assert!(nearest_subtest_at_line(&subtests, 1).is_none());
}

#[test]
fn buffered_and_streamed_variants_are_discovered() {
    let source = "subtest_buffered 'buf' => sub { ok(1); };\n\
        subtest_streamed 'str' => sub { ok(1); };\n";
    let subtests = discover(source);
    assert_eq!(subtests.len(), 2);
    assert_eq!(subtests[0].name, SubtestName::Named("buf".to_string()));
    assert_eq!(subtests[1].name, SubtestName::Named("str".to_string()));
}

/// Compose the source-backed outline with subtest discovery the way the
/// runtime document-symbol path does, then nest.
fn nested_outline(source: &str) -> Vec<DocumentSymbol> {
    let mut parser = perl_parser::Parser::new(source);
    let ast = must_with(parser.parse(), "source parses");
    let core_result =
        crate::providers::document_symbols::source_backed_document_symbols_from_ast(&ast, source);
    let mut outline = core_result.symbols;
    let subtests = discover_subtests(&ast, source);
    if !subtests.is_empty() {
        nest_subtest_symbols_in_outline(&mut outline, &subtests, source);
    }
    outline
}

fn find_named<'a>(symbols: &'a [DocumentSymbol], name: &str) -> Option<&'a DocumentSymbol> {
    symbols.iter().find(|s| s.name == name)
}

/// Depth-first symbol lookup: outline members may sit below a package symbol
/// even at file top level.
fn find_named_deep<'a>(symbols: &'a [DocumentSymbol], name: &str) -> Option<&'a DocumentSymbol> {
    for symbol in symbols {
        if symbol.name == name {
            return Some(symbol);
        }
        if let Some(found) = find_named_deep(&symbol.children, name) {
            return Some(found);
        }
    }
    None
}

#[test]
fn lexically_scoped_subtest_nests_under_its_enclosing_sub() {
    // Lines (0-based): 0=package, 1=use, 2=sub helper containing the subtest,
    // 3..5=subtest call, 6=closing brace.
    let source = "package t;\n\
        use Test2::V0;\n\
        sub helper {\n\
            subtest 'inside helper' => sub {\n\
                ok(1);\n\
            };\n\
        }\n";
    let outline = nested_outline(source);

    assert!(
        find_named(&outline, "inside helper").is_none(),
        "subtest inside a named sub must not float to the outline root; got: {outline:?}"
    );
    let helper = must_some_with(
        find_named_deep(&outline, "helper"),
        "helper subroutine symbol missing from outline",
    );
    let nested = must_some_with(
        find_named(&helper.children, "inside helper"),
        "subtest not nested under helper",
    );
    assert_eq!(nested.detail, "subtest");
    // Same kind as other outline callables (LSP Function).
    assert_eq!(nested.kind, 12);
    // Selection range points at the name argument, inside both spans.
    let nested_start = nested.selection_range.start.to_byte_offset(source);
    let nested_range_end = nested.range.end.to_byte_offset(source);
    let helper_start = helper.range.start.to_byte_offset(source);
    let helper_end = helper.range.end.to_byte_offset(source);
    assert!(nested_start >= nested.range.start.to_byte_offset(source));
    assert!(nested_start >= helper_start && nested_range_end <= helper_end);
}

#[test]
fn file_scope_subtest_without_containing_package_stays_top_level() {
    let source = "use Test2::V0;\n\
        subtest 'user lookup' => sub {\n\
            ok(1);\n\
        };\n\
        done_testing;\n";
    let outline = nested_outline(source);
    let top_names: Vec<&str> = outline.iter().map(|s| s.name.as_str()).collect();
    assert!(
        top_names.contains(&"user lookup"),
        "root-level subtest stays at the outline root; got: {top_names:?}"
    );
    // And it keeps the established conventions.
    let subtest = must_some_with(find_named(&outline, "user lookup"), "subtest present");
    assert_eq!(subtest.detail, "subtest");
    assert_eq!(subtest.kind, 12);
}

#[test]
fn role_scope_owns_subtest_with_canonical_interface_kind() {
    let source = "package Earlier;\n\
        package My::Role;\n\
        use Moo::Role;\n\
        subtest 'role test' => sub { ok(1); };\n";
    let outline = nested_outline(source);

    let role = must_some_with(find_named_deep(&outline, "My::Role"), "role symbol missing");
    assert_eq!(role.kind, SymbolKind::Role.to_lsp_kind_document_symbol());
    assert_eq!(role.children.len(), 1, "role should own exactly one subtest");
    let role_subtest = must_some_with(role.children.first(), "role subtest missing");
    assert_eq!(role_subtest.name, "role test");
    assert_eq!(role_subtest.kind, SymbolKind::Subroutine.to_lsp_kind_document_symbol());
    assert_eq!(role_subtest.detail, "subtest");
    assert!(
        find_named(&outline, "role test").is_none(),
        "role subtest must not remain at the outline root"
    );
}

#[test]
fn closed_package_block_does_not_own_following_subtest() {
    let source = "package Outer;\n\
        package Inner {\n\
            sub inner {}\n\
        }\n\
        subtest 'after block' => sub { ok(1); };\n";
    let outline = nested_outline(source);

    let outer = must_some_with(find_named_deep(&outline, "Outer"), "outer package missing");
    let inner = must_some_with(find_named(&outer.children, "Inner"), "inner package missing");
    assert!(
        find_named(&inner.children, "after block").is_none(),
        "subtest after a closed package block must not remain under Inner"
    );
    assert!(
        find_named(&outer.children, "after block").is_some(),
        "subtest after Inner must return to the enclosing package"
    );
}

#[test]
fn subtest_insertion_preserves_priority_then_source_order() {
    let source = "package P;\n\
        my $early = 1;\n\
        subtest 'middle' => sub {};\n\
        sub later {}\n";
    let outline = nested_outline(source);
    let package = must_some_with(find_named_deep(&outline, "P"), "package missing");
    let names: Vec<&str> = package.children.iter().map(|symbol| symbol.name.as_str()).collect();

    assert_eq!(
        names,
        vec!["middle", "later", "$early"],
        "callable children stay source-ordered ahead of lower-priority variables"
    );
}

#[test]
fn two_sibling_subtests_keep_source_order_inside_their_sub() {
    // Lines (0-based): 0=sub both, 1..2='alpha' subtest, 3..4='beta' subtest.
    let source = "sub both {\n\
        subtest 'alpha' => sub { ok(1); };\n\
        subtest 'beta' => sub { ok(2); };\n\
        }\n";
    let outline = nested_outline(source);
    let both = must_some_with(find_named(&outline, "both"), "both symbol missing");
    let names: Vec<&str> = both.children.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, vec!["alpha", "beta"]);
}

#[test]
fn role_and_statement_package_regions_own_only_their_lexical_members() {
    let source = "package Before;
        subtest 'before' => sub { ok(1); };
        package My::Role;
        use Moo::Role;
        subtest 'role test' => sub { ok(1); };
        package Inner {
            subtest 'inner test' => sub { ok(1); };
        }
        subtest 'after inner' => sub { ok(1); };
";
    let outline = nested_outline(source);

    let role = must_some_with(find_named_deep(&outline, "My::Role"), "role symbol missing");
    assert_eq!(role.kind, 8, "Moo::Role must retain the LSP role kind");
    assert_eq!(
        role.children.iter().map(|child| child.name.as_str()).collect::<Vec<_>>(),
        vec!["role test", "after inner"]
    );

    let inner = must_some_with(find_named_deep(&outline, "Inner"), "block package symbol missing");

    let before =
        must_some_with(find_named_deep(&outline, "Before"), "statement package symbol missing");
    assert_eq!(
        before
            .children
            .iter()
            .filter(|child| child.detail == "subtest")
            .map(|child| child.name.as_str())
            .collect::<Vec<_>>(),
        vec!["before"]
    );
    assert!(
        !inner.children.iter().any(|child| child.name == "after inner"),
        "a subtest after a block package must not remain under that package"
    );
    assert_eq!(
        outline.iter().filter(|symbol| symbol.detail == "subtest").count(),
        0,
        "all package-owned subtests should be nested"
    );
}

#[test]
fn dynamic_nested_names_preserve_cardinality_and_source_order() {
    let source = "package Dynamic;
        my $name = 'runtime';
        subtest $name => sub {
            subtest 'literal' => sub { ok(1); };
            subtest \"case $name\" => sub { ok(1); };
        };
";
    let outline = nested_outline(source);
    let package = must_some_with(find_named_deep(&outline, "Dynamic"), "package symbol missing");
    let outer = must_some_with(
        find_named(&package.children, "subtest (dynamic)"),
        "dynamic subtest missing",
    );
    assert_eq!(outer.children.len(), 2);
    assert_eq!(
        outer.children.iter().map(|child| child.name.as_str()).collect::<Vec<_>>(),
        vec!["literal", "subtest (dynamic)"]
    );
}

#[test]
fn new_subtest_keeps_compiler_priority_order_with_mixed_siblings() {
    let source = "package Mixed;
        my $value = 1;
        subtest 'middle' => sub { ok(1); };
        sub later { return 1; }
";
    let outline = nested_outline(source);
    let package = must_some_with(find_named_deep(&outline, "Mixed"), "package symbol missing");
    let names: Vec<&str> = package.children.iter().map(|child| child.name.as_str()).collect();
    assert_eq!(names, vec!["middle", "later", "$value"]);
}

#[test]
fn ordinary_array_and_hash_follow_callables_in_compiler_priority_order() {
    let source = "package Collections;
        my @items = ();
        my %items_by_name = ();
        subtest 'middle' => sub { ok(1); };
        sub later { return 1; }
";
    let outline = nested_outline(source);
    let package = must_some_with(find_named_deep(&outline, "Collections"), "package missing");
    let names: Vec<&str> = package.children.iter().map(|child| child.name.as_str()).collect();

    assert_eq!(names, vec!["middle", "later", "@items", "%items_by_name"]);
}
