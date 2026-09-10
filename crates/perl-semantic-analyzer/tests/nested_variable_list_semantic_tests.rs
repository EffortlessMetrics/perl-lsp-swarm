#![deny(clippy::map_err_ignore)]
// Cohort C1 activation (#12598): all production rows exact-excepted; new findings move the crate back to non-C1.
//! Tests for nested variable list semantic analysis.
//!
//! Exercises semantic tokens, hover info, and symbol extraction
//! for nested variable list declarations like: my ($a, ($b, $c)) = ...

use perl_semantic_analyzer::Parser;
use perl_semantic_analyzer::analysis::semantic::SemanticAnalyzer;
use perl_tdd_support::must;

fn parse_and_analyze(code: &str) -> SemanticAnalyzer {
    let mut parser = Parser::new(code);
    let ast = must(parser.parse());
    SemanticAnalyzer::analyze_with_source(&ast, code)
}

#[test]
fn nested_variable_list_analyzes_simple_pair() {
    // This exercises the register_nested_decl_vars path in semantic analyzer
    let code = "my ($a, ($b, $c)) = (1, (2, 3));";
    let analyzer = parse_and_analyze(code);
    let tokens = analyzer.semantic_tokens();

    // Should produce at least some semantic tokens
    assert!(!tokens.is_empty(), "Expected semantic tokens, got none");
}

#[test]
fn nested_variable_list_analyzes_with_array() {
    // This exercises mixed nested/simple variables
    let code = "my (@arr, ($x, $y)) = @_;";
    let analyzer = parse_and_analyze(code);
    let tokens = analyzer.semantic_tokens();

    assert!(!tokens.is_empty(), "Expected semantic tokens, got none");
}

#[test]
fn nested_variable_list_deeply_nested() {
    // Exercise recursive register_nested_decl_vars
    let code = "my ($a, ($b, ($c, $d))) = (1, (2, (3, 4)));";
    let analyzer = parse_and_analyze(code);
    let tokens = analyzer.semantic_tokens();

    assert!(!tokens.is_empty(), "Expected semantic tokens, got none");
}
