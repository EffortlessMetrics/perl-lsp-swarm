//! Source ordering at the outline root is distinct from child priority ordering.

use super::{DocumentSymbol, discover_subtests, nest_subtest_symbols_in_outline};
use perl_test_must::{must_some_with, must_with};

fn outline(source: &str) -> Vec<DocumentSymbol> {
    let mut parser = perl_parser::Parser::new(source);
    let ast = must_with(parser.parse(), "ordering fixture parses");
    let mut symbols =
        crate::providers::document_symbols::source_backed_document_symbols_from_ast(&ast, source)
            .symbols;
    let subtests = discover_subtests(&ast, source);
    assert!(!subtests.is_empty(), "fixture must exercise subtest insertion");
    nest_subtest_symbols_in_outline(&mut symbols, &subtests, source);
    symbols
}

fn names(symbols: &[DocumentSymbol]) -> Vec<&str> {
    symbols.iter().map(|symbol| symbol.name.as_str()).collect()
}

#[test]
fn root_collections_keep_source_order_when_subtest_is_inserted() {
    let source = "my @items = ();\n\
        my %items_by_name = ();\n\
        subtest 'middle' => sub { ok(1); };\n\
        sub later { return 1; }\n";
    let symbols = outline(source);
    assert_eq!(names(&symbols), vec!["@items", "%items_by_name", "middle", "later"]);
    assert_eq!(symbols[2].detail, "subtest");
}

#[test]
fn package_collections_keep_priority_order_when_subtest_is_inserted() {
    let source = "package Collections;\n\
        my @items = ();\n\
        my %items_by_name = ();\n\
        subtest 'middle' => sub { ok(1); };\n\
        sub later { return 1; }\n";
    let symbols = outline(source);
    let package = must_some_with(
        symbols.iter().find(|symbol| symbol.name == "Collections"),
        "package owns the same mixed siblings",
    );
    assert_eq!(names(&package.children), vec!["middle", "later", "@items", "%items_by_name"]);
    assert!(
        !symbols.iter().any(|symbol| symbol.detail == "subtest"),
        "package subtest must remain nested"
    );
}

#[test]
fn root_order_uses_character_position_within_one_line() {
    let symbols = outline("my @items = (); subtest 'middle' => sub {}; sub later {}");
    assert_eq!(names(&symbols), vec!["@items", "middle", "later"]);
    assert!(symbols.iter().all(|symbol| symbol.range.start.line == 0));
    assert!(symbols[0].range.start.character < symbols[1].range.start.character);
    assert!(symbols[1].range.start.character < symbols[2].range.start.character);
}

#[test]
fn multiple_root_subtests_keep_their_positions_among_existing_symbols() {
    let source = "subtest 'first' => sub {};\n\
        my @items = ();\n\
        subtest 'middle' => sub {};\n\
        sub later {}\n\
        subtest 'last' => sub {};\n";
    let symbols = outline(source);
    assert_eq!(names(&symbols), vec!["first", "@items", "middle", "later", "last"]);
    assert_eq!(symbols.iter().filter(|symbol| symbol.detail == "subtest").count(), 3);
}
