use perl_semantic_analyzer::{Parser, semantic::SemanticAnalyzer};
use perl_tdd_support::{must, must_some_with};
fn resolved(source: &str, use_spelling: &str) -> Option<String> {
    let ast = must(Parser::new(source).parse());
    let analyzer = SemanticAnalyzer::analyze_with_source(&ast, source);
    let offset = must_some_with(source.rfind(use_spelling), "fixture use exists");
    analyzer.find_definition(offset).map(|symbol| symbol.qualified_name.clone())
}
#[test]
fn bare_alias_does_not_escape_its_lexical_block() {
    assert_eq!(resolved("use strict; { our $alias = 3; } { print $alias; }", "$alias"), None);
}
#[test]
fn qualified_package_variable_survives_lexical_alias_scope() {
    assert_eq!(
        resolved("use strict; { our $alias = 3; } print $main::alias;", "$main::alias"),
        Some("main::alias".into())
    );
}
#[test]
fn qualified_lookup_does_not_substitute_another_package() {
    assert_eq!(
        resolved("use strict; { our $alias = 3; } print $Other::alias;", "$Other::alias"),
        None
    );
}
